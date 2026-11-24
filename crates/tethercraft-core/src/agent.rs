//! Agent sessions (PRD section 7): visible, view-only by default, audited,
//! rate-limited, with a host-side kill switch.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::TetherError;

/// Policy for one agent key. Input stays off until the local person allows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPolicy {
    pub allow_input: bool,
    pub max_actions_per_min: u32,
}

impl Default for AgentPolicy {
    fn default() -> Self {
        Self {
            allow_input: false,
            max_actions_per_min: 60,
        }
    }
}

/// Sliding-window rate check over recent action timestamps.
pub fn check_rate(
    history: &[DateTime<Utc>],
    now: DateTime<Utc>,
    max_per_min: u32,
) -> Result<(), TetherError> {
    let cutoff = now - chrono::Duration::minutes(1);
    let mut recent = 0u32;
    for t in history {
        if *t >= cutoff && *t <= now {
            recent = recent.saturating_add(1);
        }
    }
    if recent >= max_per_min {
        return Err(TetherError::Denied("agent rate limit hit".to_string()));
    }
    Ok(())
}

/// Host-side kill switch: revoking ends all agent sessions at once.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KillSwitch {
    pub revoked: bool,
}

impl KillSwitch {
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    pub fn check(&self) -> Result<(), TetherError> {
        if self.revoked {
            return Err(TetherError::Denied("agent sessions revoked".to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_view_only() {
        let p = AgentPolicy::default();
        assert!(!p.allow_input);
        assert!(p.max_actions_per_min == 60);
    }

    #[test]
    fn rate_and_kill_switch() {
        let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap_or(DateTime::<Utc>::MIN_UTC);
        assert!(check_rate(&[], now, 2).is_ok());
        let hist = [now, now];
        assert!(check_rate(&hist, now, 2).is_err());
        assert!(check_rate(&hist, now, 3).is_ok());
        let mut ks = KillSwitch::default();
        assert!(ks.check().is_ok());
        ks.revoke();
        assert!(ks.check().is_err());
    }
}
