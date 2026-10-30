//! GNOME and KDE on Wayland, through the desktop portals: the dictation key by GlobalShortcuts
//! and the paste keys by RemoteDesktop. The desktop asks the user once for each.

use crate::ui::{Text, text};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions};
use ashpd::desktop::{PersistMode, ResponseError, Session};
use ashpd::enumflags2::BitFlags;
use futures_channel::mpsc::{UnboundedSender, unbounded};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
use std::sync::{Mutex, OnceLock};
use tauri::async_runtime;

const APP_ID: &str = "dk.forn.eckermann";
const SHORTCUT: &str = "dictate";

/// Whether the desktop's portals carry the dictation key and the paste keys.
pub fn session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.split(':').any(|d| d == "GNOME" || d == "KDE"))
}

/// The portals know an unsandboxed app by a desktop file named for its id, then by this registration.
fn register() -> Result<(), String> {
    static DONE: OnceLock<Result<(), String>> = OnceLock::new();
    DONE.get_or_init(|| {
        let file = data_home().join(format!("applications/{APP_ID}.desktop"));
        let exe = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| "eckermann".into());
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName=Eckermann\nExec=\"{}\"\nNoDisplay=true\n",
            exe.display()
        );
        std::fs::create_dir_all(file.parent().unwrap_or(&file)).map_err(|e| e.to_string())?;
        std::fs::write(&file, entry).map_err(|e| e.to_string())?;
        let id = APP_ID.try_into().map_err(|e: ashpd::Error| e.to_string())?;
        async_runtime::block_on(ashpd::register_host_app(id)).map_err(|e| e.to_string())
    })
    .clone()
}

fn data_home() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
}

// a new combo for the running shortcuts session
static REBIND: Mutex<Option<UnboundedSender<String>>> = Mutex::new(None);

/// Binds `combo` once the user picks one, and calls `on_press` on each press.
pub fn listen(combo: Option<&str>, on_press: impl Fn() + Send + 'static) {
    if let Err(e) = register() {
        return log::error!("hotkey: {e}");
    }
    let (tx, rx) = unbounded();
    *REBIND.lock().unwrap() = Some(tx);
    let combo = combo.map(str::to_string);
    async_runtime::spawn(async move {
        if let Err(e) = shortcuts(combo, rx, on_press).await {
            log::error!("hotkey: {e}");
        }
    });
}

/// Asks the desktop to bind `combo`; it shows the user the key it suggests.
pub fn rebind(combo: &str) -> Result<(), String> {
    let rebind = REBIND.lock().unwrap();
    let tx = rebind.as_ref().ok_or("no shortcuts session")?;
    tx.unbounded_send(combo.to_string())
        .map_err(|e| e.to_string())
}

enum Event {
    Pressed(String),
    Bind(String),
}

async fn shortcuts(
    combo: Option<String>,
    combos: futures_channel::mpsc::UnboundedReceiver<String>,
    on_press: impl Fn(),
) -> ashpd::Result<()> {
    let proxy = GlobalShortcuts::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    // the desktop keeps the binding across restarts; binding again would ask the user again
    let listed = proxy
        .list_shortcuts(&session, Default::default())
        .await?
        .response()?;
    let bound = listed.shortcuts().iter().find(|s| s.id() == SHORTCUT);
    if let Some(s) = bound {
        log::info!("{} bound to {}", s.id(), s.trigger_description());
    }
    let first = combo.filter(|_| bound.is_none());
    let pressed = proxy.receive_activated().await?;
    let mut events = futures_util::stream::select(
        pressed.map(|a| Event::Pressed(a.shortcut_id().to_string())),
        futures_util::stream::iter(first)
            .chain(combos)
            .map(Event::Bind),
    );
    while let Some(event) = events.next().await {
        match event {
            Event::Pressed(id) if id == SHORTCUT => on_press(),
            Event::Pressed(_) => {}
            Event::Bind(combo) => {
                let shortcut = NewShortcut::new(SHORTCUT, text(Text::Dictate))
                    .preferred_trigger(trigger(&combo).as_str());
                let bound = proxy
                    .bind_shortcuts(&session, &[shortcut], None, Default::default())
                    .await
                    .and_then(|request| request.response());
                match bound {
                    Ok(bound) => {
                        for s in bound.shortcuts() {
                            log::info!("{} bound to {}", s.id(), s.trigger_description());
                        }
                    }
                    Err(e) => log::error!("hotkey: {e}"),
                }
            }
        }
    }
    Ok(())
}

/// A niri combo ("Ctrl+Alt+Space") in the shortcuts spec's syntax ("CTRL+ALT+space").
fn trigger(combo: &str) -> String {
    combo
        .split('+')
        .map(|key| match key {
            "Ctrl" => "CTRL".to_string(),
            "Alt" => "ALT".to_string(),
            "Shift" => "SHIFT".to_string(),
            "Super" => "LOGO".to_string(),
            "Space" => "space".to_string(),
            // keysym names of letters are lowercase
            k if k.chars().count() == 1 => k.to_lowercase(),
            k => k.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

// the user said no in the dialog; asking at every edit would nag
static DENIED: AtomicBool = AtomicBool::new(false);

/// Keys sent by the desktop, which asks the user the first time and keeps the answer.
pub struct Keys {
    proxy: RemoteDesktop,
    session: Session<RemoteDesktop>,
}

impl Keys {
    pub fn new() -> Result<Self, String> {
        if DENIED.load(SeqCst) {
            return Err("remote control was refused".into());
        }
        register()?;
        let token_file = data_home().join(format!("{APP_ID}/portal-token"));
        let started = async_runtime::block_on(async {
            let proxy = RemoteDesktop::new().await?;
            let session = proxy.create_session(Default::default()).await?;
            let token = std::fs::read_to_string(&token_file).ok();
            proxy
                .select_devices(
                    &session,
                    SelectDevicesOptions::default()
                        .set_devices(BitFlags::from(DeviceType::Keyboard))
                        .set_persist_mode(PersistMode::ExplicitlyRevoked)
                        .set_restore_token(token.as_deref()),
                )
                .await?;
            let selected = proxy
                .start(&session, None, Default::default())
                .await?
                .response()?;
            // gnome's dialog shares with its interaction switch off unless the user turns it on
            let keyboard = selected.devices().contains(DeviceType::Keyboard);
            if let Some(token) = selected.restore_token().filter(|_| keyboard) {
                let _ = std::fs::write(&token_file, token);
            }
            Ok((proxy, session, keyboard))
        });
        match started {
            Ok((proxy, session, true)) => Ok(Self { proxy, session }),
            Ok((_, _, false)) | Err(ashpd::Error::Response(ResponseError::Cancelled)) => {
                DENIED.store(true, SeqCst);
                Err("remote control was refused".into())
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn keys(&self, keysyms: &[(u32, KeyState)]) -> Result<(), String> {
        async_runtime::block_on(async {
            for &(keysym, state) in keysyms {
                self.proxy
                    .notify_keyboard_keysym(&self.session, keysym as i32, state, Default::default())
                    .await?;
            }
            Ok::<_, ashpd::Error>(())
        })
        .map_err(|e| e.to_string())
    }

    fn tap(&self, keysym: u32) -> Result<(), String> {
        self.keys(&[(keysym, KeyState::Pressed), (keysym, KeyState::Released)])
    }

    pub fn paste(&mut self) -> Result<(), String> {
        use crate::x11::{CONTROL_L, V};
        self.keys(&[
            (CONTROL_L, KeyState::Pressed),
            (V, KeyState::Pressed),
            (V, KeyState::Released),
            (CONTROL_L, KeyState::Released),
        ])
    }

    pub fn enter(&mut self) -> Result<(), String> {
        self.tap(crate::x11::RETURN)
    }

    pub fn backspace(&mut self) -> Result<(), String> {
        self.tap(crate::x11::BACKSPACE)
    }

    pub fn delete_word(&mut self) -> Result<(), String> {
        use crate::x11::{BACKSPACE, CONTROL_L};
        self.keys(&[
            (CONTROL_L, KeyState::Pressed),
            (BACKSPACE, KeyState::Pressed),
            (BACKSPACE, KeyState::Released),
            (CONTROL_L, KeyState::Released),
        ])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn writes_triggers_in_the_shortcuts_syntax() {
        assert_eq!(super::trigger("Ctrl+Alt+Space"), "CTRL+ALT+space");
        assert_eq!(super::trigger("Super+Shift+D"), "LOGO+SHIFT+d");
        assert_eq!(super::trigger("Ctrl+F2"), "CTRL+F2");
        assert_eq!(super::trigger("Alt+oslash"), "ALT+oslash");
    }
}
