//! Screen capture backends. Windows: WGC with DXGI fallback. Elsewhere: NotImplemented.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Backend {
    Wgc,
    Dxgi,
    Fake,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameMeta {
    pub width: u32,
    pub height: u32,
    pub backend: Backend,
    pub frame_no: u64,
}

/// Pick the capture backend for this machine.
pub fn select_backend() -> Result<Backend, TetherError> {
    #[cfg(windows)]
    {
        Ok(Backend::Wgc)
    }
    #[cfg(not(windows))]
    {
        Err(TetherError::NotImplemented(
            "capture is Windows-only in Tier 1".to_string(),
        ))
    }
}

/// Synthetic frame for tests and headless CI (never shipped as a real screen).
pub fn fake_frame(width: u32, height: u32, frame_no: u64) -> (FrameMeta, Vec<u8>) {
    let meta = FrameMeta {
        width,
        height,
        backend: Backend::Fake,
        frame_no,
    };
    let n = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let mut px = Vec::with_capacity(n.min(1024 * 1024));
    let cap = n.min(1024 * 1024);
    for i in 0..cap {
        let v = ((i as u64).wrapping_add(frame_no) % 251) as u8;
        px.push(v);
    }
    (meta, px)
}

/// Live capture entry point. Real WGC wiring lands with the two-machine run.
pub fn capture_one(_backend: Backend) -> Result<(FrameMeta, Vec<u8>), TetherError> {
    Err(TetherError::NotImplemented(
        "live capture needs the Windows WGC run (Tier 1 exit)".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_frame_sizes() {
        let (m, px) = fake_frame(16, 16, 1);
        assert!(m.width == 16);
        assert!(px.len() == 16 * 16 * 4);
        let (_, px2) = fake_frame(16, 16, 2);
        assert!(px != px2);
    }

    #[test]
    fn live_capture_is_explicit_not_implemented() {
        assert!(capture_one(Backend::Fake).is_err());
    }
}
