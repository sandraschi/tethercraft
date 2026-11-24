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
            {"name": "audit", "description": "audit log tail (read-only)"},
            {"name": "peer_info", "description": "one peer row (read-only)"},
            {"name": "screenshot_info", "description": "screenshot capability state (read-only, Tier 1 partial)"}
        ]
    })
}

fn ok_result(v: Value) -> Value {
    json!({"result": v})
}

fn not_impl(what: &str) -> Value {
    json!({"error": format!("not implemented (Tier 2, needs confirm): {what}")})
}

fn handle(store: &Store, method: &str, params: &Value) -> Value {
    match method {
        "tools/list" => ok_result(tool_list()),
        "status" => ok_result(json!({"tier": 1, "transport": "quic-pinned (pending live run)"})),
        "peers" => match store.list_peers() {
            Ok(p) => ok_result(json!(p)),
            Err(e) => json!({"error": e.to_string()}),
        },
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
        "connect" | "exec" | "put" | "input" => not_impl(method),
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
