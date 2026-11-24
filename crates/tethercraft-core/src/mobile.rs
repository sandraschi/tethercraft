//! Tier 5 mobile groundwork: roles, touch, power and bandwidth policy.
//!
//! The native protocol is transport-agnostic, so a mobile controller already
//! speaks the same envelopes. What Tier 5 adds on top is: touch input with a
//! fixed gesture map, controller-only enforcement (the iOS rule, applied as a
//! build role on every platform), battery and bandwidth policy, and the
//! Android host permission flow as an explicit state machine. OS bindings and
//! store builds need real devices.

use serde::{Deserialize, Serialize};

use crate::{Permission, TetherError};

/// Build role. iOS builds are controller-only because the OS forbids remote
/// control of the device; managed builds can pin the same rule anywhere so a
/// support tablet can never become a host by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Controller,
    Host,
}

pub fn can_host(role: Role) -> Result<(), TetherError> {
    match role {
        Role::Host => Ok(()),
        Role::Controller => Err(TetherError::Denied(
            "controller-only build cannot host".to_string(),
        )),
    }
}

/// Touch gestures and what they mean on the remote desktop. Fixed map, no
/// configuration surface: nothing can rebind a tap into something surprising.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchGesture {
    Tap,
    LongPress,
    Swipe,
    TwoFingerTap,
}

pub fn gesture_action(gesture: TouchGesture) -> &'static str {
    match gesture {
        TouchGesture::Tap => "left-click",
        TouchGesture::LongPress => "right-click",
        TouchGesture::Swipe => "scroll",
        TouchGesture::TwoFingerTap => "middle-click",
    }
}

/// Which host permission a gesture needs. Always input-class, never more.
pub fn gesture_permission(_gesture: TouchGesture) -> Permission {
    Permission::Touch
}

/// Frame pacing from network quality. Steady LAN stays at 16 ms; poor links
/// back off, capped at 500 ms so the session never stalls forever.
pub fn frame_interval_ms(rtt_ms: u32, loss_pct: u8) -> u32 {
    let rtt_part = rtt_ms.min(240);
    let loss_part = u32::from(loss_pct.min(100)) * 3;
    16u32
        .saturating_add(rtt_part)
        .saturating_add(loss_part)
        .min(500)
}

/// Battery saver caps for the controller renderer.
pub fn battery_fps_cap(on_battery: bool, session_active: bool) -> u32 {
    match (on_battery, session_active) {
        (true, true) => 15,
        (true, false) => 5,
        (false, _) => 60,
    }
}

/// Android host permission flow as an explicit state machine. The OS dialogs
/// (MediaProjection consent, AccessibilityService enable) are platform work;
/// the order and the revocation rules are real here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionState {
    Idle,
    Requested,
    Granted,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A11yState {
    Off,
    Requested,
    Granted,
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidHost {
    pub projection: ProjectionState,
    pub a11y: A11yState,
}

impl AndroidHost {
    pub fn new() -> Self {
        Self {
            projection: ProjectionState::Idle,
            a11y: A11yState::Off,
        }
    }

    pub fn request_projection(&mut self) {
        if self.projection == ProjectionState::Idle || self.projection == ProjectionState::Revoked {
            self.projection = ProjectionState::Requested;
        }
    }

    pub fn grant_projection(&mut self) {
        if self.projection == ProjectionState::Requested {
            self.projection = ProjectionState::Granted;
        }
    }

    pub fn revoke_projection(&mut self) {
        self.projection = ProjectionState::Revoked;
    }

    pub fn request_a11y(&mut self) {
        if self.a11y == A11yState::Off || self.a11y == A11yState::Revoked {
            self.a11y = A11yState::Requested;
        }
    }

    pub fn grant_a11y(&mut self) {
        if self.a11y == A11yState::Requested {
            self.a11y = A11yState::Granted;
        }
    }

    pub fn revoke_a11y(&mut self) {
        self.a11y = A11yState::Revoked;
    }

    pub fn can_capture(&self) -> bool {
        self.projection == ProjectionState::Granted
    }

    pub fn can_inject(&self) -> bool {
        self.a11y == A11yState::Granted
    }
}

impl Default for AndroidHost {
    fn default() -> Self {
        Self::new()
    }
}

/// What this build can do, for `tethercraft platform` and the README matrix.
#[derive(Debug, Clone, Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub controller: &'static str,
    pub host: &'static str,
    pub touch: &'static str,
    pub relay: &'static str,
}

pub fn platform_info() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        controller: "full",
        host: if cfg!(target_os = "windows") {
            "tier1-partial"
        } else {
            "planned-needs-hardware"
        },
        touch: "policy-ready",
        relay: "loopback-ready",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_builds_cannot_host() {
        assert!(can_host(Role::Host).is_ok());
        assert!(can_host(Role::Controller).is_err());
    }

    #[test]
    fn gestures_are_input_class_only() {
        let all = [
            TouchGesture::Tap,
            TouchGesture::LongPress,
            TouchGesture::Swipe,
            TouchGesture::TwoFingerTap,
        ];
        for g in all {
            assert!(gesture_permission(g) == Permission::Touch);
        }
        assert!(gesture_action(TouchGesture::Tap) == "left-click");
        assert!(gesture_action(TouchGesture::TwoFingerTap) == "middle-click");
    }

    #[test]
    fn pacing_bounds() {
        assert!(frame_interval_ms(0, 0) == 16);
        assert!(frame_interval_ms(20, 0) == 36);
        assert!(frame_interval_ms(10000, 100) == 500);
        assert!(battery_fps_cap(true, true) == 15);
        assert!(battery_fps_cap(true, false) == 5);
        assert!(battery_fps_cap(false, true) == 60);
    }

    #[test]
    fn android_flow_order() {
        let mut h = AndroidHost::new();
        assert!(!h.can_capture());
        assert!(!h.can_inject());
        h.grant_projection();
        assert!(!h.can_capture());
        h.request_projection();
        h.grant_projection();
        assert!(h.can_capture());
        h.request_a11y();
        h.grant_a11y();
        assert!(h.can_inject());
        h.revoke_projection();
        assert!(!h.can_capture());
        assert!(h.can_inject());
        h.revoke_a11y();
        assert!(!h.can_inject());
    }

    #[test]
    fn platform_info_for_this_machine() {
        let info = platform_info();
        assert!(!info.os.is_empty());
        if cfg!(target_os = "windows") {
            assert!(info.host == "tier1-partial");
        } else {
            assert!(info.host == "planned-needs-hardware");
        }
    }
}
