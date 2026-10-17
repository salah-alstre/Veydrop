// Mirrors crates/veydrop-core/src/model.rs (camelCase JSON).

export type Direction = "incoming" | "outgoing"; // incoming = phone -> PC
export type TransferState =
  | "pending" | "queued" | "active" | "paused" | "interrupted"
  | "finalizing" | "completed" | "failed" | "cancelled" | "rejected";

export interface TransferInfo {
  id: string;
  direction: Direction;
  name: string;
  relPath?: string | null;
  size: number;
  transferred: number;
  state: TransferState;
  error?: string | null;
  speedBps: number;
  etaSecs?: number | null;
  sessionId?: string | null;
  deviceName: string;
  offerId?: string | null;
  startedAt: number;
  finishedAt?: number | null;
  savedPath?: string | null;
  sha256?: string | null;
}

export interface HistoryEntry {
  id: string;
  direction: Direction;
  name: string;
  relPath?: string | null;
  size: number;
  transferred: number;
  deviceName: string;
  state: TransferState;
  error?: string | null;
  startedAt: number;
  finishedAt: number;
  durationMs: number;
  savedPath?: string | null;
  sha256?: string | null;
}

export type IfaceKind = "wifi" | "ethernet" | "hotspot" | "virtual" | "vpn" | "other";
export interface NetIface {
  name: string;
  ip: string;
  kind: IfaceKind;
  isPrivate: boolean;
  isVirtual: boolean;
  score: number;
  recommended: boolean;
}

export interface ServerStatus {
  running: boolean;
  ip?: string | null;
  port?: number | null;
  url?: string | null;
  startedAt?: number | null;
  /** A device other than this PC has reached the server. */
  phoneSeen: boolean;
  remoteIp?: string | null;
  reachedAt?: number | null;
  addressLost: boolean;
}

export interface QrMatrix { size: number; modules: string }
export interface PairingInfo {
  url: string;
  qr: QrMatrix;
  createdAt: number;
  expiresAt: number;
  used: boolean;
}

export interface PairRequestInfo {
  id: string;
  code: string;
  deviceLabel: string;
  platform: string;
  remoteIp: string;
  createdAt: number;
  expiresAt: number;
  trusted: boolean;
}

export interface SessionInfo {
  id: string;
  deviceLabel: string;
  platform: string;
  remoteIp: string;
  connectedAt: number;
  lastSeen: number;
  expiresAt?: number | null;
  online: boolean;
  trusted: boolean;
}

export interface OfferFile { clientId: string; name: string; relPath?: string | null; size: number }
export type OfferState = "pending" | "accepted" | "rejected" | "failed";
export interface OfferInfo {
  id: string;
  sessionId: string;
  deviceName: string;
  files: OfferFile[];
  totalSize: number;
  createdAt: number;
  state: OfferState;
  error?: string | null;
  destDir?: string | null;
}

export interface ShareItemInfo {
  id: string;
  name: string;
  relPath: string;
  size: number;
  sha256?: string | null;
  ready: boolean;
  missing: boolean;
}
export interface ShareInfo {
  id: string;
  createdAt: number;
  active: boolean;
  items: ShareItemInfo[];
  totalSize: number;
  indexed: boolean;
}

export interface Stats {
  sentBytes: number;
  receivedBytes: number;
  sentFiles: number;
  receivedFiles: number;
  failed: number;
}

export interface TrustedDevice { id: string; name: string; addedAt: number }

export type LangSetting = "system" | "en" | "ar";
export type ThemeSetting = "system" | "dark" | "light";
export interface Settings {
  language: LangSetting;
  theme: ThemeSetting;
  downloadDir: string;
  deviceName: string;
  notifications: boolean;
  launchAtStartup: boolean;
  minimizeToTray: boolean;
  trustKnownDevices: boolean;
  askBeforeReceive: boolean;
  chunkSizeMib: number;
  maxFileSizeGib: number;
  maxParallelUploads: number;
  partialRetentionHours: number;
  sessionMinutes: number;
  pairingSeconds: number;
  preferredPort: number;
  preferredIp: string;
  historyRetentionDays: number;
  trustedDevices: TrustedDevice[];
}

export interface Snapshot {
  server: ServerStatus;
  pairing: PairingInfo | null;
  pairRequests: PairRequestInfo[];
  sessions: SessionInfo[];
  share: ShareInfo | null;
  offers: OfferInfo[];
  transfers: TransferInfo[];
  network: NetIface[];
  settings: Settings;
  stats: Stats;
}

export interface LocalCheck {
  ok: boolean;
  url: string;
  status?: number | null;
  millis?: number | null;
  pageOk: boolean;
  error?: string | null;
}

export type FirewallState = "allowed" | "blocked" | "no_rule" | "off" | "unknown";
export interface FirewallReport {
  state: FirewallState;
  profile?: string | null;
  matchingAllow: string[];
  matchingBlock: string[];
  exe?: string | null;
  suggestedCommand?: string | null;
}

export interface ExternalReach {
  reached: boolean;
  firstAt?: number | null;
  lastAt?: number | null;
  lastIp?: string | null;
  requests: number;
}

export interface PairingDiag {
  active: boolean;
  used: boolean;
  expiresAt?: number | null;
  pendingRequests: number;
  sessions: number;
}

export interface WindowsNetInfo {
  checked: boolean;
  alias?: string | null;
  category?: string | null;
  firewallEnabled?: boolean | null;
  defaultInbound?: string | null;
  error?: string | null;
}

export interface Diagnostics {
  checkedAt: number;
  deep: boolean;
  running: boolean;
  bind?: string | null;
  interface?: NetIface | null;
  recommended?: NetIface | null;
  interfaceCount: number;
  addressLost: boolean;
  local?: LocalCheck | null;
  external: ExternalReach;
  windows: WindowsNetInfo;
  firewall?: FirewallReport | null;
  pairing: PairingDiag;
  phoneSeen: boolean;
  pairedPhones: number;
  warnings: string[];
  networkCategory?: string | null;
}

export interface AddReport {
  share: ShareInfo;
  skipped: number;
}

export interface HistoryQuery {
  search?: string;
  direction?: Direction;
  status?: "completed" | "failed" | "cancelled";
  limit?: number;
}

export type EngineEvent =
  | { type: "server"; data: ServerStatus }
  | { type: "pairing"; data: PairingInfo | null }
  | { type: "pairRequest"; data: PairRequestInfo }
  | { type: "pairRequestResolved"; data: { id: string } }
  | { type: "sessions"; data: SessionInfo[] }
  | { type: "share"; data: ShareInfo | null }
  | { type: "offer"; data: OfferInfo }
  | { type: "offerResolved"; data: { id: string } }
  | { type: "transfers"; data: TransferInfo[] }
  | { type: "transferDone"; data: TransferInfo }
  | { type: "historyChanged"; data?: null }
  | { type: "network"; data: NetIface[] }
  | { type: "settings"; data: Settings }
  | { type: "stats"; data: Stats };

export interface AppInfo {
  name: string;
  version: string;
  dataDir: string;
  startHidden: boolean;
  platform: string;
}

export interface CloseInfo {
  serverRunning: boolean;
  activeTransfers: number;
  minimizeToTray: boolean;
}

export interface CmdError { code: string; message: string }
