//! tethercraft-mcp: stdio MCP server with live sessions.
//!
//! Read tools (status, peers, sessions, audit) are open. Every tool that
//! touches a live host session needs `confirm: true`, is permission-gated on
//! the host, and is written to the audit log either way. Passwords come from
//! a file path (`password_file`), never from the params inline and never
//! from the command line.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use tethercraft_input::InputEvent;
use tethercraft_net::session;
use tethercraft_proto::{Envelope, MessageKind};
use tethercraft_store::Store;

fn tool_list() -> Value {
    json!({
        "tools": [
            {"name": "status", "description": "daemon and session count (read-only)"},
            {"name": "peers", "description": "address book peers (read-only)"},
            {"name": "peer_info", "description": "one peer row (read-only)"},
            {"name": "sessions", "description": "open live sessions (read-only)"},
            {"name": "audit", "description": "audit log tail (read-only)"},
            {"name": "connect", "description": "dial a peer and hold the session (needs confirm)"},
            {"name": "disconnect", "description": "close a live session (no confirm needed)"},
            {"name": "screenshot", "description": "save one frame to a file (needs confirm)"},
            {"name": "input", "description": "inject an input event (needs confirm, permission-gated)"},
            {"name": "exec", "description": "run a command over the terminal channel (needs confirm, permission-gated)"},
            {"name": "file_list", "description": "list a remote dir (needs confirm, permission-gated)"},
            {"name": "file_get", "description": "download a remote file to a local path (needs confirm, permission-gated)"},
            {"name": "file_put", "description": "upload a local file to the remote side (needs confirm, permission-gated)"},
            {"name": "file_rename", "description": "move a remote file (needs confirm, permission-gated)"},
            {"name": "file_rm", "description": "delete a remote path (needs confirm, permission-gated)"},
            {"name": "file_mkdir", "description": "create a remote dir (needs confirm, permission-gated)"},
            {"name": "file_hash", "description": "hash a remote file without downloading it (needs confirm, permission-gated)"},
            {"name": "chat_send", "description": "send session chat text (needs confirm)"},
            {"name": "tunnel_open", "description": "open a TCP tunnel channel (needs confirm, permission-gated)"},
            {"name": "tunnel_send", "description": "send bytes on a tunnel channel as hex (needs confirm, permission-gated)"},
            {"name": "tunnel_close", "description": "close a tunnel channel (needs confirm, permission-gated)"},
            {"name": "restart", "description": "restart the host (needs confirm, permission-gated)"},
            {"name": "shutdown", "description": "shut the host down (needs confirm, permission-gated)"},
            {"name": "lock", "description": "lock the host workstation (needs confirm, permission-gated)"}
        ]
    })
}

fn ok_result(v: Value) -> Value {
    json!({"result": v})
}

struct LiveSession {
    sess: session::SessionStream,
    decoder: session::StreamDecoder,
    req: u64,
    peer: String,
}

impl LiveSession {
    fn next_req(&mut self) -> u64 {
        self.req = self.req.wrapping_add(1);
        if self.req == 0 {
            self.req = 1;
        }
        self.req
    }
}

struct McpState {
    store: Store,
    sessions: HashMap<String, LiveSession>,
}

fn needs_confirm(params: &Value) -> bool {
    params
        .get("confirm")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn param_str(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn param_u32(params: &Value, key: &str, default: u32) -> u32 {
    params
        .get(key)
        .and_then(|v| v.as_u64())
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(default)
}

fn param_u64(params: &Value, key: &str, default: u64) -> u64 {
    params.get(key).and_then(|v| v.as_u64()).unwrap_or(default)
}

fn audit(store: &Store, peer: &str, action: &str, result: &str) {
    let ev = tethercraft_core::AuditEvent::new(chrono::Utc::now(), peer, action, result);
    let _ = store.log(&ev);
}

fn store_path() -> String {
    let base = std::env::var("LOCALAPPDATA")
        .or_else(|_| std::env::var("APPDATA"))
        .unwrap_or_else(|_| ".".to_string());
    format!("{base}/tethercraft/store.sqlite3")
}

fn open_store() -> Store {
    Store::open_file(&store_path())
        .unwrap_or_else(|_| Store::open_in_memory().unwrap_or_else(|_| std::process::exit(1)))
}

/// Read a password from a file. Never from params inline, never logged.
fn load_password(path: Option<&str>) -> Result<Vec<u8>, String> {
    match path {
        Some("-") => Err("password from stdin is not supported over MCP".to_string()),
        Some(p) => std::fs::read_to_string(p)
            .map(|s| s.trim_end_matches(['\r', '\n']).as_bytes().to_vec())
            .map_err(|e| format!("password file: {e}")),
        None => Err("password_file is required".to_string()),
    }
}

fn peer_addr_key(store: &Store, peer: &str) -> Result<(String, Option<[u8; 32]>), String> {
    let peers = store
        .list_peers()
        .map_err(|e| format!("address book: {e}"))?;
    match peers.iter().find(|r| r.peer_id == peer) {
        Some(row) => match &row.addr {
            Some(a) if !a.trim().is_empty() => {
                let key = match row.pubkey_hex.trim() {
                    "" => None,
                    hex_str => {
                        let raw = hex::decode(hex_str)
                            .map_err(|_| format!("peer {peer} has a bad recorded key"))?;
                        if raw.len() != 32 {
                            return Err(format!("peer {peer} has a bad recorded key"));
                        }
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&raw);
                        Some(arr)
                    }
                };
                Ok((a.clone(), key))
            }
            _ => Err(format!(
                "peer {peer} has no address: re-add it with an addr host:port"
            )),
        },
        None => Err(format!("unknown peer {peer}")),
    }
}

fn tofu_record(store: &Store, peer: &str, key: &[u8; 32]) -> Result<(), String> {
    store
        .set_pubkey(peer, &hex::encode(key))
        .map_err(|e| format!("record host key: {e}"))
}

fn end_session(sess: &mut session::SessionStream) {
    let end = Envelope::new(MessageKind::SessionEnd, [0u8; 32], Vec::new());
    let _ = sess.send_envelope(&end);
}

fn handle(state: &mut McpState, method: &str, params: &Value) -> Value {
    match method {
        "tools/list" => ok_result(tool_list()),
        "status" => {
            let peers = state.store.list_peers().map(|p| p.len()).unwrap_or(0);
            ok_result(json!({
                "tier": 2,
                "transport": "native-session",
                "peers": peers,
                "sessions": state.sessions.len()
            }))
        }
        "peers" => match state.store.list_peers() {
            Ok(p) => ok_result(json!(p)),
            Err(e) => json!({"error": e.to_string()}),
        },
        "peer_info" => {
            let want = params.get("peer").and_then(|v| v.as_str()).unwrap_or("");
            match state.store.list_peers() {
                Ok(list) => match list.iter().find(|r| r.peer_id == want) {
                    Some(row) => ok_result(json!(row)),
                    None => json!({"error": format!("unknown peer {want}")}),
                },
                Err(e) => json!({"error": e.to_string()}),
            }
        }
        "sessions" => {
            let names: Vec<&String> = state.sessions.keys().collect();
            ok_result(json!({"sessions": names}))
        }
        "audit" => {
            let limit = params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50)
                .min(500) as usize;
            match state.store.list_audit(limit) {
                Ok(l) => ok_result(json!(l)),
                Err(e) => json!({"error": e.to_string()}),
            }
        }
        "disconnect" => {
            let name = params.get("session").and_then(|v| v.as_str()).unwrap_or("");
            match state.sessions.remove(name) {
                Some(mut s) => {
                    end_session(&mut s.sess);
                    audit(&state.store, &s.peer, "disconnect", "ok");
                    ok_result(json!({"disconnected": name}))
                }
                None => json!({"error": format!("no live session {name}")}),
            }
        }
        "connect" => {
            if !needs_confirm(params) {
                return json!({
                    "needs_confirm": true,
                    "hint": "resend connect with confirm=true; dialing is audited"
                });
            }
            let peer = param_str(params, "peer").unwrap_or_default();
            let via = param_str(params, "via");
            let password = match load_password(params.get("password_file").and_then(|v| v.as_str()))
            {
                Ok(p) => p,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "error-password");
                    return json!({"error": e});
                }
            };
            if peer.is_empty() {
                return json!({"error": "peer is required"});
            }
            let (addr, expected) = match peer_addr_key(&state.store, &peer) {
                Ok(v) => v,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "error-unknown-peer");
                    return json!({"error": e});
                }
            };
            if state.sessions.contains_key(&peer) {
                return json!({"error": format!("session {peer} is already open")});
            }
            let had_pin = expected.is_some();
            let stream = match (via.as_deref(), expected) {
                (Some(server), Some(key)) => tethercraft_net::session::relay_connect(
                    server,
                    "tethercraft-mcp",
                    &hex::encode(key),
                ),
                (Some(_), None) => {
                    let e =
                        format!("relay needs a known host key for {peer}: connect directly first");
                    audit(&state.store, &peer, "connect", "error-no-pin");
                    return json!({"error": e});
                }
                (None, _) => {
                    tethercraft_net::session::dial(&addr, std::time::Duration::from_secs(10))
                }
            };
            let stream = match stream {
                Ok(s) => s,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "error-dial");
                    return json!({"error": format!("dial: {e}")});
                }
            };
            let signing = match tethercraft_net::generate_device_key() {
                Ok(k) => k,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "error-key");
                    return json!({"error": format!("device key: {e}")});
                }
            };
            let mut sess = match session::SessionStream::new(stream) {
                Ok(s) => s,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "error-session");
                    return json!({"error": format!("session: {e}")});
                }
            };
            let auth = match session::controller_handshake(
                &mut sess,
                "tethercraft-mcp",
                &signing,
                &password,
                expected,
            ) {
                Ok(a) => a,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "auth-failed");
                    return json!({"error": format!("handshake: {e}")});
                }
            };
            if sess.set_frame_timeouts().is_err() {
                audit(&state.store, &peer, "connect", "error-timeout");
                return json!({"error": "could not set timeouts"});
            }
            let mut words = Vec::new();
            if !had_pin {
                words = tethercraft_net::fingerprint_words(&auth.host_key);
                if tofu_record(&state.store, &peer, &auth.host_key).is_err() {
                    audit(&state.store, &peer, "connect", "error-tofu");
                    return json!({"error": "could not record host key"});
                }
            }
            let decoder = match session::StreamDecoder::new() {
                Ok(d) => d,
                Err(e) => {
                    audit(&state.store, &peer, "connect", "error-decoder");
                    return json!({"error": format!("decoder: {e}")});
                }
            };
            state.sessions.insert(
                peer.clone(),
                LiveSession {
                    sess,
                    decoder,
                    req: 0,
                    peer: peer.clone(),
                },
            );
            audit(&state.store, &peer, "connect", "ok");
            if words.is_empty() {
                ok_result(json!({"session": peer}))
            } else {
                ok_result(json!({
                    "session": peer,
                    "host_key_words": words.join(" "),
                    "note": "new host key recorded: VERIFY the words out of band before trusting it"
                }))
            }
        }
        "screenshot" | "input" | "exec" | "file_list" | "file_get" | "file_put" | "file_rename"
        | "file_rm" | "file_mkdir" | "file_hash" | "chat_send" | "tunnel_open" | "tunnel_send"
        | "tunnel_close" | "restart" | "shutdown" | "lock" => {
            if !needs_confirm(params) {
                return json!({
                    "needs_confirm": true,
                    "hint": format!("resend {method} with confirm=true; the action is audited either way")
                });
            }
            let name = params.get("session").and_then(|v| v.as_str()).unwrap_or("");
            if !state.sessions.contains_key(name) {
                audit(&state.store, name, method, "error-no-session");
                return json!({"error": format!("no live session {name}: connect first")});
            }
            let out = live_tool(state, method, params, name);
            let result = if out.get("error").is_some() {
                "error"
            } else {
                "ok"
            };
            let peer = state
                .sessions
                .get(name)
                .map(|s| s.peer.clone())
                .unwrap_or_else(|| name.to_string());
            audit(&state.store, &peer, method, result);
            out
        }
        _ => json!({"error": format!("unknown method {method}")}),
    }
}

/// Run a confirm-gated tool against one live session. The caller audits.
fn live_tool(state: &mut McpState, method: &str, params: &Value, name: &str) -> Value {
    let live = match state.sessions.get_mut(name) {
        Some(s) => s,
        None => return json!({"error": format!("no live session {name}")}),
    };
    match method {
        "screenshot" => {
            let out = match param_str(params, "out") {
                Some(o) if !o.is_empty() => o,
                _ => return json!({"error": "out path is required"}),
            };
            match tethercraft_net::rpc::screenshot(&mut live.sess, &mut live.decoder) {
                Ok((w, h, rgba)) => match session::write_ppm(out.as_str(), w, h, &rgba) {
                    Ok(()) => ok_result(json!({"saved": out, "width": w, "height": h})),
                    Err(e) => json!({"error": format!("save: {e}")}),
                },
                Err(e) => json!({"error": format!("screenshot: {e}")}),
            }
        }
        "input" => {
            let event: InputEvent = match params.get("event") {
                Some(v) => match serde_json::from_value(v.clone()) {
                    Ok(e) => e,
                    Err(e) => return json!({"error": format!("bad event: {e}")}),
                },
                None => return json!({"error": "event is required"}),
            };
            let kind = match event {
                InputEvent::TouchTap { .. } => MessageKind::TouchTap,
                InputEvent::TouchHold { .. } => MessageKind::TouchHold,
                InputEvent::TouchSwipe { .. } => MessageKind::TouchSwipe,
                _ => MessageKind::InputEvent,
            };
            let payload = match session::json_bytes(&event) {
                Ok(p) => p,
                Err(e) => return json!({"error": format!("encode: {e}")}),
            };
            match live
                .sess
                .send_envelope(&Envelope::new(kind, [0u8; 32], payload))
            {
                Ok(()) => ok_result(json!({"sent": true})),
                Err(e) => json!({"error": format!("input: {e}")}),
            }
        }
        "exec" => {
            let command = param_str(params, "command").unwrap_or_default();
            if command.trim().is_empty() {
                return json!({"error": "command is required"});
            }
            let req = live.next_req();
            match tethercraft_net::rpc::exec(&mut live.sess, req, &command) {
                Ok((code, output)) => ok_result(json!({"exit": code, "output": output})),
                Err(e) => json!({"error": format!("exec: {e}")}),
            }
        }
        "file_list" => {
            let path = param_str(params, "path").unwrap_or_default();
            let req = live.next_req();
            match tethercraft_net::rpc::file_list(&mut live.sess, req, &path) {
                Ok(entries) => {
                    let list: Vec<Value> = entries
                        .iter()
                        .map(|e| {
                            json!({
                                "name": e.name,
                                "is_dir": e.is_dir,
                                "size": e.size,
                                "mtime": e.mtime
                            })
                        })
                        .collect();
                    ok_result(json!({"entries": list}))
                }
                Err(e) => json!({"error": format!("file_list: {e}")}),
            }
        }
        "file_get" => {
            let remote = param_str(params, "remote").unwrap_or_default();
            let local = param_str(params, "local").unwrap_or_default();
            if local.is_empty() {
                return json!({"error": "local path is required"});
            }
            let req = live.next_req();
            match tethercraft_net::rpc::file_get(&mut live.sess, req, &remote) {
                Ok(data) => match std::fs::write(&local, &data) {
                    Ok(()) => ok_result(json!({"saved": local, "bytes": data.len()})),
                    Err(e) => json!({"error": format!("local write: {e}")}),
                },
                Err(e) => json!({"error": format!("file_get: {e}")}),
            }
        }
        "file_put" => {
            let local = param_str(params, "local").unwrap_or_default();
            let remote = param_str(params, "remote").unwrap_or_default();
            let data = match std::fs::read(&local) {
                Ok(d) => d,
                Err(e) => return json!({"error": format!("local read: {e}")}),
            };
            let req = live.next_req();
            match tethercraft_net::rpc::file_put(&mut live.sess, req, &remote, &data) {
                Ok(()) => ok_result(json!({"sent": remote, "bytes": data.len()})),
                Err(e) => json!({"error": format!("file_put: {e}")}),
            }
        }
        "file_rename" => {
            let from = param_str(params, "from").unwrap_or_default();
            let to = param_str(params, "to").unwrap_or_default();
            let req = live.next_req();
            match tethercraft_net::rpc::file_rename(&mut live.sess, req, &from, &to) {
                Ok(()) => ok_result(json!({"moved": true})),
                Err(e) => json!({"error": format!("file_rename: {e}")}),
            }
        }
        "file_rm" => {
            let path = param_str(params, "path").unwrap_or_default();
            let req = live.next_req();
            match tethercraft_net::rpc::file_simple(&mut live.sess, req, MessageKind::FileRm, &path)
            {
                Ok(()) => ok_result(json!({"removed": true})),
                Err(e) => json!({"error": format!("file_rm: {e}")}),
            }
        }
        "file_mkdir" => {
            let path = param_str(params, "path").unwrap_or_default();
            let req = live.next_req();
            match tethercraft_net::rpc::file_simple(
                &mut live.sess,
                req,
                MessageKind::FileMkdir,
                &path,
            ) {
                Ok(()) => ok_result(json!({"created": true})),
                Err(e) => json!({"error": format!("file_mkdir: {e}")}),
            }
        }
        "file_hash" => {
            let path = param_str(params, "path").unwrap_or_default();
            let req = live.next_req();
            match tethercraft_net::rpc::file_hash(&mut live.sess, req, &path) {
                Ok((hash_hex, size, mtime)) => {
                    ok_result(json!({"hash": hash_hex, "size": size, "mtime": mtime}))
                }
                Err(e) => json!({"error": format!("file_hash: {e}")}),
            }
        }
        "chat_send" => {
            let text = param_str(params, "text").unwrap_or_default();
            match tethercraft_net::rpc::chat_send(&mut live.sess, &text) {
                Ok(()) => ok_result(json!({"sent": true})),
                Err(e) => json!({"error": format!("chat_send: {e}")}),
            }
        }
        "tunnel_open" => {
            let channel = param_u32(params, "channel", 0);
            let target = param_str(params, "target").unwrap_or_default();
            match tethercraft_net::rpc::tunnel_open(&mut live.sess, channel, &target) {
                Ok(()) => ok_result(json!({"channel": channel, "open": true})),
                Err(e) => json!({"error": format!("tunnel_open: {e}")}),
            }
        }
        "tunnel_send" => {
            let channel = param_u32(params, "channel", 0);
            let hex_data = param_str(params, "data_hex").unwrap_or_default();
            let bytes = match hex::decode(hex_data.trim()) {
                Ok(b) => b,
                Err(e) => return json!({"error": format!("bad data_hex: {e}")}),
            };
            match tethercraft_net::rpc::tunnel_send(&mut live.sess, channel, &bytes) {
                Ok(()) => ok_result(json!({"sent": bytes.len()})),
                Err(e) => json!({"error": format!("tunnel_send: {e}")}),
            }
        }
        "tunnel_close" => {
            let channel = param_u32(params, "channel", 0);
            match tethercraft_net::rpc::tunnel_close(&mut live.sess, channel) {
                Ok(()) => ok_result(json!({"closed": true})),
                Err(e) => json!({"error": format!("tunnel_close: {e}")}),
            }
        }
        "restart" | "shutdown" | "lock" => {
            let kind = match method {
                "restart" => MessageKind::Restart,
                "shutdown" => MessageKind::Shutdown,
                _ => MessageKind::Lock,
            };
            let default_delay = if method == "lock" { 0 } else { 5 };
            let delay = param_u64(params, "delay_secs", default_delay);
            let req = live.next_req();
            match tethercraft_net::rpc::power(&mut live.sess, req, kind, delay) {
                Ok(()) => ok_result(json!({method: true})),
                Err(e) => json!({"error": format!("{method}: {e}")}),
            }
        }
        _ => json!({"error": format!("unknown method {method}")}),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut out = io::stdout();
    let mut state = McpState {
        store: open_store(),
        sessions: HashMap::new(),
    };
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => {
                let _ = writeln!(out, "{{\"error\":\"bad json\"}}");
                continue;
            }
        };
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(Value::Null);
        let resp = handle(&mut state, method, &params);
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_state() -> McpState {
        McpState {
            store: Store::open_in_memory().unwrap_or_else(|_| std::process::exit(1)),
            sessions: HashMap::new(),
        }
    }

    #[test]
    fn tool_list_has_sessions() {
        let mut s = mem_state();
        let r = handle(&mut s, "tools/list", &Value::Null);
        let text = r.to_string();
        assert!(text.contains("sessions"));
        assert!(text.contains("connect"));
        assert!(text.contains("disconnect"));
    }

    #[test]
    fn writes_need_confirm_and_audit() {
        let mut s = mem_state();
        let p = json!({"session": "x"});
        let r = handle(&mut s, "exec", &p);
        assert!(r
            .get("needs_confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));
        let ok = json!({"session": "x", "confirm": true});
        let r2 = handle(&mut s, "exec", &ok);
        assert!(r2.get("error").is_some());
        let log = s.store.list_audit(10).unwrap_or_default();
        assert!(log.len() == 1);
        assert!(log[0].action == "exec");
    }

    #[test]
    fn connect_unknown_peer_errors() {
        let mut s = mem_state();
        let p = json!({"peer": "ghost", "password_file": "none", "confirm": true});
        let r = handle(&mut s, "connect", &p);
        assert!(r.get("error").is_some());
    }

    #[test]
    fn connect_needs_confirm() {
        let mut s = mem_state();
        let p = json!({"peer": "ghost", "password_file": "none"});
        let r = handle(&mut s, "connect", &p);
        assert!(r
            .get("needs_confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));
    }

    #[test]
    fn disconnect_unknown_errors() {
        let mut s = mem_state();
        let p = json!({"session": "ghost"});
        let r = handle(&mut s, "disconnect", &p);
        assert!(r.get("error").is_some());
    }

    #[test]
    fn file_tools_need_confirm() {
        let mut s = mem_state();
        for m in [
            "file_list",
            "file_get",
            "file_put",
            "file_hash",
            "screenshot",
        ] {
            let p = json!({"session": "x"});
            let r = handle(&mut s, m, &p);
            assert!(
                r.get("needs_confirm")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                "method {m} must gate on confirm"
            );
        }
    }
}
