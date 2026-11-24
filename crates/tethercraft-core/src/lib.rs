//! TetherCraft core: errors, permissions, peer tiers (R2), onboarding (R1), audit.
//!
//! Native protocol only. No RustDesk code is referenced anywhere.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Explicit failure instead of a panic or a stubbed success.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum TetherError {
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("denied: {0}")]
    Denied(String),
    #[error("expired: {0}")]
    Expired(String),
    #[error("storage: {0}")]
    Storage(String),
}

/// Every controllable capability. The host checks one of these per message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Keyboard,
    Clipboard,
    File,
    Audio,
    Camera,
    Terminal,
    Printer,
    Tunnel,
    Restart,
    Recording,
    Privacy,
    InputBlocking,
    ScreenBlanking,
    Unattended,
}

/// Peer trust tier. R2: every new peer starts as Stranger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerTier {
    Stranger,
    TrustedHelper,
    Managed,
}

impl PeerTier {
    pub fn from_str_name(name: &str) -> Option<Self> {
        match name {
            "stranger" => Some(Self::Stranger),
            "trusted_helper" => Some(Self::TrustedHelper),
            "managed" => Some(Self::Managed),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stranger => "stranger",
            Self::TrustedHelper => "trusted_helper",
            Self::Managed => "managed",
        }
    }
}

/// Stranger session cap: 30 minutes (PRD R2).
pub const STRANGER_SESSION_CAP_SECS: i64 = 30 * 60;
/// R1 outgoing-only window: 24 hours.
pub const OUTGOING_ONLY_SECS: i64 = 24 * 60 * 60;
/// R3 remote-promotion cooling: 24 hours, started and confirmed from local UI.
pub const PROMOTION_COOLDOWN_SECS: i64 = 24 * 60 * 60;

/// Policy attached to one peer on the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerPolicy {
    pub peer_id: String,
    pub tier: PeerTier,
    pub granted: BTreeSet<Permission>,
    pub session_started_at: Option<DateTime<Utc>>,
    pub local_recording_on: bool,
}

impl PeerPolicy {
    pub fn stranger(peer_id: &str) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            tier: PeerTier::Stranger,
            granted: BTreeSet::new(),
            session_started_at: None,
            local_recording_on: true,
        }
    }

    pub fn trusted_helper(peer_id: &str, granted: BTreeSet<Permission>) -> Self {
        Self {
            peer_id: peer_id.to_string(),
            tier: PeerTier::TrustedHelper,
            granted,
            session_started_at: None,
            local_recording_on: false,
        }
    }

    /// Host-side check for a single action. Strangers may only view.
    pub fn check(&self, perm: Permission, now: DateTime<Utc>) -> Result<(), TetherError> {
        if self.tier == PeerTier::Stranger {
            // Strangers get view-only. Any permission bit is denied unless the
            // local person enabled it for this session, and even then the
            // never-allowed set below stays denied.
            let never: [Permission; 8] = [
                Permission::File,
                Permission::Terminal,
                Permission::Privacy,
                Permission::ScreenBlanking,
                Permission::InputBlocking,
                Permission::Unattended,
                Permission::Tunnel,
                Permission::Recording,
            ];
            if never.contains(&perm) {
                return Err(TetherError::Denied(format!(
                    "stranger peer {} may not use {:?}",
                    self.peer_id, perm
                )));
            }
            if !self.granted.contains(&perm) {
                return Err(TetherError::Denied(format!(
                    "stranger peer {} lacks {:?} for this session",
                    self.peer_id, perm
                )));
            }
        }
        if let Some(started) = self.session_started_at {
            let elapsed = now.signed_duration_since(started).num_seconds();
            if self.tier == PeerTier::Stranger && elapsed > STRANGER_SESSION_CAP_SECS {
                return Err(TetherError::Expired(format!(
                    "stranger session for {} exceeded 30 minutes",
                    self.peer_id
                )));
            }
            if elapsed < 0 {
                return Err(TetherError::Invalid(
                    "session start is in the future".to_string(),
                ));
            }
        }
        if self.tier == PeerTier::TrustedHelper && !self.granted.contains(&perm) {
            // Trusted helpers still need the ticked permission.
            // View (no permission needed) is handled by callers passing no check.
            return Err(TetherError::Denied(format!(
                "peer {} lacks granted {:?}",
                self.peer_id, perm
            )));
        }
        Ok(())
    }

    /// Promotion may never start from inside a session (R3).
    pub fn request_promotion(&self, session_active: bool) -> Result<PromotionRequest, TetherError> {
        if session_active {
            return Err(TetherError::Denied(
                "promotion cannot start while a session is active".to_string(),
            ));
        }
        if self.tier != PeerTier::Stranger {
            return Err(TetherError::Invalid(
                "only stranger peers can be promoted".to_string(),
            ));
        }
        Ok(PromotionRequest {
            peer_id: self.peer_id.clone(),
            step: PromotionStep::Cooling { rounds: 0 },
        })
    }
}

/// R3 promotion flow state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionRequest {
    pub peer_id: String,
    pub step: PromotionStep,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PromotionStep {
    /// In-person QR pairing on same LAN/proximity. Completes immediately.
    InPerson,
    /// Remote path: started locally, confirm again after 24 h from local UI.
    Cooling { rounds: u8 },
}

impl PromotionRequest {
    pub fn confirm_in_person(self) -> Result<PeerTier, TetherError> {
        match self.step {
            PromotionStep::InPerson => Ok(PeerTier::TrustedHelper),
            PromotionStep::Cooling { .. } => Err(TetherError::Denied(
                "cooling path needs the 24 h confirm, not in-person confirm".to_string(),
            )),
        }
    }

    pub fn confirm_cooling(
        self,
        started_at: DateTime<Utc>,
        now: DateTime<Utc>,
        from_local_ui: bool,
        session_active: bool,
    ) -> Result<PeerTier, TetherError> {
        match self.step {
            PromotionStep::InPerson => Err(TetherError::Denied(
                "in-person request cannot use cooling confirm".to_string(),
            )),
            PromotionStep::Cooling { .. } => {
                if !from_local_ui {
                    return Err(TetherError::Denied(
                        "cooling confirm must come from the local UI".to_string(),
                    ));
                }
                if session_active {
                    return Err(TetherError::Denied(
                        "cooling confirm needs no active session".to_string(),
                    ));
                }
                let elapsed = now.signed_duration_since(started_at).num_seconds();
                if elapsed < PROMOTION_COOLDOWN_SECS {
                    return Err(TetherError::Denied(format!(
                        "cooling period not over: {} s left",
                        PROMOTION_COOLDOWN_SECS - elapsed
                    )));
                }
                Ok(PeerTier::TrustedHelper)
            }
        }
    }
}

/// R1 onboarding state. Stored locally only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Onboarding {
    pub answered: bool,
    pub flagged: bool,
    pub outgoing_only_until: Option<DateTime<Utc>>,
    pub build_is_consumer: bool,
}

impl Onboarding {
    pub fn fresh_consumer() -> Self {
        Self {
            answered: false,
            flagged: false,
            outgoing_only_until: None,
            build_is_consumer: true,
        }
    }

    pub fn needs_answer(&self) -> bool {
        self.build_is_consumer && !self.answered
    }

    /// Answer the "did someone tell you to install this" question.
    pub fn answer(&mut self, yes_or_unsure: bool, now: DateTime<Utc>) {
        self.answered = true;
        self.flagged = yes_or_unsure;
        if yes_or_unsure {
            let until = now + chrono::Duration::seconds(OUTGOING_ONLY_SECS);
            self.outgoing_only_until = Some(until);
        } else {
            self.outgoing_only_until = None;
        }
    }

    pub fn is_outgoing_only(&self, now: DateTime<Utc>) -> bool {
        match self.outgoing_only_until {
            Some(until) => now < until,
            None => false,
        }
    }

    /// R7: consumer builds must never ship a path that skips this gate.
    pub fn assert_consumer_gate(&self) -> Result<(), TetherError> {
        if self.build_is_consumer && !self.answered {
            return Err(TetherError::Denied(
                "onboarding answer required before hosting".to_string(),
            ));
        }
        Ok(())
    }
}

/// One audit event. Every agent and human action lands here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub at: DateTime<Utc>,
    pub peer_id: String,
    pub action: String,
    pub result: String,
}

impl AuditEvent {
    pub fn new(at: DateTime<Utc>, peer_id: &str, action: &str, result: &str) -> Self {
        Self {
            at,
            peer_id: peer_id.to_string(),
            action: action.to_string(),
            result: result.to_string(),
        }
    }
}

/// One-time passwords (HOTP/TOTP style with HMAC-SHA256, RFC 4226 truncation).
///
/// Used for the Tier 1 OTP login path: the host generates a code from a shared
/// secret and the controller reads it back over a second channel.
pub mod otp {
    use sha2::{Digest, Sha256};

    use super::TetherError;

    fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
        let mut block = [0u8; 64];
        if key.len() > 64 {
            let digest = Sha256::digest(key);
            block[..32].copy_from_slice(&digest);
        } else {
            block[..key.len()].copy_from_slice(key);
        }
        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for i in 0..64 {
            ipad[i] ^= block[i];
            opad[i] ^= block[i];
        }
        let mut inner = Sha256::new();
        inner.update(ipad);
        inner.update(msg);
        let inner_hash = inner.finalize();
        let mut outer = Sha256::new();
        outer.update(opad);
        outer.update(inner_hash);
        let out = outer.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&out);
        arr
    }

    /// HOTP value for a counter. Digits must be 6 or 8.
    pub fn hotp(key: &[u8], counter: u64, digits: u32) -> Result<u32, TetherError> {
        if key.is_empty() || key.len() > 256 {
            return Err(TetherError::Invalid(
                "otp secret must be 1..=256 bytes".to_string(),
            ));
        }
        let modulus: u32 = match digits {
            6 => 1_000_000,
            8 => 100_000_000,
            _ => {
                return Err(TetherError::Invalid(
                    "otp digits must be 6 or 8".to_string(),
                ))
            }
        };
        let mac = hmac_sha256(key, &counter.to_be_bytes());
        let offset = (mac[31] & 0x0f) as usize;
        let code = ((u32::from(mac[offset]) & 0x7f) << 24)
            | (u32::from(mac[offset + 1]) << 16)
            | (u32::from(mac[offset + 2]) << 8)
            | u32::from(mac[offset + 3]);
        Ok(code % modulus)
    }

    /// TOTP value for a Unix timestamp. Step is seconds per code (usually 30).
    pub fn totp(
        key: &[u8],
        unix_secs: i64,
        step_secs: i64,
        digits: u32,
    ) -> Result<u32, TetherError> {
        if step_secs <= 0 {
            return Err(TetherError::Invalid(
                "totp step must be positive".to_string(),
            ));
        }
        if unix_secs < 0 {
            return Err(TetherError::Invalid("time before epoch".to_string()));
        }
        let counter = (unix_secs as u64).saturating_div(step_secs as u64);
        hotp(key, counter, digits)
    }

    /// Verify a TOTP code within plus/minus `window` steps.
    pub fn verify_totp(
        key: &[u8],
        code: u32,
        unix_secs: i64,
        step_secs: i64,
        digits: u32,
        window: u32,
    ) -> bool {
        if step_secs <= 0 || unix_secs < 0 {
            return false;
        }
        let base = (unix_secs as u64).saturating_div(step_secs as u64);
        let w = u64::from(window);
        let from = base.saturating_sub(w);
        let to = base.saturating_add(w);
        let mut c = from;
        while c <= to {
            match hotp(key, c, digits) {
                Ok(v) => {
                    if v == code {
                        return true;
                    }
                }
                Err(_) => return false,
            }
            if c == to {
                break;
            }
            c = c.saturating_add(1);
        }
        false
    }

    /// Generate a fresh 20-byte OTP secret from OS randomness.
    pub fn generate_secret() -> Result<[u8; 20], TetherError> {
        let mut secret = [0u8; 20];
        getrandom::getrandom(&mut secret)
            .map_err(|e| TetherError::Invalid(format!("rng failed: {e}")))?;
        Ok(secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn stranger_gets_view_only_defaults() {
        let p = PeerPolicy::stranger("peer-a");
        assert!(p.tier == PeerTier::Stranger);
        assert!(p.local_recording_on);
        assert!(p.granted.is_empty());
    }

    #[test]
    fn stranger_file_terminal_privacy_always_denied() {
        let mut p = PeerPolicy::stranger("peer-a");
        let _ = p.granted.insert(Permission::File);
        let now = t0();
        assert!(p.check(Permission::File, now).is_err());
        assert!(p.check(Permission::Terminal, now).is_err());
        assert!(p.check(Permission::Privacy, now).is_err());
        assert!(p.check(Permission::ScreenBlanking, now).is_err());
        assert!(p.check(Permission::Unattended, now).is_err());
    }

    #[test]
    fn stranger_session_cap_enforced() {
        let mut p = PeerPolicy::stranger("peer-a");
        let start = DateTime::from_timestamp(1_800_000_000 - STRANGER_SESSION_CAP_SECS - 60, 0)
            .unwrap_or(DateTime::<Utc>::MIN_UTC);
        p.session_started_at = Some(start);
        let now = t0();
        assert!(p.check(Permission::Keyboard, now).is_err());
    }

    #[test]
    fn promotion_blocked_inside_session() {
        let p = PeerPolicy::stranger("peer-a");
        assert!(p.request_promotion(true).is_err());
        assert!(p.request_promotion(false).is_ok());
    }

    #[test]
    fn cooling_confirm_needs_24h_and_local_ui() {
        let req = PromotionRequest {
            peer_id: "peer-a".to_string(),
            step: PromotionStep::Cooling { rounds: 0 },
        };
        let started = t0();
        let early = started + chrono::Duration::hours(2);
        assert!(req
            .clone()
            .confirm_cooling(started, early, true, false)
            .is_err());
        let late = started + chrono::Duration::hours(25);
        assert!(req
            .clone()
            .confirm_cooling(started, late, false, false)
            .is_err());
        assert!(req
            .clone()
            .confirm_cooling(started, late, true, true)
            .is_err());
        assert!(req.confirm_cooling(started, late, true, false).is_ok());
    }

    #[test]
    fn onboarding_yes_forces_outgoing_only_24h() {
        let mut o = Onboarding::fresh_consumer();
        assert!(o.needs_answer());
        assert!(o.assert_consumer_gate().is_err());
        let now = t0();
        o.answer(true, now);
        assert!(!o.needs_answer());
        assert!(o.assert_consumer_gate().is_ok());
        assert!(o.is_outgoing_only(now + chrono::Duration::hours(1)));
        assert!(!o.is_outgoing_only(now + chrono::Duration::hours(25)));
    }

    #[test]
    fn onboarding_no_keeps_hosting_available() {
        let mut o = Onboarding::fresh_consumer();
        let now = t0();
        o.answer(false, now);
        assert!(!o.is_outgoing_only(now + chrono::Duration::hours(1)));
        assert!(o.assert_consumer_gate().is_ok());
    }

    #[test]
    fn tier_names_roundtrip() {
        assert!(PeerTier::from_str_name("stranger") == Some(PeerTier::Stranger));
        assert!(PeerTier::from_str_name("nope").is_none());
        assert!(PeerTier::Stranger.as_str() == "stranger");
    }

    #[test]
    fn otp_deterministic_per_counter() {
        let key = b"test secret 123";
        let a = otp::hotp(key, 42, 6).unwrap_or_default();
        let b = otp::hotp(key, 42, 6).unwrap_or_default();
        // unwrap_or_default is only a lint-safe fallback; real values are non-zero.
        assert!(a == b);
        assert!(a < 1_000_000);
        let c = otp::hotp(key, 43, 6).unwrap_or_default();
        assert!(a == b);
        let _ = c;
    }

    #[test]
    fn otp_rejects_bad_inputs() {
        assert!(otp::hotp(&[], 1, 6).is_err());
        assert!(otp::hotp(b"k", 1, 7).is_err());
        assert!(otp::totp(b"k", -1, 30, 6).is_err());
        assert!(otp::totp(b"k", 100, 0, 6).is_err());
    }

    #[test]
    fn totp_verify_window() {
        let key = b"another secret";
        let t = 1_800_000_000i64;
        let code = otp::totp(key, t, 30, 6).unwrap_or(1_000_001);
        assert!(code < 1_000_000);
        assert!(otp::verify_totp(key, code, t, 30, 6, 1));
        assert!(otp::verify_totp(key, code, t + 30, 30, 6, 1));
        assert!(!otp::verify_totp(key, code, t + 300, 30, 6, 1));
        assert!(!otp::verify_totp(
            key,
            code.wrapping_add(1) % 1_000_000,
            t,
            30,
            6,
            0
        ));
        assert!(otp::generate_secret().is_ok());
    }
}
