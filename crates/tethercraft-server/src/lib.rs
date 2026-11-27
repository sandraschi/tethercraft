//! Self-hosted rendezvous + relay (Tier 3, one binary).
//!
//! Newline-delimited JSON over TCP. Rendezvous (register, heartbeat, lookup)
//! and relay mailbox routing are real and tested over loopback. The relay
//! only ever sees opaque hex bytes (threat model item 9): it never parses
//! session payloads. WAN hole punching, IPv6 paths beyond dual-stack bind,
//! and WebSocket transports are later steps.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};
use tethercraft_core::TetherError;

const HEARTBEAT_FRESH_SECS: u64 = 120;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRecord {
    pub id: String,
    pub key: String,
    pub addr: String,
}

struct Entry {
    key: String,
    addr: String,
    last_seen: Instant,
}

struct Shared {
    reg: HashMap<String, Entry>,
    boxes: HashMap<String, mpsc::Sender<String>>,
    /// Host ids waiting for a controller: (None, since) = waiting,
    /// (Some(token), since) = a controller asked and the token is ready.
    awaiting: HashMap<String, (Option<String>, Instant)>,
    /// In-progress relay pairings, keyed by one-time token.
    pending: HashMap<String, PendingBridge>,
    token_counter: u64,
}

struct PendingBridge {
    a: Option<TcpStream>,
    b: Option<TcpStream>,
    created: Instant,
}

/// Pending pairings older than this are dropped on the next bridge op.
const BRIDGE_TIMEOUT: Duration = Duration::from_secs(60);

fn next_token(counter: &mut u64) -> String {
    *counter = counter.wrapping_add(1);
    format!(
        "{:016x}",
        (*counter).wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0)
        )
    )
}

fn sweep_bridges(shared: &mut Shared) {
    let now = Instant::now();
    let live: std::collections::HashSet<String> = shared
        .pending
        .iter()
        .filter(|(_, p)| now.duration_since(p.created) <= BRIDGE_TIMEOUT)
        .map(|(k, _)| k.clone())
        .collect();
    shared.pending.retain(|k, _| live.contains(k));
    shared.awaiting.retain(|_, v| match v {
        (None, _) => true,
        (Some(tok), _) => live.contains(tok),
    });
}

/// Copy bytes both ways until either side goes away. The relay never looks
/// at the payload: after the handshake this is opaque session traffic.
fn bridge_pair(a: TcpStream, b: TcpStream) {
    let (mut a_read, mut b_write) = match (a.try_clone(), b.try_clone()) {
        (Ok(ar), Ok(bw)) => (ar, bw),
        _ => return,
    };
    let mut b_read = b;
    let mut a_write = a;
    let t = std::thread::spawn(move || {
        let _ = std::io::copy(&mut a_read, &mut b_write);
        let _ = b_write.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut b_read, &mut a_write);
    let _ = a_write.shutdown(std::net::Shutdown::Write);
    let _ = t.join();
}

/// One client request. `bytes` carries opaque hex (relay frames); the server
/// never decodes it.
#[derive(Debug, Deserialize)]
struct Req {
    op: String,
    id: Option<String>,
    key: Option<String>,
    to: Option<String>,
    bytes: Option<String>,
    token: Option<String>,
    role: Option<String>,
}

fn reply(stream: &mut TcpStream, v: &serde_json::Value) -> Result<(), TetherError> {
    let mut s =
        serde_json::to_string(v).map_err(|e| TetherError::Invalid(format!("reply encode: {e}")))?;
    s.push('\n');
    stream
        .write_all(s.as_bytes())
        .map_err(|e| TetherError::Invalid(format!("reply write: {e}")))?;
    stream
        .flush()
        .map_err(|e| TetherError::Invalid(format!("reply flush: {e}")))?;
    Ok(())
}

fn lock_shared(shared: &Arc<Mutex<Shared>>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

fn handle_conn(shared: Arc<Mutex<Shared>>, stream: TcpStream) {
    let reader_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;
    let peer_addr = writer
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "?".to_string());
    let mut my_id: Option<String> = None;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(_) => break,
        }
        let req: Req = match serde_json::from_str(line.trim()) {
            Ok(r) => r,
            Err(_) => {
                let _ = reply(
                    &mut writer,
                    &serde_json::json!({"ok": false, "error": "bad json"}),
                );
                continue;
            }
        };
        match req.op.as_str() {
            "register" => {
                let (id, key) = match (req.id, req.key) {
                    (Some(i), Some(k)) if !i.is_empty() && !k.is_empty() => (i, k),
                    _ => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "id and key required"}),
                        );
                        continue;
                    }
                };
                lock_shared(&shared).reg.insert(
                    id.clone(),
                    Entry {
                        key,
                        addr: peer_addr.clone(),
                        last_seen: Instant::now(),
                    },
                );
                let _ = reply(&mut writer, &serde_json::json!({"ok": true}));
            }
            "heartbeat" => {
                let ok = match req.id {
                    Some(id) => {
                        let mut sh = lock_shared(&shared);
                        match sh.reg.get_mut(&id) {
                            Some(e) => {
                                e.last_seen = Instant::now();
                                true
                            }
                            None => false,
                        }
                    }
                    None => false,
                };
                let _ = reply(&mut writer, &serde_json::json!({"ok": ok}));
            }
            "lookup" => {
                let found = req.id.as_ref().and_then(|id| {
                    lock_shared(&shared).reg.get(id).and_then(|e| {
                        if e.last_seen.elapsed() <= Duration::from_secs(HEARTBEAT_FRESH_SECS) {
                            Some(PeerRecord {
                                id: id.clone(),
                                key: e.key.clone(),
                                addr: e.addr.clone(),
                            })
                        } else {
                            None
                        }
                    })
                });
                match found {
                    Some(rec) => {
                        let _ = reply(&mut writer, &serde_json::json!({"ok": true, "peer": rec}));
                    }
                    None => {
                        let _ = reply(&mut writer, &serde_json::json!({"ok": false}));
                    }
                }
            }
            "list" => {
                let ids: Vec<String> = lock_shared(&shared).reg.keys().cloned().collect();
                let _ = reply(&mut writer, &serde_json::json!({"ok": true, "peers": ids}));
            }
            "attach_relay" => {
                let id = match req.id {
                    Some(i) if !i.is_empty() => i,
                    _ => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "id required"}),
                        );
                        continue;
                    }
                };
                let (tx, rx) = mpsc::channel::<String>();
                lock_shared(&shared).boxes.insert(id.clone(), tx);
                my_id = Some(id);
                let mut out = match writer.try_clone() {
                    Ok(s) => s,
                    Err(_) => break,
                };
                std::thread::spawn(move || {
                    for msg in rx {
                        let mut s = msg;
                        s.push('\n');
                        if out.write_all(s.as_bytes()).is_err() {
                            break;
                        }
                    }
                });
                let _ = reply(&mut writer, &serde_json::json!({"ok": true}));
            }
            "send" => {
                let from = my_id.clone().unwrap_or_default();
                let routed = match (req.to, req.bytes) {
                    (Some(to), Some(bytes)) if !from.is_empty() && !to.is_empty() => {
                        let msg = serde_json::json!({"op": "frame", "from": from, "bytes": bytes})
                            .to_string();
                        match lock_shared(&shared).boxes.get(&to) {
                            Some(tx) => tx.send(msg).is_ok(),
                            None => false,
                        }
                    }
                    _ => false,
                };
                let _ = reply(&mut writer, &serde_json::json!({"ok": routed}));
            }
            "await_bridge" => {
                let id = match req.id {
                    Some(i) if !i.is_empty() => i,
                    _ => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "id required"}),
                        );
                        continue;
                    }
                };
                {
                    let mut sh = lock_shared(&shared);
                    sweep_bridges(&mut sh);
                    sh.awaiting.insert(id, (None, Instant::now()));
                }
                let _ = reply(&mut writer, &serde_json::json!({"ok": true}));
            }
            "check" => {
                let id = match req.id {
                    Some(i) if !i.is_empty() => i,
                    _ => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "id required"}),
                        );
                        continue;
                    }
                };
                let found = {
                    let mut sh = lock_shared(&shared);
                    sweep_bridges(&mut sh);
                    match sh.awaiting.get(&id) {
                        Some((Some(token), _)) => Some(token.clone()),
                        _ => None,
                    }
                };
                match found {
                    Some(token) => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": true, "token": token}),
                        );
                    }
                    None => {
                        let _ = reply(&mut writer, &serde_json::json!({"ok": false}));
                    }
                }
            }
            "bridge" => {
                let to = match req.to {
                    Some(t) if !t.is_empty() => t,
                    _ => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "target required"}),
                        );
                        continue;
                    }
                };
                let token = {
                    let mut sh = lock_shared(&shared);
                    sweep_bridges(&mut sh);
                    match sh.awaiting.get_mut(&to) {
                        Some((None, _)) => {
                            let token = next_token(&mut sh.token_counter);
                            if let Some(slot) = sh.awaiting.get_mut(&to) {
                                *slot = (Some(token.clone()), Instant::now());
                            }
                            sh.pending.insert(
                                token.clone(),
                                PendingBridge {
                                    a: None,
                                    b: None,
                                    created: Instant::now(),
                                },
                            );
                            Some(token)
                        }
                        _ => None,
                    }
                };
                match token {
                    Some(t) => {
                        let _ = reply(&mut writer, &serde_json::json!({"ok": true, "token": t}));
                    }
                    None => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "target not waiting (host must await first)"}),
                        );
                    }
                }
            }
            "claim" => {
                let (token, role) = match (req.token, req.role) {
                    (Some(t), Some(r)) if !t.is_empty() && (r == "a" || r == "b") => (t, r),
                    _ => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "token and role a|b required"}),
                        );
                        continue;
                    }
                };
                enum ClaimOutcome {
                    Waiting,
                    Paired(TcpStream, TcpStream),
                    Gone,
                }
                let outcome = {
                    let mut sh = lock_shared(&shared);
                    sweep_bridges(&mut sh);
                    match sh.pending.get_mut(&token) {
                        None => ClaimOutcome::Gone,
                        Some(p) => {
                            let slot = if role == "a" { &mut p.a } else { &mut p.b };
                            if slot.is_some() {
                                ClaimOutcome::Gone
                            } else {
                                match writer.try_clone() {
                                    Err(_) => ClaimOutcome::Gone,
                                    Ok(mine) => {
                                        *slot = Some(mine);
                                        match (&p.a, &p.b) {
                                            (Some(_), Some(_)) => match sh.pending.remove(&token) {
                                                Some(e) => match (e.a, e.b) {
                                                    (Some(a), Some(b)) => {
                                                        ClaimOutcome::Paired(a, b)
                                                    }
                                                    _ => ClaimOutcome::Gone,
                                                },
                                                None => ClaimOutcome::Gone,
                                            },
                                            _ => ClaimOutcome::Waiting,
                                        }
                                    }
                                }
                            }
                        }
                    }
                };
                match outcome {
                    ClaimOutcome::Gone => {
                        let _ = reply(
                            &mut writer,
                            &serde_json::json!({"ok": false, "error": "bad or expired token"}),
                        );
                    }
                    ClaimOutcome::Waiting => {
                        // Registered; stop reading here so no handler thread
                        // competes with the copier threads for session bytes.
                        let _ = reply(&mut writer, &serde_json::json!({"ok": true}));
                        return;
                    }
                    ClaimOutcome::Paired(a, b) => {
                        let _ = reply(&mut writer, &serde_json::json!({"ok": true}));
                        std::thread::spawn(move || bridge_pair(a, b));
                        return;
                    }
                }
            }
            _ => {
                let _ = reply(
                    &mut writer,
                    &serde_json::json!({"ok": false, "error": "unknown op"}),
                );
            }
        }
    }
    // Connection dropped: remove our relay mailbox so peers get ok:false.
    if let Some(id) = my_id {
        lock_shared(&shared).boxes.remove(&id);
    }
}

/// Serve rendezvous + relay until `stop` is set. One thread per connection.
pub fn serve(listener: TcpListener, stop: Arc<AtomicBool>) -> Result<(), TetherError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| TetherError::Invalid(format!("nonblocking: {e}")))?;
    let shared = Arc::new(Mutex::new(Shared {
        reg: HashMap::new(),
        boxes: HashMap::new(),
        awaiting: HashMap::new(),
        pending: HashMap::new(),
        token_counter: 0,
    }));
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, _)) => {
                // Accepted sockets inherit nonblocking mode on some platforms;
                // the handler protocol is blocking reads, so switch back.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let sh = shared.clone();
                std::thread::spawn(move || handle_conn(sh, stream));
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                return Err(TetherError::Invalid(format!("accept: {e}")));
            }
        }
    }
    Ok(())
}

/// NAT traversal candidates (Tier 3 remainder). Sorting by priority is real;
/// actual UDP hole punching across NATs needs the two-network run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateKind {
    Host,
    Reflexive,
    Relay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub addr: String,
    pub kind: CandidateKind,
    pub priority: u64,
}

/// Order candidates best-first: host, then reflexive, then relay; higher
/// priority value first within a kind.
pub fn sort_candidates(cands: &mut [Candidate]) {
    cands.sort_by(|a, b| {
        let rank = |k: &CandidateKind| match k {
            CandidateKind::Host => 0,
            CandidateKind::Reflexive => 1,
            CandidateKind::Relay => 2,
        };
        rank(&a.kind)
            .cmp(&rank(&b.kind))
            .then(b.priority.cmp(&a.priority))
    });
}

/// UDP hole punch entry point. Real NAT traversal is verified with two
/// separate networks, not loopback; this returns NotImplemented until then.
pub fn punch_udp(_peer: &Candidate) -> Result<(), TetherError> {
    Err(TetherError::NotImplemented(
        "UDP hole punching needs the two-network run (Tier 3 exit)".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};

    struct Client {
        r: BufReader<TcpStream>,
        w: TcpStream,
    }

    impl Client {
        fn dial(port: u16) -> Option<Self> {
            let s = TcpStream::connect(format!("127.0.0.1:{port}")).ok()?;
            let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
            let r = BufReader::new(s.try_clone().ok()?);
            Some(Self { r, w: s })
        }

        fn req(&mut self, v: serde_json::Value) -> serde_json::Value {
            let mut s = v.to_string();
            s.push('\n');
            if self.w.write_all(s.as_bytes()).is_err() {
                return serde_json::json!({"ok": false});
            }
            let mut line = String::new();
            if self.r.read_line(&mut line).is_err() {
                return serde_json::json!({"ok": false});
            }
            serde_json::from_str(line.trim()).unwrap_or(serde_json::json!({"ok": false}))
        }
    }

    fn test_server() -> (u16, Arc<AtomicBool>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|_| {
            // Test-only fallback: loopback refusing bind means the test host
            // cannot run sockets; bind failure is reported by the caller.
            TcpListener::bind("127.0.0.1:0").unwrap_or_else(|_| std::process::exit(111))
        });
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::spawn(move || {
            let _ = serve(listener, stop2);
        });
        (port, stop)
    }

    #[test]
    fn register_heartbeat_lookup() {
        let (port, stop) = test_server();
        if port == 0 {
            return;
        }
        let mut a = Client::dial(port).unwrap_or_else(|| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(112)
        });
        assert!(a.req(serde_json::json!({"op": "register", "id": "a", "key": "k1"}))["ok"] == true);
        assert!(a.req(serde_json::json!({"op": "heartbeat", "id": "a"}))["ok"] == true);
        let found = a.req(serde_json::json!({"op": "lookup", "id": "a"}));
        assert!(found["ok"] == true);
        assert!(found["peer"]["key"] == "k1");
        assert!(a.req(serde_json::json!({"op": "lookup", "id": "ghost"}))["ok"] == false);
        assert!(a.req(serde_json::json!({"op": "bogus"}))["ok"] == false);
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn relay_routes_opaque_frames() {
        let (port, stop) = test_server();
        if port == 0 {
            return;
        }
        let mut a = Client::dial(port).unwrap_or_else(|| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(112)
        });
        let mut b = Client::dial(port).unwrap_or_else(|| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(113)
        });
        assert!(a.req(serde_json::json!({"op": "attach_relay", "id": "a"}))["ok"] == true);
        assert!(b.req(serde_json::json!({"op": "attach_relay", "id": "b"}))["ok"] == true);
        // Opaque bytes: the relay never parses them, only routes by id.
        assert!(
            a.req(serde_json::json!({"op": "send", "to": "b", "bytes": "deadbeef"}))["ok"] == true
        );
        let mut line = String::new();
        assert!(b.r.read_line(&mut line).is_ok());
        let frame: serde_json::Value = serde_json::from_str(line.trim()).unwrap_or_default();
        assert!(frame["op"] == "frame");
        assert!(frame["from"] == "a");
        assert!(frame["bytes"] == "deadbeef");
        // Unknown peer: routed nowhere, ok:false, nothing delivered.
        assert!(
            a.req(serde_json::json!({"op": "send", "to": "ghost", "bytes": "00"}))["ok"] == false
        );
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn bridge_connects_two_peers() {
        use std::io::{BufRead, BufReader, Write};
        let (port, stop) = test_server();
        if port == 0 {
            return;
        }
        let mut host = Client::dial(port).unwrap_or_else(|| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(114)
        });
        let mut ctrl = Client::dial(port).unwrap_or_else(|| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(115)
        });
        assert!(host.req(serde_json::json!({"op": "await_bridge", "id": "host-1"}))["ok"] == true);
        // Unknown target is refused without touching any state.
        let no = ctrl.req(serde_json::json!({"op": "bridge", "to": "nobody"}));
        assert!(no["ok"] == false);
        let bridge = ctrl.req(serde_json::json!({"op": "bridge", "to": "host-1"}));
        assert!(bridge["ok"] == true);
        let token = bridge["token"].as_str().unwrap_or("").to_string();
        assert!(!token.is_empty());
        // Host discovers the token by polling, like the real host does.
        let mut seen = String::new();
        for _ in 0..50 {
            let check = host.req(serde_json::json!({"op": "check", "id": "host-1"}));
            if check["ok"] == true {
                seen = check["token"].as_str().unwrap_or("").to_string();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen == token);
        // Both sides open fresh connections and claim their half.
        let mut hc = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap_or_else(|_| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(116)
        });
        let _ = hc.set_read_timeout(Some(Duration::from_secs(5)));
        let mut hr = BufReader::new(hc.try_clone().unwrap_or_else(|_| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(117)
        }));
        let mut cc = TcpStream::connect(format!("127.0.0.1:{port}")).unwrap_or_else(|_| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(118)
        });
        let _ = cc.set_read_timeout(Some(Duration::from_secs(5)));
        let mut cr = BufReader::new(cc.try_clone().unwrap_or_else(|_| {
            stop.store(true, Ordering::Relaxed);
            std::process::exit(119)
        }));
        let claim_b =
            serde_json::json!({"op": "claim", "token": token, "role": "b"}).to_string() + "\n";
        let claim_a =
            serde_json::json!({"op": "claim", "token": token, "role": "a"}).to_string() + "\n";
        assert!(hc.write_all(claim_b.as_bytes()).is_ok());
        assert!(cc.write_all(claim_a.as_bytes()).is_ok());
        let mut line = String::new();
        assert!(hr.read_line(&mut line).is_ok());
        assert!(line.contains("\"ok\":true"));
        line.clear();
        assert!(cr.read_line(&mut line).is_ok());
        assert!(line.contains("\"ok\":true"));
        // Raw bytes now flow controller -> host through the relay.
        assert!(cc.write_all(b"PING?\n").is_ok());
        let mut got = String::new();
        assert!(hr.read_line(&mut got).is_ok());
        assert!(got.trim() == "PING?");
        // And back the other way.
        assert!(hc.write_all(b"PONG!\n").is_ok());
        got.clear();
        assert!(cr.read_line(&mut got).is_ok());
        assert!(got.trim() == "PONG!");
        stop.store(true, Ordering::Relaxed);
    }

    #[test]
    fn candidates_sort_host_first() {
        let mut c = vec![
            Candidate {
                addr: "r".to_string(),
                kind: CandidateKind::Relay,
                priority: 99,
            },
            Candidate {
                addr: "x".to_string(),
                kind: CandidateKind::Reflexive,
                priority: 10,
            },
            Candidate {
                addr: "h".to_string(),
                kind: CandidateKind::Host,
                priority: 5,
            },
            Candidate {
                addr: "h2".to_string(),
                kind: CandidateKind::Host,
                priority: 50,
            },
        ];
        sort_candidates(&mut c);
        assert!(c[0].addr == "h2");
        assert!(c[1].addr == "h");
        assert!(c[2].addr == "x");
        assert!(c[3].addr == "r");
        assert!(punch_udp(&c[0]).is_err());
    }
}
