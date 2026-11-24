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

/// A physical or virtual monitor. Enumeration needs the OS; until then the
/// controller works with an explicit id and the host validates it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Monitor {
    pub id: u32,
    pub name: String,
    pub primary: bool,
    pub width: u32,
    pub height: u32,
}

/// Two synthetic monitors for tests and headless CI.
pub fn fake_monitors() -> Vec<Monitor> {
    vec![
        Monitor {
            id: 0,
            name: "Fake-0".to_string(),
            primary: true,
            width: 1920,
            height: 1080,
        },
        Monitor {
            id: 1,
            name: "Fake-1".to_string(),
            primary: false,
            width: 1280,
            height: 720,
        },
    ]
}

/// Enumerate real monitors. Pending OS enumeration on every platform.
pub fn list_monitors() -> Result<Vec<Monitor>, TetherError> {
    Err(TetherError::NotImplemented(
        "monitor enumeration ships with the Tier 4 capture path".to_string(),
    ))
}

/// Pick a monitor by id, or the primary when `None`.
pub fn pick_monitor(monitors: &[Monitor], id: Option<u32>) -> Result<&Monitor, TetherError> {
    match id {
        Some(want) => monitors
            .iter()
            .find(|m| m.id == want)
            .ok_or_else(|| TetherError::Invalid("unknown monitor".to_string())),
        None => monitors
            .iter()
            .find(|m| m.primary)
            .ok_or_else(|| TetherError::Invalid("no primary monitor".to_string())),
    }
}

/// Headless virtual display request (Tier 4). The config is real; attaching an
/// OS virtual display driver (IddSample-class on Windows) is pending.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualDisplayConfig {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u8,
}

pub fn request_virtual_display(cfg: &VirtualDisplayConfig) -> Result<(), TetherError> {
    if cfg.width == 0 || cfg.height == 0 || cfg.width > 7680 || cfg.height > 4320 {
        return Err(TetherError::Invalid(
            "virtual display size out of range".to_string(),
        ));
    }
    if cfg.refresh_hz == 0 || cfg.refresh_hz > 240 {
        return Err(TetherError::Invalid(
            "refresh must be 1..=240 Hz".to_string(),
        ));
    }
    Err(TetherError::NotImplemented(
        "virtual display driver ships with Tier 4".to_string(),
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

    #[test]
    fn monitor_pick() {
        let mons = fake_monitors();
        assert!(matches!(pick_monitor(&mons, None), Ok(m) if m.id == 0));
        assert!(matches!(pick_monitor(&mons, Some(1)), Ok(m) if m.width == 1280));
        assert!(pick_monitor(&mons, Some(9)).is_err());
        assert!(pick_monitor(&[], None).is_err());
        assert!(list_monitors().is_err());
        let bad = VirtualDisplayConfig {
            width: 0,
            height: 1080,
            refresh_hz: 60,
        };
        assert!(request_virtual_display(&bad).is_err());
        let ok = VirtualDisplayConfig {
            width: 1920,
            height: 1080,
            refresh_hz: 60,
        };
        // Driver pending: validation passes, attach reports NotImplemented.
        assert!(matches!(
            request_virtual_display(&ok),
            Err(tethercraft_core::TetherError::NotImplemented(_))
        ));
    }
}
