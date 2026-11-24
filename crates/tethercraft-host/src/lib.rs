//! Host policy enforcement: every message checked, stranger defaults, caps.

use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};
use tethercraft_core::{PeerPolicy, PeerTier, Permission, TetherError};
use tethercraft_proto::MessageKind;

fn permission_for_kind(kind: MessageKind) -> Option<Permission> {
    match kind {
        MessageKind::InputEvent => Some(Permission::Keyboard),
        MessageKind::ClipboardText | MessageKind::ClipboardImage => Some(Permission::Clipboard),
        MessageKind::Frame => None,
        MessageKind::SessionStart
        | MessageKind::SessionEnd
        | MessageKind::PairingHello
        | MessageKind::PairingWelcome
        | MessageKind::PakeA
        | MessageKind::PakeB
        | MessageKind::Ping
        | MessageKind::Pong => None,
    }
}

/// A live host session.
#[derive(Debug, Clone)]
pub struct HostSession {
    pub peer_id: String,
    pub tier: PeerTier,
    pub started_at: DateTime<Utc>,
    pub recording_on: bool,
}

pub struct Host {
    policies: HashMap<String, PeerPolicy>,
    sessions: HashMap<String, HostSession>,
}

impl Host {
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            sessions: HashMap::new(),
        }
    }

    pub fn peer_or_stranger(&mut self, peer_id: &str) -> &PeerPolicy {
        self.policies
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerPolicy::stranger(peer_id))
    }

    /// Start a session. Unknown peers become Stranger; R1 outgoing-only blocks hosting.
    pub fn session_start(
        &mut self,
        peer_id: &str,
        now: DateTime<Utc>,
        outgoing_only: bool,
    ) -> Result<HostSession, TetherError> {
        if outgoing_only {
            return Err(TetherError::Denied(
                "hosting disabled during R1 outgoing-only window".to_string(),
            ));
        }
        let policy = self
            .policies
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerPolicy::stranger(peer_id));
        policy.session_started_at = Some(now);
        let sess = HostSession {
            peer_id: peer_id.to_string(),
            tier: policy.tier,
            started_at: now,
            recording_on: policy.tier == PeerTier::Stranger || policy.local_recording_on,
        };
        self.sessions.insert(peer_id.to_string(), sess.clone());
        Ok(sess)
    }

    /// Check one incoming message against the host policy. Never trusts the controller.
    pub fn authorize(
        &self,
        peer_id: &str,
        kind: MessageKind,
        now: DateTime<Utc>,
    ) -> Result<(), TetherError> {
        let policy = match self.policies.get(peer_id) {
            Some(p) => p,
            None => {
                return Err(TetherError::Denied(
                    "unknown peer has no session".to_string(),
                ))
            }
        };
        match permission_for_kind(kind) {
            None => Ok(()),
            Some(perm) => policy.check(perm, now),
        }
    }

    pub fn granted_mut(&mut self, peer_id: &str) -> &mut BTreeSet<Permission> {
        &mut self
            .policies
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerPolicy::stranger(peer_id))
            .granted
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap_or(DateTime::<Utc>::MIN_UTC)
    }

    #[test]
    fn stranger_input_denied_clipboard_denied() {
        let mut h = Host::new();
        let now = t0();
        let _ = h.session_start("s1", now, false);
        assert!(h.authorize("s1", MessageKind::InputEvent, now).is_err());
        assert!(h.authorize("s1", MessageKind::ClipboardText, now).is_err());
        assert!(h.authorize("s1", MessageKind::Ping, now).is_ok());
    }

    #[test]
    fn outgoing_only_blocks_hosting() {
        let mut h = Host::new();
        assert!(h.session_start("s1", t0(), true).is_err());
    }

    #[test]
    fn per_session_enable_allows_keyboard_only() {
        let mut h = Host::new();
        let now = t0();
        let _ = h.session_start("s1", now, false);
        let _ = h.granted_mut("s1").insert(Permission::Keyboard);
        assert!(h.authorize("s1", MessageKind::InputEvent, now).is_ok());
        assert!(h.authorize("s1", MessageKind::ClipboardText, now).is_err());
    }
}
