//! The engine: one object the desktop shell talks to. It owns the stores, the
//! lazily started HTTP server, the pairing token and a housekeeping task, and
//! it publishes every state change on the event bus.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex as AsyncMutex};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::auth::Auth;
use crate::bus::Bus;
use crate::error::{Error, Result};
use crate::history::{HistoryQuery, HistoryStore};
use crate::incoming::Incoming;
use crate::model::*;
use crate::diag::{self, FirewallReport, LocalCheck, WinNet};
use crate::server::{self, Core, Ctx, Reach, ReachSnapshot};
use crate::settings::{Settings, SettingsStore};
use crate::shares::{AddReport, Shares};
use crate::transfers::Transfers;
use crate::{net, qr};

const PORT_RANGE: std::ops::Range<u16> = 47821..47900;
const JANITOR_EVERY: Duration = Duration::from_secs(30);
const STALL_AFTER: Duration = Duration::from_secs(45);
const KEEP_FINISHED: Duration = Duration::from_secs(60 * 30);

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Where settings, history and partial-upload bookkeeping live.
    pub data_dir: PathBuf,
    /// The built phone web app (served as-is).
    pub mobile_dir: PathBuf,
}

/// Everything the UI needs to paint itself from a cold start.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub server: ServerStatus,
    pub pairing: Option<PairingInfo>,
    pub pair_requests: Vec<PairRequestInfo>,
    pub sessions: Vec<SessionInfo>,
    pub share: Option<ShareInfo>,
    pub offers: Vec<OfferInfo>,
    pub transfers: Vec<TransferInfo>,
    pub network: Vec<NetIface>,
    pub settings: Settings,
    pub stats: Stats,
}

/// Whether a device other than this PC has actually reached the server. This
/// is the only evidence that the phone's network path works.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalReach {
    pub reached: bool,
    pub first_at: Option<Ms>,
    pub last_at: Option<Ms>,
    pub last_ip: Option<String>,
    pub requests: u64,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingDiag {
    pub active: bool,
    pub used: bool,
    pub expires_at: Option<Ms>,
    pub pending_requests: usize,
    pub sessions: usize,
}

/// What Windows says about the network the server is bound to.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsNetInfo {
    /// False when the Windows query was not run (quick check) or failed.
    pub checked: bool,
    pub alias: Option<String>,
    /// "Public" | "Private" | "DomainAuthenticated"
    pub category: Option<String>,
    pub firewall_enabled: Option<bool>,
    pub default_inbound: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub checked_at: Ms,
    /// True when the Windows network/firewall query ran for this report.
    pub deep: bool,
    pub running: bool,
    /// The socket address the server is really listening on.
    pub bind: Option<String>,
    /// The adapter the server is bound to.
    pub interface: Option<NetIface>,
    /// The adapter Veydrop would pick right now.
    pub recommended: Option<NetIface>,
    pub interface_count: usize,
    pub address_lost: bool,
    /// This PC talking to its own server. NOT proof a phone can connect.
    pub local: Option<LocalCheck>,
    /// A different device reached the server. This is the real proof.
    pub external: ExternalReach,
    pub windows: WindowsNetInfo,
    /// None until a deep check has run.
    pub firewall: Option<FirewallReport>,
    pub pairing: PairingDiag,
    pub phone_seen: bool,
    pub paired_phones: usize,
    /// Stable identifiers the UI maps to translated advice.
    pub warnings: Vec<String>,
    pub network_category: Option<String>,
}

struct Running {
    ip: IpAddr,
    port: u16,
    shutdown: CancellationToken,
    serve: JoinHandle<()>,
    janitor: JoinHandle<()>,
    reach: Arc<Reach>,
}

struct Inner {
    cfg: EngineConfig,
    core: Arc<Core>,
    lifecycle: AsyncMutex<()>,
    running: Mutex<Option<Running>>,
    status: Mutex<ServerStatus>,
    ifaces: Mutex<Vec<NetIface>>,
    /// URL and QR of the current pairing token (the token itself is only in
    /// the URL fragment; Auth stores just its hash).
    pairing: Mutex<Option<(String, QrMatrix)>>,
    /// Last Windows network/firewall query, so a quick refresh can reuse it.
    winnet: Mutex<Option<(IpAddr, u16, WinNet)>>,
    /// Set while an automatic re-bind after an address change is in flight.
    rebinding: AtomicBool,
}

#[derive(Clone)]
pub struct Engine(Arc<Inner>);

impl Engine {
    pub async fn new(cfg: EngineConfig) -> Result<Engine> {
        std::fs::create_dir_all(&cfg.data_dir)?;
        let bus = Bus::new();
        let settings = Arc::new(SettingsStore::load(&cfg.data_dir));
        let history = HistoryStore::load(&cfg.data_dir);
        history.start_flusher();
        let transfers = Transfers::new(bus.clone(), history.clone());
        let shares = Shares::new(bus.clone());
        let auth = Arc::new(Auth::new());
        let incoming = Incoming::new(transfers.clone(), bus.clone(), settings.clone(), &cfg.data_dir);
        let core = Arc::new(Core { auth, bus, incoming, shares, transfers, settings, history });

        let s = core.settings.get();
        core.history.prune(s.history_retention_days);
        let ifaces = net::list_interfaces(s.preferred_ip.as_deref());

        let engine = Engine(Arc::new(Inner {
            cfg,
            core,
            lifecycle: AsyncMutex::new(()),
            running: Mutex::new(None),
            status: Mutex::new(ServerStatus::default()),
            ifaces: Mutex::new(ifaces),
            pairing: Mutex::new(None),
            winnet: Mutex::new(None),
            rebinding: AtomicBool::new(false),
        }));
        engine.spawn_network_watcher();
        engine.spawn_pairing_watcher();
        Ok(engine)
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.0.core
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.0.core.bus.subscribe()
    }

    pub fn bound_addr(&self) -> Option<SocketAddr> {
        self.0.running.lock().as_ref().map(|r| SocketAddr::new(r.ip, r.port))
    }

    pub fn snapshot(&self) -> Snapshot {
        let c = &self.0.core;
        Snapshot {
            server: self.0.status.lock().clone(),
            pairing: self.pairing_info(),
            pair_requests: c.auth.pending(),
            sessions: c.auth.sessions(),
            share: c.shares.info(),
            offers: c.incoming.pending_offers(),
            transfers: c.transfers.list(),
            network: self.0.ifaces.lock().clone(),
            settings: c.settings.get(),
            stats: c.transfers.stats(),
        }
    }

    // ---- network ---------------------------------------------------------

    pub fn refresh_network(&self) -> Vec<NetIface> {
        let preferred = self.0.core.settings.get().preferred_ip;
        let list = net::list_interfaces(preferred.as_deref());
        *self.0.ifaces.lock() = list.clone();
        self.0.core.bus.emit(EngineEvent::Network(list.clone()));
        list
    }

    fn spawn_network_watcher(&self) {
        let weak = Arc::downgrade(&self.0);
        let mut rx = net::watch_changes();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(()) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
                // Adapters flap; let things settle and merge bursts.
                tokio::time::sleep(Duration::from_millis(700)).await;
                while rx.try_recv().is_ok() {}
                let Some(inner) = weak.upgrade() else { break };
                let engine = Engine(inner);
                engine.refresh_network();
                engine.check_address();
            }
        });
    }

    /// Re-evaluates whether the bound address still exists on this PC. If it is
    /// gone and nobody is paired, the server moves to the best remaining
    /// interface by itself (new address, new QR). With phones paired it only
    /// flags the problem: a silent re-bind would drop their sessions.
    fn check_address(&self) {
        let ip = match self.0.running.lock().as_ref() {
            Some(r) => r.ip,
            None => return,
        };
        let lost = !net::ip_present(&ip.to_string());
        let changed = {
            let mut st = self.0.status.lock();
            let changed = st.address_lost != lost;
            st.address_lost = lost;
            changed
        };
        if changed {
            self.emit_status();
        }
        if !lost {
            return;
        }
        let c = &self.0.core;
        let idle = c.auth.sessions().is_empty() && c.auth.pending().is_empty();
        let has_other = self.0.ifaces.lock().iter().any(|i| i.recommended && i.ip != ip.to_string());
        if idle && has_other && tokio::runtime::Handle::try_current().is_ok() {
            if self.0.rebinding.swap(true, Ordering::SeqCst) {
                return;
            }
            let engine = self.clone();
            tokio::spawn(async move {
                let _ = engine.restart_server().await;
                engine.0.rebinding.store(false, Ordering::SeqCst);
            });
        }
    }

    /// When a phone uses the one-time token, repaint the (now used) QR.
    fn spawn_pairing_watcher(&self) {
        let weak = Arc::downgrade(&self.0);
        let mut rx = self.0.core.bus.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(EngineEvent::PairRequest(_)) | Err(broadcast::error::RecvError::Lagged(_)) => {
                        let Some(inner) = weak.upgrade() else { break };
                        let engine = Engine(inner);
                        engine.0.core.bus.emit(EngineEvent::Pairing(engine.pairing_info()));
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    // ---- server lifecycle ------------------------------------------------

    fn emit_status(&self) {
        let st = self.0.status.lock().clone();
        self.0.core.bus.emit(EngineEvent::Server(st));
    }

    pub fn server_status(&self) -> ServerStatus {
        self.0.status.lock().clone()
    }

    /// Starts the server (if needed) and returns a fresh pairing QR.
    pub async fn start_server(&self) -> Result<ServerStatus> {
        let _guard = self.0.lifecycle.lock().await;
        if self.0.running.lock().is_some() {
            return Ok(self.server_status());
        }
        let list = self.refresh_network();
        let iface = list
            .iter()
            .find(|i| i.recommended)
            .or_else(|| list.first())
            .ok_or(Error::NoNetwork)?
            .clone();
        let ip: Ipv4Addr = iface.ip.parse().map_err(|_| Error::NoNetwork)?;
        let preferred_port = self.0.core.settings.get().preferred_port;
        let (listener, port) = bind_port(ip, preferred_port).await?;

        let shutdown = CancellationToken::new();
        let weak = Arc::downgrade(&self.0);
        let ctx = Ctx::new(
            self.0.core.clone(),
            self.0.cfg.mobile_dir.clone(),
            IpAddr::V4(ip),
            port,
            shutdown.clone(),
            move |snap: &ReachSnapshot| {
                if let Some(inner) = weak.upgrade() {
                    let engine = Engine(inner);
                    {
                        let mut st = engine.0.status.lock();
                        st.phone_seen = true;
                        st.remote_ip = snap.last_ip.clone();
                        st.reached_at = snap.first_at;
                    }
                    engine.emit_status();
                }
            },
        );
        let reach = ctx.reach.clone();
        let serve = tokio::spawn(server::serve(listener, ctx));
        let janitor = self.spawn_janitor(shutdown.clone());
        *self.0.running.lock() = Some(Running { ip: IpAddr::V4(ip), port, shutdown, serve, janitor, reach });
        *self.0.status.lock() = ServerStatus {
            running: true,
            ip: Some(ip.to_string()),
            port: Some(port),
            url: Some(format!("http://{ip}:{port}/")),
            started_at: Some(now_ms()),
            phone_seen: false,
            remote_ip: None,
            reached_at: None,
            address_lost: false,
        };
        self.emit_status();
        self.make_pairing(ip, port)?;
        Ok(self.server_status())
    }

    /// Stops the server and revokes every phone session.
    pub async fn stop_server(&self) {
        let _guard = self.0.lifecycle.lock().await;
        let Some(run) = self.0.running.lock().take() else { return };
        let core = &self.0.core;

        for id in core.auth.revoke_all() {
            core.incoming.on_session_gone(&id);
            core.cancel_outgoing_for(&id);
        }
        core.incoming.on_server_stop();
        core.auth.clear_pairing();
        *self.0.pairing.lock() = None;
        core.bus.phone(None, "revoked", serde_json::json!({}));

        run.shutdown.cancel();
        run.janitor.abort();
        let mut serve = run.serve;
        if tokio::time::timeout(Duration::from_secs(2), &mut serve).await.is_err() {
            serve.abort();
        }

        *self.0.status.lock() = ServerStatus::default();
        core.bus.emit(EngineEvent::Server(ServerStatus::default()));
        core.bus.emit(EngineEvent::Pairing(None));
        core.bus.emit(EngineEvent::Sessions(core.auth.sessions()));
    }

    /// Stops and starts the server again on the currently recommended
    /// interface. Every phone session is revoked (the address changes), so the
    /// UI must ask first.
    pub async fn restart_server(&self) -> Result<ServerStatus> {
        self.stop_server().await;
        self.start_server().await
    }

    pub fn is_running(&self) -> bool {
        self.0.running.lock().is_some()
    }

    fn spawn_janitor(&self, stop: CancellationToken) -> JoinHandle<()> {
        let weak = Arc::downgrade(&self.0);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(JANITOR_EVERY);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = stop.cancelled() => break,
                }
                let Some(inner) = weak.upgrade() else { break };
                Engine(inner).housekeeping();
            }
        })
    }

    /// One pass of expiry and cleanup work. Public so tests can drive it.
    pub fn housekeeping(&self) {
        let c = &self.0.core;
        let report = c.auth.tick();
        for id in &report.expired_sessions {
            c.end_session(id);
        }
        for id in report.expired_pending {
            c.bus.emit(EngineEvent::PairRequestResolved { id });
        }
        if report.pairing_expired {
            *self.0.pairing.lock() = None;
            c.bus.emit(EngineEvent::Pairing(None));
        }
        c.incoming.tick();
        for _id in c.transfers.mark_stalled(STALL_AFTER) {}
        c.transfers.prune(KEEP_FINISHED);
        c.incoming.sweep_partials();
        self.check_address();
    }

    // ---- pairing and sessions -------------------------------------------

    fn make_pairing(&self, ip: Ipv4Addr, port: u16) -> Result<()> {
        let secs = self.0.core.settings.get().pairing_seconds as u64;
        let token = self.0.core.auth.new_pairing(Duration::from_secs(secs));
        let url = format!("http://{ip}:{port}/#t={token}");
        let matrix = qr::encode(&url)?;
        *self.0.pairing.lock() = Some((url, matrix));
        self.0.core.bus.emit(EngineEvent::Pairing(self.pairing_info()));
        Ok(())
    }

    fn pairing_info(&self) -> Option<PairingInfo> {
        let meta = self.0.core.auth.pairing_meta()?;
        let (url, qr) = self.0.pairing.lock().clone()?;
        Some(PairingInfo { url, qr, created_at: meta.created_at, expires_at: meta.expires_at, used: meta.used })
    }

    /// Replaces the QR/token; the previous one stops working at once.
    pub async fn regenerate_pairing(&self) -> Result<PairingInfo> {
        if !self.is_running() {
            self.start_server().await?;
        } else {
            let (ip, port) = {
                let r = self.0.running.lock();
                let r = r.as_ref().ok_or(Error::ServerNotRunning)?;
                match r.ip {
                    IpAddr::V4(v4) => (v4, r.port),
                    IpAddr::V6(_) => return Err(Error::NoNetwork),
                }
            };
            self.make_pairing(ip, port)?;
        }
        self.pairing_info().ok_or(Error::ServerNotRunning)
    }

    pub fn pairing(&self) -> Option<PairingInfo> {
        self.pairing_info()
    }

    pub fn decide_pair(&self, id: &str, approve: bool) -> Result<()> {
        self.0.core.decide_pair(id, approve)
    }

    pub fn revoke_session(&self, id: &str) -> bool {
        self.0.core.end_session(id)
    }

    pub fn disconnect_all(&self) {
        let c = &self.0.core;
        for id in c.auth.revoke_all() {
            c.incoming.on_session_gone(&id);
            c.cancel_outgoing_for(&id);
        }
        c.bus.phone(None, "revoked", serde_json::json!({}));
        c.bus.emit(EngineEvent::Sessions(c.auth.sessions()));
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        self.0.core.auth.sessions()
    }

    pub fn forget_trusted(&self, device_id: &str) -> Result<Settings> {
        let mut list = self.0.core.settings.get().trusted_devices;
        list.retain(|d| d.id != device_id);
        self.update_settings(serde_json::json!({ "trustedDevices": list }))
    }

    // ---- shares ----------------------------------------------------------

    pub async fn add_share_paths(&self, paths: Vec<PathBuf>) -> Result<AddReport> {
        self.0.core.shares.add_paths(paths).await
    }

    pub fn remove_share_item(&self, id: &str) -> Result<()> {
        self.0.core.shares.remove_item(id)
    }

    pub fn clear_share(&self) {
        self.0.core.shares.clear();
        self.cancel_all_outgoing();
    }

    pub fn set_share_active(&self, active: bool) -> Result<()> {
        self.0.core.shares.set_active(active)?;
        if !active {
            self.cancel_all_outgoing();
        }
        Ok(())
    }

    fn cancel_all_outgoing(&self) {
        let t = &self.0.core.transfers;
        for tr in t.list() {
            if tr.direction == TransferDirection::Outgoing && !tr.state.is_terminal() {
                t.cancel(&tr.id);
            }
        }
    }

    pub fn share(&self) -> Option<ShareInfo> {
        self.0.core.shares.info()
    }

    // ---- incoming offers and transfers ------------------------------------

    pub fn decide_offer(&self, id: &str, accept: bool, dest: Option<PathBuf>) -> Result<()> {
        self.0.core.incoming.decide(id, accept, dest.as_deref())
    }

    pub fn pending_offers(&self) -> Vec<OfferInfo> {
        self.0.core.incoming.pending_offers()
    }

    pub fn transfers(&self) -> Vec<TransferInfo> {
        self.0.core.transfers.list()
    }

    pub fn pause_transfer(&self, id: &str) -> Result<TransferInfo> {
        self.0.core.incoming.set_paused(None, id, true)
    }

    pub fn resume_transfer(&self, id: &str) -> Result<TransferInfo> {
        self.0.core.incoming.set_paused(None, id, false)
    }

    pub fn cancel_transfer(&self, id: &str) -> Result<()> {
        let c = &self.0.core;
        match c.transfers.get(id) {
            None => Err(Error::NotFound),
            Some(t) if t.direction == TransferDirection::Outgoing => {
                c.transfers.cancel(id);
                Ok(())
            }
            Some(_) => c.incoming.cancel(None, id),
        }
    }

    // ---- history ---------------------------------------------------------

    pub fn history(&self, q: &HistoryQuery) -> Vec<HistoryEntry> {
        self.0.core.history.list(q)
    }

    pub fn remove_history(&self, id: &str) {
        self.0.core.history.remove(id);
        self.0.core.bus.emit(EngineEvent::HistoryChanged);
    }

    pub fn clear_history(&self) {
        self.0.core.history.clear();
        self.0.core.bus.emit(EngineEvent::HistoryChanged);
    }

    // ---- settings --------------------------------------------------------

    pub fn settings(&self) -> Settings {
        self.0.core.settings.get()
    }

    pub fn update_settings(&self, patch: serde_json::Value) -> Result<Settings> {
        let before = self.0.core.settings.get();
        let lang_before = self.0.core.settings.effective_language();
        let next = self.0.core.settings.patch(patch)?;
        self.broadcast_language_if_changed(&lang_before);
        if next.history_retention_days != before.history_retention_days {
            self.0.core.history.prune(next.history_retention_days);
            self.0.core.bus.emit(EngineEvent::HistoryChanged);
        }
        if next.preferred_ip != before.preferred_ip {
            self.refresh_network();
        }
        self.0.core.bus.emit(EngineEvent::Settings(next.clone()));
        Ok(next)
    }

    /// The desktop UI reports the language it resolved for the "system"
    /// setting. Connected phones follow if that changes what is shown.
    pub fn set_system_language(&self, lang: &str) {
        let before = self.0.core.settings.effective_language();
        self.0.core.settings.set_system_language(lang);
        self.broadcast_language_if_changed(&before);
    }

    /// The language every phone must show (the desktop is the single authority).
    pub fn language(&self) -> String {
        self.0.core.settings.effective_language()
    }

    fn broadcast_language_if_changed(&self, before: &str) {
        let now = self.0.core.settings.effective_language();
        if now != before {
            self.0.core.bus.phone(None, "language", serde_json::json!({ "language": now }));
        }
    }

    // ---- diagnostics -----------------------------------------------------

    /// `deep` additionally asks Windows (read-only) for the network profile and
    /// the firewall rules that apply to this program and port. The quick form
    /// reuses the last deep result for the same address.
    pub async fn diagnostics(&self, deep: bool) -> Diagnostics {
        let ifaces = self.refresh_network();
        let status = self.server_status();
        let (bound, reach) = {
            let r = self.0.running.lock();
            (r.as_ref().map(|r| (r.ip, r.port)), r.as_ref().map(|r| r.reach.snapshot()))
        };
        let interface = status.ip.as_deref().and_then(|ip| ifaces.iter().find(|i| i.ip == ip).cloned());
        let recommended = ifaces.iter().find(|i| i.recommended).cloned();
        let current = interface.clone().or_else(|| recommended.clone());

        let local = match bound {
            Some((ip, port)) => Some(diag::local_http_check(ip, port).await),
            None => None,
        };

        let exe = std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned());
        let win: Option<WinNet> = match bound {
            Some((ip, port)) if deep => {
                let w = diag::query_windows(ip, port, exe.as_deref()).await;
                *self.0.winnet.lock() = Some((ip, port, w.clone()));
                Some(w)
            }
            Some((ip, port)) => self
                .0
                .winnet
                .lock()
                .as_ref()
                .filter(|(i, p, _)| *i == ip && *p == port)
                .map(|(_, _, w)| w.clone()),
            None => None,
        };
        let firewall = match (&win, bound) {
            (Some(w), Some((_, port))) => Some(diag::evaluate(w, exe.as_deref(), port)),
            _ => None,
        };
        let windows = match &win {
            Some(w) => WindowsNetInfo {
                checked: w.error.is_none(),
                alias: w.alias.clone(),
                category: w.category.clone(),
                firewall_enabled: w.fw_enabled.as_deref().map(|v| v != "False"),
                default_inbound: w.default_inbound.clone(),
                error: w.error.clone(),
            },
            None => WindowsNetInfo::default(),
        };

        let external = match &reach {
            Some(r) => ExternalReach {
                reached: r.requests > 0,
                first_at: r.first_at,
                last_at: r.last_at,
                last_ip: r.last_ip.clone(),
                requests: r.requests,
            },
            None => ExternalReach::default(),
        };

        let core = &self.0.core;
        let sessions = core.auth.sessions().len();
        let meta = core.auth.pairing_meta();
        let pairing = PairingDiag {
            active: meta.as_ref().map(|m| !m.used).unwrap_or(false),
            used: meta.as_ref().map(|m| m.used).unwrap_or(false),
            expires_at: meta.as_ref().map(|m| m.expires_at),
            pending_requests: core.auth.pending().len(),
            sessions,
        };

        let mut warnings = Vec::new();
        if ifaces.is_empty() {
            warnings.push("no_network".to_string());
        }
        if status.address_lost {
            warnings.push("address_lost".into());
        }
        if let Some(i) = &current {
            if i.is_virtual {
                warnings.push("virtual_adapter".into());
            }
            if !i.is_private {
                warnings.push("not_private_range".into());
            }
        }
        if ifaces.len() > 1 {
            warnings.push("multiple_adapters".into());
        }
        if let (Some(i), Some(r)) = (&interface, &recommended) {
            if i.ip != r.ip {
                warnings.push("better_interface".into());
            }
        }
        if let Some(l) = &local {
            if !l.ok {
                warnings.push("local_check_failed".into());
            }
        }
        if windows.category.as_deref() == Some("Public") {
            warnings.push("public_profile".into());
        }
        if let Some(f) = &firewall {
            match f.state.as_str() {
                "blocked" => warnings.push("firewall_blocked".into()),
                "no_rule" => warnings.push("firewall_no_rule".into()),
                _ => {}
            }
        }
        if status.running && !external.reached {
            warnings.push("not_reached".into());
        }

        Diagnostics {
            checked_at: now_ms(),
            deep,
            running: status.running,
            bind: bound.map(|(ip, port)| format!("{ip}:{port}")),
            interface: current,
            recommended,
            interface_count: ifaces.len(),
            address_lost: status.address_lost,
            local,
            external,
            network_category: windows.category.clone(),
            windows,
            firewall,
            pairing,
            phone_seen: status.phone_seen,
            paired_phones: sessions,
            warnings,
        }
    }

    // ---- shutdown --------------------------------------------------------

    /// Full stop: server, sessions, partial bookkeeping and stores.
    pub async fn shutdown(&self) {
        self.stop_server().await;
        self.0.core.incoming.flush();
        self.0.core.history.flush();
    }

    pub fn weak(&self) -> Weak<impl Sized> {
        Arc::downgrade(&self.0)
    }
}

async fn bind_port(ip: Ipv4Addr, preferred: u16) -> Result<(TcpListener, u16)> {
    let mut candidates: Vec<u16> = Vec::new();
    if preferred != 0 {
        candidates.push(preferred);
    }
    candidates.extend(PORT_RANGE.filter(|p| *p != preferred));
    candidates.push(0);
    let mut last: Option<std::io::Error> = None;
    for p in candidates {
        match TcpListener::bind(SocketAddr::new(IpAddr::V4(ip), p)).await {
            Ok(l) => {
                let port = l.local_addr()?.port();
                return Ok((l, port));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => last = Some(e),
            Err(e) => return Err(e.into()),
        }
    }
    Err(last.map(Error::from).unwrap_or(Error::NoNetwork))
}
