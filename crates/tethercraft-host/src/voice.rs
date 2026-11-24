//! Voice call state plus audio frame validation (Tier 4).
//!
//! The call state machine (ring, accept locally, mute, hangup) and the
//! bounds checks on PCM frames are real. Encoding and duplex transport ride
//! the `AudioFrame` channel once the Tier 4 audio path lands.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

/// Accepted sample rates and channel counts; 16-bit PCM; at most one second
/// per frame (48 kHz stereo = 192000 bytes).
pub const RATES: [u32; 4] = [8000, 16000, 24000, 48000];
pub const MAX_AUDIO_BYTES: usize = 192000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallState {
    Idle,
    Ringing,
    Active,
    Ended,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceCall {
    pub peer_id: String,
    pub state: CallState,
    pub muted: bool,
}

impl VoiceCall {
    pub fn ring(peer_id: &str) -> Result<Self, TetherError> {
        if peer_id.is_empty() {
            return Err(TetherError::Invalid("peer id required".to_string()));
        }
        Ok(Self {
            peer_id: peer_id.to_string(),
            state: CallState::Ringing,
            muted: false,
        })
    }

    /// Only the local person answers. The controller can never auto-answer.
    pub fn accept_local(&mut self) -> Result<(), TetherError> {
        if self.state != CallState::Ringing {
            return Err(TetherError::Invalid(
                "no incoming call to accept".to_string(),
            ));
        }
        self.state = CallState::Active;
        Ok(())
    }

    pub fn set_muted(&mut self, muted: bool) -> Result<(), TetherError> {
        if self.state != CallState::Active {
            return Err(TetherError::Invalid("no active call".to_string()));
        }
        self.muted = muted;
        Ok(())
    }

    pub fn hangup(&mut self) {
        self.state = CallState::Ended;
        self.muted = false;
    }
}

pub fn validate_audio(rate: u32, channels: u8, bytes: &[u8]) -> Result<(), TetherError> {
    if !RATES.contains(&rate) {
        return Err(TetherError::Invalid(
            "audio rate must be 8/16/24/48 kHz".to_string(),
        ));
    }
    if channels == 0 || channels > 2 {
        return Err(TetherError::Invalid(
            "audio channels must be 1 or 2".to_string(),
        ));
    }
    if bytes.is_empty() || bytes.len() > MAX_AUDIO_BYTES {
        return Err(TetherError::Invalid(
            "audio frame must be 1 byte..=192000".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_flow() {
        assert!(VoiceCall::ring("").is_err());
        let mut c = VoiceCall::ring("p1").unwrap_or(VoiceCall {
            peer_id: String::new(),
            state: CallState::Idle,
            muted: false,
        });
        assert!(c.set_muted(true).is_err());
        assert!(c.accept_local().is_ok());
        assert!(c.set_muted(true).is_ok());
        assert!(c.muted);
        c.hangup();
        assert!(c.state == CallState::Ended);
        assert!(c.accept_local().is_err());
    }

    #[test]
    fn audio_bounds() {
        assert!(validate_audio(48000, 2, &[0u8; 1920]).is_ok());
        assert!(validate_audio(44100, 2, &[0u8; 100]).is_err());
        assert!(validate_audio(48000, 3, &[0u8; 100]).is_err());
        assert!(validate_audio(48000, 1, &[]).is_err());
        assert!(validate_audio(8000, 1, &[0u8; MAX_AUDIO_BYTES + 1]).is_err());
    }
}
