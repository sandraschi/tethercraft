//! Host-side input injection via enigo.
//!
//! Validation lives in `tethercraft-input`; this module is the OS endpoint.
//! Real on Windows, `NotImplemented` elsewhere. Key codes are Windows
//! virtual-key codes (u16); the controller maps its keys to them.

#[cfg(windows)]
use enigo::{Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use tethercraft_core::TetherError;
use tethercraft_input::InputEvent;

/// Validate a raw key code for the `Keyboard::raw` path.
pub fn map_vk(code: u32) -> Result<u16, TetherError> {
    if code == 0 || code > 0xFFFF {
        return Err(TetherError::Invalid(
            "key code must be a 16-bit VK".to_string(),
        ));
    }
    Ok(code as u16)
}

#[cfg(windows)]
fn backend() -> Result<Enigo, TetherError> {
    Enigo::new(&Settings::default())
        .map_err(|e| TetherError::Invalid(format!("input backend: {e:?}")))
}

#[cfg(windows)]
fn fail(e: enigo::InputError) -> TetherError {
    TetherError::Invalid(format!("inject: {e:?}"))
}

#[cfg(windows)]
fn click(e: &mut Enigo, button: u8) -> Result<(), TetherError> {
    let b = match button {
        0 => Button::Left,
        1 => Button::Right,
        2 => Button::Middle,
        _ => {
            return Err(TetherError::Invalid(
                "mouse buttons 0-2 are injected; 3+ need a mapping".to_string(),
            ))
        }
    };
    e.button(b, Direction::Click).map_err(fail)
}

#[cfg(windows)]
pub fn inject_event(ev: &InputEvent) -> Result<(), TetherError> {
    tethercraft_input::validate(ev)?;
    let mut e = backend()?;
    match ev {
        InputEvent::MouseMove { x, y } => e.move_mouse(*x, *y, Coordinate::Abs).map_err(fail)?,
        InputEvent::MouseClick { button } => click(&mut e, *button)?,
        InputEvent::MouseScroll { dx, dy } => {
            if *dx != 0 {
                e.scroll(*dx, enigo::Axis::Horizontal).map_err(fail)?;
            }
            if *dy != 0 {
                e.scroll(*dy, enigo::Axis::Vertical).map_err(fail)?;
            }
        }
        InputEvent::Key { code, down } => {
            let vk = map_vk(*code)?;
            let dir = if *down {
                Direction::Press
            } else {
                Direction::Release
            };
            e.raw(vk, dir).map_err(fail)?;
        }
        InputEvent::TouchTap { x, y } => {
            e.move_mouse(*x, *y, Coordinate::Abs).map_err(fail)?;
            click(&mut e, 0)?;
        }
        InputEvent::TouchHold { x, y } => {
            // A genuine long-press: down, hold, up. The event carries no
            // duration, so the milestone uses a fixed 500 ms hold.
            e.move_mouse(*x, *y, Coordinate::Abs).map_err(fail)?;
            e.button(Button::Left, Direction::Press).map_err(fail)?;
            std::thread::sleep(std::time::Duration::from_millis(500));
            e.button(Button::Left, Direction::Release).map_err(fail)?;
        }
        InputEvent::TouchSwipe { x0, y0, x1, y1 } => {
            e.move_mouse(*x0, *y0, Coordinate::Abs).map_err(fail)?;
            e.button(Button::Left, Direction::Press).map_err(fail)?;
            e.move_mouse(*x1, *y1, Coordinate::Abs).map_err(fail)?;
            e.button(Button::Left, Direction::Release).map_err(fail)?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn inject_event(ev: &InputEvent) -> Result<(), TetherError> {
    tethercraft_input::validate(ev)?;
    Err(TetherError::NotImplemented(
        "input injection is Windows-only in Tier 1".to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vk_bounds() {
        assert!(map_vk(0x0D).is_ok());
        assert!(map_vk(0).is_err());
        assert!(map_vk(0x1_0000).is_err());
    }
}
