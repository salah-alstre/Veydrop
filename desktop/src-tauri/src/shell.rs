//! Window, tray, event forwarding and autostart plumbing.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use parking_lot::Mutex;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Window, Wry};
use tauri_plugin_autostart::ManagerExt;
use veydrop_core::engine::Engine;
use veydrop_core::model::EngineEvent;
use veydrop_core::settings::Settings;

use crate::AppState;

struct TrayParts {
    tray: TrayIcon<Wry>,
    open: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    quit: MenuItem<Wry>,
}

pub struct TrayHandles(Mutex<TrayParts>);

impl TrayHandles {
    /// Labels come from the UI so the tray follows the selected language.
    pub fn apply(&self, open: &str, stop: &str, quit: &str, tooltip: &str) {
        let t = self.0.lock();
        let _ = t.open.set_text(open);
        let _ = t.stop.set_text(stop);
        let _ = t.quit.set_text(quit);
        let _ = t.tray.set_tooltip(Some(tooltip));
    }

    pub fn set_stop_enabled(&self, enabled: bool) {
        let _ = self.0.lock().stop.set_enabled(enabled);
    }
}

/// Where settings, history and partial uploads live. `VEYDROP_DATA_DIR` overrides the
/// platform default; it exists so tests and screenshot runs never touch a real profile.
pub fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    match std::env::var_os("VEYDROP_DATA_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => Ok(PathBuf::from(dir)),
        None => app.path().app_data_dir(),
    }
}

pub fn resolve_mobile_dir(app: &AppHandle) -> PathBuf {
    let bundled = app
        .path()
        .resource_dir()
        .map(|d| d.join("mobile"))
        .unwrap_or_default();
    if bundled.join("index.html").is_file() {
        return bundled;
    }
    // Development builds run straight from the source tree.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("mobile")
}

pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Graceful quit: stop the server, revoke sessions, flush state, then exit.
pub fn quit(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        app.exit(0);
        return;
    };
    state.quitting.store(true, Ordering::SeqCst);
    let engine = state.engine.clone();
    let h = app.clone();
    tauri::async_runtime::spawn(async move {
        engine.shutdown().await;
        h.exit(0);
    });
}

pub fn build_tray(app: &AppHandle) -> tauri::Result<TrayHandles> {
    let open = MenuItem::with_id(app, "open", "Open Veydrop", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop sharing", false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Veydrop", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &stop, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or_else(|| tauri::Error::AssetNotFound("window icon".into()))?;

    let tray = TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Veydrop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main(app),
            "stop" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let engine = state.engine.clone();
                    tauri::async_runtime::spawn(async move { engine.stop_server().await });
                }
            }
            "quit" => self::quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(TrayHandles(Mutex::new(TrayParts {
        tray,
        open,
        stop,
        quit,
    })))
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CloseInfo {
    server_running: bool,
    active_transfers: usize,
    minimize_to_tray: bool,
}

/// Returns true when the close must be intercepted (the UI decides).
pub fn on_close_requested(window: &Window) -> bool {
    let app = window.app_handle();
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    if state.quitting.load(Ordering::SeqCst) {
        return false;
    }
    let settings = state.engine.settings();
    let server_running = state.engine.is_running();
    let active_transfers = state
        .engine
        .transfers()
        .iter()
        .filter(|t| !t.state.is_terminal())
        .count();
    if !settings.minimize_to_tray && !server_running && active_transfers == 0 {
        return false;
    }
    let _ = window.emit(
        "close-requested",
        CloseInfo {
            server_running,
            active_transfers,
            minimize_to_tray: settings.minimize_to_tray,
        },
    );
    true
}

pub fn spawn_event_forwarder(app: AppHandle, engine: Engine) {
    use tokio::sync::broadcast::error::RecvError;
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let _ = app.emit("engine", &ev);
                    on_event(&app, &ev);
                }
                Err(RecvError::Lagged(_)) => {
                    let _ = app.emit("engine-resync", ());
                }
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn on_event(app: &AppHandle, ev: &EngineEvent) {
    match ev {
        // A pairing approval or incoming offer needs a human decision: surface the window.
        EngineEvent::PairRequest(_) | EngineEvent::Offer(_) => show_main(app),
        EngineEvent::Server(s) => {
            if let Some(state) = app.try_state::<AppState>() {
                state.tray.set_stop_enabled(s.running);
            }
        }
        EngineEvent::Settings(s) => sync_autostart(app, s),
        _ => {}
    }
}

/// Keeps the Windows autostart entry equal to the persisted setting.
pub fn sync_autostart(app: &AppHandle, s: &Settings) {
    let al = app.autolaunch();
    let enabled = al.is_enabled().unwrap_or(false);
    if s.launch_at_startup && !enabled {
        let _ = al.enable();
    } else if !s.launch_at_startup && enabled {
        let _ = al.disable();
    }
}
