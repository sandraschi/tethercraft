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
    SessionStart,
    SessionEnd,
    Frame,
    InputEvent,
    ClipboardText,
    ClipboardImage,
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
}
