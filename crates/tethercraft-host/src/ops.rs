//! Host-side command execution: files, one-shot exec, power actions.
//!
//! Every function assumes the caller already checked the relevant permission
//! on the host (`host.authorize`). They validate inputs again anyway: defence
//! in depth, and the validation is unit-tested without a network.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tethercraft_core::TetherError;
use tethercraft_proto as proto;

use super::files::FileManager;

/// In-progress upload from one controller: req_id -> (path, bytes so far).
pub type Uploads = HashMap<u64, (String, Vec<u8>)>;

fn reply(
    kind: tethercraft_proto::MessageKind,
    v: &impl serde::Serialize,
) -> Option<(tethercraft_proto::MessageKind, Vec<u8>)> {
    tethercraft_net::session::json_bytes(v)
        .ok()
        .map(|p| (kind, p))
}

fn ack(req_id: u64, ok: bool, error: impl Into<String>) -> proto::FileDoneAck {
    proto::FileDoneAck {
        req_id,
        ok,
        error: error.into(),
    }
}

/// Handle one file-protocol message. Returns zero or more reply envelopes'
/// worth of (kind, payload) pairs; the serve loop sends them.
pub fn handle_file(
    fm: &FileManager,
    kind: proto::MessageKind,
    payload: &[u8],
    uploads: &mut Uploads,
) -> Vec<(proto::MessageKind, Vec<u8>)> {
    use proto::MessageKind as K;
    let mut out = Vec::new();
    match kind {
        K::FileList => {
            let req: Result<proto::FileReq, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => match fm.list(&r.path) {
                    Ok(entries) => {
                        let entries = entries
                            .into_iter()
                            .map(|e| proto::DirEntry {
                                name: e.name,
                                is_dir: e.is_dir,
                                size: e.size,
                                mtime: e.mtime,
                            })
                            .collect();
                        if let Some(m) = reply(
                            K::FileListResp,
                            &proto::FileListResp {
                                req_id: r.req_id,
                                entries,
                            },
                        ) {
                            out.push(m);
                        }
                    }
                    Err(e) => {
                        if let Some(m) = reply(
                            K::FileDone,
                            &ack(r.req_id, false, format!("list failed: {e}")),
                        ) {
                            out.push(m);
                        }
                    }
                },
            }
        }
        K::FileGet => {
            let req: Result<proto::FileReq, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => match fm.resolve(&r.path) {
                    Err(e) => {
                        if let Some(m) =
                            reply(K::FileDone, &ack(r.req_id, false, format!("bad path: {e}")))
                        {
                            out.push(m);
                        }
                    }
                    Ok(full) => match std::fs::read(&full) {
                        Err(e) => {
                            if let Some(m) =
                                reply(K::FileDone, &ack(r.req_id, false, format!("read: {e}")))
                            {
                                out.push(m);
                            }
                        }
                        Ok(data) => {
                            if data.len() as u64 > proto::MAX_TRANSFER_BYTES {
                                if let Some(m) = reply(
                                    K::FileDone,
                                    &ack(
                                        r.req_id,
                                        false,
                                        "file too large for transfer".to_string(),
                                    ),
                                ) {
                                    out.push(m);
                                }
                                return out;
                            }
                            let mut off = 0usize;
                            if data.is_empty() {
                                let b = proto::FileBlock {
                                    req_id: r.req_id,
                                    offset: 0,
                                    data_hex: String::new(),
                                    eof: true,
                                };
                                if let Some(m) = reply(K::FileBlock, &b) {
                                    out.push(m);
                                }
                                return out;
                            }
                            while off < data.len() {
                                let end = (off + proto::FILE_BLOCK_BYTES).min(data.len());
                                let b = proto::FileBlock {
                                    req_id: r.req_id,
                                    offset: off as u64,
                                    data_hex: hex::encode(&data[off..end]),
                                    eof: end == data.len(),
                                };
                                if let Some(m) = reply(K::FileBlock, &b) {
                                    out.push(m);
                                } else {
                                    break;
                                }
                                off = end;
                            }
                        }
                    },
                },
            }
        }
        K::FilePut => {
            let req: Result<proto::FilePutReq, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => {
                    if r.total > proto::MAX_TRANSFER_BYTES {
                        if let Some(m) = reply(
                            K::FileDone,
                            &ack(r.req_id, false, "file too large for transfer".to_string()),
                        ) {
                            out.push(m);
                        }
                    } else if fm.resolve(&r.path).is_err() {
                        if let Some(m) =
                            reply(K::FileDone, &ack(r.req_id, false, "bad path".to_string()))
                        {
                            out.push(m);
                        }
                    } else {
                        uploads.insert(r.req_id, (r.path, Vec::new()));
                    }
                }
            }
        }
        K::FileChunk => {
            let b: Result<proto::FileBlock, _> = tethercraft_net::session::parse_json(payload);
            match b {
                Err(_) => {}
                Ok(block) => {
                    let done = match uploads.get_mut(&block.req_id) {
                        None => None,
                        Some((_, buf)) => {
                            if block.offset as usize != buf.len() {
                                Some(Err("out-of-order block".to_string()))
                            } else if buf.len() as u64 + block.data_hex.len() as u64 / 2
                                > proto::MAX_TRANSFER_BYTES
                            {
                                Some(Err("file too large for transfer".to_string()))
                            } else {
                                match hex::decode(&block.data_hex) {
                                    Err(_) => Some(Err("bad block bytes".to_string())),
                                    Ok(bytes) => {
                                        buf.extend_from_slice(&bytes);
                                        if block.eof {
                                            Some(Ok(()))
                                        } else {
                                            None
                                        }
                                    }
                                }
                            }
                        }
                    };
                    match done {
                        None => {}
                        Some(Err(e)) => {
                            uploads.remove(&block.req_id);
                            if let Some(m) = reply(K::FileDone, &ack(block.req_id, false, e)) {
                                out.push(m);
                            }
                        }
                        Some(Ok(())) => {
                            if let Some((path, buf)) = uploads.remove(&block.req_id) {
                                let saved = fm
                                    .write_bytes(&path, &buf, false)
                                    .map_err(|e| e.to_string());
                                let (ok, err) = match saved {
                                    Ok(_) => (true, String::new()),
                                    Err(e) => (false, e),
                                };
                                if let Some(m) = reply(K::FileDone, &ack(block.req_id, ok, err)) {
                                    out.push(m);
                                }
                            }
                        }
                    }
                }
            }
        }
        K::FileHash => {
            let req: Result<proto::FileHashReq, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => match fm.resolve(&r.path) {
                    Err(e) => {
                        if let Some(m) =
                            reply(K::FileDone, &ack(r.req_id, false, format!("bad path: {e}")))
                        {
                            out.push(m);
                        }
                    }
                    Ok(full) => {
                        let meta = std::fs::metadata(&full);
                        let data = std::fs::read(&full);
                        match (meta, data) {
                            (Ok(m), Ok(d))
                                if !m.is_dir() && d.len() as u64 <= proto::MAX_TRANSFER_BYTES =>
                            {
                                let mtime = m
                                    .modified()
                                    .ok()
                                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                    .map(|d| d.as_secs())
                                    .unwrap_or(0);
                                let resp = proto::FileHashResp {
                                    req_id: r.req_id,
                                    hash_hex: crate::sync::hash_bytes(&d),
                                    size: d.len() as u64,
                                    mtime,
                                };
                                if let Some(m) = reply(K::FileHashResp, &resp) {
                                    out.push(m);
                                }
                            }
                            _ => {
                                if let Some(m) = reply(
                                    K::FileDone,
                                    &ack(
                                        r.req_id,
                                        false,
                                        "cannot hash (dir or too large)".to_string(),
                                    ),
                                ) {
                                    out.push(m);
                                }
                            }
                        }
                    }
                },
            }
        }
        K::FileRm => {
            let req: Result<proto::FileReq, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => {
                    let res = fm.remove(&r.path).map_err(|e| e.to_string());
                    let (ok, err) = match res {
                        Ok(()) => (true, String::new()),
                        Err(e) => (false, e),
                    };
                    if let Some(m) = reply(K::FileDone, &ack(r.req_id, ok, err)) {
                        out.push(m);
                    }
                }
            }
        }
        K::FileMv => {
            #[derive(serde::Deserialize)]
            struct Mv {
                req_id: u64,
                from: String,
                to: String,
            }
            let req: Result<Mv, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => {
                    let res = fm
                        .resolve(&r.from)
                        .and_then(|_| fm.resolve(&r.to))
                        .and_then(|_| fm.rename(&r.from, &r.to));
                    let (ok, err) = match res {
                        Ok(_) => (true, String::new()),
                        Err(e) => (false, format!("{e}")),
                    };
                    if let Some(m) = reply(K::FileDone, &ack(r.req_id, ok, err)) {
                        out.push(m);
                    }
                }
            }
        }
        K::FileMkdir => {
            let req: Result<proto::FileReq, _> = tethercraft_net::session::parse_json(payload);
            match req {
                Err(_) => {}
                Ok(r) => {
                    let res = fm.mkdir_all(&r.path).map_err(|e| e.to_string());
                    let (ok, err) = match res {
                        Ok(()) => (true, String::new()),
                        Err(e) => (false, e),
                    };
                    if let Some(m) = reply(K::FileDone, &ack(r.req_id, ok, err)) {
                        out.push(m);
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Run a command and wait, with a kill timeout and a capped tail of output.
pub fn run_exec(command: &str) -> Result<(i32, String), TetherError> {
    #[cfg(not(windows))]
    {
        let _ = command;
        return Err(TetherError::NotImplemented(
            "remote exec is Windows-only in this tier".to_string(),
        ));
    }
    #[cfg(windows)]
    {
        run_exec_windows(command)
    }
}

#[cfg(windows)]
fn run_exec_windows(command: &str) -> Result<(i32, String), TetherError> {
    use std::process::Command;
    if command.trim().is_empty() {
        return Err(TetherError::Invalid("empty command".to_string()));
    }
    let mut child = Command::new("cmd")
        .args(["/C", command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| TetherError::Invalid(format!("spawn failed: {e}")))?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Err(e) => {
                let _ = child.kill();
                return Err(TetherError::Invalid(format!("wait failed: {e}")));
            }
            Ok(Some(status)) => {
                let code = status.code().unwrap_or(-1);
                let mut text = String::new();
                if let Some(mut o) = child.stdout.take() {
                    use std::io::Read;
                    let _ = o.read_to_string(&mut text);
                }
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let mut err = String::new();
                    let _ = e.read_to_string(&mut err);
                    if !err.is_empty() {
                        text.push_str("\n[stderr]\n");
                        text.push_str(&err);
                    }
                }
                if text.len() > proto::MAX_EXEC_OUTPUT {
                    let cut = text.len() - proto::MAX_EXEC_OUTPUT;
                    text = format!(
                        "...[truncated {cut} bytes]\n{}",
                        &text[text.len() - proto::MAX_EXEC_OUTPUT..]
                    );
                }
                return Ok((code, text));
            }
            Ok(None) => {
                if Instant::now() > deadline {
                    let _ = child.kill();
                    return Err(TetherError::Invalid(
                        "command timed out after 30 s".to_string(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

pub enum PowerAction {
    Restart,
    Shutdown,
    Lock,
}

/// Power actions. Windows-only like the rest of the host; shutdown.exe and
/// rundll32 exist on every Windows install, no extra dependency.
pub fn power(action: PowerAction, delay_secs: u64) -> Result<(), TetherError> {
    if delay_secs > 600 {
        return Err(TetherError::Invalid(
            "delay must be 0..=600 seconds".to_string(),
        ));
    }
    #[cfg(not(windows))]
    {
        let _ = (&action, delay_secs);
        return Err(TetherError::NotImplemented(
            "remote power actions are Windows-only in this tier".to_string(),
        ));
    }
    #[cfg(windows)]
    {
        use std::process::Command;
        let status = match action {
            PowerAction::Restart => Command::new("shutdown")
                .args([
                    "/r",
                    "/t",
                    &delay_secs.to_string(),
                    "/c",
                    "TetherCraft remote restart",
                ])
                .status(),
            PowerAction::Shutdown => Command::new("shutdown")
                .args([
                    "/s",
                    "/t",
                    &delay_secs.to_string(),
                    "/c",
                    "TetherCraft remote shutdown",
                ])
                .status(),
            PowerAction::Lock => Command::new("rundll32.exe")
                .args(["user32.dll,LockWorkStation"])
                .status(),
        };
        match status {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(TetherError::Invalid(format!(
                "os refused the request (exit {})",
                s.code().unwrap_or(-1)
            ))),
            Err(e) => Err(TetherError::Invalid(format!("spawn failed: {e}"))),
        }
    }
}
