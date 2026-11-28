//! Host policy enforcement: every message checked, stranger defaults, caps.

pub mod agent;
pub mod camera;
pub mod files;
pub mod health;
pub mod inject;
pub mod ops;
pub mod printer;
pub mod privacy;
pub mod serve;
pub mod shield;
pub mod sync;
pub mod terminal;
pub mod voice;

use chrono::{DateTime, Utc};
use std::collections::{BTreeSet, HashMap};
use tethercraft_core::{PeerPolicy, PeerTier, Permission, TetherError};
use tethercraft_proto::MessageKind;

fn permission_for_kind(kind: MessageKind) -> Option<Permission> {
    match kind {
        MessageKind::InputEvent => Some(Permission::Keyboard),
        MessageKind::TouchTap | MessageKind::TouchHold | MessageKind::TouchSwipe => {
            Some(Permission::Touch)
        }
        MessageKind::ClipboardText | MessageKind::ClipboardImage | MessageKind::ClipboardFiles => {
            Some(Permission::Clipboard)
        }
        MessageKind::FileList
        | MessageKind::FileGet
        | MessageKind::FilePut
        | MessageKind::FileChunk
        | MessageKind::FileDone
        | MessageKind::FileMv
        | MessageKind::FileMkdir
        | MessageKind::FileRm
        | MessageKind::FileHash
        | MessageKind::FileHashResp
        | MessageKind::SyncPlan
        | MessageKind::SyncChunk => Some(Permission::File),
        MessageKind::TermOpen
        | MessageKind::TermData
        | MessageKind::TermClose
        | MessageKind::ExecRequest => Some(Permission::Terminal),
        MessageKind::TunnelOpen | MessageKind::TunnelData | MessageKind::TunnelClose => {
            Some(Permission::Tunnel)
        }
        MessageKind::AudioFrame => Some(Permission::Audio),
        MessageKind::PrivacyRequest | MessageKind::PrivacyEnd => Some(Permission::Privacy),
        MessageKind::Restart | MessageKind::Shutdown | MessageKind::Lock => {
            Some(Permission::Restart)
        }
        MessageKind::Frame
        | MessageKind::FrameH264
        | MessageKind::FileListResp
        | MessageKind::FileBlock
        | MessageKind::ExecDone
        | MessageKind::PowerAck
        | MessageKind::TunnelAck
        | MessageKind::ChatMsg
        | MessageKind::NoteSet
        | MessageKind::SessionStart
        | MessageKind::SessionEnd
        | MessageKind::PairingHello
        | MessageKind::PairingWelcome
        | MessageKind::PakeA
        | MessageKind::PakeB
        | MessageKind::OtpProof
        | MessageKind::HealthReq
        | MessageKind::HealthResp
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

    /// Move a peer to a stored tier (TOFU record lookup). Grants stay
    /// session-scoped and are never escalated by this.
    pub fn set_tier(&mut self, peer_id: &str, tier: PeerTier) {
        self.policies
            .entry(peer_id.to_string())
            .or_insert_with(|| PeerPolicy::stranger(peer_id))
            .tier = tier;
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

/// In-session text chat and shared notes. Session-level: any peer with a live
/// session may exchange chat; notes are stored per peer on the host.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatEntry {
    pub at: DateTime<Utc>,
    pub from: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct ChatLog {
    entries: Vec<ChatEntry>,
    pub notes: std::collections::HashMap<String, String>,
}

impl ChatLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn post(&mut self, from: &str, text: &str, now: DateTime<Utc>) -> Result<(), TetherError> {
        if text.is_empty() || text.len() > 4096 {
            return Err(TetherError::Invalid(
                "chat text must be 1..=4096 chars".to_string(),
            ));
        }
        self.entries.push(ChatEntry {
            at: now,
            from: from.to_string(),
            text: text.to_string(),
        });
        if self.entries.len() > 1000 {
            let excess = self.entries.len() - 1000;
            self.entries.drain(..excess);
        }
        Ok(())
    }

    pub fn recent(&self, limit: usize) -> Vec<ChatEntry> {
        let n = limit.min(self.entries.len());
        self.entries[self.entries.len() - n..].to_vec()
    }

    pub fn set_note(&mut self, peer: &str, text: &str) -> Result<(), TetherError> {
        if text.len() > 16384 {
            return Err(TetherError::Invalid("note too long".to_string()));
        }
        self.notes.insert(peer.to_string(), text.to_string());
        Ok(())
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

    #[test]
    fn tier2_kinds_map_to_permissions() {
        let mut h = Host::new();
        let now = t0();
        let _ = h.session_start("s1", now, false);
        // Chat and notes are session-level: allowed for strangers.
        assert!(h.authorize("s1", MessageKind::ChatMsg, now).is_ok());
        assert!(h.authorize("s1", MessageKind::NoteSet, now).is_ok());
        // Files, terminal, tunnel, audio all denied for strangers.
        assert!(h.authorize("s1", MessageKind::FileList, now).is_err());
        assert!(h.authorize("s1", MessageKind::TermOpen, now).is_err());
        assert!(h.authorize("s1", MessageKind::ExecRequest, now).is_err());
        assert!(h.authorize("s1", MessageKind::TunnelOpen, now).is_err());
        // Audio, privacy, and camera-adjacent controls denied for strangers too.
        assert!(h.authorize("s1", MessageKind::AudioFrame, now).is_err());
        assert!(h.authorize("s1", MessageKind::PrivacyRequest, now).is_err());
        assert!(h.authorize("s1", MessageKind::TouchTap, now).is_err());
        assert!(h.authorize("s1", MessageKind::FileGet, now).is_err());
        assert!(h.authorize("s1", MessageKind::ExecRequest, now).is_err());
        assert!(h.authorize("s1", MessageKind::Restart, now).is_err());
        assert!(h.authorize("s1", MessageKind::TunnelOpen, now).is_err());
        assert!(h.authorize("s1", MessageKind::OtpProof, now).is_ok());
    }

    #[test]
    fn chat_log_bounds_and_notes() {
        let mut log = ChatLog::new();
        let now = t0();
        assert!(log.post("p1", "", now).is_err());
        assert!(log.post("p1", "hi", now).is_ok());
        assert!(log.recent(10).len() == 1);
        assert!(log.set_note("p1", "callback number").is_ok());
        assert!(log.set_note("p1", &"x".repeat(20000)).is_err());
    }
}
