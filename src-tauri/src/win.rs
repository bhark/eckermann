//! Windows: the microphone consent, and an overlay that shows without taking the focus.

use tauri::WebviewWindow;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNOACTIVATE, ShowWindow};
use windows::core::{HSTRING, w};

const CONSENT: &str =
    r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";

#[derive(serde::Serialize)]
pub struct Permissions {
    microphone: bool,
    accessibility: bool,
}

/// Keys need no permission here; a denied microphone records silence.
pub fn permissions() -> Permissions {
    // both switches in Settings, all apps and desktop apps; no value means allowed
    let microphone = [CONSENT.to_string(), format!(r"{CONSENT}\NonPackaged")]
        .iter()
        .all(|key| consent(key).as_deref() != Some("Deny"));
    Permissions {
        microphone,
        accessibility: true,
    }
}

fn consent(key: &str) -> Option<String> {
    let mut value = [0u16; 16];
    let mut bytes = size_of_val(&value) as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(key),
            w!("Value"),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    }
    .ok()
    .ok()?;
    let len = (bytes as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&value[..len]))
}

/// Opens the microphone page of Settings, the only permission there is to give.
pub fn grant(what: &str) {
    if what != "microphone" {
        return;
    }
    let opened = std::process::Command::new("explorer")
        .arg("ms-settings:privacy-microphone")
        .spawn();
    if let Err(e) = opened {
        log::error!("open microphone settings: {e}");
    }
}

// tao shows with SW_SHOW, which takes the focus and with it the paste
pub fn show_inactive(window: &WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
    }
}

// tao never saw the window shown, so its hide would do nothing
pub fn hide(window: &WebviewWindow) {
    if let Ok(hwnd) = window.hwnd() {
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    }
}
