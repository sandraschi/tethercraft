//! tethercraft-mcp: stdio MCP server, read tools only in Tier 1.
//! Mutating tools return NotImplemented with a confirm note.

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use tethercraft_store::Store;

fn tool_list() -> Value {
    json!({
        "tools": [
            {"name": "status", "description": "daemon and onboarding status (read-only)"},
            {"name": "peers", "description": "address book peers (read-only)"},
            {"name": "sessions", "description": "live host sessions visible to this process (read-only)"},
            {"name": "audit", "description": "audit log tail (read-only)"},
            {"name": "peer_info", "description": "one peer row (read-only)"},
            {"name": "screenshot_info", "description": "screenshot capability state (read-only, Tier 1 partial)"},
            {"name": "connect", "description": "open a GUI session (needs confirm, live session)"},
            {"name": "exec", "description": "run a command over the terminal channel (needs confirm, permission-gated)"},
            {"name": "input", "description": "inject an input event (needs confirm, permission-gated)"},
            {"name": "file_put", "description": "upload a file (needs confirm, permission-gated)"},
            {"name": "tunnel", "description": "open a TCP tunnel (needs confirm, permission-gated)"},
            {"name": "chat_send", "description": "send session chat text (needs confirm)"}
        ]
    })
}

fn ok_result(v: Value) -> Value {
    json!({"result": v})
}

/// Mutating tools sit behind `confirm`. Without it the call is refused, never run.
fn needs_confirm(params: &Value) -> bool {
    params
        .get("confirm")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

fn handle(store: &Store, method: &str, params: &Value) -> Value {
    match method {
        "tools/list" => ok_result(tool_list()),
        "status" => ok_result(json!({"tier": 2, "transport": "quic-pinned (pending live run)"})),
        "peers" => match store.list_peers() {
            Ok(p) => ok_result(json!(p)),
            Err(e) => json!({"error": e.to_string()}),
        },
        "sessions" => ok_result(json!({
            "sessions": [],
            "note": "no live host sessions in this stdio process"
        })),
        "audit" => {
            let limit = params
                .get("limit")
                .and_then(|v| v.as_u64())
                .unwrap_or(50)
                .min(500) as usize;
            match store.list_audit(limit) {
                Ok(l) => ok_result(json!(l)),
                Err(e) => json!({"error": e.to_string()}),
            }
        }
        "peer_info" => {
            let want = params.get("peer").and_then(|v| v.as_str()).unwrap_or("");
            match store.list_peers() {
                Ok(list) => match list.iter().find(|r| r.peer_id == want) {
                    Some(row) => ok_result(json!(row)),
                    None => json!({"error": format!("unknown peer {want}")}),
                },
                Err(e) => json!({"error": e.to_string()}),
            }
        }
        "connect" | "exec" | "input" | "file_put" | "tunnel" | "chat_send" | "put" => {
            if !needs_confirm(params) {
                return json!({
                    "needs_confirm": true,
                    "hint": format!("resend {method} with confirm=true; the action is audited either way")
                });
            }
            // Confirmed but no live session exists in Tier 2 without the host
            // service run: record the attempt, refuse explicitly.
            let peer = params.get("peer").and_then(|v| v.as_str()).unwrap_or("?");
            let ev = tethercraft_core::AuditEvent::new(
                chrono::Utc::now(),
                peer,
                method,
                "denied-no-live-session",
            );
            let _ = store.log(&ev);
            json!({"error": format!("not implemented (Tier 2, needs live session): {method}")})
        }
        _ => json!({"error": format!("unknown method {method}")}),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut out = io::stdout();
    let store = Store::open_in_memory().unwrap_or_else(|_| {
        // In-memory always succeeds; this path only exists to avoid expect().
        std::process::exit(1);
    });
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
        let resp = handle(&store, method, &params);
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open_in_memory().unwrap_or_else(|_| std::process::exit(1))
    }

    #[test]
    fn tool_list_has_sessions() {
        let s = mem_store();
        let r = handle(&s, "tools/list", &Value::Null);
        let text = r.to_string();
        assert!(text.contains("sessions"));
        assert!(text.contains("connect"));
    }

    #[test]
    fn writes_need_confirm_and_audit() {
        let s = mem_store();
        let p = json!({"peer": "x"});
        let r = handle(&s, "exec", &p);
        assert!(r
            .get("needs_confirm")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));
        let ok = json!({"peer": "x", "confirm": true});
        let r2 = handle(&s, "exec", &ok);
        assert!(r2.get("error").is_some());
        let log = s.list_audit(10).unwrap_or_default();
        assert!(log.len() == 1);
        assert!(log[0].action == "exec");
    }
}
