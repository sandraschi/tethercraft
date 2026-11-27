//! Native TetherCraft wire schema, own design. Versioned envelope over postcard.
//!
//! This is NOT RustDesk's protocol. Interop (Tier 3b) is a separate mode and is
//! not implemented here.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

/// Current native wire version. Receivers reject anything else.
pub const WIRE_VERSION: u8 = 1;
/// Hard cap for one decoded envelope (8 MiB).
pub const MAX_ENVELOPE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    PairingHello,
    PairingWelcome,
    PakeA,
    PakeB,
    OtpProof,
    TouchTap,
    TouchHold,
    TouchSwipe,
    FrameH264,
    SessionStart,
    SessionEnd,
    Frame,
    InputEvent,
    ClipboardText,
    ClipboardImage,
    ClipboardFiles,
    FileList,
    FileGet,
    FilePut,
    FileChunk,
    FileDone,
    FileMv,
    FileMkdir,
    FileRm,
    FileHash,
    FileHashResp,
    FileListResp,
    FileBlock,
    ExecDone,
    Restart,
    Shutdown,
    Lock,
    PowerAck,
    TunnelAck,
    ChatMsg,
    NoteSet,
    TermOpen,
    TermData,
    TermClose,
    TunnelOpen,
    TunnelData,
    TunnelClose,
    AudioFrame,
    PrivacyRequest,
    PrivacyEnd,
    SyncPlan,
    SyncChunk,
    ExecRequest,
    Ping,
    Pong,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub version: u8,
    pub kind: MessageKind,
    pub sender_key: [u8; 32],
    pub payload: Vec<u8>,
}

impl Envelope {
    pub fn new(kind: MessageKind, sender_key: [u8; 32], payload: Vec<u8>) -> Self {
        Self {
            version: WIRE_VERSION,
            kind,
            sender_key,
            payload,
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, TetherError> {
        if self.payload.len() > MAX_ENVELOPE_BYTES {
            return Err(TetherError::Invalid("payload too large".to_string()));
        }
        postcard::to_stdvec(self).map_err(|e| TetherError::Invalid(format!("encode failed: {e}")))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, TetherError> {
        if bytes.len() > MAX_ENVELOPE_BYTES + 128 {
            return Err(TetherError::Invalid("frame too large".to_string()));
        }
        let (env, rest): (Envelope, &[u8]) = postcard::take_from_bytes(bytes)
            .map_err(|e| TetherError::Invalid(format!("decode failed: {e}")))?;
        if !rest.is_empty() {
            return Err(TetherError::Invalid(
                "trailing bytes after envelope".to_string(),
            ));
        }
        if env.version != WIRE_VERSION {
            return Err(TetherError::Invalid(format!(
                "unsupported wire version {}",
                env.version
            )));
        }
        if env.payload.len() > MAX_ENVELOPE_BYTES {
            return Err(TetherError::Invalid("payload too large".to_string()));
        }
        Ok(env)
    }
}

/// File transfer paths (Tier 2 reuses this): no absolute paths, no `..`.
pub fn validate_transfer_path(path: &str) -> Result<(), TetherError> {
    if path.is_empty() {
        return Err(TetherError::Invalid("empty path".to_string()));
    }
    if path.len() > 4096 {
        return Err(TetherError::Invalid("path too long".to_string()));
    }
    let norm = path.replace('\\', "/");
    if norm.starts_with('/') || norm.starts_with("//") {
        return Err(TetherError::Invalid(
            "absolute paths not allowed".to_string(),
        ));
    }
    if norm.split('/').any(|seg| seg == "..") {
        return Err(TetherError::Invalid(
            "parent escape not allowed".to_string(),
        ));
    }
    if path.as_bytes().contains(&0) {
        return Err(TetherError::Invalid("nul byte in path".to_string()));
    }
    Ok(())
}

/// Request payloads: every file/exec/power request carries a client-chosen
/// request id so responses can be matched on a multiplexed session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReq {
    pub req_id: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePutReq {
    pub req_id: u64,
    pub path: String,
    pub total: u64,
}

/// One chunk of a file in either direction. `eof` on an empty payload ends
/// the transfer; the receiver acks with `FileDone`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileBlock {
    pub req_id: u64,
    pub offset: u64,
    pub data_hex: String,
    pub eof: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    /// Last-modified time, unix seconds. Zero when the platform withholds it.
    pub mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHashReq {
    pub req_id: u64,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHashResp {
    pub req_id: u64,
    pub hash_hex: String,
    pub size: u64,
    pub mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileListResp {
    pub req_id: u64,
    pub entries: Vec<DirEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDoneAck {
    pub req_id: u64,
    pub ok: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecReq {
    pub req_id: u64,
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecDone {
    pub req_id: u64,
    pub exit_code: i32,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerReq {
    pub req_id: u64,
    pub delay_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerAck {
    pub req_id: u64,
    pub ok: bool,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelOpen {
    pub channel: u32,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelBlock {
    pub channel: u32,
    pub data_hex: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelClose {
    pub channel: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelAck {
    pub channel: u32,
    pub ok: bool,
    pub error: String,
}

/// Biggest single file block on the wire (60 KiB hex-encoded stays far below
/// the 8 MiB envelope cap).
pub const FILE_BLOCK_BYTES: usize = 60 * 1024;
/// Biggest single file transfer accepted in memory (8 MiB).
pub const MAX_TRANSFER_BYTES: u64 = 8 * 1024 * 1024;
/// Biggest exec output tail returned (64 KiB).
pub const MAX_EXEC_OUTPUT: usize = 64 * 1024;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let env = Envelope::new(MessageKind::Ping, [7u8; 32], vec![1, 2, 3]);
        let bytes = env.encode().unwrap_or_default();
        assert!(!bytes.is_empty());
        let back = Envelope::decode(&bytes);
        assert!(back.is_ok());
        if let Ok(b) = back {
            assert!(b.kind == MessageKind::Ping);
            assert!(b.sender_key == [7u8; 32]);
        }
    }

    #[test]
    fn wrong_version_rejected() {
        let env = Envelope {
            version: 9,
            kind: MessageKind::Ping,
            sender_key: [0u8; 32],
            payload: Vec::new(),
        };
        let bytes = postcard::to_stdvec(&env).unwrap_or_default();
        assert!(!bytes.is_empty());
        assert!(Envelope::decode(&bytes).is_err());
    }

    #[test]
    fn trailing_bytes_rejected() {
        let env = Envelope::new(MessageKind::Pong, [1u8; 32], vec![9]);
        let mut bytes = env.encode().unwrap_or_default();
        assert!(!bytes.is_empty());
        bytes.push(0xFF);
        assert!(Envelope::decode(&bytes).is_err());
    }

    #[test]
    fn transfer_paths() {
        assert!(validate_transfer_path("docs/notes.txt").is_ok());
        assert!(validate_transfer_path("../secret.txt").is_err());
        assert!(validate_transfer_path("/etc/passwd").is_err());
        assert!(validate_transfer_path("a/../../b").is_err());
        assert!(validate_transfer_path("").is_err());
    }

    #[test]
    fn every_tier2_kind_roundtrips() {
        let kinds = [
            MessageKind::OtpProof,
            MessageKind::TouchTap,
            MessageKind::TouchHold,
            MessageKind::TouchSwipe,
            MessageKind::FrameH264,
            MessageKind::ClipboardFiles,
            MessageKind::FileList,
            MessageKind::FileGet,
            MessageKind::FilePut,
            MessageKind::FileChunk,
            MessageKind::FileDone,
            MessageKind::FileMv,
            MessageKind::FileMkdir,
            MessageKind::FileRm,
            MessageKind::FileListResp,
            MessageKind::FileBlock,
            MessageKind::ExecDone,
            MessageKind::Restart,
            MessageKind::Shutdown,
            MessageKind::Lock,
            MessageKind::PowerAck,
            MessageKind::TunnelAck,
            MessageKind::ChatMsg,
            MessageKind::NoteSet,
            MessageKind::TermOpen,
            MessageKind::TermData,
            MessageKind::TermClose,
            MessageKind::TunnelOpen,
            MessageKind::TunnelData,
            MessageKind::TunnelClose,
            MessageKind::AudioFrame,
            MessageKind::PrivacyRequest,
            MessageKind::PrivacyEnd,
            MessageKind::SyncPlan,
            MessageKind::SyncChunk,
            MessageKind::ExecRequest,
        ];
        for kind in kinds {
            let env = Envelope::new(kind, [3u8; 32], vec![7]);
            let bytes = env.encode().unwrap_or_default();
            assert!(!bytes.is_empty());
            let back = Envelope::decode(&bytes).unwrap_or(Envelope::new(
                MessageKind::Ping,
                [0u8; 32],
                Vec::new(),
            ));
            assert!(back.kind == kind);
        }
    }

    #[test]
    fn garbage_never_panics_only_errors() {
        let samples: [&[u8]; 6] = [
            b"",
            b"\x00",
            b"hello world",
            &[255u8; 64],
            &[1, 2, 3],
            &[0xde, 0xad, 0xbe, 0xef],
        ];
        for s in samples {
            // Must return Err, never panic (deny panic is enforced by the crate lints).
            assert!(Envelope::decode(s).is_err());
        }
        // Oversize frame is rejected before parsing.
        let big = vec![9u8; MAX_ENVELOPE_BYTES + 256];
        assert!(Envelope::decode(&big).is_err());
    }
}
