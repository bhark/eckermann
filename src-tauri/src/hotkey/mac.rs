//! An event tap on the session's keys. macOS lets it see keys only with the accessibility permission.

use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGPreflightPostEventAccess,
};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::Duration;

const DEFAULT: &str = "Fn";
// kVK_Function
const FN_KEY: i64 = 63;
const MODIFIERS: CGEventFlags = CGEventFlags::MaskControl
    .union(CGEventFlags::MaskAlternate)
    .union(CGEventFlags::MaskShift)
    .union(CGEventFlags::MaskCommand);

#[derive(Clone, Copy, Debug, PartialEq)]
enum Key {
    /// fn pressed and released alone; with another key it stays a modifier
    Fn,
    Combo {
        code: u16,
        flags: CGEventFlags,
    },
}

static KEY: Mutex<Key> = Mutex::new(Key::Fn);

/// Listens for the saved key, fn when none is saved, and calls `on_press` on each press.
pub fn start(combo: Option<&str>, on_press: impl Fn() + Send + 'static) {
    if let Err(e) = apply(combo.unwrap_or(DEFAULT)) {
        log::error!("hotkey: {e}");
    }
    std::thread::spawn(move || listen(Box::new(on_press)));
}

/// Makes `combo` (e.g. "Ctrl+Alt+Space", or "Fn") the dictation key.
pub fn apply(combo: &str) -> Result<(), String> {
    *KEY.lock().unwrap() = parse(combo)?;
    Ok(())
}

struct Tap {
    on_press: Box<dyn Fn() + Send>,
    port: Option<CFRetained<CFMachPort>>,
    fn_alone: bool,
    // the key whose press we swallowed, so its release goes too
    held: Option<i64>,
}

fn listen(on_press: Box<dyn Fn() + Send>) {
    let mask = [
        CGEventType::KeyDown,
        CGEventType::KeyUp,
        CGEventType::FlagsChanged,
    ]
    .iter()
    .fold(0, |mask, kind| mask | 1 << kind.0);
    // the tap lives as long as the app
    let tap = Box::leak(Box::new(Tap {
        on_press,
        port: None,
        fn_alone: false,
        held: None,
    }));
    let port = loop {
        // ponytail: polls until the user grants accessibility in System Settings
        if CGPreflightPostEventAccess() {
            let port = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    mask,
                    Some(callback),
                    tap as *mut Tap as *mut c_void,
                )
            };
            if let Some(port) = port {
                break port;
            }
            log::warn!("hotkey: no event tap yet");
        }
        std::thread::sleep(Duration::from_secs(2));
    };
    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&port), 0) else {
        return log::error!("hotkey: no run loop source");
    };
    let Some(run_loop) = CFRunLoop::current() else {
        return log::error!("hotkey: no run loop");
    };
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&port, true);
    tap.port = Some(port);
    log::info!("hotkey: listening");
    CFRunLoop::run();
}

unsafe extern "C-unwind" fn callback(
    _: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    tap: *mut c_void,
) -> *mut CGEvent {
    let tap = unsafe { &mut *(tap as *mut Tap) };
    let pass = event.as_ptr();
    let event = Some(unsafe { event.as_ref() });
    if kind == CGEventType::TapDisabledByTimeout || kind == CGEventType::TapDisabledByUserInput {
        if let Some(port) = &tap.port {
            CGEvent::tap_enable(port, true);
        }
        return pass;
    }
    let code = CGEvent::integer_value_field(event, CGEventField::KeyboardEventKeycode);
    let flags = CGEvent::flags(event);
    match *KEY.lock().unwrap() {
        Key::Fn => match kind {
            CGEventType::FlagsChanged if code == FN_KEY => {
                if flags.contains(CGEventFlags::MaskSecondaryFn) {
                    tap.fn_alone = true;
                } else if std::mem::take(&mut tap.fn_alone) {
                    (tap.on_press)();
                }
            }
            _ => tap.fn_alone = false,
        },
        Key::Combo {
            code: key,
            flags: mods,
        } => match kind {
            CGEventType::KeyDown if code == key as i64 && flags & MODIFIERS == mods => {
                if CGEvent::integer_value_field(event, CGEventField::KeyboardEventAutorepeat) == 0 {
                    (tap.on_press)();
                }
                tap.held = Some(code);
                return std::ptr::null_mut();
            }
            CGEventType::KeyUp if tap.held == Some(code) => {
                tap.held = None;
                return std::ptr::null_mut();
            }
            _ => {}
        },
    }
    pass
}

fn parse(combo: &str) -> Result<Key, String> {
    if combo == "Fn" {
        return Ok(Key::Fn);
    }
    let (mods, key) = combo.rsplit_once('+').unwrap_or(("", combo));
    let mut flags = CGEventFlags::empty();
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        flags |= match m {
            "Ctrl" => CGEventFlags::MaskControl,
            "Alt" => CGEventFlags::MaskAlternate,
            "Shift" => CGEventFlags::MaskShift,
            "Super" => CGEventFlags::MaskCommand,
            _ => return Err(format!("unknown modifier {m} in {combo}")),
        };
    }
    let code = keycode(key).ok_or_else(|| format!("no key {key} on a Mac"))?;
    Ok(Key::Combo { code, flags })
}

/// kVK codes from HIToolbox's Events.h, by position on the keyboard.
fn keycode(name: &str) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        0, 11, 8, 2, 14, 3, 5, 4, 34, 38, 40, 37, 46, 45, 31, 35, 12, 15, 1, 17, 32, 9, 13, 7, 16,
        6,
    ];
    const DIGITS: [u16; 10] = [29, 18, 19, 20, 21, 23, 22, 26, 28, 25];
    const F_KEYS: [u16; 20] = [
        122, 120, 99, 118, 96, 97, 98, 100, 101, 109, 103, 111, 105, 107, 113, 106, 64, 79, 80, 90,
    ];
    let mut chars = name.chars();
    if let (Some(c @ 'A'..='Z'), None) = (chars.next(), chars.next()) {
        return Some(LETTERS[c as usize - 'A' as usize]);
    }
    if let Ok(digit @ 0..=9) = name.parse::<usize>() {
        return Some(DIGITS[digit]);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<usize>().ok()) {
        return F_KEYS.get(n.wrapping_sub(1)).copied();
    }
    Some(match name {
        "Space" => 49,
        "Return" => 36,
        "Tab" => 48,
        "BackSpace" => 51,
        "Delete" => 117,
        "Home" => 115,
        "End" => 119,
        "Page_Up" => 116,
        "Page_Down" => 121,
        "Left" => 123,
        "Right" => 124,
        "Down" => 125,
        "Up" => 126,
        // where a Danish keyboard has them
        "ae" => 41,
        "oslash" => 39,
        "aring" => 33,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos_to_key_codes() {
        assert_eq!(parse("Fn"), Ok(Key::Fn));
        assert_eq!(
            parse("Ctrl+Alt+Space"),
            Ok(Key::Combo {
                code: 49,
                flags: CGEventFlags::MaskControl | CGEventFlags::MaskAlternate
            })
        );
        assert_eq!(
            parse("Super+V"),
            Ok(Key::Combo {
                code: 9,
                flags: CGEventFlags::MaskCommand
            })
        );
        assert_eq!(
            parse("F5"),
            Ok(Key::Combo {
                code: 96,
                flags: CGEventFlags::empty()
            })
        );
        assert_eq!(
            parse("Shift+7"),
            Ok(Key::Combo {
                code: 26,
                flags: CGEventFlags::MaskShift
            })
        );
        assert!(parse("Insert").is_err());
        assert!(parse("F21").is_err());
        assert!(parse("F0").is_err());
        assert!(parse("Hyper+A").is_err());
    }
}
