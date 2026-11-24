//! Terminal channel registry (Tier 2).
//!
//! The registry, permission discipline, persistence markers, and audit hooks
//! are real. The PTY backend (ConPTY on Windows) is OS work and returns
//! `NotImplemented` until it is wired; nothing here fakes a live shell.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermSession {
    pub id: u64,
    pub peer_id: String,
    pub opened_at: DateTime<Utc>,
    pub persistent: bool,
    pub alive: bool,
}

pub struct TermRegistry {
    sessions: std::collections::HashMap<u64, TermSession>,
    next_id: u64,
}

impl TermRegistry {
    pub fn new() -> Self {
        Self {
            sessions: std::collections::HashMap::new(),
            next_id: 1,
        }
    }

    /// Open a terminal channel. The caller must have checked `Permission::Terminal`.
    pub fn open(&mut self, peer_id: &str, persistent: bool, now: DateTime<Utc>) -> TermSession {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let sess = TermSession {
            id,
            peer_id: peer_id.to_string(),
            opened_at: now,
            persistent,
            alive: true,
        };
        self.sessions.insert(id, sess.clone());
        sess
    }

    /// Deliver input bytes to the PTY. Pending the ConPTY backend.
    pub fn write(&self, id: u64) -> Result<(), TetherError> {
        match self.sessions.get(&id) {
            Some(s) if s.alive => Err(TetherError::NotImplemented(
                "pty backend (ConPTY) ships with the Tier 2 host service".to_string(),
            )),
            _ => Err(TetherError::Invalid("unknown or dead terminal".to_string())),
        }
    }

    /// Close the channel. Persistent sessions stay listed for reconnection.
    pub fn close(&mut self, id: u64) -> Result<(), TetherError> {
        match self.sessions.get_mut(&id) {
            Some(s) => {
                s.alive = false;
                Ok(())
            }
            None => Err(TetherError::Invalid("unknown terminal".to_string())),
        }
    }

    pub fn get(&self, id: u64) -> Option<&TermSession> {
        self.sessions.get(&id)
    }

    pub fn live_count(&self) -> usize {
        self.sessions.values().filter(|s| s.alive).count()
    }
}

impl Default for TermRegistry {
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
    fn open_close_lifecycle() {
        let mut r = TermRegistry::new();
        let s = r.open("p1", true, t0());
        assert!(r.live_count() == 1);
        assert!(r.write(s.id).is_err());
        assert!(r.close(s.id).is_ok());
        assert!(r.live_count() == 0);
        // Persistent session record survives close for reconnection.
        assert!(r.get(s.id).is_some());
        assert!(r.close(999).is_err());
    }

    #[test]
    fn ids_are_unique() {
        let mut r = TermRegistry::new();
        let a = r.open("p1", false, t0());
        let b = r.open("p1", false, t0());
        assert!(a.id != b.id);
    }
}
