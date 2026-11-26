//! Video codecs: one hardware encoder plus VP9 software fallback (interface).

#[cfg(feature = "h264")]
pub mod h264;
pub mod rec;

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EncoderKind {
    HardwareH264,
    Vp9Software,
}

pub fn vp9_available() -> bool {
    true
}

#[cfg(windows)]
pub fn hardware_available() -> bool {
    false
}

#[cfg(not(windows))]
pub fn hardware_available() -> bool {
    false
}

pub fn pick_encoder() -> EncoderKind {
    if hardware_available() {
        EncoderKind::HardwareH264
    } else {
        EncoderKind::Vp9Software
    }
}

/// Convert decoded RGBA into an egui image for the controller view.
pub fn to_egui_image(
    width: usize,
    height: usize,
    rgba: &[u8],
) -> Result<egui::ColorImage, TetherError> {
    if width == 0 || height == 0 {
        return Err(TetherError::Invalid("zero-size frame".to_string()));
    }
    let need = width.saturating_mul(height).saturating_mul(4);
    if rgba.len() < need {
        return Err(TetherError::Invalid("short frame buffer".to_string()));
    }
    let mut px = Vec::with_capacity(width.saturating_mul(height));
    let mut i = 0;
    while i + 4 <= need {
        px.push(egui::Color32::from_rgba_unmultiplied(
            rgba[i],
            rgba[i + 1],
            rgba[i + 2],
            rgba[i + 3],
        ));
        i += 4;
    }
    Ok(egui::ColorImage::new([width, height], px))
}

/// Real encode lands with GPU wiring; the fake path keeps tests honest.
pub fn encode_fake(frame_no: u64, bytes: &[u8]) -> Result<Vec<u8>, TetherError> {
    if bytes.is_empty() {
        return Err(TetherError::Invalid("empty frame".to_string()));
    }
    let mut out = Vec::with_capacity(bytes.len() + 8);
    out.extend_from_slice(&frame_no.to_le_bytes());
    out.extend_from_slice(bytes);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_vp9_until_gpu_lands() {
        assert!(pick_encoder() == EncoderKind::Vp9Software);
        assert!(vp9_available());
    }

    #[test]
    fn egui_image_conversion() {
        let rgba = vec![255u8; 4 * 2 * 2];
        let img = to_egui_image(2, 2, &rgba);
        assert!(img.is_ok());
        if let Ok(im) = img {
            assert!(im.width() == 2);
            assert!(im.height() == 2);
            assert!(im.pixels.len() == 4);
        }
        assert!(to_egui_image(0, 2, &rgba).is_err());
        assert!(to_egui_image(2, 2, &[]).is_err());
    }

    #[test]
    fn fake_encode_roundtrip_marker() {
        let b = vec![1u8, 2, 3];
        let out = encode_fake(7, &b).unwrap_or_default();
        assert!(out.len() == 11);
        assert!(encode_fake(1, &[]).is_err());
    }
}
