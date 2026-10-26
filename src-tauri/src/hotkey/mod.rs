//! Global dictation key: a niri bind on Linux, an event tap on macOS.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(not(target_os = "macos"))]
mod niri;

#[cfg(target_os = "macos")]
pub use mac::{Status, apply, start, status};
#[cfg(not(target_os = "macos"))]
pub use niri::{Status, apply, start, status};
