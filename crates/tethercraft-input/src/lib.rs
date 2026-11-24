//! Input events. Validation only; the host enforces permission per message.

use serde::{Deserialize, Serialize};
use tethercraft_core::TetherError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InputEvent {
    MouseMove { x: i32, y: i32 },
    MouseClick { button: u8 },
    MouseScroll { dx: i32, dy: i32 },
    Key { code: u32, down: bool },
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
        assert!(inject(&InputEvent::Key {
            code: 30,
            down: true
        })
        .is_err());
    }
}
