//! macOS permissions: the microphone, and accessibility for the dictation key and the paste keys.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_core_graphics::CGRequestPostEventAccess;
use objc2_foundation::NSString;
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

// AVAuthorizationStatus
const NOT_DETERMINED: isize = 0;
const AUTHORIZED: isize = 3;

#[derive(serde::Serialize)]
pub struct Permissions {
    microphone: bool,
    accessibility: bool,
}

pub fn permissions() -> Permissions {
    Permissions {
        microphone: microphone() == AUTHORIZED,
        accessibility: accessibility(),
    }
}

/// Shows the system's request dialogs; macOS shows each only once.
pub fn request() {
    if microphone() == NOT_DETERMINED {
        request_microphone();
    }
    if !accessibility() {
        CGRequestPostEventAccess();
    }
}

// CGPreflightPostEventAccess keeps its first answer for the life of the process
fn accessibility() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Asks for `what`, "microphone" or "accessibility": the dialog if it was never shown, else System Settings.
pub fn grant(what: &str) {
    if what == "microphone" && microphone() == NOT_DETERMINED {
        return request_microphone();
    }
    let pane = match what {
        "microphone" => "Privacy_Microphone",
        _ => "Privacy_Accessibility",
    };
    let url = format!("x-apple.systempreferences:com.apple.preference.security?{pane}");
    if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
        log::error!("open {pane}: {e}");
    }
}

fn microphone() -> isize {
    unsafe { msg_send![class!(AVCaptureDevice), authorizationStatusForMediaType: AVMediaTypeAudio] }
}

fn request_microphone() {
    let done = RcBlock::new(|_: Bool| {});
    unsafe {
        let _: () = msg_send![
            class!(AVCaptureDevice),
            requestAccessForMediaType: AVMediaTypeAudio,
            completionHandler: &*done
        ];
    }
}

/// Whether the app opens at login, as a login item that System Settings lists by name.
pub fn autostart() -> bool {
    unsafe { SMAppService::mainAppService().status() == SMAppServiceStatus::Enabled }
}

pub fn set_autostart(on: bool) -> Result<(), String> {
    let app = unsafe { SMAppService::mainAppService() };
    let status = unsafe { app.status() };
    if on && status == SMAppServiceStatus::RequiresApproval {
        // registered, but the user switched it off in System Settings
        unsafe { SMAppService::openSystemSettingsLoginItems() };
        return Ok(());
    }
    if on == (status == SMAppServiceStatus::Enabled) {
        return Ok(());
    }
    match on {
        true => unsafe { app.registerAndReturnError() },
        false => unsafe { app.unregisterAndReturnError() },
    }
    .map_err(|e| e.localizedDescription().to_string())
}
