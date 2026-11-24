//! Folder sync planner (Tier 2, PRD section 16).
//!
//! Hash-based diff between a local tree and a remote manifest, plan first,
//! resumable, deletes only with `delete` plus confirm. The planner and the
//! local apply path are real; the remote manifest arrives over `SyncPlan` /
//! `SyncChunk` envelopes once live transport exists.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Push,
    Pull,
    Both,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    Newer,
    KeepBoth,
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMeta {
    pub rel: String,
    pub size: u64,
    pub hash: String,
    pub mtime: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncPlan {
    pub to_push: Vec<FileMeta>,
    pub to_pull: Vec<FileMeta>,
    pub conflicts: Vec<ConflictEntry>,
    pub delete_local: Vec<String>,
    pub delete_remote: Vec<String>,
    /// `Ask` conflicts stop here until the operator answers; nothing transfers.
    pub needs_answer: Vec<ConflictEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConflictEntry {
    pub rel: String,
    pub local_mtime: i64,
    pub remote_mtime: i64,
}

impl SyncPlan {
    pub fn is_empty(&self) -> bool {
        self.to_push.is_empty()
            && self.to_pull.is_empty()
            && self.conflicts.is_empty()
            && self.delete_local.is_empty()
            && self.delete_remote.is_empty()
            && self.needs_answer.is_empty()
    }

    pub fn summary(&self) -> String {
        format!(
            "push {p}, pull {u}, conflicts {c}, delete-local {dl}, delete-remote {dr}, needs-answer {a}",
            p = self.to_push.len(),
            u = self.to_pull.len(),
            c = self.conflicts.len(),
            dl = self.delete_local.len(),
            dr = self.delete_remote.len(),
            a = self.needs_answer.len()
        )
    }
}

pub fn hash_bytes(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

/// Walk a local tree and hash every file. Symlinks are not followed.
pub fn scan_local(root: &Path) -> Result<Vec<FileMeta>, TetherError> {
    if !root.is_dir() {
        return Err(TetherError::Invalid("sync root must exist".to_string()));
    }
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read =
            std::fs::read_dir(&dir).map_err(|e| TetherError::Invalid(format!("scan: {e}")))?;
        for entry in read {
            let entry = entry.map_err(|e| TetherError::Invalid(format!("scan: {e}")))?;
            let path = entry.path();
            let ftype = entry
                .file_type()
                .map_err(|e| TetherError::Invalid(format!("scan: {e}")))?;
            if ftype.is_symlink() {
                continue;
            }
            if ftype.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .map_err(|e| TetherError::Invalid(format!("scan: {e}")))?
                .to_string_lossy()
                .replace('\\', "/");
            let data =
                std::fs::read(&path).map_err(|e| TetherError::Invalid(format!("scan: {e}")))?;
            let meta = path
                .metadata()
                .map_err(|e| TetherError::Invalid(format!("scan: {e}")))?;
            let mtime = meta
                .modified()
                .map_err(|e| TetherError::Invalid(format!("scan: {e}")))?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            out.push(FileMeta {
                rel,
                size: data.len() as u64,
                hash: hash_bytes(&data),
                mtime,
            });
        }
    }
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

/// Diff local against remote. Deletes are listed only when `delete` is set;
/// the CLI additionally requires confirm before applying them.
pub fn plan(
    local: &[FileMeta],
    remote: &[FileMeta],
    direction: Direction,
    conflict: Conflict,
    delete: bool,
) -> SyncPlan {
    let mut p = SyncPlan::default();
    let mut remote_by_rel = std::collections::HashMap::new();
    for f in remote {
        remote_by_rel.insert(f.rel.clone(), f);
    }
    let mut local_by_rel = std::collections::HashMap::new();
    for f in local {
        local_by_rel.insert(f.rel.clone(), f);
    }
    let push_ok = direction == Direction::Push || direction == Direction::Both;
    let pull_ok = direction == Direction::Pull || direction == Direction::Both;

    for f in local {
        match remote_by_rel.get(&f.rel) {
            None => {
                if push_ok {
                    p.to_push.push(f.clone());
                }
            }
            Some(r) => {
                if f.hash == r.hash {
                    continue;
                }
                match conflict {
                    Conflict::Newer => {
                        if f.mtime >= r.mtime {
                            if push_ok {
                                p.to_push.push(f.clone());
                            }
                            p.conflicts.push(ConflictEntry {
                                rel: f.rel.clone(),
                                local_mtime: f.mtime,
                                remote_mtime: r.mtime,
                            });
                        } else if pull_ok {
                            p.to_pull.push((*r).clone());
                            p.conflicts.push(ConflictEntry {
                                rel: f.rel.clone(),
                                local_mtime: f.mtime,
                                remote_mtime: r.mtime,
                            });
                        }
                    }
                    Conflict::KeepBoth => {
                        if push_ok {
                            p.to_push.push(f.clone());
                        }
                        if pull_ok {
                            p.to_pull.push((*r).clone());
                        }
                        p.conflicts.push(ConflictEntry {
                            rel: f.rel.clone(),
                            local_mtime: f.mtime,
                            remote_mtime: r.mtime,
                        });
                    }
                    Conflict::Ask => {
                        p.needs_answer.push(ConflictEntry {
                            rel: f.rel.clone(),
                            local_mtime: f.mtime,
                            remote_mtime: r.mtime,
                        });
                    }
                }
            }
        }
    }
    for f in remote {
        if !local_by_rel.contains_key(&f.rel) {
            if pull_ok {
                p.to_pull.push(f.clone());
            }
            if delete && push_ok {
                p.delete_remote.push(f.rel.clone());
            }
        }
    }
    if delete && pull_ok {
        for f in local {
            if !remote_by_rel.contains_key(&f.rel) && !p.to_push.iter().any(|x| x.rel == f.rel) {
                // Local-only files already transfer under push; only list for
                // deletion when the direction carries remote state.
                if direction == Direction::Pull || direction == Direction::Both {
                    p.delete_local.push(f.rel.clone());
                }
            }
        }
    }
    p
}

/// Apply a plan between two local roots (tests and loopback). Identical hashes
/// are skipped, so a re-run after an interruption resumes instead of redoing.
pub fn apply_local(
    plan: &SyncPlan,
    local_root: &Path,
    remote_root: &Path,
) -> Result<usize, TetherError> {
    let mut copied = 0usize;
    for f in &plan.to_push {
        let src = local_root.join(f.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let dst = remote_root.join(f.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        copy_if_different(&src, &dst)?;
        copied = copied.saturating_add(1);
    }
    for f in &plan.to_pull {
        let src = remote_root.join(f.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let dst = local_root.join(f.rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        copy_if_different(&src, &dst)?;
        copied = copied.saturating_add(1);
    }
    Ok(copied)
}

fn copy_if_different(src: &Path, dst: &Path) -> Result<(), TetherError> {
    let data = std::fs::read(src).map_err(|e| TetherError::Invalid(format!("sync read: {e}")))?;
    if dst.exists() {
        let cur =
            std::fs::read(dst).map_err(|e| TetherError::Invalid(format!("sync read: {e}")))?;
        if hash_bytes(&cur) == hash_bytes(&data) {
            return Ok(());
        }
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| TetherError::Invalid(format!("sync mkdir: {e}")))?;
    }
    std::fs::write(dst, data).map_err(|e| TetherError::Invalid(format!("sync write: {e}")))?;
    Ok(())
}

/// Sandbox a test tree: returns the root with the given files written.
#[cfg(test)]
pub fn test_tree(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!("tether-sync-{name}"));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap_or(());
    for (rel, content) in files {
        let p = root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap_or(());
        }
        std::fs::write(&p, content).unwrap_or(());
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_trees_have_empty_plan() {
        let a = test_tree("idem-a", &[("x.txt", "same"), ("sub/y.txt", "data")]);
        let b = test_tree("idem-b", &[("x.txt", "same"), ("sub/y.txt", "data")]);
        let la = scan_local(&a).unwrap_or_default();
        let lb = scan_local(&b).unwrap_or_default();
        let p = plan(&la, &lb, Direction::Both, Conflict::Newer, false);
        assert!(p.is_empty());
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }

    #[test]
    fn newer_wins_and_ask_blocks() {
        let local = vec![FileMeta {
            rel: "f.txt".to_string(),
            size: 1,
            hash: "aa".to_string(),
            mtime: 200,
        }];
        let remote = vec![FileMeta {
            rel: "f.txt".to_string(),
            size: 1,
            hash: "bb".to_string(),
            mtime: 100,
        }];
        let p = plan(&local, &remote, Direction::Both, Conflict::Newer, false);
        assert!(p.to_push.len() == 1);
        assert!(p.to_pull.is_empty());
        let q = plan(&local, &remote, Direction::Both, Conflict::Ask, false);
        assert!(q.to_push.is_empty() && q.to_pull.is_empty());
        assert!(q.needs_answer.len() == 1);
    }

    #[test]
    fn deletes_need_the_flag() {
        let local = vec![];
        let remote = vec![FileMeta {
            rel: "gone.txt".to_string(),
            size: 1,
            hash: "aa".to_string(),
            mtime: 100,
        }];
        let without = plan(&local, &remote, Direction::Push, Conflict::Newer, false);
        assert!(without.delete_remote.is_empty());
        let with = plan(&local, &remote, Direction::Push, Conflict::Newer, true);
        assert!(with.delete_remote.len() == 1);
    }

    #[test]
    fn apply_copies_and_resumes() {
        let a = test_tree("apply-a", &[("n.txt", "v1")]);
        let b = test_tree("apply-b", &[]);
        let la = scan_local(&a).unwrap_or_default();
        let lb = scan_local(&b).unwrap_or_default();
        let p = plan(&la, &lb, Direction::Both, Conflict::Newer, false);
        assert!(!p.summary().is_empty());
        let n = apply_local(&p, &a, &b).unwrap_or(usize::MAX);
        assert!(n == 1);
        let lb2 = scan_local(&b).unwrap_or_default();
        let p2 = plan(&la, &lb2, Direction::Both, Conflict::Newer, false);
        assert!(p2.is_empty());
        let _ = std::fs::remove_dir_all(&a);
        let _ = std::fs::remove_dir_all(&b);
    }
}
