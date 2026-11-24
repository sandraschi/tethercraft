//! Host-side file transfer: root-jailed local operations.
//!
//! Every public op takes the already-checked permission result from the caller:
//! the host authorizes the message kind first, then the manager touches disk.
//! All relative paths go through [`tethercraft_proto::validate_transfer_path`]
//! plus a lexical jail check, so `..` and absolute paths never escape.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

pub struct FileManager {
    root: PathBuf,
}

impl FileManager {
    pub fn new(root: &Path) -> Result<Self, TetherError> {
        if !root.is_dir() {
            return Err(TetherError::Invalid("file root must exist".to_string()));
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// Resolve a validated relative path inside the jail.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, TetherError> {
        tethercraft_proto::validate_transfer_path(rel)?;
        let joined = self
            .root
            .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        // Lexical jail: the joined path must stay under root.
        if !joined.starts_with(&self.root) {
            return Err(TetherError::Invalid("path escapes share root".to_string()));
        }
        Ok(joined)
    }

    pub fn list(&self, rel: &str) -> Result<Vec<DirEntry>, TetherError> {
        let dir = self.resolve(rel)?;
        let read = std::fs::read_dir(&dir)
            .map_err(|e| TetherError::Invalid(format!("list failed: {e}")))?;
        let mut out = Vec::new();
        for entry in read {
            let entry = entry.map_err(|e| TetherError::Invalid(format!("list failed: {e}")))?;
            let meta = entry
                .metadata()
                .map_err(|e| TetherError::Invalid(format!("stat: {e}")))?;
            let name = entry.file_name().to_string_lossy().to_string();
            out.push(DirEntry {
                name,
                is_dir: meta.is_dir(),
                size: meta.len(),
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn read_bytes(&self, rel: &str, offset: u64, len: usize) -> Result<Vec<u8>, TetherError> {
        let path = self.resolve(rel)?;
        let data = std::fs::read(&path).map_err(|e| TetherError::Invalid(format!("read: {e}")))?;
        let start = (offset as usize).min(data.len());
        let end = start.saturating_add(len).min(data.len());
        Ok(data[start..end].to_vec())
    }

    pub fn write_bytes(&self, rel: &str, data: &[u8], append: bool) -> Result<u64, TetherError> {
        let path = self.resolve(rel)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| TetherError::Invalid(format!("mkdir: {e}")))?;
        }
        if append && path.exists() {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .map_err(|e| TetherError::Invalid(format!("append: {e}")))?;
            f.write_all(data)
                .map_err(|e| TetherError::Invalid(format!("append: {e}")))?;
            let meta = path
                .metadata()
                .map_err(|e| TetherError::Invalid(format!("stat: {e}")))?;
            return Ok(meta.len());
        }
        std::fs::write(&path, data).map_err(|e| TetherError::Invalid(format!("write: {e}")))?;
        Ok(data.len() as u64)
    }

    pub fn remove(&self, rel: &str) -> Result<(), TetherError> {
        let path = self.resolve(rel)?;
        if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| TetherError::Invalid(format!("rmdir: {e}")))?;
        } else {
            std::fs::remove_file(&path).map_err(|e| TetherError::Invalid(format!("rm: {e}")))?;
        }
        Ok(())
    }

    pub fn mkdir_all(&self, rel: &str) -> Result<(), TetherError> {
        let path = self.resolve(rel)?;
        std::fs::create_dir_all(&path).map_err(|e| TetherError::Invalid(format!("mkdir: {e}")))?;
        Ok(())
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<(), TetherError> {
        let a = self.resolve(from)?;
        let b = self.resolve(to)?;
        std::fs::rename(&a, &b).map_err(|e| TetherError::Invalid(format!("mv: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("tether-test-{name}"));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap_or(());
        p
    }

    #[test]
    fn write_list_read_remove() {
        let root = tmp_root("files1");
        let fm = FileManager::new(&root).unwrap_or(FileManager { root: root.clone() });
        assert!(fm.write_bytes("a/b.txt", b"hello", false).is_ok());
        let list = fm.list("a").unwrap_or_default();
        assert!(list.len() == 1);
        assert!(list[0].name == "b.txt");
        let data = fm.read_bytes("a/b.txt", 1, 3).unwrap_or_default();
        assert!(data == b"ell");
        assert!(fm.remove("a/b.txt").is_ok());
        assert!(fm.list("a").unwrap_or_default().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn jail_holds() {
        let root = tmp_root("files2");
        let fm = FileManager::new(&root).unwrap_or(FileManager { root: root.clone() });
        assert!(fm.resolve("../escape.txt").is_err());
        assert!(fm.resolve("/abs.txt").is_err());
        assert!(fm.resolve("ok/sub.txt").is_ok());
        assert!(fm.rename("a", "b").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resume_append_and_mkdir() {
        let root = tmp_root("files3");
        let fm = FileManager::new(&root).unwrap_or(FileManager { root: root.clone() });
        assert!(fm.mkdir_all("d/e").is_ok());
        assert!(fm.write_bytes("d/e/f.bin", b"0123", false).is_ok());
        assert!(fm.write_bytes("d/e/f.bin", b"4567", true).is_ok());
        let data = fm.read_bytes("d/e/f.bin", 0, 64).unwrap_or_default();
        assert!(data == b"01234567");
        let _ = std::fs::remove_dir_all(&root);
    }
}
