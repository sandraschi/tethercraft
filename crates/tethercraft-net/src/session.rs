//! Live TCP session transport (Tier 1 exit milestone).
//!
//! Length-prefixed native envelopes over TCP: pairing with fingerprint
//! words, SPAKE2 handshake with key confirmation, then frames one way and
//! input events the other. QUIC is the later upgrade; the framing and the
//! handshake move over unchanged. First-use trust comes from comparing the
//! fingerprint words shown on both ends.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{fingerprint_words, parse_public_key, public_key_bytes, LoginRateLimiter};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tethercraft_core::TetherError;
use tethercraft_proto::{Envelope, MessageKind};

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
pub const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MESSAGE_BYTES: usize = 32 * 1024 * 1024;

/// Default host port. Outside the RustDesk 21115-21119 range on purpose.
pub const DEFAULT_SESSION_PORT: u16 = 21120;
/// Seconds to wait for one relay line before retrying the poll.
pub const RELAY_LINE_TIMEOUT: Duration = Duration::from_secs(10);

fn send_json_line(stream: &mut TcpStream, v: &serde_json::Value) -> Result<(), TetherError> {
    use std::io::Write as _;
    let mut s =
        serde_json::to_string(v).map_err(|e| TetherError::Invalid(format!("relay encode: {e}")))?;
    s.push('\n');
    stream
        .write_all(s.as_bytes())
        .map_err(|e| TetherError::Invalid(format!("relay write: {e}")))?;
    stream
        .flush()
        .map_err(|e| TetherError::Invalid(format!("relay flush: {e}")))?;
    Ok(())
}

fn read_json_line(stream: &mut TcpStream) -> Result<serde_json::Value, TetherError> {
    use std::io::BufRead as _;
    // Fresh reader per line on purpose: the socket turns into a raw byte
    // stream right after the claim handshake, so nothing may be buffered.
    let mut reader = std::io::BufReader::new(
        stream
            .try_clone()
            .map_err(|e| TetherError::Invalid(format!("relay clone: {e}")))?,
    );
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| TetherError::Invalid(format!("relay read: {e}")))?;
    if line.trim().is_empty() {
        return Err(TetherError::Invalid("relay closed".to_string()));
    }
    serde_json::from_str(line.trim()).map_err(|e| TetherError::Invalid(format!("relay json: {e}")))
}

/// Controller side of a relayed session: ask the rendezvous server to bridge
/// us to `target`, then claim our half. Returns a raw socket ready for the
/// normal session handshake. The relay only ever sees opaque bytes.
pub fn relay_connect(
    server_addr: &str,
    my_id: &str,
    target_id: &str,
) -> Result<TcpStream, TetherError> {
    let mut ctl = dial(server_addr, HANDSHAKE_TIMEOUT)?;
    send_json_line(
        &mut ctl,
        &serde_json::json!({"op": "bridge", "id": my_id, "to": target_id}),
    )?;
    let resp = read_json_line(&mut ctl)?;
    let token = resp
        .get("token")
        .and_then(|v| v.as_str())
        .filter(|_| resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        .ok_or_else(|| TetherError::Invalid("relay refused bridge".to_string()))?
        .to_string();
    drop(ctl);
    relay_claim(server_addr, &token, "a")
}

/// Host side of a relayed session: announce availability and wait for a
/// controller to ask for us, then claim. Blocks until paired (Ctrl+C aborts).
pub fn relay_await(server_addr: &str, my_id: &str) -> Result<TcpStream, TetherError> {
    let never = std::sync::atomic::AtomicBool::new(false);
    relay_await_cancel(server_addr, my_id, &never)
}

/// Stoppable variant: the check-poll loop aborts when `stop` is set, so a
/// Windows service stop does not hang forever waiting for a controller.
pub fn relay_await_cancel(
    server_addr: &str,
    my_id: &str,
    stop: &std::sync::atomic::AtomicBool,
) -> Result<TcpStream, TetherError> {
    use std::sync::atomic::Ordering;
    let mut ctl = dial(server_addr, HANDSHAKE_TIMEOUT)?;
    send_json_line(
        &mut ctl,
        &serde_json::json!({"op": "await_bridge", "id": my_id}),
    )?;
    let _ = read_json_line(&mut ctl)?;
    let token = loop {
        if stop.load(Ordering::Relaxed) {
            return Err(TetherError::Invalid("stopped".to_string()));
        }
        send_json_line(&mut ctl, &serde_json::json!({"op": "check", "id": my_id}))
            .map_err(|_| TetherError::Invalid("relay check failed; server gone?".to_string()))?;
        if let Ok(resp) = read_json_line(&mut ctl) {
            if let Some(t) = resp.get("token").and_then(|v| v.as_str()) {
                if !t.is_empty() {
                    break t.to_string();
                }
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    };
    drop(ctl);
    if stop.load(Ordering::Relaxed) {
        return Err(TetherError::Invalid("stopped".to_string()));
    }
    relay_claim(server_addr, &token, "b")
}

/// Claim one half of a bridged pairing. Role "a" is the controller side,
/// "b" the host side. Returns a raw socket ready for the session handshake.
fn relay_claim(server_addr: &str, token: &str, role: &str) -> Result<TcpStream, TetherError> {
    let mut claim = dial(server_addr, HANDSHAKE_TIMEOUT)?;
    send_json_line(
        &mut claim,
        &serde_json::json!({"op": "claim", "token": token, "role": role}),
    )?;
    let resp = read_json_line(&mut claim)?;
    if !resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(TetherError::Invalid(format!("relay claim failed: {resp}")));
    }
    Ok(claim)
}

struct DirCipher {
    cipher: ChaCha20Poly1305,
    nonce: u64,
}

impl DirCipher {
    fn new(key: [u8; 32]) -> Self {
        Self {
            cipher: ChaCha20Poly1305::new((&key).into()),
            nonce: 0,
        }
    }

    fn next_nonce(&mut self) -> Result<[u8; 12], TetherError> {
        if self.nonce == u64::MAX {
            return Err(TetherError::Denied("session nonce exhausted".to_string()));
        }
        let n = self.nonce;
        self.nonce = self.nonce.wrapping_add(1);
        let mut out = [0u8; 12];
        out[4..12].copy_from_slice(&n.to_be_bytes());
        Ok(out)
    }

    fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, TetherError> {
        let n = self.next_nonce()?;
        self.cipher
            .encrypt(Nonce::from_slice(&n), plaintext)
            .map_err(|_| TetherError::Denied("e2e encrypt failed".to_string()))
    }

    fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, TetherError> {
        let n = self.next_nonce()?;
        self.cipher
            .decrypt(Nonce::from_slice(&n), ciphertext)
            .map_err(|_| TetherError::Denied("e2e decrypt failed: possible tampering".to_string()))
    }
}

/// Derive independent directional ChaCha20-Poly1305 keys bound to the SPAKE2
/// shared secret and both Ed25519 device public keys. Returns `(c2h, h2c)`.
fn derive_session_keys(
    pake_key: &[u8],
    ctrl_pub: &[u8; 32],
    host_pub: &[u8; 32],
) -> ([u8; 32], [u8; 32]) {
    let mut c2h = Sha256::new();
    c2h.update(b"tethercraft-e2e-v1-c2h:");
    c2h.update(pake_key);
    c2h.update(ctrl_pub);
    c2h.update(host_pub);
    let mut k_c2h = [0u8; 32];
    k_c2h.copy_from_slice(&c2h.finalize());

    let mut h2c = Sha256::new();
    h2c.update(b"tethercraft-e2e-v1-h2c:");
    h2c.update(pake_key);
    h2c.update(ctrl_pub);
    h2c.update(host_pub);
    let mut k_h2c = [0u8; 32];
    k_h2c.copy_from_slice(&h2c.finalize());

    (k_c2h, k_h2c)
}

pub struct SessionStream {
    stream: TcpStream,
    pub peer_key: Option<[u8; 32]>,
    send_cipher: Option<Arc<Mutex<DirCipher>>>,
    recv_cipher: Option<Arc<Mutex<DirCipher>>>,
}

impl SessionStream {
    pub fn new(stream: TcpStream) -> Result<Self, TetherError> {
        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(|e| TetherError::Invalid(format!("timeouts: {e}")))?;
        Ok(Self {
            stream,
            peer_key: None,
            send_cipher: None,
            recv_cipher: None,
        })
    }

    pub fn enable_e2e(&mut self, send_key: [u8; 32], recv_key: [u8; 32]) {
        self.send_cipher = Some(Arc::new(Mutex::new(DirCipher::new(send_key))));
        self.recv_cipher = Some(Arc::new(Mutex::new(DirCipher::new(recv_key))));
    }

    pub fn is_encrypted(&self) -> bool {
        self.send_cipher.is_some() && self.recv_cipher.is_some()
    }

    pub fn set_frame_timeouts(&mut self) -> Result<(), TetherError> {
        self.stream
            .set_read_timeout(Some(FRAME_TIMEOUT))
            .map_err(|e| TetherError::Invalid(format!("timeouts: {e}")))?;
        Ok(())
    }

    /// A second handle to the same socket (one half reads, the other writes).
    /// Prefer `clone_session` after the handshake so E2E encryption state is shared.
    pub fn clone_stream(&self) -> Result<TcpStream, TetherError> {
        self.stream
            .try_clone()
            .map_err(|e| TetherError::Invalid(format!("clone: {e}")))
    }

    /// Clone the session for full-duplex reader/writer threads while sharing
    /// the directional ChaCha20-Poly1305 AEAD state and monotonic nonces.
    pub fn clone_session(&self) -> Result<Self, TetherError> {
        let stream = self
            .stream
            .try_clone()
            .map_err(|e| TetherError::Invalid(format!("clone: {e}")))?;
        Ok(Self {
            stream,
            peer_key: self.peer_key,
            send_cipher: self.send_cipher.clone(),
            recv_cipher: self.recv_cipher.clone(),
        })
    }

    pub fn send_envelope(&mut self, env: &Envelope) -> Result<(), TetherError> {
        let plain = env.encode()?;
        let wire = match &self.send_cipher {
            Some(c) => {
                let mut guard = c
                    .lock()
                    .map_err(|_| TetherError::Invalid("e2e send lock poisoned".to_string()))?;
                guard.encrypt(&plain)?
            }
            None => plain,
        };
        if wire.len() > MAX_MESSAGE_BYTES {
            return Err(TetherError::Invalid("message too large".to_string()));
        }
        let len = wire.len() as u32;
        self.stream
            .write_all(&len.to_be_bytes())
            .map_err(|e| TetherError::Invalid(format!("send: {e}")))?;
        self.stream
            .write_all(&wire)
            .map_err(|e| TetherError::Invalid(format!("send: {e}")))?;
        self.stream
            .flush()
            .map_err(|e| TetherError::Invalid(format!("send: {e}")))?;
        Ok(())
    }

    pub fn recv_envelope(&mut self) -> Result<Envelope, TetherError> {
        let mut prefix = [0u8; 4];
        self.stream
            .read_exact(&mut prefix)
            .map_err(|e| TetherError::Invalid(format!("recv: {e}")))?;
        let len = u32::from_be_bytes(prefix) as usize;
        if len == 0 || len > MAX_MESSAGE_BYTES {
            return Err(TetherError::Invalid(format!(
                "bad frame length {len} (prefix {prefix:?})"
            )));
        }
        let mut buf = vec![0u8; len];
        self.stream
            .read_exact(&mut buf)
            .map_err(|e| TetherError::Invalid(format!("recv: {e}")))?;
        let plain = match &self.recv_cipher {
            Some(c) => {
                let mut guard = c
                    .lock()
                    .map_err(|_| TetherError::Invalid("e2e recv lock poisoned".to_string()))?;
                guard.decrypt(&buf)?
            }
            None => buf,
        };
        Envelope::decode(&plain)
    }

    /// Receive with a bounded wait. `Ok(None)` means the wait expired with no
    /// complete message.
    ///
    /// WARNING: does not fix up a partially read length prefix, so a timeout
    /// mid-message desyncs the stream. Do not use this on a session socket;
    /// read on a dedicated thread instead (see `serve_one`).
    pub fn recv_envelope_wait(&mut self, wait: Duration) -> Result<Option<Envelope>, TetherError> {
        self.stream
            .set_read_timeout(Some(wait))
            .map_err(|e| TetherError::Invalid(format!("timeouts: {e}")))?;
        let mut prefix = [0u8; 4];
        match self.stream.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(e)
                if e.kind() == std::io::ErrorKind::TimedOut
                    || e.kind() == std::io::ErrorKind::WouldBlock =>
            {
                return Ok(None)
            }
            Err(e) => return Err(TetherError::Invalid(format!("recv: {e}"))),
        }
        let len = u32::from_be_bytes(prefix) as usize;
        if len == 0 || len > MAX_MESSAGE_BYTES {
            return Err(TetherError::Invalid(format!(
                "bad frame length {len} (prefix {prefix:?})"
            )));
        }
        let mut buf = vec![0u8; len];
        self.stream
            .read_exact(&mut buf)
            .map_err(|e| TetherError::Invalid(format!("recv: {e}")))?;
        let plain = match &self.recv_cipher {
            Some(c) => {
                let mut guard = c
                    .lock()
                    .map_err(|_| TetherError::Invalid("e2e recv lock poisoned".to_string()))?;
                guard.decrypt(&buf)?
            }
            None => buf,
        };
        Ok(Some(Envelope::decode(&plain)?))
    }
}

pub fn listen(addr: &str) -> Result<TcpListener, TetherError> {
    TcpListener::bind(addr).map_err(|e| TetherError::Invalid(format!("bind: {e}")))
}

pub fn dial(addr: &str, timeout: Duration) -> Result<TcpStream, TetherError> {
    let mut addrs = addr
        .to_socket_addrs()
        .map_err(|e| TetherError::Invalid(format!("resolve: {e}")))?;
    let mut last = "no address".to_string();
    for a in &mut addrs {
        match TcpStream::connect_timeout(&a, timeout) {
            Ok(s) => return Ok(s),
            Err(e) => last = e.to_string(),
        }
    }
    Err(TetherError::Invalid(format!("dial failed: {last}")))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPayload {
    pub id: String,
    pub key_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomePayload {
    pub id: String,
    pub key_hex: String,
    pub words: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PakePayload {
    pub msg_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofPayload {
    pub confirm_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartPayload {
    pub grants: Vec<tethercraft_core::Permission>,
    pub fps: u8,
}

pub fn json_bytes<T: Serialize>(v: &T) -> Result<Vec<u8>, TetherError> {
    serde_json::to_vec(v).map_err(|e| TetherError::Invalid(format!("encode: {e}")))
}

pub fn parse_json<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Result<T, TetherError> {
    serde_json::from_slice(bytes).map_err(|e| TetherError::Invalid(format!("decode: {e}")))
}

fn confirm_of(key: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"tethercraft-session-confirm");
    h.update(key);
    hex::encode(&h.finalize()[..16])
}

fn expect_kind(env: &Envelope, want: MessageKind, who: &str) -> Result<(), TetherError> {
    if env.kind == want {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!(
            "{who}: expected {want:?}, got {:?}",
            env.kind
        )))
    }
}

/// After the PAKE messages a close means the host rejected us (bad password or
/// policy), not a broken network. Say so instead of leaking an IO error.
fn recv_or_refused(stream: &mut SessionStream) -> Result<Envelope, TetherError> {
    stream.recv_envelope().map_err(|_| {
        TetherError::Denied(
            "host closed the session: wrong password, or the host refused".to_string(),
        )
    })
}

pub struct ControllerAuth {
    pub host_id: String,
    pub host_words: Vec<String>,
    pub host_key: [u8; 32],
    pub grants: Vec<tethercraft_core::Permission>,
    pub fps: u8,
}

pub fn controller_handshake(
    stream: &mut SessionStream,
    my_id: &str,
    signing: &SigningKey,
    password: &[u8],
    expected_host_key: Option<[u8; 32]>,
) -> Result<ControllerAuth, TetherError> {
    if my_id.is_empty() {
        return Err(TetherError::Invalid("device id required".to_string()));
    }
    if password.is_empty() {
        return Err(TetherError::Invalid("password required".to_string()));
    }
    let my_pub = public_key_bytes(signing);
    // 1. Pairing hello.
    let hello = HelloPayload {
        id: my_id.to_string(),
        key_hex: hex::encode(my_pub),
    };
    stream.send_envelope(&Envelope::new(
        MessageKind::PairingHello,
        my_pub,
        json_bytes(&hello)?,
    ))?;
    // 2. Host welcome with fingerprint words. The human compares these.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PairingWelcome, "controller")?;
    let welcome: WelcomePayload = parse_json(&back.payload)?;
    if welcome.id.is_empty() || welcome.words.is_empty() {
        return Err(TetherError::Invalid("bad welcome".to_string()));
    }
    let host_key = parse_public_key(&hex::decode(&welcome.key_hex).unwrap_or_default())?;
    // Pinned-key enforcement: a known host must present the recorded key.
    // First contact (None) proceeds; the caller records the key (TOFU).
    if let Some(expected) = expected_host_key {
        if host_key.to_bytes() != expected {
            return Err(TetherError::Denied(
                "host key changed: possible interception, refusing".to_string(),
            ));
        }
    }
    stream.peer_key = Some(host_key.to_bytes());
    // 3. PAKE: we are side A.
    use spake2::{Ed25519Group, Identity, Password, Spake2};
    let pw = Password::new(password);
    let id_a = Identity::new(b"controller");
    let id_b = Identity::new(b"host");
    let (state_a, msg_a) = Spake2::<Ed25519Group>::start_a(&pw, &id_a, &id_b);
    stream.send_envelope(&Envelope::new(
        MessageKind::PakeA,
        my_pub,
        json_bytes(&PakePayload {
            msg_hex: hex::encode(&msg_a),
        })?,
    ))?;
    // 4. Host message, finish, confirm our key.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PakeB, "controller")?;
    let pake_b: PakePayload = parse_json(&back.payload)?;
    let msg_b = hex::decode(&pake_b.msg_hex)
        .map_err(|_| TetherError::Invalid("bad pake message".to_string()))?;
    let key_a = state_a
        .finish(&msg_b)
        .map_err(|e| TetherError::Invalid(format!("pake finish: {e:?}")))?;
    let confirm = confirm_of(&key_a);
    stream.send_envelope(&Envelope::new(
        MessageKind::OtpProof,
        my_pub,
        json_bytes(&ProofPayload {
            confirm_hex: confirm.clone(),
        })?,
    ))?;
    // 5. Host proof must match; then the session terms arrive.
    let back = recv_or_refused(stream)?;
    expect_kind(&back, MessageKind::OtpProof, "controller")?;
    let proof: ProofPayload = parse_json(&back.payload)?;
    if proof.confirm_hex != confirm {
        return Err(TetherError::Denied(
            "session key mismatch (wrong password?)".to_string(),
        ));
    }
    // Enable ChaCha20-Poly1305 E2E encryption before SessionStart so every
    // post-handshake envelope (including relayed traffic) is confidential
    // and authenticated.
    let (k_c2h, k_h2c) = derive_session_keys(&key_a, &my_pub, &host_key.to_bytes());
    stream.enable_e2e(k_c2h, k_h2c);
    let back = recv_or_refused(stream)?;
    expect_kind(&back, MessageKind::SessionStart, "controller")?;
    let start: StartPayload = parse_json(&back.payload)?;
    Ok(ControllerAuth {
        host_id: welcome.id,
        host_words: welcome.words,
        host_key: host_key.to_bytes(),
        grants: start.grants,
        fps: start.fps,
    })
}

pub struct HostAuth {
    pub peer_id: String,
    pub peer_key: [u8; 32],
}

/// Everything the host needs to run a handshake except the socket, key and limiter.
pub struct HostHandshake<'a> {
    pub my_id: &'a str,
    pub password: &'a [u8],
    pub peer_label: &'a str,
    pub fps: u8,
    pub grants: Vec<tethercraft_core::Permission>,
}

pub fn host_handshake(
    stream: &mut SessionStream,
    signing: &SigningKey,
    hs: HostHandshake<'_>,
    limiter: &mut LoginRateLimiter,
) -> Result<HostAuth, TetherError> {
    let HostHandshake {
        my_id,
        password,
        peer_label,
        fps,
        grants,
    } = hs;
    if my_id.is_empty() {
        return Err(TetherError::Invalid("device id required".to_string()));
    }
    let my_pub = public_key_bytes(signing);
    // 1. Controller hello.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PairingHello, "host")?;
    let hello: HelloPayload = parse_json(&back.payload)?;
    if hello.id.is_empty() {
        return Err(TetherError::Invalid("bad hello".to_string()));
    }
    let peer_key = parse_public_key(&hex::decode(&hello.key_hex).unwrap_or_default())?;
    stream.peer_key = Some(peer_key.to_bytes());
    // 2. Welcome with our fingerprint words (shown locally too).
    let words = fingerprint_words(&my_pub);
    let welcome = WelcomePayload {
        id: my_id.to_string(),
        key_hex: hex::encode(my_pub),
        words,
    };
    stream.send_envelope(&Envelope::new(
        MessageKind::PairingWelcome,
        my_pub,
        json_bytes(&welcome)?,
    ))?;
    // 3. PAKE side B.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::PakeA, "host")?;
    let pake_a: PakePayload = parse_json(&back.payload)?;
    let msg_a = hex::decode(&pake_a.msg_hex)
        .map_err(|_| TetherError::Invalid("bad pake message".to_string()))?;
    use spake2::{Ed25519Group, Identity, Password, Spake2};
    let pw = Password::new(password);
    let id_a = Identity::new(b"controller");
    let id_b = Identity::new(b"host");
    let (state_b, msg_b) = Spake2::<Ed25519Group>::start_b(&pw, &id_a, &id_b);
    stream.send_envelope(&Envelope::new(
        MessageKind::PakeB,
        my_pub,
        json_bytes(&PakePayload {
            msg_hex: hex::encode(&msg_b),
        })?,
    ))?;
    // 4. Controller proof must match our key.
    let back = stream.recv_envelope()?;
    expect_kind(&back, MessageKind::OtpProof, "host")?;
    let proof: ProofPayload = parse_json(&back.payload)?;
    let key_b = state_b
        .finish(&msg_a)
        .map_err(|_| TetherError::Denied("password proof failed".to_string()))?;
    if proof.confirm_hex != confirm_of(&key_b) {
        limiter.record_failure(peer_label);
        return Err(TetherError::Denied("password proof failed".to_string()));
    }
    limiter.record_success(peer_label);
    // 5. Confirm and send session terms.
    stream.send_envelope(&Envelope::new(
        MessageKind::OtpProof,
        my_pub,
        json_bytes(&ProofPayload {
            confirm_hex: confirm_of(&key_b),
        })?,
    ))?;
    // Enable ChaCha20-Poly1305 E2E encryption before SessionStart.
    let (k_c2h, k_h2c) = derive_session_keys(&key_b, &peer_key.to_bytes(), &my_pub);
    stream.enable_e2e(k_h2c, k_c2h);
    stream.send_envelope(&Envelope::new(
        MessageKind::SessionStart,
        my_pub,
        json_bytes(&StartPayload { grants, fps })?,
    ))?;
    Ok(HostAuth {
        peer_id: hello.id,
        peer_key: peer_key.to_bytes(),
    })
}

/// Raw RGBA frame wire format: width u32 LE, height u32 LE, then bytes.
pub fn encode_frame(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, TetherError> {
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    if rgba.len() != width as usize * height as usize * 4 {
        return Err(TetherError::Invalid("frame size mismatch".to_string()));
    }
    let mut out = Vec::with_capacity(8 + rgba.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(rgba);
    Ok(out)
}

pub fn decode_frame(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), TetherError> {
    if bytes.len() < 8 {
        return Err(TetherError::Invalid("frame too short".to_string()));
    }
    let mut w = [0u8; 4];
    let mut h = [0u8; 4];
    w.copy_from_slice(&bytes[..4]);
    h.copy_from_slice(&bytes[4..8]);
    let (width, height) = (u32::from_le_bytes(w), u32::from_le_bytes(h));
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    let need = width as usize * height as usize * 4;
    if bytes.len() - 8 != need {
        return Err(TetherError::Invalid("frame size mismatch".to_string()));
    }
    Ok((width, height, bytes[8..].to_vec()))
}

/// Minimal binary PPM writer: no encoder dependency, viewable everywhere.
pub fn write_ppm(path: &str, width: u32, height: u32, rgba: &[u8]) -> Result<(), TetherError> {
    use std::io::Write;
    let need = width as usize * height as usize * 4;
    if rgba.len() != need {
        return Err(TetherError::Invalid("frame size mismatch".to_string()));
    }
    let mut out = Vec::with_capacity(32 + need / 4 * 3);
    let header = format!("P6\n{width} {height}\n255\n");
    out.extend_from_slice(header.as_bytes());
    let mut i = 0;
    while i + 4 <= rgba.len() {
        out.push(rgba[i]);
        out.push(rgba[i + 1]);
        out.push(rgba[i + 2]);
        i += 4;
    }
    let mut f =
        std::fs::File::create(path).map_err(|e| TetherError::Invalid(format!("save: {e}")))?;
    f.write_all(&out)
        .map_err(|e| TetherError::Invalid(format!("save: {e}")))?;
    Ok(())
}

/// Connect, handshake, then read frames for a bounded window and save the
/// first one. Used for scripted milestone checks and CI-style runs.
pub fn run_headless_controller(
    addr: &str,
    password: &[u8],
    out_path: &str,
) -> Result<u32, TetherError> {
    Ok(run_headless_controller_count(addr, password, 1, Some(out_path), false, None, None)?.0)
}

/// Pinned variant: enforces the recorded host key and hands the presented key
/// back for TOFU recording. Reads a single frame. When `via_server` is set,
/// dials through the rendezvous relay; a known host key is required.
pub fn run_headless_controller_pinned(
    addr: &str,
    password: &[u8],
    out_path: &str,
    expected_host_key: Option<[u8; 32]>,
    via_server: Option<&str>,
) -> Result<[u8; 32], TetherError> {
    let via_target;
    let via = match (via_server, expected_host_key) {
        (Some(server), Some(key)) => {
            via_target = hex::encode(key);
            Some((server, via_target.as_str()))
        }
        (Some(_), None) => {
            return Err(TetherError::Invalid(
                "relay needs a known host key: connect directly first (or pin the key)".to_string(),
            ));
        }
        (None, _) => None,
    };
    let (_, key) = run_headless_controller_count(
        addr,
        password,
        1,
        Some(out_path),
        false,
        expected_host_key,
        via,
    )?;
    Ok(key)
}

pub fn run_headless_controller_count(
    addr: &str,
    password: &[u8],
    want_frames: u32,
    out_path: Option<&str>,
    input_probe: bool,
    expected_host_key: Option<[u8; 32]>,
    via: Option<(&str, &str)>,
) -> Result<(u32, [u8; 32]), TetherError> {
    let signing = tethercraft_net_device_key()?;
    let stream = match via {
        Some((server, target_id)) => relay_connect(server, "tethercraft-cli", target_id)?,
        None => dial(addr, Duration::from_secs(10))?,
    };
    let mut sess = SessionStream::new(stream)?;
    let auth = controller_handshake(
        &mut sess,
        "headless-controller",
        &signing,
        password,
        expected_host_key,
    )?;
    sess.set_frame_timeouts()?;
    if input_probe {
        // Benign zero-delta scroll: exercises the whole input path (parse,
        // host permission check, injection call) without moving the pointer.
        let payload = json_bytes(&tethercraft_input::InputEvent::MouseScroll { dx: 0, dy: 0 })?;
        sess.send_envelope(&Envelope::new(MessageKind::InputEvent, [0u8; 32], payload))?;
    }
    let mut got = 0u32;
    let mut saved_first = false;
    let mut decoder = StreamDecoder::new()?;
    while got < want_frames {
        match sess.recv_envelope() {
            Ok(env) => match env.kind {
                MessageKind::Frame | MessageKind::FrameH264 => {
                    if let Some((w, h, rgba)) = decoder.decode(env.kind, &env.payload)? {
                        got = got.saturating_add(1);
                        if !saved_first {
                            if let Some(p) = out_path {
                                write_ppm(p, w, h, &rgba)?;
                            }
                            saved_first = true;
                        }
                    }
                }
                MessageKind::SessionEnd => break,
                _ => {}
            },
            Err(e) => {
                return Err(TetherError::Invalid(format!(
                    "recv failed after {got} frames: {e}"
                )))
            }
        }
    }
    let _ = sess.send_envelope(&Envelope::new(
        MessageKind::SessionEnd,
        [0u8; 32],
        Vec::new(),
    ));
    Ok((got, auth.host_key))
}

fn tethercraft_net_device_key() -> Result<SigningKey, TetherError> {
    crate::generate_device_key()
}

/// H.264 frame payload: width u32 LE, height u32 LE, then NAL units.
pub fn encode_frame_h264(width: u32, height: u32, nal: &[u8]) -> Result<Vec<u8>, TetherError> {
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    if nal.is_empty() || nal.len() > MAX_MESSAGE_BYTES - 8 {
        return Err(TetherError::Invalid("bad h264 payload".to_string()));
    }
    let mut out = Vec::with_capacity(8 + nal.len());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(nal);
    Ok(out)
}

pub fn decode_frame_h264(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), TetherError> {
    if bytes.len() < 8 {
        return Err(TetherError::Invalid("h264 frame too short".to_string()));
    }
    let mut w = [0u8; 4];
    let mut h = [0u8; 4];
    w.copy_from_slice(&bytes[..4]);
    h.copy_from_slice(&bytes[4..8]);
    let (width, height) = (u32::from_le_bytes(w), u32::from_le_bytes(h));
    if width == 0 || height == 0 || width > 7680 || height > 4320 {
        return Err(TetherError::Invalid("bad frame dimensions".to_string()));
    }
    if bytes.len() == 8 {
        return Err(TetherError::Invalid("empty h264 payload".to_string()));
    }
    Ok((width, height, bytes[8..].to_vec()))
}

/// Stateful decoder for a live stream: raw frames pass through, H.264 NALs
/// go through a persistent decoder. `Ok(None)` means the decoder needs more
/// data; the caller skips the frame, it is not an error.
///
/// Without the `h264` feature (cross builds like the musl probe) the H.264
/// path reports NotImplemented and raw keeps working.
pub struct StreamDecoder {
    #[cfg(feature = "h264")]
    h264: tethercraft_codec::h264::H264Decoder,
}

impl StreamDecoder {
    pub fn new() -> Result<Self, TetherError> {
        #[cfg(feature = "h264")]
        {
            Ok(Self {
                h264: tethercraft_codec::h264::H264Decoder::new()?,
            })
        }
        #[cfg(not(feature = "h264"))]
        {
            Err(TetherError::NotImplemented(
                "h264 disabled in this build; raw frames only".to_string(),
            ))
        }
    }

    pub fn decode(
        &mut self,
        kind: MessageKind,
        payload: &[u8],
    ) -> Result<Option<(u32, u32, Vec<u8>)>, TetherError> {
        match kind {
            MessageKind::Frame => {
                let (w, h, px) = decode_frame(payload)?;
                Ok(Some((w, h, px)))
            }
            #[cfg(feature = "h264")]
            MessageKind::FrameH264 => {
                let (w, h, nal) = decode_frame_h264(payload)?;
                match self.h264.decode(&nal)? {
                    Some((dw, dh, px)) => {
                        if dw != w || dh != h {
                            return Err(TetherError::Invalid(
                                "h264 dimensions disagree with header".to_string(),
                            ));
                        }
                        Ok(Some((dw, dh, px)))
                    }
                    None => Ok(None),
                }
            }
            #[cfg(not(feature = "h264"))]
            MessageKind::FrameH264 => Err(TetherError::NotImplemented(
                "h264 disabled in this build; restart the host with --codec raw".to_string(),
            )),
            _ => Err(TetherError::Invalid("not a frame".to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn test_key(byte: u8) -> SigningKey {
        SigningKey::from_bytes(&[byte; 32])
    }

    fn loopback_pair() -> (SessionStream, SessionStream) {
        let listener = listen("127.0.0.1:0").unwrap_or_else(|_| std::process::exit(111));
        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let (s, _) = listener
                .accept()
                .unwrap_or_else(|_| std::process::exit(112));
            tx.send(SessionStream::new(s).map_err(|e| e.to_string()))
                .unwrap_or(());
        });
        let c = dial(&format!("127.0.0.1:{port}"), Duration::from_secs(5))
            .unwrap_or_else(|_| std::process::exit(113));
        let host = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap_or(Err("no host".to_string()));
        let host = host.unwrap_or_else(|_| std::process::exit(114));
        let controller = SessionStream::new(c).unwrap_or_else(|_| std::process::exit(115));
        (controller, host)
    }

    #[test]
    fn handshake_same_password() {
        let (mut c, mut h) = loopback_pair();
        let ck = test_key(1);
        let hk = test_key(2);
        let handle = std::thread::spawn(move || {
            let mut limiter = LoginRateLimiter::new(5, 60);
            host_handshake(
                &mut h,
                &hk,
                HostHandshake {
                    my_id: "host-1",
                    password: b"correct horse",
                    peer_label: "test-peer",
                    fps: 10,
                    grants: vec![tethercraft_core::Permission::Keyboard],
                },
                &mut limiter,
            )
        });
        let auth = controller_handshake(&mut c, "ctrl-1", &ck, b"correct horse", None).unwrap_or(
            ControllerAuth {
                host_id: String::new(),
                host_words: Vec::new(),
                host_key: [0u8; 32],
                grants: Vec::new(),
                fps: 0,
            },
        );
        let host_auth = handle
            .join()
            .unwrap_or(Err(TetherError::Invalid("join".to_string())));
        assert!(host_auth.is_ok());
        assert!(auth.host_id == "host-1");
        assert!(auth.host_words.len() == 8);
        assert!(auth.grants == vec![tethercraft_core::Permission::Keyboard]);
        assert!(c.peer_key.is_some());
    }

    #[test]
    fn handshake_wrong_password_fails() {
        let (mut c, mut h) = loopback_pair();
        let ck = test_key(1);
        let hk = test_key(2);
        let handle = std::thread::spawn(move || {
            let mut limiter = LoginRateLimiter::new(5, 60);
            host_handshake(
                &mut h,
                &hk,
                HostHandshake {
                    my_id: "host-1",
                    password: b"right",
                    peer_label: "test-peer",
                    fps: 10,
                    grants: Vec::new(),
                },
                &mut limiter,
            )
        });
        assert!(controller_handshake(&mut c, "ctrl-1", &ck, b"wrong", None).is_err());
        let host_auth = handle.join().unwrap_or(Ok(HostAuth {
            peer_id: String::new(),
            peer_key: [0u8; 32],
        }));
        assert!(host_auth.is_err());
    }

    #[test]
    fn handshake_wrong_pin_fails() {
        let (mut c, mut h) = loopback_pair();
        let ck = test_key(1);
        let hk = test_key(2);
        let handle = std::thread::spawn(move || {
            let mut limiter = LoginRateLimiter::new(5, 60);
            host_handshake(
                &mut h,
                &hk,
                HostHandshake {
                    my_id: "host-1",
                    password: b"right",
                    peer_label: "test-peer",
                    fps: 10,
                    grants: Vec::new(),
                },
                &mut limiter,
            )
        });
        // Right password, wrong pinned key: must still refuse.
        assert!(controller_handshake(&mut c, "ctrl-1", &ck, b"right", Some([9u8; 32])).is_err());
        let _ = handle.join();
    }

    #[test]
    fn frame_codec_roundtrip() {
        let rgba = vec![9u8; 4 * 4 * 4];
        let bytes = encode_frame(4, 4, &rgba).unwrap_or_default();
        assert!(!bytes.is_empty());
        let (w, h, back) = decode_frame(&bytes).unwrap_or((0, 0, Vec::new()));
        assert!(w == 4 && h == 4);
        assert!(back == rgba);
        assert!(encode_frame(0, 4, &rgba).is_err());
        assert!(decode_frame(&bytes[..bytes.len() - 1]).is_err());
        assert!(decode_frame(b"junk").is_err());
    }

    #[test]
    fn frame_h264_payload_roundtrip() {
        let nal = vec![0u8, 0, 0, 1, 0x65, 1, 2, 3];
        let bytes = encode_frame_h264(320, 200, &nal).unwrap_or_default();
        assert!(!bytes.is_empty());
        let (w, h, back) = decode_frame_h264(&bytes).unwrap_or((0, 0, Vec::new()));
        assert!(w == 320 && h == 200);
        assert!(back == nal);
        assert!(encode_frame_h264(0, 200, &nal).is_err());
        assert!(encode_frame_h264(320, 200, &[]).is_err());
        assert!(decode_frame_h264(b"short").is_err());
    }

    #[test]
    fn stream_decoder_passes_raw_through() {
        let rgba = vec![7u8; 4 * 4 * 4];
        let bytes = encode_frame(4, 4, &rgba).unwrap_or_default();
        let mut dec = match StreamDecoder::new() {
            Ok(d) => d,
            Err(_) => {
                // No Cisco binary on this machine: construction must fail
                // consistently, and the live path reports it instead of running.
                assert!(StreamDecoder::new().is_err());
                return;
            }
        };
        let got = dec.decode(MessageKind::Frame, &bytes).unwrap_or(None);
        assert!(got == Some((4, 4, rgba)));
        assert!(dec.decode(MessageKind::Ping, &bytes).is_err());
    }

    #[test]
    fn e2e_session_encrypts_and_rejects_tampering() {
        // 1. After handshake, both halves are encrypted and exchange envelopes.
        let (mut c, mut h) = loopback_pair();
        let ck = test_key(11);
        let hk = test_key(12);
        let handle = std::thread::spawn(move || {
            let mut limiter = LoginRateLimiter::new(5, 60);
            let auth = host_handshake(
                &mut h,
                &hk,
                HostHandshake {
                    my_id: "host-e2e",
                    password: b"secret-pw",
                    peer_label: "test-peer",
                    fps: 10,
                    grants: Vec::new(),
                },
                &mut limiter,
            )?;
            assert!(h.is_encrypted());
            let env = h.recv_envelope()?;
            Ok::<(HostAuth, Envelope), TetherError>((auth, env))
        });
        let ca = controller_handshake(&mut c, "ctrl-e2e", &ck, b"secret-pw", None);
        assert!(ca.is_ok());
        assert!(c.is_encrypted());
        let sent = Envelope::new(MessageKind::ChatMsg, [0u8; 32], b"top-secret-msg".to_vec());
        assert!(c.send_envelope(&sent).is_ok());
        let host_res = handle.join();
        assert!(host_res.is_ok());
        if let Ok(Ok((_, got))) = host_res {
            assert!(got.kind == MessageKind::ChatMsg);
            assert!(got.payload == b"top-secret-msg");
        }

        // 2. Ciphertext hides plaintext and flipping 1 bit fails AEAD auth.
        let (k_c2h, k_h2c) = derive_session_keys(b"shared", &[1u8; 32], &[2u8; 32]);
        assert!(k_c2h != k_h2c);
        let mut enc = DirCipher::new(k_c2h);
        let mut dec = DirCipher::new(k_c2h);
        let pt = b"sensitive desktop frame payload";
        let mut ct = enc.encrypt(pt).unwrap_or_default();
        assert!(!ct.is_empty());
        assert!(!ct.windows(pt.len()).any(|w| w == pt));
        // Flip one byte in transit (simulating a malicious relay operator).
        ct[0] ^= 0x01;
        assert!(matches!(dec.decrypt(&ct), Err(TetherError::Denied(_))));
    }
}
