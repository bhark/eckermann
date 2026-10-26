//! Paste and edit keys as CGEvents, which macOS delivers only with the accessibility permission.

use objc2_core_graphics::{
    CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode, CGPreflightPostEventAccess,
};

// kVK_ANSI_V, kVK_Return, kVK_Delete
const V: CGKeyCode = 9;
const RETURN: CGKeyCode = 36;
const BACKSPACE: CGKeyCode = 51;

pub struct Keys;

impl Keys {
    pub fn new() -> Result<Self, String> {
        match CGPreflightPostEventAccess() {
            true => Ok(Self),
            false => Err("no accessibility permission".into()),
        }
    }

    pub fn paste(&mut self) -> Result<(), String> {
        tap(V, CGEventFlags::MaskCommand)
    }

    pub fn enter(&mut self) -> Result<(), String> {
        tap(RETURN, CGEventFlags::empty())
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        tap(BACKSPACE, CGEventFlags::empty())
    }
}

fn tap(key: CGKeyCode, flags: CGEventFlags) -> Result<(), String> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(None, key, down)
            .ok_or_else(|| "cannot make a key event".to_string())?;
        // set, not added to: fn or another key the user still holds must not join in
        CGEvent::set_flags(Some(&event), flags);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
    Ok(())
}
