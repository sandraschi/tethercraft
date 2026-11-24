//! Host enforcement for agent sessions (PRD section 7, R9).
//!
//! The host shows a distinct "agent connected" indicator, defaults the agent
//! to view-only, audits every action with the tool call that caused it, rate
//! limits input and exec, and offers a kill switch ending all agent sessions.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tethercraft_core::agent::{check_rate, AgentPolicy, KillSwitch};
use tethercraft_core::{PeerPolicy, Permission, TetherError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostAgentSession {
    pub agent_name: String,
    pub policy: AgentPolicy,
    pub started_at: DateTime<Utc>,
    pub history: Vec<DateTime<Utc>>,
    pub action_count: u64,
}

impl HostAgentSession {
    pub fn new(agent_name: &str, policy: AgentPolicy, now: DateTime<Utc>) -> Self {
        Self {
            agent_name: agent_name.to_string(),
            policy,
            started_at: now,
            history: Vec::new(),
            action_count: 0,
        }
    }

    /// Check one agent action: kill switch, rate limit, then the host policy.
    /// View-only default means input needs `policy.allow_input` plus the peer
    /// holding the matching granted permission on the host.
    pub fn check_action(
        &mut self,
        kill: &KillSwitch,
        peer: &PeerPolicy,
        perm: Option<Permission>,
        tool_call: &str,
        now: DateTime<Utc>,
    ) -> Result<(), TetherError> {
        if tool_call.is_empty() {
            return Err(TetherError::Invalid(
                "tool call name required for audit".to_string(),
            ));
        }
        kill.check()?;
        check_rate(&self.history, now, self.policy.max_actions_per_min)?;
        if let Some(p) = perm {
            if !self.policy.allow_input {
                return Err(TetherError::Denied(format!(
                    "agent {n} is view-only",
                    n = self.agent_name
                )));
            }
            peer.check(p, now)?;
        }
        self.history.push(now);
        // Keep the window bounded; old entries fall out of the 1-minute check.
        if self.history.len() > 1024 {
            let excess = self.history.len() - 1024;
            self.history.drain(..excess);
        }
        self.action_count = self.action_count.saturating_add(1);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tethercraft_core::PeerTier;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn agent_view_only_by_default() {
        let mut s = HostAgentSession::new("helper", AgentPolicy::default(), t0());
        let kill = KillSwitch::default();
        let mut peer = PeerPolicy::stranger("agent-key");
        peer.tier = PeerTier::TrustedHelper;
        let _ = peer.granted.insert(Permission::Keyboard);
        // Policy is view-only: even a granted peer permission is not enough.
        assert!(s
            .check_action(&kill, &peer, Some(Permission::Keyboard), "key", t0())
            .is_err());
        // View (no permission needed) is audited and allowed.
        assert!(s
            .check_action(&kill, &peer, None, "screenshot", t0())
            .is_ok());
        assert!(s.check_action(&kill, &peer, None, "", t0()).is_err());
    }

    #[test]
    fn agent_rate_limit_and_kill_switch() {
        let policy = AgentPolicy {
            allow_input: true,
            max_actions_per_min: 2,
        };
        let mut s = HostAgentSession::new("helper", policy, t0());
        let kill = KillSwitch::default();
        let peer = PeerPolicy::stranger("agent-key");
        assert!(s.check_action(&kill, &peer, None, "a", t0()).is_ok());
        assert!(s.check_action(&kill, &peer, None, "b", t0()).is_ok());
        assert!(s.check_action(&kill, &peer, None, "c", t0()).is_err());
        let mut kill2 = KillSwitch::default();
        kill2.revoke();
        assert!(s.check_action(&kill2, &peer, None, "d", t0()).is_err());
    }
}
