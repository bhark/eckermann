pub mod audio;
pub mod commands;
mod hotkey;
pub mod ipc;
#[cfg(target_os = "linux")]
mod layer;
mod overlay;
mod paste;
mod pipeline;
pub mod punct;
mod settings;
pub mod stt;
pub mod text;

use ipc::Command;
use pipeline::Pipeline;
use settings::Settings;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};

struct AppState {
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
}

/// Forwards a CLI command to the running instance, or starts the app if none runs.
pub fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = ipc::from_args(&args).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(2)
    });
    match ipc::send(cmd.unwrap_or(Command::Show)) {
        Ok(()) => {}
        Err(e) if cmd.is_some() => {
            eprintln!("eckermann is not running: {e}");
            std::process::exit(1)
        }
        Err(_) => run(),
    }
}

fn run() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                // whisper.cpp logs each vad pass in eight lines, which would rotate the log away
                .level_for("whisper_rs::whisper_logging_hook", log::LevelFilter::Warn)
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            hotkey_status,
            set_hotkey
        ])
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = settings::load(&settings_path);
            // rewrites the bind with this executable's path
            if let Some(Err(e)) = settings.hotkey.as_deref().map(hotkey::apply) {
                log::error!("hotkey: {e}");
            }
            let settings = Arc::new(Mutex::new(settings));
            let models = app.path().app_data_dir()?.join("models");
            let overlay = overlay::Overlay::new(app.handle())?;
            app.manage(Pipeline::new(models, settings.clone(), move |event| {
                overlay.handle(event)
            }));
            app.manage(AppState {
                settings,
                settings_path,
            });
            let handle = app.handle().clone();
            ipc::listen(move |cmd| on_command(&handle, cmd))?;
            tray(app.handle())?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build app");
    app.run(|_, event| {
        // background app: closing the last window must not quit
        if let RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
        }
    });
}

fn on_command(app: &AppHandle, cmd: Command) {
    match cmd {
        Command::Show => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || show_settings(&handle));
        }
        Command::PttStart => app.state::<Pipeline>().start(),
        Command::PttStop => app.state::<Pipeline>().stop(),
        Command::PttToggle => app.state::<Pipeline>().toggle(),
    }
}

// the panel plus the transparent margin around it, as in Settings.svelte
const SETTINGS_WIDTH: i32 = 496;
const SETTINGS_HEIGHT: i32 = 560;
const SETTINGS_MARGIN: i32 = 28;

fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Eckermann")
        .inner_size(SETTINGS_WIDTH.into(), SETTINGS_HEIGHT.into())
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .visible(false)
        .build();
    let window = match built {
        Ok(window) => window,
        Err(e) => return log::error!("settings window: {e}"),
    };
    #[cfg(target_os = "linux")]
    if let Ok(gtk) = window.gtk_window() {
        layer::panel(&gtk, SETTINGS_WIDTH, SETTINGS_HEIGHT, SETTINGS_MARGIN);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "Indstillinger", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Afslut", true, None::<&str>)?;
    TrayIconBuilder::new()
        .icon(Image::from_bytes(include_bytes!("../icons/tray.png"))?)
        .tooltip("Eckermann")
        .menu(&Menu::with_items(app, &[&settings, &quit])?)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn set_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    let mut current = state.settings.lock().unwrap();
    // only set_hotkey changes the key, since it must bind it too
    let settings = Settings {
        hotkey: current.hotkey.clone(),
        ..settings
    };
    settings::save(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    *current = settings;
    Ok(())
}

#[tauri::command]
fn hotkey_status() -> hotkey::Status {
    hotkey::status()
}

#[tauri::command]
fn set_hotkey(state: State<AppState>, combo: String) -> Result<(), String> {
    hotkey::apply(&combo)?;
    let mut settings = state.settings.lock().unwrap();
    settings.hotkey = Some(combo);
    settings::save(&state.settings_path, &settings).map_err(|e| e.to_string())
}
