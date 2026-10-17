//! Veydrop desktop shell: owns the window, tray, native dialogs and the
//! `Engine`. All transfer logic lives in `veydrop-core`; this crate is glue.

mod commands;
mod shell;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{Manager, RunEvent};
use veydrop_core::engine::{Engine, EngineConfig};

pub struct AppState {
    pub engine: Engine,
    /// Set once a real quit was requested; close events then stop intercepting.
    pub quitting: AtomicBool,
    /// Started by autostart with `--minimized`: stay in the tray until opened.
    pub start_hidden: bool,
    pub tray: shell::TrayHandles,
}

pub fn run() {
    let start_hidden = std::env::args().any(|a| a == "--minimized");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(move |app| {
            let handle = app.handle().clone();
            let data_dir = shell::data_dir(&handle)?;
            let mobile_dir = shell::resolve_mobile_dir(&handle);
            let engine = tauri::async_runtime::block_on(Engine::new(EngineConfig {
                data_dir,
                mobile_dir,
            }))?;

            let tray = shell::build_tray(&handle)?;
            app.manage(AppState {
                engine: engine.clone(),
                quitting: AtomicBool::new(false),
                start_hidden,
                tray,
            });

            shell::sync_autostart(&handle, &engine.settings());
            shell::spawn_event_forwarder(handle.clone(), engine);

            // Safety net: never leave the user without a window if the UI fails to load.
            if !start_hidden {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(6)).await;
                    shell::show_main(&h);
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" && shell::on_close_requested(window) {
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::ui_ready,
            commands::get_snapshot,
            commands::refresh_network,
            commands::start_server,
            commands::restart_server,
            commands::stop_server,
            commands::regenerate_pairing,
            commands::decide_pair,
            commands::revoke_session,
            commands::disconnect_all,
            commands::forget_trusted,
            commands::add_share_paths,
            commands::remove_share_item,
            commands::clear_share,
            commands::set_share_active,
            commands::decide_offer,
            commands::pause_transfer,
            commands::resume_transfer,
            commands::cancel_transfer,
            commands::get_history,
            commands::remove_history,
            commands::clear_history,
            commands::update_settings,
            commands::set_system_language,
            commands::diagnostics,
            commands::pick_files,
            commands::pick_folder,
            commands::open_dir,
            commands::reveal_file,
            commands::notify,
            commands::set_tray_labels,
            commands::set_window_theme,
            commands::hide_to_tray,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Veydrop");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            // Any exit path (last window closed, OS shutdown) stops the server
            // and flushes state before the process ends.
            if let Some(state) = app_handle.try_state::<AppState>() {
                if !state.quitting.swap(true, Ordering::SeqCst) {
                    api.prevent_exit();
                    let h = app_handle.clone();
                    let engine = state.engine.clone();
                    tauri::async_runtime::spawn(async move {
                        engine.shutdown().await;
                        h.exit(0);
                    });
                }
            }
        }
    });
}
