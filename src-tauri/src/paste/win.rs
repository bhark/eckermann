//! Paste and edit keys through SendInput. Windows of apps that run as administrator ignore them.

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_RETURN, VK_V,
};

pub struct Keys;

impl Keys {
    pub fn new() -> Result<Self, String> {
        Ok(Self)
    }

    pub fn paste(&mut self) -> Result<(), String> {
        ctrl(VK_V)
    }

    pub fn enter(&mut self) -> Result<(), String> {
        send(&[(VK_RETURN, true), (VK_RETURN, false)])
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        send(&[(VK_BACK, true), (VK_BACK, false)])
    }

    pub fn delete_word(&mut self) -> Result<(), String> {
        ctrl(VK_BACK)
    }
}

fn ctrl(key: VIRTUAL_KEY) -> Result<(), String> {
    send(&[
        (VK_CONTROL, true),
        (key, true),
        (key, false),
        (VK_CONTROL, false),
    ])
}

fn send(keys: &[(VIRTUAL_KEY, bool)]) -> Result<(), String> {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&(key, down)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    dwFlags: if down {
                        KEYBD_EVENT_FLAGS(0)
                    } else {
                        KEYEVENTF_KEYUP
                    },
                    ..Default::default()
                },
            },
        })
        .collect();
    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    match sent as usize == inputs.len() {
        true => Ok(()),
        false => Err(format!("SendInput took {sent} of {} keys", inputs.len())),
    }
}
