//! Input events. Validation only; the host enforces permission per message.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    MouseMove { x: i32, y: i32 },
    MouseClick { button: u8 },
    MouseScroll { dx: i32, dy: i32 },
    Key { code: u32, down: bool },
    TouchTap { x: i32, y: i32 },
    TouchHold { x: i32, y: i32 },
    TouchSwipe { x0: i32, y0: i32, x1: i32, y1: i32 },
}

pub fn validate(ev: &InputEvent) -> Result<(), TetherError> {
    match ev {
        InputEvent::MouseMove { x, y } => {
            if *x < -100_000 || *x > 100_000 || *y < -100_000 || *y > 100_000 {
                return Err(TetherError::Invalid("cursor out of range".to_string()));
            }
            Ok(())
        }
        InputEvent::MouseClick { button } => {
            if *button > 5 {
                return Err(TetherError::Invalid("bad mouse button".to_string()));
            }
            Ok(())
        }
        InputEvent::MouseScroll { dx, dy } => {
            if dx.abs() > 10_000 || dy.abs() > 10_000 {
                return Err(TetherError::Invalid("scroll out of range".to_string()));
            }
            Ok(())
        }
        InputEvent::Key { code, .. } => {
            if *code > 0x2FF {
                return Err(TetherError::Invalid("bad key code".to_string()));
            }
            Ok(())
        }
        InputEvent::TouchTap { x, y } | InputEvent::TouchHold { x, y } => {
            if *x < -100_000 || *x > 100_000 || *y < -100_000 || *y > 100_000 {
                return Err(TetherError::Invalid("touch out of range".to_string()));
            }
            Ok(())
        }
        InputEvent::TouchSwipe { x0, y0, x1, y1 } => {
            for v in [x0, y0, x1, y1] {
                if *v < -100_000 || *v > 100_000 {
                    return Err(TetherError::Invalid("swipe out of range".to_string()));
                }
            }
            Ok(())
        }
    }
}

/// Injection is OS work; Tier 1 models the gate, real injection is verified live.
pub fn inject(_ev: &InputEvent) -> Result<(), TetherError> {
    Err(TetherError::NotImplemented(
        "input injection needs live host run".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_ranges() {
        assert!(validate(&InputEvent::MouseMove { x: 10, y: 20 }).is_ok());
        assert!(validate(&InputEvent::MouseMove { x: 999_999, y: 0 }).is_err());
        assert!(validate(&InputEvent::MouseClick { button: 6 }).is_err());
        assert!(validate(&InputEvent::Key {
            code: 30,
            down: true
        })
        .is_ok());
        assert!(validate(&InputEvent::TouchTap { x: 100, y: 200 }).is_ok());
        assert!(validate(&InputEvent::TouchHold { x: 999_999, y: 0 }).is_err());
        assert!(validate(&InputEvent::TouchSwipe {
            x0: 0,
            y0: 0,
            x1: 50,
            y1: 500
        })
        .is_ok());
        assert!(validate(&InputEvent::TouchSwipe {
            x0: 0,
            y0: 0,
            x1: 999_999,
            y1: 0
        })
        .is_err());
        assert!(inject(&InputEvent::Key {
            code: 30,
            down: true
        })
        .is_err());
    }
}
