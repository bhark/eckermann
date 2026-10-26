//! Status window at the bottom of the screen. It never takes focus or pointer input,
//! so the paste still lands where the user was typing.

use crate::pipeline::Event;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Listener, Manager, WebviewUrl, WebviewWindowBuilder};

const LABEL: &str = "overlay";
const WIDTH: i32 = 600;
const HEIGHT: i32 = 200;
const NOTICE_FOR: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Shown {
    recording: bool,
    pending: usize,
    notices: usize,
    visible: bool,
}

impl Shown {
    fn wanted(&self) -> bool {
        self.recording || self.pending > 0 || self.notices > 0
    }
}

pub struct Overlay {
    app: AppHandle,
    shown: Mutex<Shown>,
    // false where the window would take the focus, and with it the paste
    usable: bool,
}

impl Overlay {
    /// Creates the hidden window; call on the main thread during setup.
    pub fn new(app: &AppHandle) -> tauri::Result<Arc<Self>> {
        let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("overlay.html".into()))
            .title("Eckermann")
            .inner_size(WIDTH.into(), HEIGHT.into())
            .decorations(false)
            .transparent(true)
            .shadow(false)
            .resizable(false)
            .always_on_top(true)
            .visible_on_all_workspaces(true)
            .skip_taskbar(true)
            .focused(false)
            .focusable(false)
            .visible(false)
            .build()?;
        #[cfg(target_os = "linux")]
        let layered = crate::layer::overlay(&window.gtk_window()?, WIDTH, HEIGHT);
        #[cfg(not(target_os = "linux"))]
        let layered = false;
        // a plain wayland window gets the focus; an X11 one that refuses it does not
        #[cfg(target_os = "linux")]
        let usable = layered || crate::x11::session();
        #[cfg(not(target_os = "linux"))]
        let usable = true;
        if usable && !layered {
            // on the bottom edge of the work area, so above a dock; gtk got its input region in layer::overlay
            #[cfg(not(target_os = "linux"))]
            window.set_ignore_cursor_events(true)?;
            if let Some(monitor) = window.primary_monitor()? {
                let area = monitor.work_area();
                let size = tauri::LogicalSize::new(WIDTH, HEIGHT)
                    .to_physical::<i32>(monitor.scale_factor());
                window.set_position(tauri::PhysicalPosition::new(
                    area.position.x + (area.size.width as i32 - size.width) / 2,
                    area.position.y + area.size.height as i32 - size.height,
                ))?;
            }
        }
        let overlay = Arc::new(Self {
            app: app.clone(),
            shown: Mutex::new(Shown::default()),
            usable,
        });
        let left = overlay.clone();
        app.listen("overlay-left", move |_| left.left());
        Ok(overlay)
    }

    pub fn handle(self: &Arc<Self>, event: Event) {
        if !self.usable {
            if let Event::Notice(text) = event {
                notify(&text);
            }
            return;
        }
        let mut shown = self.shown.lock().unwrap();
        match &event {
            Event::Recording(on) => shown.recording = *on,
            Event::Pending(n) => shown.pending = *n,
            Event::Notice(_) => {
                shown.notices += 1;
                let overlay = self.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(NOTICE_FOR);
                    overlay.notice_over();
                });
            }
            Event::Level(_) | Event::Preview(_) => {}
        }
        // mapped first, so the page's enter transition is seen from its start
        self.apply(&mut shown);
        let _ = self.app.emit_to(LABEL, "overlay", &event);
    }

    fn notice_over(&self) {
        let mut shown = self.shown.lock().unwrap();
        shown.notices -= 1;
        self.apply(&mut shown);
        if shown.notices == 0 {
            let _ = self
                .app
                .emit_to(LABEL, "overlay", Event::Notice(String::new()));
        }
    }

    fn apply(&self, shown: &mut Shown) {
        let wanted = shown.wanted();
        if wanted == shown.visible {
            return;
        }
        shown.visible = wanted;
        if wanted && let Some(window) = self.app.get_webview_window(LABEL) {
            #[cfg(windows)]
            crate::win::show_inactive(&window);
            #[cfg(not(windows))]
            let _ = window.show();
        }
        let _ = self.app.emit_to(LABEL, "shown", wanted);
    }

    /// The page's leave transition is over.
    fn left(&self) {
        // locked, so a show cannot slip in between the check and the hide
        let shown = self.shown.lock().unwrap();
        if !shown.visible
            && let Some(window) = self.app.get_webview_window(LABEL)
        {
            #[cfg(windows)]
            crate::win::hide(&window);
            #[cfg(not(windows))]
            let _ = window.hide();
        }
    }
}

/// A desktop notification, for where the overlay cannot show.
fn notify(text: &str) {
    // ponytail: libnotify's command; the notification portal if a desktop lacks it
    let sent = std::process::Command::new("notify-send")
        .args(["--app-name=Eckermann", "Eckermann", text])
        .spawn();
    if let Err(e) = sent {
        log::warn!("notify-send: {e}; notice was {text:?}");
    }
}
