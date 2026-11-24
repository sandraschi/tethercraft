//! Session recording (Tier 4): `.tetherrec` files anyone can read back.
//!
//! Container of our own design: magic + version, then timestamped payload
//! records (encoder output bytes, e.g. VP9). Both the local controller side
//! and the host side write the same format; the host shows recording state in
//! the session so nothing records silently. Playback validates everything.

use tethercraft_core::TetherError;

const MAGIC: &[u8; 6] = b"TREC1\n";
const FORMAT_VERSION: u8 = 1;
/// Refuse absurd files before allocating (512 MiB of payload).
const MAX_TOTAL_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub timestamp_ms: u64,
    pub payload: Vec<u8>,
}

pub struct Recorder {
    buf: Vec<u8>,
    pub frames: u64,
}

impl Recorder {
    pub fn new() -> Self {
        let mut buf = Vec::with_capacity(1024);
        buf.extend_from_slice(MAGIC);
        buf.push(FORMAT_VERSION);
        Self { buf, frames: 0 }
    }

    pub fn push(&mut self, timestamp_ms: u64, payload: &[u8]) -> Result<(), TetherError> {
        if payload.is_empty() || payload.len() > 64 * 1024 * 1024 {
            return Err(TetherError::Invalid(
                "frame must be 1 byte..=64 MiB".to_string(),
            ));
        }
        if self.buf.len().saturating_add(payload.len()) > MAX_TOTAL_BYTES {
            return Err(TetherError::Invalid("recording too large".to_string()));
        }
        self.buf.extend_from_slice(&timestamp_ms.to_le_bytes());
        let len = payload.len() as u32;
        self.buf.extend_from_slice(&len.to_le_bytes());
        self.buf.extend_from_slice(payload);
        self.frames = self.frames.saturating_add(1);
        Ok(())
    }

    pub fn finish(self) -> Vec<u8> {
        self.buf
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

/// Read a recording back. Timestamps must be non-decreasing; anything else is
/// corruption, not a quirk we smooth over.
pub fn play_back(bytes: &[u8]) -> Result<Vec<Record>, TetherError> {
    if bytes.len() < MAGIC.len() + 1 {
        return Err(TetherError::Invalid(
            "too short for a recording".to_string(),
        ));
    }
    if &bytes[..MAGIC.len()] != MAGIC {
        return Err(TetherError::Invalid("bad recording magic".to_string()));
    }
    if bytes[MAGIC.len()] != FORMAT_VERSION {
        return Err(TetherError::Invalid(
            "unsupported recording version".to_string(),
        ));
    }
    let mut out = Vec::new();
    let mut pos = MAGIC.len() + 1;
    let mut last_ts = 0u64;
    let mut first = true;
    while pos < bytes.len() {
        if bytes.len() - pos < 12 {
            return Err(TetherError::Invalid("truncated record header".to_string()));
        }
        let mut ts = [0u8; 8];
        ts.copy_from_slice(&bytes[pos..pos + 8]);
        let ts = u64::from_le_bytes(ts);
        let mut ln = [0u8; 4];
        ln.copy_from_slice(&bytes[pos + 8..pos + 12]);
        let len = u32::from_le_bytes(ln) as usize;
        pos += 12;
        if len == 0 || len > 64 * 1024 * 1024 || bytes.len() - pos < len {
            return Err(TetherError::Invalid("truncated record payload".to_string()));
        }
        if !first && ts < last_ts {
            return Err(TetherError::Invalid("timestamps go backwards".to_string()));
        }
        first = false;
        last_ts = ts;
        out.push(Record {
            timestamp_ms: ts,
            payload: bytes[pos..pos + len].to_vec(),
        });
        pos += len;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_ordered() {
        let mut r = Recorder::new();
        assert!(r.push(100, b"frame1").is_ok());
        assert!(r.push(140, b"frame2").is_ok());
        assert!(r.push(140, b"frame3").is_ok());
        let bytes = r.finish();
        let back = play_back(&bytes).unwrap_or_default();
        assert!(back.len() == 3);
        assert!(back[0].payload == b"frame1");
        assert!(back[2].timestamp_ms == 140);
        assert!(r_empty_rejected());
    }

    fn r_empty_rejected() -> bool {
        let mut r = Recorder::new();
        r.push(0, b"").is_err()
    }

    #[test]
    fn corruption_rejected() {
        assert!(play_back(b"").is_err());
        assert!(play_back(b"NOPE1234567890").is_err());
        let mut r = Recorder::new();
        let _ = r.push(5, b"x");
        let mut bytes = r.finish();
        // Bad magic.
        bytes[0] = b'X';
        assert!(play_back(&bytes).is_err());
    }

    #[test]
    fn truncation_and_reorder_rejected() {
        let mut r = Recorder::new();
        let _ = r.push(10, b"abcdef");
        let mut bytes = r.finish();
        bytes.truncate(bytes.len() - 2);
        assert!(play_back(&bytes).is_err());
        // Backwards timestamps.
        let mut r2 = Recorder::new();
        let _ = r2.push(50, b"a");
        let _ = r2.push(40, b"b");
        assert!(play_back(&r2.finish()).is_err());
    }
}
