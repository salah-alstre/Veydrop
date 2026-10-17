//! Tauri commands: thin wrappers over `Engine` plus native dialogs/OS helpers.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use serde::{Serialize, Serializer};
use serde_json::Value;
use tauri::{AppHandle, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use veydrop_core::engine::{Diagnostics, Snapshot};
use veydrop_core::history::HistoryQuery;
use veydrop_core::model::*;
use veydrop_core::settings::Settings;
use veydrop_core::shares::AddReport;

use crate::{shell, AppState};

/// Errors cross the IPC boundary as `{ code, message }`; the UI translates `errors.<code>`.
pub struct CmdError {
    code: &'static str,
    message: String,
}

impl From<veydrop_core::Error> for CmdError {
    fn from(e: veydrop_core::Error) -> Self {
        CmdError {
            code: e.code(),
            message: e.to_string(),
        }
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        veydrop_core::Error::Io(e).into()
    }
}

impl Serialize for CmdError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("CmdError", 2)?;
        st.serialize_field("code", self.code)?;
        st.serialize_field("message", &self.message)?;
        st.end()
    }
}

fn other(message: impl Into<String>) -> CmdError {
    CmdError {
        code: "other",
        message: message.into(),
    }
}

type CmdResult<T> = Result<T, CmdError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    data_dir: String,
    start_hidden: bool,
    platform: &'static str,
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        name: "Veydrop",
        version: env!("CARGO_PKG_VERSION"),
        data_dir: crate::shell::data_dir(&app)
            .map(|p| p.display().to_string())
            .unwrap_or_default(),
        start_hidden: state.start_hidden,
        platform: std::env::consts::OS,
    }
}

/// The UI calls this once it has painted, so the window never flashes white.
#[tauri::command]
pub fn ui_ready(app: AppHandle, state: State<'_, AppState>) {
    if !state.start_hidden {
        shell::show_main(&app);
    }
}

#[tauri::command]
pub fn get_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.engine.snapshot()
}

#[tauri::command]
pub fn refresh_network(state: State<'_, AppState>) -> Vec<NetIface> {
    state.engine.refresh_network()
}

#[tauri::command]
pub async fn start_server(state: State<'_, AppState>) -> CmdResult<ServerStatus> {
    Ok(state.engine.start_server().await?)
}

/// Stop and start on the currently recommended interface. Revokes every phone
/// session, so the UI asks first.
#[tauri::command]
pub async fn restart_server(state: State<'_, AppState>) -> CmdResult<ServerStatus> {
    Ok(state.engine.restart_server().await?)
}

#[tauri::command]
pub async fn stop_server(state: State<'_, AppState>) -> CmdResult<()> {
    state.engine.stop_server().await;
    Ok(())
}

#[tauri::command]
pub async fn regenerate_pairing(state: State<'_, AppState>) -> CmdResult<PairingInfo> {
    Ok(state.engine.regenerate_pairing().await?)
}

#[tauri::command]
pub fn decide_pair(state: State<'_, AppState>, id: String, approve: bool) -> CmdResult<()> {
    Ok(state.engine.decide_pair(&id, approve)?)
}

#[tauri::command]
pub fn revoke_session(state: State<'_, AppState>, id: String) -> bool {
    state.engine.revoke_session(&id)
}

#[tauri::command]
pub fn disconnect_all(state: State<'_, AppState>) {
    state.engine.disconnect_all();
}

#[tauri::command]
pub fn forget_trusted(state: State<'_, AppState>, device_id: String) -> CmdResult<Settings> {
    Ok(state.engine.forget_trusted(&device_id)?)
}

#[tauri::command]
pub async fn add_share_paths(
    state: State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<AddReport> {
    let paths = paths.into_iter().map(PathBuf::from).collect();
    Ok(state.engine.add_share_paths(paths).await?)
}

#[tauri::command]
pub fn remove_share_item(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.engine.remove_share_item(&id)?)
}

#[tauri::command]
pub fn clear_share(state: State<'_, AppState>) {
    state.engine.clear_share();
}

#[tauri::command]
pub fn set_share_active(state: State<'_, AppState>, active: bool) -> CmdResult<()> {
    Ok(state.engine.set_share_active(active)?)
}

#[tauri::command]
pub fn decide_offer(
    state: State<'_, AppState>,
    id: String,
    accept: bool,
    dest: Option<String>,
) -> CmdResult<()> {
    Ok(state
        .engine
        .decide_offer(&id, accept, dest.map(PathBuf::from))?)
}

#[tauri::command]
pub fn pause_transfer(state: State<'_, AppState>, id: String) -> CmdResult<TransferInfo> {
    Ok(state.engine.pause_transfer(&id)?)
}

#[tauri::command]
pub fn resume_transfer(state: State<'_, AppState>, id: String) -> CmdResult<TransferInfo> {
    Ok(state.engine.resume_transfer(&id)?)
}

#[tauri::command]
pub fn cancel_transfer(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    Ok(state.engine.cancel_transfer(&id)?)
}

#[tauri::command]
pub fn get_history(state: State<'_, AppState>, query: Option<HistoryQuery>) -> Vec<HistoryEntry> {
    state.engine.history(&query.unwrap_or_default())
}

#[tauri::command]
pub fn remove_history(state: State<'_, AppState>, id: String) {
    state.engine.remove_history(&id);
}

#[tauri::command]
pub fn clear_history(state: State<'_, AppState>) {
    state.engine.clear_history();
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: Value,
) -> CmdResult<Settings> {
    let s = state.engine.update_settings(patch)?;
    shell::sync_autostart(&app, &s);
    Ok(s)
}

/// The desktop UI tells the engine which language it resolved for "system",
/// so connected phones can follow what the PC actually shows.
#[tauri::command]
pub fn set_system_language(state: State<'_, AppState>, lang: String) {
    state.engine.set_system_language(&lang);
}

#[tauri::command]
pub async fn diagnostics(state: State<'_, AppState>, deep: Option<bool>) -> CmdResult<Diagnostics> {
    Ok(state.engine.diagnostics(deep.unwrap_or(false)).await)
}

// ---------------------------------------------------------------- native dialogs

#[tauri::command]
pub async fn pick_files(app: AppHandle) -> CmdResult<Vec<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_files(move |p| {
        let _ = tx.send(p);
    });
    let picked = rx.await.map_err(|_| other("dialog closed"))?;
    Ok(picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|p| p.display().to_string())
        .collect())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> CmdResult<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |p| {
        let _ = tx.send(p);
    });
    let picked = rx.await.map_err(|_| other("dialog closed"))?;
    Ok(picked
        .and_then(|p| p.into_path().ok())
        .map(|p| p.display().to_string()))
}

#[cfg(windows)]
fn explorer(raw: &str) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("explorer.exe")
        .raw_arg(raw)
        .spawn()
        .map(|_| ())
}

#[cfg(not(windows))]
fn explorer(_raw: &str) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "only supported on Windows",
    ))
}

/// Opens a directory in Explorer. Files are never launched, only folders.
#[tauri::command]
pub fn open_dir(path: String) -> CmdResult<()> {
    let p = Path::new(&path);
    if !p.is_dir() {
        return Err(veydrop_core::Error::NotFound.into());
    }
    explorer(&format!("\"{}\"", p.display()))?;
    Ok(())
}

/// Opens the containing folder with the file selected; falls back to the folder.
#[tauri::command]
pub fn reveal_file(path: String) -> CmdResult<()> {
    let p = Path::new(&path);
    if p.is_file() {
        explorer(&format!("/select,\"{}\"", p.display()))?;
        return Ok(());
    }
    match p.parent().filter(|d| d.is_dir()) {
        Some(dir) => {
            explorer(&format!("\"{}\"", dir.display()))?;
            Ok(())
        }
        None => Err(veydrop_core::Error::NotFound.into()),
    }
}

// ---------------------------------------------------------------- OS integration

#[tauri::command]
pub fn notify(app: AppHandle, state: State<'_, AppState>, title: String, body: String) {
    if !state.engine.settings().notifications {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

#[tauri::command]
pub fn set_tray_labels(
    state: State<'_, AppState>,
    open: String,
    stop: String,
    quit: String,
    tooltip: String,
) {
    state.tray.apply(&open, &stop, &quit, &tooltip);
}

#[tauri::command]
pub fn set_window_theme(window: WebviewWindow, theme: Option<String>) {
    let t = match theme.as_deref() {
        Some("dark") => Some(tauri::Theme::Dark),
        Some("light") => Some(tauri::Theme::Light),
        _ => None,
    };
    let _ = window.set_theme(t);
}

#[tauri::command]
pub fn hide_to_tray(window: WebviewWindow) {
    let _ = window.hide();
}

#[tauri::command]
pub fn quit_app(app: AppHandle, state: State<'_, AppState>) {
    state.quitting.store(true, Ordering::SeqCst);
    shell::quit(&app);
}
