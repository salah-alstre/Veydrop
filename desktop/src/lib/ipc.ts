import { invoke } from "@tauri-apps/api/core";
import type {
  AddReport, AppInfo, CmdError, Diagnostics, HistoryEntry, HistoryQuery, NetIface,
  PairingInfo, ServerStatus, Settings, Snapshot, TransferInfo,
} from "./types";

/** Normalizes anything thrown by invoke() to {code, message}. */
export function toCmdError(e: unknown): CmdError {
  if (e && typeof e === "object" && "code" in e) {
    const o = e as { code?: unknown; message?: unknown };
    return { code: String(o.code), message: String(o.message ?? "") };
  }
  return { code: "other", message: typeof e === "string" ? e : String((e as Error)?.message ?? e) };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toCmdError(e);
  }
}

export const ipc = {
  appInfo: () => call<AppInfo>("app_info"),
  uiReady: () => call<void>("ui_ready"),
  snapshot: () => call<Snapshot>("get_snapshot"),
  refreshNetwork: () => call<NetIface[]>("refresh_network"),
  startServer: () => call<ServerStatus>("start_server"),
  stopServer: () => call<void>("stop_server"),
  regeneratePairing: () => call<PairingInfo>("regenerate_pairing"),
  decidePair: (id: string, approve: boolean) => call<void>("decide_pair", { id, approve }),
  revokeSession: (id: string) => call<boolean>("revoke_session", { id }),
  disconnectAll: () => call<void>("disconnect_all"),
  forgetTrusted: (deviceId: string) => call<Settings>("forget_trusted", { deviceId }),
  addSharePaths: (paths: string[]) => call<AddReport>("add_share_paths", { paths }),
  removeShareItem: (id: string) => call<void>("remove_share_item", { id }),
  clearShare: () => call<void>("clear_share"),
  setShareActive: (active: boolean) => call<void>("set_share_active", { active }),
  decideOffer: (id: string, accept: boolean, dest?: string | null) =>
    call<void>("decide_offer", { id, accept, dest: dest ?? null }),
  pauseTransfer: (id: string) => call<TransferInfo>("pause_transfer", { id }),
  resumeTransfer: (id: string) => call<TransferInfo>("resume_transfer", { id }),
  cancelTransfer: (id: string) => call<void>("cancel_transfer", { id }),
  history: (query?: HistoryQuery) => call<HistoryEntry[]>("get_history", { query: query ?? null }),
  removeHistory: (id: string) => call<void>("remove_history", { id }),
  clearHistory: () => call<void>("clear_history"),
  updateSettings: (patch: Partial<Settings>) => call<Settings>("update_settings", { patch }),
  setSystemLanguage: (lang: string) => call<void>("set_system_language", { lang }),
  /** `deep` also asks Windows (read-only) for the network profile and firewall rules. */
  diagnostics: (deep = false) => call<Diagnostics>("diagnostics", { deep }),
  restartServer: () => call<ServerStatus>("restart_server"),
  pickFiles: () => call<string[]>("pick_files"),
  pickFolder: () => call<string | null>("pick_folder"),
  openDir: (path: string) => call<void>("open_dir", { path }),
  revealFile: (path: string) => call<void>("reveal_file", { path }),
  notify: (title: string, body: string) => call<void>("notify", { title, body }),
  setTrayLabels: (open: string, stop: string, quit: string, tooltip: string) =>
    call<void>("set_tray_labels", { open, stop, quit, tooltip }),
  setWindowTheme: (theme: "dark" | "light" | null) => call<void>("set_window_theme", { theme }),
  hideToTray: () => call<void>("hide_to_tray"),
  quitApp: () => call<void>("quit_app"),
};
