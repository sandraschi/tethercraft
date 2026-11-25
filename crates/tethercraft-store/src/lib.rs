//! SQLite store: address book, settings, audit log.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tethercraft_core::{AuditEvent, TetherError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerRow {
    pub peer_id: String,
    pub alias: Option<String>,
    pub pubkey_hex: String,
    pub tier: String,
    pub addr: Option<String>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open_in_memory() -> Result<Self, TetherError> {
        let conn =
            Connection::open_in_memory().map_err(|e| TetherError::Storage(format!("open: {e}")))?;
        let s = Self { conn };
        s.init()?;
        Ok(s)
    }

    pub fn open_file(path: &str) -> Result<Self, TetherError> {
        let conn =
            Connection::open(path).map_err(|e| TetherError::Storage(format!("open: {e}")))?;
        let s = Self { conn };
        s.init()?;
        Ok(s)
    }

    fn init(&self) -> Result<(), TetherError> {
        self.conn
            .execute_batch(
                "CREATE TABLE IF NOT EXISTS peers(
                    peer_id TEXT PRIMARY KEY,
                    alias TEXT,
                    pubkey_hex TEXT NOT NULL,
                    tier TEXT NOT NULL DEFAULT 'stranger'
                );
                CREATE TABLE IF NOT EXISTS audit(
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    ts TEXT NOT NULL,
                    peer_id TEXT NOT NULL,
                    action TEXT NOT NULL,
                    result TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS blocked(
                    peer_id TEXT PRIMARY KEY,
                    reason TEXT NOT NULL,
                    at TEXT NOT NULL
                );",
            )
            .map_err(|e| TetherError::Storage(format!("init: {e}")))?;
        // Best-effort migration for stores created before the addr column.
        let _ = self
            .conn
            .execute("ALTER TABLE peers ADD COLUMN addr TEXT", []);
        Ok(())
    }

    pub fn add_peer(
        &self,
        peer_id: &str,
        alias: Option<&str>,
        pubkey_hex: &str,
        addr: Option<&str>,
    ) -> Result<(), TetherError> {
        if peer_id.is_empty() || pubkey_hex.is_empty() {
            return Err(TetherError::Invalid("peer id and key required".to_string()));
        }
        self.conn
            .execute(
                "INSERT INTO peers(peer_id, alias, pubkey_hex, tier, addr)
                 VALUES(?1, ?2, ?3, 'stranger', ?4)
                 ON CONFLICT(peer_id) DO UPDATE SET alias=excluded.alias, pubkey_hex=excluded.pubkey_hex, addr=excluded.addr",
                params![peer_id, alias, pubkey_hex, addr],
            )
            .map_err(|e| TetherError::Storage(format!("add_peer: {e}")))?;
        Ok(())
    }

    pub fn list_peers(&self) -> Result<Vec<PeerRow>, TetherError> {
        let mut stmt = self
            .conn
            .prepare("SELECT peer_id, alias, pubkey_hex, tier, addr FROM peers ORDER BY peer_id")
            .map_err(|e| TetherError::Storage(format!("list: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PeerRow {
                    peer_id: r.get(0)?,
                    alias: r.get(1)?,
                    pubkey_hex: r.get(2)?,
                    tier: r.get(3)?,
                    addr: r.get(4)?,
                })
            })
            .map_err(|e| TetherError::Storage(format!("list: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| TetherError::Storage(format!("row: {e}")))?);
        }
        Ok(out)
    }

    /// Find a peer by its device key hex (TOFU lookup on session start).
    pub fn find_by_key(&self, key_hex: &str) -> Result<Option<PeerRow>, TetherError> {
        let mut stmt = self
            .conn
            .prepare("SELECT peer_id, alias, pubkey_hex, tier, addr FROM peers WHERE pubkey_hex=?1")
            .map_err(|e| TetherError::Storage(format!("find: {e}")))?;
        let mut rows = stmt
            .query_map(params![key_hex], |r| {
                Ok(PeerRow {
                    peer_id: r.get(0)?,
                    alias: r.get(1)?,
                    pubkey_hex: r.get(2)?,
                    tier: r.get(3)?,
                    addr: r.get(4)?,
                })
            })
            .map_err(|e| TetherError::Storage(format!("find: {e}")))?;
        match rows.next() {
            Some(r) => Ok(Some(
                r.map_err(|e| TetherError::Storage(format!("row: {e}")))?,
            )),
            None => Ok(None),
        }
    }

    /// Promote or demote a stored peer. Never called from inside a session;
    /// the CLI owns it, which is the local UI for this milestone.
    pub fn set_tier(&self, peer_id: &str, tier: &str) -> Result<(), TetherError> {
        match tier {
            "stranger" | "trusted_helper" | "managed" => {}
            _ => {
                return Err(TetherError::Invalid(
                    "tier must be stranger, trusted_helper or managed".to_string(),
                ))
            }
        }
        let n = self
            .conn
            .execute(
                "UPDATE peers SET tier=?1 WHERE peer_id=?2",
                params![tier, peer_id],
            )
            .map_err(|e| TetherError::Storage(format!("tier: {e}")))?;
        if n == 0 {
            return Err(TetherError::Invalid(format!("unknown peer {peer_id}")));
        }
        Ok(())
    }

    pub fn log(&self, ev: &AuditEvent) -> Result<(), TetherError> {
        self.conn
            .execute(
                "INSERT INTO audit(ts, peer_id, action, result) VALUES(?1, ?2, ?3, ?4)",
                params![ev.at.to_rfc3339(), ev.peer_id, ev.action, ev.result],
            )
            .map_err(|e| TetherError::Storage(format!("audit: {e}")))?;
        Ok(())
    }

    /// R6 "This was a scam": block the peer ID so it can never reconnect.
    pub fn block_peer(
        &self,
        peer_id: &str,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<(), TetherError> {
        if peer_id.is_empty() {
            return Err(TetherError::Invalid("peer id required".to_string()));
        }
        self.conn
            .execute(
                "INSERT INTO blocked(peer_id, reason, at) VALUES(?1, ?2, ?3)
                 ON CONFLICT(peer_id) DO UPDATE SET reason=excluded.reason, at=excluded.at",
                params![peer_id, reason, now.to_rfc3339()],
            )
            .map_err(|e| TetherError::Storage(format!("block: {e}")))?;
        Ok(())
    }

    pub fn is_blocked(&self, peer_id: &str) -> bool {
        let mut stmt = match self.conn.prepare("SELECT 1 FROM blocked WHERE peer_id=?1") {
            Ok(s) => s,
            Err(_) => return false,
        };
        stmt.exists(params![peer_id]).unwrap_or(false)
    }

    pub fn list_audit(&self, limit: usize) -> Result<Vec<AuditEvent>, TetherError> {
        let mut stmt = self
            .conn
            .prepare("SELECT ts, peer_id, action, result FROM audit ORDER BY id DESC LIMIT ?1")
            .map_err(|e| TetherError::Storage(format!("audit list: {e}")))?;
        let rows = stmt
            .query_map(params![limit as i64], |r| {
                let ts: String = r.get(0)?;
                Ok((
                    ts,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| TetherError::Storage(format!("audit list: {e}")))?;
        let mut out = Vec::new();
        for r in rows {
            let (ts, peer_id, action, result) =
                r.map_err(|e| TetherError::Storage(format!("row: {e}")))?;
            let at = ts
                .parse::<DateTime<Utc>>()
                .unwrap_or(DateTime::<Utc>::MIN_UTC);
            out.push(AuditEvent {
                at,
                peer_id,
                action,
                result,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peers_and_audit() {
        let opened = Store::open_in_memory();
        assert!(opened.is_ok());
        if let Ok(s) = opened {
            assert!(s
                .add_peer("p1", Some("Office PC"), "ab12", Some("127.0.0.1:21120"))
                .is_ok());
            assert!(s.add_peer("", None, "x", None).is_err());
            let peers = s.list_peers().unwrap_or_default();
            assert!(peers.len() == 1);
            assert!(peers[0].tier == "stranger");
            let ev = AuditEvent::new(Utc::now(), "p1", "connect", "ok");
            assert!(s.log(&ev).is_ok());
            let log = s.list_audit(10).unwrap_or_default();
            assert!(log.len() == 1);
            // R6 blocklist.
            assert!(!s.is_blocked("x"));
            assert!(s.block_peer("", "scam", Utc::now()).is_err());
            assert!(s
                .block_peer("scammer-1", "tech-support scam", Utc::now())
                .is_ok());
            assert!(s.is_blocked("scammer-1"));
        }
    }
}
