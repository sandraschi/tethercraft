//! Camera view of the remote host (Tier 4).
//!
//! Session state plus raw-frame validation are real. Opening the OS camera
//! device is platform work and returns `NotImplemented`.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

/// Raw RGB24 frames, capped at 4K and 64 MiB.
pub const MAX_DIM: u32 = 4096;
pub const MAX_FRAME_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CameraSpec {
    pub width: u32,
    pub height: u32,
    pub fps: u8,
}

pub fn validate_frame(spec: &CameraSpec, bytes: &[u8]) -> Result<(), TetherError> {
    if spec.width == 0 || spec.height == 0 || spec.width > MAX_DIM || spec.height > MAX_DIM {
        return Err(TetherError::Invalid("bad camera dimensions".to_string()));
    }
    if spec.fps == 0 || spec.fps > 60 {
        return Err(TetherError::Invalid(
            "camera fps must be 1..=60".to_string(),
        ));
    }
    let need = spec.width as usize * spec.height as usize * 3;
    if bytes.len() != need || bytes.len() > MAX_FRAME_BYTES {
        return Err(TetherError::Invalid(
            "camera frame size mismatch".to_string(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraSession {
    pub peer_id: String,
    pub spec: CameraSpec,
    pub active: bool,
}

impl CameraSession {
    /// Open a camera view. The caller must hold `Permission::Camera`.
    pub fn open(peer_id: &str, spec: CameraSpec) -> Result<Self, TetherError> {
        if peer_id.is_empty() {
            return Err(TetherError::Invalid("peer id required".to_string()));
        }
        if spec.width == 0 || spec.fps == 0 {
            return Err(TetherError::Invalid("bad camera spec".to_string()));
        }
        Ok(Self {
            peer_id: peer_id.to_string(),
            spec,
            active: true,
        })
    }

    /// Grab one frame from the device. Pending OS camera wiring.
    pub fn next_frame(&self) -> Result<Vec<u8>, TetherError> {
        Err(TetherError::NotImplemented(
            "OS camera capture ships with the Tier 4 host service".to_string(),
        ))
    }

    pub fn close(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> CameraSpec {
        CameraSpec {
            width: 4,
            height: 2,
            fps: 30,
        }
    }

    #[test]
    fn validation() {
        assert!(validate_frame(&spec(), &[0u8; 24]).is_ok());
        assert!(validate_frame(&spec(), &[0u8; 23]).is_err());
        let bad = CameraSpec {
            width: 0,
            height: 2,
            fps: 30,
        };
        assert!(validate_frame(&bad, &[]).is_err());
        let bad_fps = CameraSpec {
            width: 4,
            height: 2,
            fps: 61,
        };
        assert!(validate_frame(&bad_fps, &[0u8; 24]).is_err());
    }

    #[test]
    fn session_lifecycle() {
        assert!(CameraSession::open("", spec()).is_err());
        let mut s = CameraSession::open("p1", spec()).unwrap_or(CameraSession {
            peer_id: String::new(),
            spec: spec(),
            active: false,
        });
        assert!(s.active);
        assert!(s.next_frame().is_err());
        s.close();
        assert!(!s.active);
    }
}
