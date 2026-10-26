//! X11 sessions: the dictation key by a key grab, and the paste keys by XTest.

use std::collections::HashMap;
use std::sync::OnceLock;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event;
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    ConnectionExt as _, Grab, GrabMode, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, Keycode, ModMask,
    Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

// keysyms
pub(crate) const CONTROL_L: u32 = 0xffe3;
pub(crate) const V: u32 = 0x76;
pub(crate) const RETURN: u32 = 0xff0d;
pub(crate) const BACKSPACE: u32 = 0xff08;

/// Whether this is an X11 session, where gtk and the clipboard talk to X as well.
pub fn session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_none() && std::env::var_os("DISPLAY").is_some()
}

struct Display {
    conn: RustConnection,
    root: Window,
}

fn connect() -> Result<Display, String> {
    let (conn, screen) = x11rb::connect(None).map_err(text)?;
    let root = conn.setup().roots[screen].root;
    Ok(Display { conn, root })
}

// grabs belong to the connection that made them, so rebinding uses this one
static GRABBER: OnceLock<Display> = OnceLock::new();

/// Grabs `combo` and calls `on_press` on each press, on a thread of its own.
pub fn listen(combo: Option<&str>, on_press: impl Fn() + Send + 'static) {
    let combo = combo.map(String::from);
    std::thread::spawn(move || {
        if let Err(e) = run(combo.as_deref(), on_press) {
            log::error!("hotkey: {e}");
        }
    });
}

fn run(combo: Option<&str>, on_press: impl Fn()) -> Result<(), String> {
    let display = connect()?;
    // a held key then repeats presses without the releases between them
    display
        .conn
        .xkb_use_extension(1, 0)
        .map_err(text)?
        .reply()
        .map_err(text)?;
    let flag = xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT;
    display
        .conn
        .xkb_per_client_flags(
            xkb::ID::USE_CORE_KBD.into(),
            flag,
            flag,
            0u32.into(),
            0u32.into(),
            0u32.into(),
        )
        .map_err(text)?
        .reply()
        .map_err(text)?;
    let display = GRABBER.get_or_init(|| display);
    if let Some(combo) = combo {
        grab(combo)?;
    }
    let mut held = false;
    loop {
        match display.conn.wait_for_event().map_err(text)? {
            Event::KeyPress(_) if !held => {
                held = true;
                on_press();
            }
            Event::KeyRelease(_) => held = false,
            _ => {}
        }
    }
}

/// Grabs `combo` (niri syntax, whose key names are xkb keysym names) in place of the last one.
pub fn grab(combo: &str) -> Result<(), String> {
    let display = GRABBER.get().ok_or("no X11 connection")?;
    let (mods, key) = combo.rsplit_once('+').unwrap_or(("", combo));
    let mut mask = ModMask::from(0u16);
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        mask |= match m {
            "Ctrl" => ModMask::CONTROL,
            "Alt" => ModMask::M1,
            "Shift" => ModMask::SHIFT,
            "Super" => ModMask::M4,
            _ => return Err(format!("unknown modifier {m} in {combo}")),
        };
    }
    let sym = keysym(key).ok_or_else(|| format!("unknown key {key}"))?;
    let code = *keycodes(&display.conn)?
        .get(&sym)
        .ok_or_else(|| format!("no {key} on this keyboard"))?;
    let conn = &display.conn;
    conn.ungrab_key(Grab::ANY, display.root, ModMask::ANY)
        .map_err(text)?;
    // X counts caps lock and num lock as modifiers, so grab with and without them
    for extra in [
        ModMask::from(0u16),
        ModMask::LOCK,
        ModMask::M2,
        ModMask::LOCK | ModMask::M2,
    ] {
        conn.grab_key(
            false,
            display.root,
            mask | extra,
            code,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )
        .map_err(text)?
        .check()
        .map_err(|_| format!("another program holds the key combination {combo}"))?;
    }
    Ok(())
}

/// Ctrl+V, Enter and Backspace as XTest key events.
pub struct Keys {
    display: Display,
    control: Keycode,
    v: Keycode,
    enter: Keycode,
    backspace: Keycode,
}

impl Keys {
    pub fn new() -> Result<Self, String> {
        let display = connect()?;
        if display
            .conn
            .extension_information("XTEST")
            .map_err(text)?
            .is_none()
        {
            return Err("the X server lacks XTEST".into());
        }
        let codes = keycodes(&display.conn)?;
        let code = |sym: u32| {
            codes
                .get(&sym)
                .copied()
                .ok_or_else(|| format!("no keysym {sym:#x} on this keyboard"))
        };
        Ok(Self {
            control: code(CONTROL_L)?,
            v: code(V)?,
            enter: code(RETURN)?,
            backspace: code(BACKSPACE)?,
            display,
        })
    }

    pub fn paste(&mut self) -> Result<(), String> {
        self.tap(&[self.control, self.v])
    }

    pub fn enter(&mut self) -> Result<(), String> {
        self.tap(&[self.enter])
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        self.tap(&[self.backspace])
    }

    fn tap(&self, keys: &[Keycode]) -> Result<(), String> {
        let (conn, root) = (&self.display.conn, self.display.root);
        for &key in keys {
            conn.xtest_fake_input(KEY_PRESS_EVENT, key, 0, root, 0, 0, 0)
                .map_err(text)?;
        }
        for &key in keys.iter().rev() {
            conn.xtest_fake_input(KEY_RELEASE_EVENT, key, 0, root, 0, 0, 0)
                .map_err(text)?;
        }
        conn.sync().map_err(text)
    }
}

/// The first keycode that types each keysym on the server's keyboard.
fn keycodes(conn: &RustConnection) -> Result<HashMap<u32, Keycode>, String> {
    let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
    let map = conn
        .get_keyboard_mapping(min, max - min + 1)
        .map_err(text)?
        .reply()
        .map_err(text)?;
    let mut codes = HashMap::new();
    for (code, syms) in (min..=max).zip(map.keysyms.chunks(map.keysyms_per_keycode.max(1) as usize))
    {
        for &sym in syms.iter().filter(|&&sym| sym != 0) {
            codes.entry(sym).or_insert(code);
        }
    }
    Ok(codes)
}

/// Keysym of a niri key name; letters by their lower case keysym, as the layout types them.
fn keysym(name: &str) -> Option<u32> {
    let mut chars = name.chars();
    if let (Some(c @ ('A'..='Z' | '0'..='9')), None) = (chars.next(), chars.next()) {
        return Some(c.to_ascii_lowercase() as u32);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        return (1..=35).contains(&n).then_some(0xffbe + n - 1);
    }
    Some(match name {
        "Space" => 0x20,
        "Return" => 0xff0d,
        "Tab" => 0xff09,
        "BackSpace" => 0xff08,
        "Delete" => 0xffff,
        "Insert" => 0xff63,
        "Home" => 0xff50,
        "End" => 0xff57,
        "Page_Up" => 0xff55,
        "Page_Down" => 0xff56,
        "Left" => 0xff51,
        "Up" => 0xff52,
        "Right" => 0xff53,
        "Down" => 0xff54,
        "Pause" => 0xff13,
        "Scroll_Lock" => 0xff14,
        "Print" => 0xff61,
        "Menu" => 0xff67,
        "ae" => 0xe6,
        "oslash" => 0xf8,
        "aring" => 0xe5,
        _ => return None,
    })
}

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_niri_keys_as_keysyms() {
        assert_eq!(keysym("A"), Some(0x61));
        assert_eq!(keysym("7"), Some(0x37));
        assert_eq!(keysym("F1"), Some(0xffbe));
        assert_eq!(keysym("F12"), Some(0xffc9));
        assert_eq!(keysym("oslash"), Some(0xf8));
        assert_eq!(keysym("F0"), None);
        assert_eq!(keysym("Hyper"), None);
    }
}
