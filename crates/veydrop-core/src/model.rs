//! Serializable data model shared between the engine, the desktop UI
//! (via Tauri) and the phone web UI (via the HTTP API).

use serde::{Deserialize, Serialize};

pub type Ms = u64;

pub fn now_ms() -> Ms {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    /// phone -> PC
    Incoming,
    /// PC -> phone
    Outgoing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    /// Waiting for the user on the PC to accept the offer.
    Pending,
    /// Accepted, waiting for the phone to start sending.
    Queued,
    Active,
    Paused,
    /// Connection dropped; can be resumed while the session is alive.
    Interrupted,
    /// All bytes received, verifying the checksum and moving into place.
    Finalizing,
    Completed,
    Failed,
    Cancelled,
    Rejected,
}

impl TransferState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TransferState::Completed
                | TransferState::Failed
                | TransferState::Cancelled
                | TransferState::Rejected
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferInfo {
    pub id: String,
    pub direction: TransferDirection,
    pub name: String,
    pub rel_path: Option<String>,
    pub size: u64,
    pub transferred: u64,
    pub state: TransferState,
    /// Stable error code (see `Error::code`) when `state == Failed`.
    pub error: Option<String>,
    pub speed_bps: f64,
    pub eta_secs: Option<u64>,
    pub session_id: Option<String>,
    pub device_name: String,
    pub offer_id: Option<String>,
    pub started_at: Ms,
    pub finished_at: Option<Ms>,
    pub saved_path: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub direction: TransferDirection,
    pub name: String,
    pub rel_path: Option<String>,
    pub size: u64,
    pub transferred: u64,
    pub device_name: String,
    pub state: TransferState,
    pub error: Option<String>,
    pub started_at: Ms,
    pub finished_at: Ms,
    pub duration_ms: u64,
    pub saved_path: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetIface {
    pub name: String,
    pub ip: String,
    /// wifi | ethernet | hotspot | virtual | vpn | other
    pub kind: String,
    pub is_private: bool,
    pub is_virtual: bool,
    pub score: i32,
    pub recommended: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub running: bool,
    pub ip: Option<String>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub started_at: Option<Ms>,
    /// True once a device *other than this PC* has reached the server since
    /// start. Requests from this PC to itself never set it.
    pub phone_seen: bool,
    /// Address of the first such device, and when it first connected.
    pub remote_ip: Option<String>,
    pub reached_at: Option<Ms>,
    /// The address the server is bound to is no longer present on this PC.
    pub address_lost: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QrMatrix {
    pub size: usize,
    /// `size*size` characters, `'1'` = dark module, row-major.
    pub modules: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingInfo {
    pub url: String,
    pub qr: QrMatrix,
    pub created_at: Ms,
    pub expires_at: Ms,
    /// The token has been scanned by a phone (one-time use).
    pub used: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequestInfo {
    pub id: String,
    /// 4-digit code shown on both screens so the user can confirm it is the
    /// phone they are holding.
    pub code: String,
    pub device_label: String,
    pub platform: String,
    pub remote_ip: String,
    pub created_at: Ms,
    pub expires_at: Ms,
    pub trusted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub device_label: String,
    pub platform: String,
    pub remote_ip: String,
    pub connected_at: Ms,
    pub last_seen: Ms,
    pub expires_at: Option<Ms>,
    pub online: bool,
    pub trusted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferFile {
    pub client_id: String,
    pub name: String,
    pub rel_path: Option<String>,
    pub size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OfferState {
    Pending,
    Accepted,
    Rejected,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfferInfo {
    pub id: String,
    pub session_id: String,
    pub device_name: String,
    pub files: Vec<OfferFile>,
    pub total_size: u64,
    pub created_at: Ms,
    pub state: OfferState,
    pub error: Option<String>,
    pub dest_dir: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareItemInfo {
    pub id: String,
    pub name: String,
    pub rel_path: String,
    pub size: u64,
    pub sha256: Option<String>,
    /// Hash/CRC computed; the item can be included in a ZIP.
    pub ready: bool,
    pub missing: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    pub id: String,
    pub created_at: Ms,
    /// Visible to connected phones. Staged-but-inactive shares are never listed to them.
    pub active: bool,
    pub items: Vec<ShareItemInfo>,
    pub total_size: u64,
    pub indexed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub sent_bytes: u64,
    pub received_bytes: u64,
    pub sent_files: u64,
    pub received_files: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum EngineEvent {
    Server(ServerStatus),
    Pairing(Option<PairingInfo>),
    PairRequest(PairRequestInfo),
    PairRequestResolved { id: String },
    Sessions(Vec<SessionInfo>),
    Share(Option<ShareInfo>),
    Offer(OfferInfo),
    OfferResolved { id: String },
    /// Delta: only transfers changed since the last event.
    Transfers(Vec<TransferInfo>),
    TransferDone(TransferInfo),
    HistoryChanged,
    Network(Vec<NetIface>),
    Settings(crate::settings::Settings),
    Stats(Stats),
}

/// Events pushed to phones over SSE. `target = None` broadcasts to all.
#[derive(Clone, Debug)]
pub struct PhoneEvent {
    pub target: Option<String>,
    pub kind: &'static str,
    pub data: serde_json::Value,
}
