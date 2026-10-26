//! Puts text into the focused app: clipboard first, then a synthetic paste shortcut.
//! Line breaks are typed as Enter keys.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux::Keys;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::Keys;

// ponytail: no key sender elsewhere, so text stays on the clipboard
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
enum Keys {}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Keys {
    fn new() -> Result<Self, String> {
        Err("no key sender on this platform".into())
    }

    fn paste(&mut self) -> Result<(), String> {
        match *self {}
    }

    fn enter(&mut self) -> Result<(), String> {
        match *self {}
    }

    fn backspace(&mut self) -> Result<(), String> {
        match *self {}
    }
}

use crate::text::Edit;
use std::time::Duration;

// apps fetch the clipboard asynchronously after the shortcut; restoring sooner pastes the old text
const RESTORE_AFTER: Duration = Duration::from_millis(300);

pub struct Paster {
    // must outlive the paste: on Wayland the owning process serves the clipboard
    clipboard: arboard::Clipboard,
    keys: Option<Keys>,
}

impl Paster {
    pub fn new() -> Result<Self, String> {
        let clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        let mut paster = Self {
            clipboard,
            keys: None,
        };
        paster.find_keys();
        Ok(paster)
    }

    // tried again before each edit: on macOS the permission may come while the app runs
    fn find_keys(&mut self) {
        if self.keys.is_none() {
            self.keys = Keys::new()
                .inspect_err(|e| log::warn!("cannot send the paste shortcut: {e}"))
                .ok();
        }
    }

    /// Returns false when the text is only on the clipboard and the user must paste it.
    pub fn apply(&mut self, edit: &Edit) -> Result<bool, String> {
        self.find_keys();
        if edit.delete > 0 {
            let Some(keys) = &mut self.keys else {
                return Ok(false);
            };
            for _ in 0..edit.delete {
                keys.backspace()?;
            }
        }
        match edit.text.as_str() {
            "" => Ok(true),
            text => self.paste(text),
        }
    }

    fn paste(&mut self, text: &str) -> Result<bool, String> {
        // ponytail: only text is restored; an image on the clipboard is lost
        let previous = self.clipboard.get_text().ok();
        let Some(keys) = &mut self.keys else {
            self.clipboard.set_text(text).map_err(|e| e.to_string())?;
            return Ok(false);
        };
        // line breaks are typed as Enter, like a person would; a pasted one did not show in testing
        for (i, line) in text.split('\n').enumerate() {
            if i > 0 {
                keys.enter()?;
            }
            if !line.is_empty() {
                self.clipboard.set_text(line).map_err(|e| e.to_string())?;
                keys.paste()?;
                // the next clipboard change or key waits until the app has fetched this text
                std::thread::sleep(RESTORE_AFTER);
            }
        }
        if let Some(previous) = previous {
            let _ = self.clipboard.set_text(previous);
        }
        Ok(true)
    }
}
