//! Controller-side request/response helpers for the command verbs.
//!
//! Every helper runs on an already-handshaked [`SessionStream`]. Frames keep
//! arriving during any call, so each helper skips frame envelopes until the
//! reply of the expected kind arrives (or the budget runs out). One call at a
//! time per stream; concurrent calls need separate sessions.

use std::time::{Duration, Instant};

use tethercraft_core::TetherError;
use tethercraft_proto::{
    self as proto, Envelope, FileBlock, FileDoneAck, FileListResp, MessageKind,
};

use super::session::{json_bytes, parse_json, SessionStream};

const RPC_BUDGET: Duration = Duration::from_secs(120);
const RPC_POLL: Duration = Duration::from_secs(30);

/// Send one request and wait for the first reply that is not a frame, a
/// session end, or noise. Returns SessionEnd as an error: the host went away.
pub fn rpc(
    sess: &mut SessionStream,
    kind: MessageKind,
    payload: &[u8],
) -> Result<Envelope, TetherError> {
    sess.send_envelope(&Envelope::new(kind, [0u8; 32], payload.to_vec()))?;
    let deadline = Instant::now() + RPC_BUDGET;
    loop {
        if Instant::now() > deadline {
            return Err(TetherError::Invalid(
                "timed out waiting for host reply".to_string(),
            ));
        }
        match sess.recv_envelope_wait(RPC_POLL) {
            Err(e) => return Err(e),
            Ok(None) => continue,
            Ok(Some(env)) => match env.kind {
                MessageKind::Frame | MessageKind::FrameH264 => continue,
                MessageKind::SessionEnd => {
                    return Err(TetherError::Invalid(
                        "host ended the session mid-request".to_string(),
                    ))
                }
                _ => return Ok(env),
            },
        }
    }
}

fn expect_kind(env: &Envelope, want: MessageKind, what: &str) -> Result<(), TetherError> {
    if env.kind == want {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!(
            "host answered {what} with {:?}",
            env.kind
        )))
    }
}

/// Counter for request ids. Starts at 1; zero is never used so a missing
/// field cannot be mistaken for a real request.
#[derive(Debug, Default)]
pub struct ReqId(u64);

impl ReqId {
    pub fn next_id(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(1);
        if self.0 == 0 {
            self.0 = 1;
        }
        self.0
    }
}

pub fn file_list(
    sess: &mut SessionStream,
    req_id: u64,
    path: &str,
) -> Result<Vec<proto::DirEntry>, TetherError> {
    tethercraft_proto::validate_transfer_path(path)?;
    let req = proto::FileReq {
        req_id,
        path: path.to_string(),
    };
    let env = rpc(sess, MessageKind::FileList, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::FileListResp, "file list")?;
    let resp: FileListResp = parse_json(&env.payload)?;
    if resp.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    Ok(resp.entries)
}

/// Download a whole file, chunked on the wire, capped by the protocol limit.
pub fn file_get(
    sess: &mut SessionStream,
    req_id: u64,
    remote: &str,
) -> Result<Vec<u8>, TetherError> {
    tethercraft_proto::validate_transfer_path(remote)?;
    let req = proto::FileReq {
        req_id,
        path: remote.to_string(),
    };
    sess.send_envelope(&Envelope::new(
        MessageKind::FileGet,
        [0u8; 32],
        json_bytes(&req)?,
    ))?;
    let deadline = Instant::now() + RPC_BUDGET;
    let mut out = Vec::new();
    let mut offset = 0u64;
    loop {
        if Instant::now() > deadline {
            return Err(TetherError::Invalid(
                "timed out downloading file".to_string(),
            ));
        }
        let env = match sess.recv_envelope_wait(RPC_POLL)? {
            None => continue,
            Some(e) => e,
        };
        match env.kind {
            MessageKind::Frame | MessageKind::FrameH264 => continue,
            MessageKind::SessionEnd => {
                return Err(TetherError::Invalid(
                    "host ended the session mid-download".to_string(),
                ))
            }
            MessageKind::FileDone => {
                let ack: FileDoneAck = parse_json(&env.payload)?;
                if ack.req_id != req_id {
                    continue;
                }
                if !ack.ok {
                    return Err(TetherError::Invalid(format!(
                        "host refused download: {}",
                        ack.error
                    )));
                }
                break;
            }
            MessageKind::FileBlock => {
                let b: FileBlock = parse_json(&env.payload)?;
                if b.req_id != req_id {
                    continue;
                }
                let bytes = hex::decode(&b.data_hex)
                    .map_err(|_| TetherError::Invalid("bad file bytes".to_string()))?;
                if b.offset != offset {
                    return Err(TetherError::Invalid("out-of-order file block".to_string()));
                }
                if out.len() as u64 + bytes.len() as u64 > proto::MAX_TRANSFER_BYTES {
                    return Err(TetherError::Invalid(
                        "file too large for transfer".to_string(),
                    ));
                }
                offset += bytes.len() as u64;
                out.extend_from_slice(&bytes);
                if b.eof {
                    break;
                }
            }
            _ => continue,
        }
    }
    Ok(out)
}

/// Upload bytes in 60 KiB blocks, then wait for the host ack.
pub fn file_put(
    sess: &mut SessionStream,
    req_id: u64,
    remote: &str,
    data: &[u8],
) -> Result<(), TetherError> {
    tethercraft_proto::validate_transfer_path(remote)?;
    if data.len() as u64 > proto::MAX_TRANSFER_BYTES {
        return Err(TetherError::Invalid(
            "file too large for transfer".to_string(),
        ));
    }
    let put = proto::FilePutReq {
        req_id,
        path: remote.to_string(),
        total: data.len() as u64,
    };
    sess.send_envelope(&Envelope::new(
        MessageKind::FilePut,
        [0u8; 32],
        json_bytes(&put)?,
    ))?;
    let mut offset = 0usize;
    loop {
        let end = (offset + proto::FILE_BLOCK_BYTES).min(data.len());
        let last = end == data.len();
        let block = FileBlock {
            req_id,
            offset: offset as u64,
            data_hex: hex::encode(&data[offset..end]),
            eof: last,
        };
        sess.send_envelope(&Envelope::new(
            MessageKind::FileBlock,
            [0u8; 32],
            json_bytes(&block)?,
        ))?;
        offset = end;
        if last {
            break;
        }
    }
    let env = rpc(sess, MessageKind::FileDone, &[])?;
    let ack: FileDoneAck = parse_json(&env.payload)?;
    if ack.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    if ack.ok {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!(
            "host refused upload: {}",
            ack.error
        )))
    }
}

pub fn file_simple(
    sess: &mut SessionStream,
    req_id: u64,
    kind: MessageKind,
    path: &str,
) -> Result<(), TetherError> {
    tethercraft_proto::validate_transfer_path(path)?;
    let req = proto::FileReq {
        req_id,
        path: path.to_string(),
    };
    let env = rpc(sess, kind, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::FileDone, "file op")?;
    let ack: FileDoneAck = parse_json(&env.payload)?;
    if ack.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    if ack.ok {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!("host refused: {}", ack.error)))
    }
}

pub fn file_rename(
    sess: &mut SessionStream,
    req_id: u64,
    from: &str,
    to: &str,
) -> Result<(), TetherError> {
    tethercraft_proto::validate_transfer_path(from)?;
    tethercraft_proto::validate_transfer_path(to)?;
    let payload = serde_json::json!({"req_id": req_id, "from": from, "to": to});
    let bytes =
        serde_json::to_vec(&payload).map_err(|e| TetherError::Invalid(format!("encode: {e}")))?;
    // Reuse FilePut kind namespace is wrong; Mv has its own kind.
    let env = rpc(sess, MessageKind::FileMv, &bytes)?;
    expect_kind(&env, MessageKind::FileDone, "file move")?;
    let ack: FileDoneAck = parse_json(&env.payload)?;
    if !ack.ok {
        return Err(TetherError::Invalid(format!(
            "host refused move: {}",
            ack.error
        )));
    }
    Ok(())
}

/// Run a command on the host and wait for the result. The host enforces the
/// Terminal grant, a 30 s timeout, and a 64 KiB output cap.
pub fn exec(
    sess: &mut SessionStream,
    req_id: u64,
    command: &str,
) -> Result<(i32, String), TetherError> {
    if command.trim().is_empty() {
        return Err(TetherError::Invalid("empty command".to_string()));
    }
    let req = proto::ExecReq {
        req_id,
        command: command.to_string(),
    };
    let env = rpc(sess, MessageKind::ExecRequest, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::ExecDone, "exec")?;
    let done: proto::ExecDone = parse_json(&env.payload)?;
    if done.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    Ok((done.exit_code, done.output))
}

/// Ask the host to restart, shut down, or lock. Returns once the host accepts
/// (the session then ends; the controller must not treat that as an error).
pub fn power(
    sess: &mut SessionStream,
    req_id: u64,
    kind: MessageKind,
    delay_secs: u64,
) -> Result<(), TetherError> {
    match kind {
        MessageKind::Restart | MessageKind::Shutdown | MessageKind::Lock => {}
        _ => return Err(TetherError::Invalid("not a power action".to_string())),
    }
    if delay_secs > 600 {
        return Err(TetherError::Invalid(
            "delay must be 0..=600 seconds".to_string(),
        ));
    }
    let req = proto::PowerReq { req_id, delay_secs };
    let env = rpc(sess, kind, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::PowerAck, "power action")?;
    let ack: proto::PowerAck = parse_json(&env.payload)?;
    if ack.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    if ack.ok {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!("host refused: {}", ack.error)))
    }
}

/// Ask for a file's hash without downloading it. Used for sync planning.
pub fn file_hash(
    sess: &mut SessionStream,
    req_id: u64,
    path: &str,
) -> Result<(String, u64, u64), TetherError> {
    tethercraft_proto::validate_transfer_path(path)?;
    let req = proto::FileHashReq {
        req_id,
        path: path.to_string(),
    };
    let env = rpc(sess, MessageKind::FileHash, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::FileHashResp, "file hash")?;
    let resp: proto::FileHashResp = parse_json(&env.payload)?;
    if resp.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    Ok((resp.hash_hex, resp.size, resp.mtime))
}

/// Grab the next decodable frame (drops anything else). Used for screenshots
/// and one-shot captures; the caller owns pacing.
pub fn screenshot(
    sess: &mut SessionStream,
    decoder: &mut super::session::StreamDecoder,
) -> Result<(u32, u32, Vec<u8>), TetherError> {
    let deadline = std::time::Instant::now() + RPC_BUDGET;
    loop {
        if std::time::Instant::now() > deadline {
            return Err(TetherError::Invalid(
                "timed out waiting for a frame".to_string(),
            ));
        }
        match sess.recv_envelope_wait(RPC_POLL)? {
            None => continue,
            Some(env) => match env.kind {
                MessageKind::Frame | MessageKind::FrameH264 => {
                    if let Some(img) = decoder.decode(env.kind, &env.payload)? {
                        return Ok(img);
                    }
                }
                MessageKind::SessionEnd => {
                    return Err(TetherError::Invalid("host ended the session".to_string()))
                }
                _ => continue,
            },
        }
    }
}

/// Open a TCP tunnel channel through the session. The caller pumps bytes both
/// ways afterwards; see `tunnel_send` and the serve-side counterpart.
pub fn tunnel_open(
    sess: &mut SessionStream,
    channel: u32,
    target: &str,
) -> Result<(), TetherError> {
    if channel == 0 {
        return Err(TetherError::Invalid("channel must be nonzero".to_string()));
    }
    if target.trim().is_empty() || target.len() > 256 {
        return Err(TetherError::Invalid("bad tunnel target".to_string()));
    }
    let req = proto::TunnelOpen {
        channel,
        target: target.to_string(),
    };
    let env = rpc(sess, MessageKind::TunnelOpen, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::TunnelAck, "tunnel open")?;
    let ack: proto::TunnelAck = parse_json(&env.payload)?;
    if ack.channel != channel {
        return Err(TetherError::Invalid("mismatched channel".to_string()));
    }
    if ack.ok {
        Ok(())
    } else {
        Err(TetherError::Invalid(format!(
            "host refused tunnel: {}",
            ack.error
        )))
    }
}

/// Queue one chunk toward the remote end of a channel. 60 KiB max per call.
pub fn tunnel_send(
    sess: &mut SessionStream,
    channel: u32,
    bytes: &[u8],
) -> Result<(), TetherError> {
    if bytes.is_empty() || bytes.len() > proto::FILE_BLOCK_BYTES {
        return Err(TetherError::Invalid(
            "tunnel chunk must be 1 byte..=60 KiB".to_string(),
        ));
    }
    let block = proto::TunnelBlock {
        channel,
        data_hex: hex::encode(bytes),
    };
    sess.send_envelope(&Envelope::new(
        MessageKind::TunnelData,
        [0u8; 32],
        json_bytes(&block)?,
    ))?;
    Ok(())
}

/// Close a channel from our side. Fire-and-forget: the host cleans up.
pub fn tunnel_close(sess: &mut SessionStream, channel: u32) -> Result<(), TetherError> {
    let close = proto::TunnelClose { channel };
    sess.send_envelope(&Envelope::new(
        MessageKind::TunnelClose,
        [0u8; 32],
        json_bytes(&close)?,
    ))?;
    Ok(())
}

/// Send one chat line. Fire-and-forget: the host stores it in the session log.
pub fn chat_send(sess: &mut SessionStream, text: &str) -> Result<(), TetherError> {
    if text.is_empty() || text.len() > 4096 {
        return Err(TetherError::Invalid(
            "chat text must be 1..=4096 chars".to_string(),
        ));
    }
    sess.send_envelope(&Envelope::new(
        MessageKind::ChatMsg,
        [0u8; 32],
        text.as_bytes().to_vec(),
    ))?;
    Ok(())
}

/// Query the remote host's structured "all is well" health snapshot (CPU, RAM,
/// GPU, disks, watched services/apps).
pub fn health(
    sess: &mut SessionStream,
    req_id: u64,
    watch: &[String],
) -> Result<proto::HealthReport, TetherError> {
    let req = proto::HealthReq {
        req_id,
        watch: watch.to_vec(),
    };
    let env = rpc(sess, MessageKind::HealthReq, &json_bytes(&req)?)?;
    expect_kind(&env, MessageKind::HealthResp, "health")?;
    let resp: proto::HealthReport = parse_json(&env.payload)?;
    if resp.req_id != req_id {
        return Err(TetherError::Invalid("mismatched request id".to_string()));
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn req_ids_never_zero() {
        let mut r = ReqId::default();
        assert!(r.next_id() == 1);
        assert!(r.next_id() == 2);
        let mut w = ReqId(u64::MAX);
        assert!(w.next_id() == 1);
    }
}
