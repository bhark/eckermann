//! Global dictation key: a niri bind, an X11 key grab or the GNOME/KDE portal on Linux, an event
//! tap on macOS, a registered hotkey on Windows. Other desktops get the command to bind in their own settings.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "linux")]
mod niri;
#[cfg(windows)]
mod win;

#[cfg(target_os = "macos")]
pub use mac::{apply, start};
use serde::Serialize;
#[cfg(windows)]
pub use win::{apply, start};

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Status {
    #[cfg(target_os = "linux")]
    Niri {
        included: bool,
        include_line: String,
    },
    #[cfg(target_os = "linux")]
    X11,
    #[cfg(target_os = "linux")]
    Portal,
    #[cfg(target_os = "linux")]
    Manual {
        command: String,
        snippet: Option<String>,
    },
    #[cfg(target_os = "macos")]
    Mac,
    #[cfg(windows)]
    Windows,
}

#[cfg(target_os = "macos")]
pub fn status() -> Status {
    Status::Mac
}

#[cfg(windows)]
pub fn status() -> Status {
    Status::Windows
}

#[cfg(target_os = "linux")]
pub fn status() -> Status {
    if let Some(status) = niri::status() {
        return status;
    }
    if crate::x11::session() {
        return Status::X11;
    }
    if crate::portal::session() {
        return Status::Portal;
    }
    let command = format!("{} --ptt toggle", sender().display());
    // F2 only as an example
    let snippet = if std::env::var_os("SWAYSOCK").is_some() {
        Some(format!("bindsym F2 exec {command}"))
    } else if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
        Some(format!("bind = , F2, exec, {command}"))
    } else {
        None
    };
    Status::Manual { command, snippet }
}

/// Binds the saved key; on niri again with this executable's path, as niri runs the sender, not `on_press`.
#[cfg(target_os = "linux")]
pub fn start(combo: Option<&str>, on_press: impl Fn() + Send + 'static) {
    if crate::x11::session() {
        return crate::x11::listen(combo, on_press);
    }
    if portal() {
        return crate::portal::listen(combo, on_press);
    }
    if let Some(Err(e)) = combo.map(niri::apply) {
        log::error!("hotkey: {e}");
    }
}

/// Makes `combo` (niri key syntax, e.g. "Ctrl+Alt+Space") the dictation key.
#[cfg(target_os = "linux")]
pub fn apply(combo: &str) -> Result<(), String> {
    if crate::x11::session() {
        return crate::x11::grab(combo);
    }
    match portal() {
        true => crate::portal::rebind(combo),
        false => niri::apply(combo),
    }
}

#[cfg(target_os = "linux")]
fn portal() -> bool {
    niri::status().is_none() && crate::portal::session()
}

/// The small sender beside us, else the app itself.
#[cfg(target_os = "linux")]
fn sender() -> std::path::PathBuf {
    // inside an AppImage current_exe is a temporary mount path
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        return appimage.into();
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| "eckermann".into());
    // cargo run builds only the app
    let sender = exe.with_file_name("eckermann-ptt");
    if sender.exists() { sender } else { exe }
}
