//! R3 trusted-helper promotion: in-person token and 24 h cooling path.
//!
//! Promotion is impossible from inside a session and impossible from the
//! controller side. Both paths complete in the local UI only.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{PeerTier, TetherError, PROMOTION_COOLDOWN_SECS};

/// In-person pairing token shown on the host as hex for read-out or typing.
/// Valid 15 minutes, single use is enforced by the caller rotating the token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingToken {
    pub code: String,
    pub issued_at: DateTime<Utc>,
    pub host_fingerprint: String,
}

/// Token lifetime: 15 minutes.
pub const TOKEN_VALID_SECS: i64 = 15 * 60;

pub fn issue_pairing_token(
    host_fingerprint: &str,
    now: DateTime<Utc>,
) -> Result<PairingToken, TetherError> {
    if host_fingerprint.is_empty() {
        return Err(TetherError::Invalid(
            "host fingerprint required".to_string(),
        ));
    }
    let mut raw = [0u8; 16];
    getrandom::getrandom(&mut raw).map_err(|e| TetherError::Invalid(format!("rng failed: {e}")))?;
    Ok(PairingToken {
        code: hex::encode(raw),
        issued_at: now,
        host_fingerprint: host_fingerprint.to_string(),
    })
}

/// Claim the token from the local UI. `same_room_attested` is the local person
/// confirming the helper is present (QR scanned on the same LAN/proximity).
pub fn claim_in_person(
    token: &PairingToken,
    presented: &str,
    same_room_attested: bool,
    now: DateTime<Utc>,
) -> Result<PeerTier, TetherError> {
    if !same_room_attested {
        return Err(TetherError::Denied(
            "in-person claim needs same-room confirmation".to_string(),
        ));
    }
    if now.signed_duration_since(token.issued_at).num_seconds() > TOKEN_VALID_SECS {
        return Err(TetherError::Expired("pairing token expired".to_string()));
    }
    if presented != token.code {
        return Err(TetherError::Denied(
            "pairing code does not match".to_string(),
        ));
    }
    Ok(PeerTier::TrustedHelper)
}

/// Remote promotion path: started locally with no session active, confirmed
/// again after 24 h from the local UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoolingPromotion {
    pub peer_id: String,
    pub started_at: DateTime<Utc>,
}

pub fn start_cooling(peer_id: &str, now: DateTime<Utc>) -> Result<CoolingPromotion, TetherError> {
    if peer_id.is_empty() {
        return Err(TetherError::Invalid("peer id required".to_string()));
    }
    Ok(CoolingPromotion {
        peer_id: peer_id.to_string(),
        started_at: now,
    })
}

pub fn confirm_cooling(
    state: &CoolingPromotion,
    now: DateTime<Utc>,
    from_local_ui: bool,
    session_active: bool,
) -> Result<PeerTier, TetherError> {
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
    let elapsed = now.signed_duration_since(state.started_at).num_seconds();
    if elapsed < PROMOTION_COOLDOWN_SECS {
        return Err(TetherError::Denied(format!(
            "cooling period not over: {left} s left",
            left = PROMOTION_COOLDOWN_SECS - elapsed
        )));
    }
    Ok(PeerTier::TrustedHelper)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn token_roundtrip_and_expiry() {
        let now = t0();
        let tok = issue_pairing_token("fp123", now).unwrap_or(PairingToken {
            code: String::new(),
            issued_at: now,
            host_fingerprint: String::new(),
        });
        assert!(!tok.code.is_empty());
        assert!(issue_pairing_token("", now).is_err());
        assert!(claim_in_person(&tok, &tok.code.clone(), true, now).is_ok());
        assert!(claim_in_person(&tok, "wrong", true, now).is_err());
        assert!(claim_in_person(&tok, &tok.code.clone(), false, now).is_err());
        let late = now + chrono::Duration::minutes(16);
        assert!(claim_in_person(&tok, &tok.code.clone(), true, late).is_err());
    }

    #[test]
    fn cooling_path_rules() {
        let now = t0();
        let c = start_cooling("p1", now).unwrap_or(CoolingPromotion {
            peer_id: String::new(),
            started_at: now,
        });
        assert!(start_cooling("", now).is_err());
        assert!(confirm_cooling(&c, now, true, false).is_err());
        let late = now + chrono::Duration::hours(25);
        assert!(confirm_cooling(&c, late, false, false).is_err());
        assert!(confirm_cooling(&c, late, true, true).is_err());
        assert!(confirm_cooling(&c, late, true, false).is_ok());
    }
}
