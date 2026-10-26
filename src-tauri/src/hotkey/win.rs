//! RegisterHotKey on a thread of its own, since the key's messages go to the registering thread.

use std::sync::{Mutex, mpsc};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MAPVK_VSC_TO_VK, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    MapVirtualKeyW, RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetMessageW, MSG, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, WM_APP, WM_HOTKEY,
};

type Rebind = (String, mpsc::Sender<Result<(), String>>);

// the hotkey thread's id, to wake it, and where it reads new keys
static THREAD: Mutex<Option<(u32, mpsc::Sender<Rebind>)>> = Mutex::new(None);

struct Bound {
    id: i32,
    combo: String,
}

/// Listens for the saved key, none until the user picks one, and calls `on_press` on each press.
pub fn start(combo: Option<&str>, on_press: impl Fn() + Send + 'static) {
    let (tx, rx) = mpsc::channel::<Rebind>();
    let (ready, thread) = mpsc::channel();
    std::thread::spawn(move || {
        let mut msg = MSG::default();
        // makes the queue, so a PostThreadMessage from now on is not lost
        let _ = unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE) };
        let _ = ready.send(unsafe { GetCurrentThreadId() });
        let mut bound = None;
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            match msg.message {
                WM_HOTKEY => on_press(),
                WM_APP => {
                    for (combo, reply) in rx.try_iter() {
                        let _ = reply.send(bind(&mut bound, &combo));
                    }
                }
                _ => {}
            }
        }
    });
    let Ok(thread) = thread.recv() else {
        return log::error!("hotkey: no message thread");
    };
    *THREAD.lock().unwrap() = Some((thread, tx));
    if let Some(Err(e)) = combo.map(apply) {
        log::error!("hotkey: {e}");
    }
}

/// Makes `combo` (e.g. "Ctrl+Alt+Space") the dictation key.
pub fn apply(combo: &str) -> Result<(), String> {
    let (reply, answer) = mpsc::channel();
    {
        let thread = THREAD.lock().unwrap();
        let (id, tx) = thread.as_ref().ok_or("no hotkey thread")?;
        tx.send((combo.to_string(), reply))
            .map_err(|e| e.to_string())?;
        unsafe { PostThreadMessageW(*id, WM_APP, WPARAM(0), LPARAM(0)) }
            .map_err(|e| e.to_string())?;
    }
    answer.recv().map_err(|e| e.to_string())?
}

// runs on the hotkey thread, which owns the registration
fn bind(bound: &mut Option<Bound>, combo: &str) -> Result<(), String> {
    if bound.as_ref().is_some_and(|b| b.combo == combo) {
        return Ok(());
    }
    let (mods, vk) = parse(combo)?;
    // the new key under a new id first, so a key another app holds leaves the old one working
    let id = if bound.as_ref().is_some_and(|b| b.id == 1) {
        2
    } else {
        1
    };
    unsafe { RegisterHotKey(None, id, mods | MOD_NOREPEAT, vk) }
        .map_err(|_| format!("another app uses {combo}"))?;
    let old = bound.replace(Bound {
        id,
        combo: combo.to_string(),
    });
    if let Some(old) = old {
        let _ = unsafe { UnregisterHotKey(None, old.id) };
    }
    Ok(())
}

fn parse(combo: &str) -> Result<(HOT_KEY_MODIFIERS, u32), String> {
    let (mods, key) = combo.rsplit_once('+').unwrap_or(("", combo));
    let mut flags = HOT_KEY_MODIFIERS(0);
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        flags |= match m {
            "Ctrl" => MOD_CONTROL,
            "Alt" => MOD_ALT,
            "Shift" => MOD_SHIFT,
            "Super" => MOD_WIN,
            _ => return Err(format!("unknown modifier {m} in {combo}")),
        };
    }
    let vk = virtual_key(key).ok_or_else(|| format!("no key {key} on this keyboard"))?;
    Ok((flags, vk))
}

/// Letters, digits and æøå by their place on the keyboard, which the layout turns into a key code.
fn virtual_key(name: &str) -> Option<u32> {
    const LETTERS: [u32; 26] = [
        0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32, 0x31, 0x18,
        0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C,
    ];
    const DIGITS: [u32; 10] = [0x0B, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A];
    let mut chars = name.chars();
    let scan = if let (Some(c @ 'A'..='Z'), None) = (chars.next(), chars.next()) {
        Some(LETTERS[c as usize - 'A' as usize])
    } else if let Ok(digit @ 0..=9) = name.parse::<usize>() {
        Some(DIGITS[digit])
    } else {
        // where a Danish keyboard has them
        match name {
            "ae" => Some(0x27),
            "oslash" => Some(0x28),
            "aring" => Some(0x1A),
            _ => None,
        }
    };
    if let Some(scan) = scan {
        return match unsafe { MapVirtualKeyW(scan, MAPVK_VSC_TO_VK) } {
            0 => None,
            vk => Some(vk),
        };
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=24).contains(&n).then(|| 0x6F + n);
    }
    Some(match name {
        "Space" => 0x20,
        "Return" => 0x0D,
        "Tab" => 0x09,
        "BackSpace" => 0x08,
        "Delete" => 0x2E,
        "Insert" => 0x2D,
        "Home" => 0x24,
        "End" => 0x23,
        "Page_Up" => 0x21,
        "Page_Down" => 0x22,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Pause" => 0x13,
        "Scroll_Lock" => 0x91,
        "Print" => 0x2C,
        "Menu" => 0x5D,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combos_to_key_codes() {
        assert_eq!(parse("Ctrl+Alt+Space"), Ok((MOD_CONTROL | MOD_ALT, 0x20)));
        assert_eq!(parse("F5"), Ok((HOT_KEY_MODIFIERS(0), 0x74)));
        assert_eq!(parse("Super+F24"), Ok((MOD_WIN, 0x87)));
        // the runners have a US layout, where places and letters agree
        assert_eq!(parse("Shift+D"), Ok((MOD_SHIFT, 0x44)));
        assert_eq!(parse("Ctrl+7"), Ok((MOD_CONTROL, 0x37)));
        assert!(parse("F25").is_err());
        assert!(parse("F0").is_err());
        assert!(parse("Hyper+A").is_err());
    }
}
