//! Ctrl+V, Enter and Backspace through the Wayland virtual keyboard protocol (wlroots compositors, niri, Hyprland).

use std::io::Write;
use std::os::fd::AsFd;
use std::time::Instant;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_registry::WlRegistry, wl_seat::WlSeat};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, delegate_noop};
use wayland_protocols_misc::zwp_virtual_keyboard_v1::client::{
    zwp_virtual_keyboard_manager_v1::ZwpVirtualKeyboardManagerV1,
    zwp_virtual_keyboard_v1::ZwpVirtualKeyboardV1,
};

// evdev code; KEYMAP uses the xkb keycode, which is 8 higher
const KEY_V: u32 = 47;
const KEY_ENTER: u32 = 28;
const KEY_BACKSPACE: u32 = 14;
const CONTROL_MASK: u32 = 1 << 2;
const KEYMAP_FORMAT_XKB_V1: u32 = 1;

const KEYMAP: &str = r#"xkb_keymap {
    xkb_keycodes "eckermann" { minimum = 8; maximum = 255; <LCTL> = 37; <V> = 55; <RTRN> = 36; <BKSP> = 22; };
    xkb_types "eckermann" { include "complete" };
    xkb_compatibility "eckermann" { include "complete" };
    xkb_symbols "eckermann" {
        key <LCTL> { [ Control_L ] };
        key <V> { [ v, V ] };
        key <RTRN> { [ Return ] };
        key <BKSP> { [ BackSpace ] };
        modifier_map Control { <LCTL> };
    };
};
"#;

pub struct Keys {
    conn: Connection,
    queue: EventQueue<State>,
    keyboard: ZwpVirtualKeyboardV1,
    epoch: Instant,
}

impl Keys {
    pub fn new() -> Result<Self, String> {
        let conn = Connection::connect_to_env().map_err(|e| e.to_string())?;
        let (globals, mut queue) =
            registry_queue_init::<State>(&conn).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let seat: WlSeat = globals.bind(&qh, 1..=1, ()).map_err(|e| e.to_string())?;
        let manager: ZwpVirtualKeyboardManagerV1 = globals
            .bind(&qh, 1..=1, ())
            .map_err(|_| "compositor lacks zwp_virtual_keyboard_manager_v1")?;
        let keyboard = manager.create_virtual_keyboard(&seat, &qh, ());
        let file = keymap_file()?;
        keyboard.keymap(KEYMAP_FORMAT_XKB_V1, file.as_fd(), KEYMAP.len() as u32 + 1);
        // surfaces protocol errors (e.g. unauthorized) now rather than on the first paste
        queue.roundtrip(&mut State).map_err(|e| e.to_string())?;
        Ok(Self {
            conn,
            queue,
            keyboard,
            epoch: Instant::now(),
        })
    }

    pub fn paste(&mut self) -> Result<(), String> {
        self.tap(KEY_V, CONTROL_MASK)
    }

    pub fn enter(&mut self) -> Result<(), String> {
        self.tap(KEY_ENTER, 0)
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        self.tap(KEY_BACKSPACE, 0)
    }

    fn tap(&mut self, key: u32, modifiers: u32) -> Result<(), String> {
        let t = self.epoch.elapsed().as_millis() as u32;
        // same sequence as wtype: modifier state set directly, no Ctrl key events
        self.keyboard.modifiers(modifiers, 0, 0, 0);
        self.keyboard.key(t, key, 1);
        self.keyboard.key(t + 1, key, 0);
        self.keyboard.modifiers(0, 0, 0, 0);
        self.queue
            .roundtrip(&mut State)
            .map_err(|e| e.to_string())?;
        self.conn.flush().map_err(|e| e.to_string())
    }
}

fn keymap_file() -> Result<std::fs::File, String> {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").ok_or("XDG_RUNTIME_DIR is not set")?;
    let path = std::path::Path::new(&dir).join(format!("eckermann-keymap-{}", std::process::id()));
    // read access too: the compositor mmaps this fd
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    // the protocol expects a NUL-terminated string
    file.write_all(KEYMAP.as_bytes())
        .and_then(|()| file.write_all(&[0]))
        .map_err(|e| e.to_string())?;
    Ok(file)
}

struct State;

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as wayland_client::Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore WlSeat);
delegate_noop!(State: ZwpVirtualKeyboardManagerV1);
delegate_noop!(State: ZwpVirtualKeyboardV1);
