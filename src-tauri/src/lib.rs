pub mod audio;
pub mod commands;
mod edda;
mod hotkey;
pub mod ipc;
pub mod lang;
#[cfg(target_os = "linux")]
mod layer;
#[cfg(target_os = "macos")]
mod macos;
pub mod models;
mod overlay;
mod paste;
mod pipeline;
pub mod punct;
mod settings;
pub mod stt;
pub mod text;
mod ui;
#[cfg(target_os = "linux")]
mod x11;

use ipc::Command;
use lang::Language;
use pipeline::Pipeline;
use settings::Settings;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{
    AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, RunEvent, State, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_updater::UpdaterExt;
use ui::{Text, text};

// the app runs for weeks, so it looks again now and then
const UPDATE_EVERY: Duration = Duration::from_secs(12 * 3600);

struct AppState {
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    models: PathBuf,
    download: Mutex<Download>,
}

#[derive(Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum Download {
    Running { done: u64, total: u64 },
    Failed,
    Done,
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
    // reqwest's rustls comes without a crypto provider; ring, as the updater uses
    let _ = rustls::crypto::ring::default_provider().install_default();
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                // whisper.cpp logs each vad pass in eight lines, which would rotate the log away
                .level_for("whisper_rs::whisper_logging_hook", log::LevelFilter::Warn)
                .build(),
        )
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            get_settings,
            set_settings,
            hotkey_status,
            set_hotkey,
            download_status,
            download_models,
            autostart,
            set_autostart,
            permissions,
            grant,
            microphones,
            fit_settings,
            ui_language
        ])
        .setup(|app| {
            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = settings::load(&settings_path);
            let handle = app.handle().clone();
            hotkey::start(settings.hotkey.as_deref(), move || {
                let handle = handle.clone();
                // the event tap must return at once
                std::thread::spawn(move || on_command(&handle, Command::PttToggle));
            });
            let profile = settings.language.profile();
            let settings = Arc::new(Mutex::new(settings));
            let models = app.path().app_data_dir()?.join("models");
            let overlay = overlay::Overlay::new(app.handle())?;
            app.manage(Pipeline::new(
                models.clone(),
                settings.clone(),
                move |event| overlay.handle(event),
            ));
            // no models yet: a first install, which starts at login from now on
            let first_run = !models::missing(&models, profile).is_empty();
            app.manage(AppState {
                settings,
                settings_path,
                models,
                download: Mutex::new(Download::Done),
            });
            if first_run {
                if let Err(e) = app.autolaunch().enable() {
                    log::error!("autostart: {e}");
                }
                #[cfg(target_os = "macos")]
                macos::request();
                fetch_models(app.handle());
                show_settings(app.handle());
            }
            let handle = app.handle().clone();
            ipc::listen(move |cmd| on_command(&handle, cmd))?;
            tray(app.handle())?;
            keep_updated(app.handle().clone());
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
    let pipeline = app.state::<Pipeline>();
    let download = app.state::<AppState>().download.lock().unwrap().clone();
    match (cmd, download) {
        (Command::Show, _) => {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || show_settings(&handle));
        }
        (Command::PttStop, _) => pipeline.stop(),
        (_, Download::Running { .. }) => pipeline.notice(text(Text::Downloading)),
        (_, Download::Failed) => pipeline.notice(text(Text::ModelsMissing)),
        (Command::PttStart, Download::Done) => pipeline.start(),
        (Command::PttToggle, Download::Done) => pipeline.toggle(),
    }
}

/// Downloads the model files the chosen language misses, unless a download runs already.
fn fetch_models(app: &AppHandle) {
    let state = app.state::<AppState>();
    let profile = state.settings.lock().unwrap().language.profile();
    let files = models::missing(&state.models, profile);
    {
        let mut download = state.download.lock().unwrap();
        if matches!(*download, Download::Running { .. }) {
            return;
        }
        *download = match files.is_empty() {
            true => Download::Done,
            false => Download::Running { done: 0, total: 0 },
        };
    }
    if files.is_empty() {
        return;
    }
    let app = app.clone();
    let dir = state.models.clone();
    tauri::async_runtime::spawn(async move {
        let report = |download: Download| {
            let _ = app.emit("download", &download);
            *app.state::<AppState>().download.lock().unwrap() = download;
        };
        let mut shown = None;
        let result = models::fetch(&dir, &files, |done, total| {
            // a thousand steps are plenty for the bar
            let step = done * 1000 / total;
            if shown != Some(step) {
                shown = Some(step);
                report(Download::Running { done, total });
            }
        })
        .await;
        match result {
            Ok(()) => {
                report(Download::Done);
                // the language may have changed while this ran
                fetch_models(&app);
            }
            Err(e) => {
                log::error!("model download: {e}");
                report(Download::Failed);
            }
        }
    });
}

/// Installs new releases, then restarts once nobody is dictating.
fn keep_updated(app: AppHandle) {
    // the bundler also patches target/release, and the updater would replace that binary with the AppImage
    let installed = match tauri::utils::platform::bundle_type() {
        #[cfg(target_os = "linux")]
        Some(tauri::utils::config::BundleType::AppImage) => app.env().appimage.is_some(),
        bundle => bundle.is_some(),
    };
    if !installed {
        return;
    }
    std::thread::spawn(move || {
        loop {
            match tauri::async_runtime::block_on(install_update(&app)) {
                Ok(true) => {
                    while app.state::<Pipeline>().busy() {
                        std::thread::sleep(Duration::from_secs(5));
                    }
                    app.restart();
                }
                Ok(false) => {}
                Err(e) => log::warn!("update: {e}"),
            }
            std::thread::sleep(UPDATE_EVERY);
        }
    });
}

async fn install_update(app: &AppHandle) -> tauri_plugin_updater::Result<bool> {
    let Some(update) = app.updater()?.check().await? else {
        return Ok(false);
    };
    log::info!("installing version {}", update.version);
    update.download_and_install(|_, _| {}, || {}).await?;
    Ok(true)
}

// the panel plus the transparent margin around it, as in Settings.svelte; the page fits the height
const SETTINGS_WIDTH: i32 = 496;
const SETTINGS_HEIGHT: i32 = 700;
#[cfg(target_os = "linux")]
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
        .center()
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

/// Fits the settings window to `height` logical pixels, margin included, within the screen.
#[tauri::command]
fn fit_settings(window: WebviewWindow, height: f64) -> tauri::Result<()> {
    let area = window.current_monitor()?.map(|m| {
        let (area, scale) = (m.work_area(), m.scale_factor());
        (
            area.position.to_logical::<f64>(scale),
            area.size.to_logical::<f64>(scale),
        )
    });
    let height = area
        .map_or(height, |(_, size)| height.min(size.height))
        .ceil() as i32;
    #[cfg(target_os = "linux")]
    if layer::fit(
        &window.gtk_window()?,
        SETTINGS_WIDTH,
        height,
        SETTINGS_MARGIN,
    ) {
        return Ok(());
    }
    window.set_size(LogicalSize::new(SETTINGS_WIDTH, height))?;
    // by hand, as center() reads the size from before the resize on x11
    if let Some((pos, size)) = area {
        let x = pos.x + (size.width - f64::from(SETTINGS_WIDTH)) / 2.0;
        let y = pos.y + (size.height - f64::from(height)) / 2.0;
        window.set_position(LogicalPosition::new(x, y))?;
    }
    Ok(())
}

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", text(Text::Settings), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", text(Text::Quit), true, None::<&str>)?;
    // the menu bar tints a monochrome template for light and dark mode
    #[cfg(target_os = "macos")]
    let icon = include_bytes!("../icons/tray-template.png");
    #[cfg(not(target_os = "macos"))]
    let icon = include_bytes!("../icons/tray.png");
    TrayIconBuilder::new()
        .icon(Image::from_bytes(icon)?)
        .icon_as_template(cfg!(target_os = "macos"))
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
fn set_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<(), String> {
    let mut current = state.settings.lock().unwrap();
    // only set_hotkey changes the key, since it must bind it too
    let settings = Settings {
        hotkey: current.hotkey.clone(),
        ..settings
    };
    settings::save(&state.settings_path, &settings).map_err(|e| e.to_string())?;
    let new_language = settings.language != current.language;
    *current = settings;
    drop(current);
    if new_language {
        fetch_models(&app);
    }
    Ok(())
}

// async, so the device scan runs off the main thread
#[tauri::command]
async fn microphones() -> Vec<audio::Mic> {
    audio::microphones()
}

#[tauri::command]
fn download_status(state: State<AppState>) -> Download {
    state.download.lock().unwrap().clone()
}

#[tauri::command]
fn download_models(app: AppHandle) {
    fetch_models(&app);
}

#[tauri::command]
fn autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<(), String> {
    let launch = app.autolaunch();
    match on {
        true => launch.enable(),
        false => launch.disable(),
    }
    .map_err(|e| e.to_string())
}

#[cfg(target_os = "macos")]
#[tauri::command]
fn permissions() -> Option<macos::Permissions> {
    Some(macos::permissions())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
fn permissions() -> Option<()> {
    None
}

#[tauri::command]
fn grant(what: String) {
    #[cfg(target_os = "macos")]
    macos::grant(&what);
    #[cfg(not(target_os = "macos"))]
    let _ = what;
}

#[tauri::command]
fn ui_language() -> Language {
    ui::language()
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
