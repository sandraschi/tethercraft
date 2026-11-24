//! Privacy mode (Tier 4): the host blanks what the controller sees.
//!
//! A stranger can never get here (the host denies `Permission::Privacy` for
//! the stranger tier outright). A trusted helper's request lands in
//! `pending_local_approval` and activates only after a local click. While
//! active, the capture path runs frames through [`blank_rgba`] per the scope.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyScope {
    AllMonitors,
    Monitor(u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyMode {
    pub active: bool,
    pub scope: PrivacyScope,
    pub pending_local_approval: bool,
}

impl PrivacyMode {
    pub fn new() -> Self {
        Self {
            active: false,
            scope: PrivacyScope::AllMonitors,
            pending_local_approval: false,
        }
    }

    /// Controller request. The caller must already hold `Permission::Privacy`.
    pub fn request(&mut self, scope: PrivacyScope) {
        self.scope = scope;
        self.pending_local_approval = true;
    }

    pub fn approve_local(&mut self) {
        if self.pending_local_approval {
            self.pending_local_approval = false;
            self.active = true;
        }
    }

    pub fn deny_local(&mut self) {
        self.pending_local_approval = false;
    }

    pub fn end(&mut self) {
        self.active = false;
        self.pending_local_approval = false;
        self.scope = PrivacyScope::AllMonitors;
    }
}

impl Default for PrivacyMode {
    fn default() -> Self {
        Self::new()
    }
}

/// Blank an RGBA frame in place (RGB to zero, alpha kept). Real function used
/// by the capture path when privacy mode is active.
pub fn blank_rgba(pixels: &mut [u8]) -> Result<(), TetherError> {
    if !pixels.len().is_multiple_of(4) {
        return Err(TetherError::Invalid(
            "RGBA buffer must be a multiple of 4".to_string(),
        ));
    }
    let mut i = 0;
    while i + 4 <= pixels.len() {
        pixels[i] = 0;
        pixels[i + 1] = 0;
        pixels[i + 2] = 0;
        i += 4;
    }
    Ok(())
}

/// Is `monitor` covered by `scope`?
pub fn in_scope(scope: PrivacyScope, monitor: u32) -> bool {
    match scope {
        PrivacyScope::AllMonitors => true,
        PrivacyScope::Monitor(id) => id == monitor,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_flow() {
        let mut p = PrivacyMode::new();
        assert!(!p.active);
        p.request(PrivacyScope::Monitor(1));
        assert!(!p.active);
        assert!(p.pending_local_approval);
        p.approve_local();
        assert!(p.active);
        assert!(in_scope(p.scope, 1));
        assert!(!in_scope(p.scope, 2));
        p.end();
        assert!(!p.active);
    }

    #[test]
    fn deny_clears_pending() {
        let mut p = PrivacyMode::new();
        p.request(PrivacyScope::AllMonitors);
        p.deny_local();
        assert!(!p.pending_local_approval);
        p.approve_local();
        assert!(!p.active);
    }

    #[test]
    fn blanking_keeps_shape() {
        let mut px = vec![255u8; 4 * 4 * 2];
        assert!(blank_rgba(&mut px).is_ok());
        assert!(px.len() == 4 * 4 * 2);
        assert!(px.iter().take(3).all(|b| *b == 0));
        assert!(blank_rgba(&mut [1u8, 2, 3]).is_err());
    }
}
