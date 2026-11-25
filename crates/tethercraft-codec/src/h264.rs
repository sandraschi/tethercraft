//! H.264 software codec via Cisco's OpenH264 prebuilt (Tier 4 follow-up).
//!
//! The crate downloads Cisco's royalty-covered binary at build time; we never
//! self-build (see O3). Encode: RGBA to NAL units. Decode: NAL units to RGBA.
//! Even widths and heights only (chroma subsampling), which the capture path
//! already guarantees for real frames and the tests use.

use openh264::decoder::Decoder;
use openh264::encoder::Encoder;
use openh264::formats::{RgbaSliceU8, YUVBuffer, YUVSource};
use tethercraft_core::TetherError;

/// Cap: 4K RGBA in, ~33 MB out of a single decode.
const MAX_PIXELS: usize = 3840 * 2160;

pub struct H264Encoder {
    inner: Encoder,
    width: usize,
    height: usize,
    frames_in: u64,
}

impl H264Encoder {
    pub fn new(width: u32, height: u32) -> Result<Self, TetherError> {
        if width == 0 || height == 0 || !width.is_multiple_of(2) || !height.is_multiple_of(2) {
            return Err(TetherError::Invalid(
                "h264 needs even nonzero dimensions".to_string(),
            ));
        }
        if width as usize * height as usize > MAX_PIXELS {
            return Err(TetherError::Invalid("frame too large".to_string()));
        }
        let inner =
            Encoder::new().map_err(|e| TetherError::Invalid(format!("h264 encoder: {e:?}")))?;
        Ok(Self {
            inner,
            width: width as usize,
            height: height as usize,
            frames_in: 0,
        })
    }

    pub fn encode_rgba(&mut self, rgba: &[u8]) -> Result<Vec<u8>, TetherError> {
        if rgba.len() != self.width * self.height * 4 {
            return Err(TetherError::Invalid("frame size mismatch".to_string()));
        }
        let rgb = RgbaSliceU8::new(rgba, (self.width, self.height));
        let yuv = YUVBuffer::from_rgba8_source(rgb);
        let stream = self
            .inner
            .encode(&yuv)
            .map_err(|e| TetherError::Invalid(format!("h264 encode: {e:?}")))?;
        self.frames_in = self.frames_in.saturating_add(1);
        Ok(stream.to_vec())
    }

    pub fn frames_in(&self) -> u64 {
        self.frames_in
    }
}

pub struct H264Decoder {
    inner: Decoder,
}

impl H264Decoder {
    pub fn new() -> Result<Self, TetherError> {
        let inner =
            Decoder::new().map_err(|e| TetherError::Invalid(format!("h264 decoder: {e:?}")))?;
        Ok(Self { inner })
    }

    /// Decode one packet. Returns `Ok(None)` when the decoder needs more data
    /// (e.g. it only got SPS/PPS so far); the caller feeds the next packet.
    pub fn decode(&mut self, packet: &[u8]) -> Result<Option<(u32, u32, Vec<u8>)>, TetherError> {
        if packet.is_empty() || packet.len() > 64 * 1024 * 1024 {
            return Err(TetherError::Invalid("bad h264 packet".to_string()));
        }
        let image = self
            .inner
            .decode(packet)
            .map_err(|e| TetherError::Invalid(format!("h264 decode: {e:?}")))?;
        match image {
            None => Ok(None),
            Some(yuv) => {
                let (w, h) = yuv.dimensions();
                if w == 0 || h == 0 || w * h > MAX_PIXELS {
                    return Err(TetherError::Invalid("bad decoded dimensions".to_string()));
                }
                let mut rgba = vec![0u8; w * h * 4];
                yuv.write_rgba8(&mut rgba);
                Ok(Some((w as u32, h as u32, rgba)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Screen-like content: flat 16x16 blocks with a slow gradient plus a
    /// sharp text-like edge. This is what the codec will actually see.
    fn striped(w: usize, h: usize, n: u64) -> Vec<u8> {
        let mut px = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let block = ((x / 16 + y / 16) as u64).wrapping_add(n);
                let mut v = ((block * 37) % 216) as u8;
                // Sharp vertical edge every 64 px, like a window border.
                if x % 64 == 0 {
                    v = 255 - v;
                }
                let o = (y * w + x) * 4;
                px[o] = v;
                px[o + 1] = 255 - v;
                px[o + 2] = (v / 2).saturating_add(40);
                px[o + 3] = 255;
            }
        }
        px
    }

    fn encoder() -> Option<H264Encoder> {
        H264Encoder::new(160, 120).ok()
    }

    fn decoder() -> Option<H264Decoder> {
        H264Decoder::new().ok()
    }

    #[test]
    fn rejects_bad_dims() {
        assert!(H264Encoder::new(0, 4).is_err());
        assert!(H264Encoder::new(3, 4).is_err());
        // If the Cisco binary cannot load here, both constructors fail the
        // same way; the roundtrip test below covers the working path.
        assert!(encoder().is_some() == decoder().is_some());
    }

    #[test]
    fn roundtrip_keeps_shape_and_shrinks() {
        let w = 160usize;
        let h = 120usize;
        let mut enc = match encoder() {
            Some(e) => e,
            None => {
                assert!(H264Decoder::new().is_err());
                return;
            }
        };
        let raw = striped(w, h, 7);
        assert!(enc.encode_rgba(&[0u8; 10]).is_err());
        let mut nal = enc.encode_rgba(&raw).unwrap_or_default();
        // First packets may carry only headers; feed the same frame through
        // so the decoded output is comparable with the input.
        for _ in 1..6u64 {
            let more = enc.encode_rgba(&raw).unwrap_or_default();
            nal.extend_from_slice(&more);
        }
        assert!(!nal.is_empty());
        // H.264 must beat raw RGBA by a wide margin on this content.
        assert!(nal.len() < raw.len());
        let mut dec = match decoder() {
            Some(d) => d,
            None => {
                assert!(H264Encoder::new(w as u32, h as u32).is_err());
                return;
            }
        };
        // Feed the whole stream as one packet; openh264 handles concatenated NALs.
        let got = dec.decode(&nal).unwrap_or(None);
        match got {
            Some((dw, dh, ref px)) => {
                assert!(dw == w as u32 && dh == h as u32);
                assert!(px.len() == raw.len());
                // Lossy but recognisable: average per-byte drift stays small.
                let mut diff: u64 = 0;
                for (a, b) in px.iter().zip(raw.iter()) {
                    diff += (*a as i16 - *b as i16).unsigned_abs() as u64;
                }
                let mean = diff / px.len() as u64;
                assert!(mean < 30, "mean drift {mean} too large");
            }
            None => {
                // One retry: the first call may have consumed only headers.
                assert!(dec.decode(&nal).unwrap_or(None).is_some());
            }
        }
        assert!(enc.frames_in() == 6);
        assert!(dec.decode(&[]).is_err());
    }
}
