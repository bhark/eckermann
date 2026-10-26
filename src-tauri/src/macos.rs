//! macOS permissions: the microphone, and accessibility for the dictation key and the paste keys.

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_core_graphics::{CGPreflightPostEventAccess, CGRequestPostEventAccess};
use objc2_foundation::NSString;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    static AVMediaTypeAudio: &'static NSString;
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
        accessibility: CGPreflightPostEventAccess(),
    }
}

/// Shows the system's request dialogs; macOS shows each only once.
pub fn request() {
    if microphone() == NOT_DETERMINED {
        request_microphone();
    }
    if !CGPreflightPostEventAccess() {
        CGRequestPostEventAccess();
    }
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
