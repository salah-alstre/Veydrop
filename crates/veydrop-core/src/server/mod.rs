//! The phone-facing HTTP server.
//!
//! Security model (see docs/SECURITY.md):
//! * bound only to the chosen LAN address, never `0.0.0.0`;
//! * `Host` must be one of the server's own addresses (DNS-rebinding
//!   protection) and `Origin`, when present, must match it;
//! * no CORS headers at all; state-changing requests need `X-Veydrop: 1`;
//! * everything except the static app, `/api/info`, `/api/session` and the
//!   pairing endpoints requires the session cookie issued after the user
//!   approved the phone on the PC.

mod api_download;
mod api_events;
mod api_pair;
mod api_upload;
mod static_files;

use std::collections::HashSet;
use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Request, State};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{request::Parts, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;

use crate::auth::Auth;
use crate::bus::Bus;
use crate::error::Error;
use crate::history::HistoryStore;
use crate::incoming::Incoming;
use crate::model::{now_ms, EngineEvent, Ms, SessionInfo};
use crate::net;
use crate::settings::SettingsStore;
use crate::shares::Shares;
use crate::transfers::Transfers;

pub const COOKIE_NAME: &str = "vd_s";

/// Everything the server and the engine share.
pub struct Core {
    pub auth: Arc<Auth>,
    pub bus: Bus,
    pub incoming: Arc<Incoming>,
    pub shares: Arc<Shares>,
    pub transfers: Arc<Transfers>,
    pub settings: Arc<SettingsStore>,
    pub history: Arc<HistoryStore>,
}

impl Core {
    /// Approves or rejects a pending pairing request and performs the
    /// bookkeeping both the desktop UI and the trusted-device fast path need.
    pub fn decide_pair(&self, id: &str, approve: bool) -> crate::Result<()> {
        let minutes = self.settings.get().session_minutes;
        let d = self.auth.decide(id, approve, minutes)?;
        for old in &d.replaced {
            self.incoming.on_session_gone(old);
            self.cancel_outgoing_for(old);
        }
        if approve && d.session.is_some() {
            if let Err(e) = self.settings.add_trusted(&d.device_id, &d.device_label) {
                tracing::warn!("could not remember trusted device: {e}");
            }
        }
        self.bus.emit(EngineEvent::PairRequestResolved { id: id.to_string() });
        self.bus.emit(EngineEvent::Sessions(self.auth.sessions()));
        Ok(())
    }

    /// Ends one phone session completely: auth, uploads, downloads, SSE.
    pub fn end_session(&self, id: &str) -> bool {
        let existed = self.auth.revoke(id);
        self.incoming.on_session_gone(id);
        self.cancel_outgoing_for(id);
        self.bus.phone(Some(id), "revoked", serde_json::json!({}));
        self.bus.emit(EngineEvent::Sessions(self.auth.sessions()));
        existed
    }

    /// Cancels every unfinished download of a session that just went away.
    pub fn cancel_outgoing_for(&self, session_id: &str) {
        for t in self.transfers.list_for_session(session_id) {
            if t.direction == crate::model::TransferDirection::Outgoing && !t.state.is_terminal() {
                self.transfers.cancel(&t.id);
            }
        }
    }
}

/// Per-server-run state.
pub struct Ctx {
    pub core: Arc<Core>,
    pub mobile_dir: PathBuf,
    /// Lower-case `host:port` values accepted in the `Host` header.
    pub hosts: Vec<String>,
    pub shutdown: CancellationToken,
    /// Which *other* devices have reached this server (see [`Reach`]).
    pub reach: Arc<Reach>,
    on_phone_seen: Box<dyn Fn(&ReachSnapshot) + Send + Sync>,
    locals: parking_lot::Mutex<(Instant, HashSet<IpAddr>)>,
}

/// How long the set of this PC's own addresses is reused before re-reading it.
const LOCALS_TTL: Duration = Duration::from_secs(5);

/// A point-in-time copy of [`Reach`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReachSnapshot {
    pub first_at: Option<Ms>,
    pub last_at: Option<Ms>,
    pub last_ip: Option<String>,
    pub requests: u64,
}

/// Evidence that a device other than this PC reached the server.
///
/// Connections from loopback or from any of this PC's own addresses are
/// deliberately ignored: Windows does not apply the firewall to a PC talking to
/// itself, so such a request proves nothing about a phone being able to connect.
#[derive(Default)]
pub struct Reach(parking_lot::Mutex<ReachSnapshot>);

impl Reach {
    pub fn snapshot(&self) -> ReachSnapshot {
        self.0.lock().clone()
    }

    /// Records one external request; returns true if it was the first.
    fn record(&self, ip: IpAddr) -> bool {
        let mut s = self.0.lock();
        let now = now_ms();
        let first = s.first_at.is_none();
        if first {
            s.first_at = Some(now);
        }
        s.last_at = Some(now);
        s.last_ip = Some(ip.to_string());
        s.requests += 1;
        first
    }
}

impl Ctx {
    pub fn new(
        core: Arc<Core>,
        mobile_dir: PathBuf,
        ip: IpAddr,
        port: u16,
        shutdown: CancellationToken,
        on_phone_seen: impl Fn(&ReachSnapshot) + Send + Sync + 'static,
    ) -> Arc<Self> {
        let hosts = vec![format!("{ip}:{port}"), format!("localhost:{port}"), format!("127.0.0.1:{port}")];
        Arc::new(Self {
            core,
            mobile_dir,
            hosts,
            shutdown,
            reach: Arc::new(Reach::default()),
            on_phone_seen: Box::new(on_phone_seen),
            locals: parking_lot::Mutex::new((Instant::now() - LOCALS_TTL * 2, HashSet::new())),
        })
    }

    fn is_own_address(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() || ip.is_unspecified() {
            return true;
        }
        let mut l = self.locals.lock();
        if l.0.elapsed() > LOCALS_TTL {
            *l = (Instant::now(), net::local_ips());
        }
        l.1.contains(&ip)
    }

    /// Called for every request, before any validation: even a request with a
    /// wrong `Host` header proves the network path works.
    fn note_peer(&self, ip: IpAddr) {
        if self.is_own_address(ip) {
            return;
        }
        if self.reach.record(ip) {
            (self.on_phone_seen)(&self.reach.snapshot());
        }
    }
}

// ---- error mapping ----------------------------------------------------------

pub struct ApiError(pub Error);

pub type ApiResult<T> = Result<T, ApiError>;

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(e: std::io::Error) -> Self {
        ApiError(Error::from(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let e = self.0;
        let status = match &e {
            Error::Invalid(_) => StatusCode::BAD_REQUEST,
            Error::NotFound => StatusCode::NOT_FOUND,
            Error::Conflict(_) | Error::Cancelled | Error::OffsetMismatch(_) => StatusCode::CONFLICT,
            Error::Unauthorized => StatusCode::UNAUTHORIZED,
            Error::Forbidden(_) => StatusCode::FORBIDDEN,
            Error::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Error::PairingExpired | Error::PairingUsed => StatusCode::GONE,
            Error::ServerNotRunning | Error::NoNetwork => StatusCode::SERVICE_UNAVAILABLE,
            Error::InsufficientSpace => StatusCode::INSUFFICIENT_STORAGE,
            Error::TooLarge | Error::ZipTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Error::HashMismatch => StatusCode::UNPROCESSABLE_ENTITY,
            Error::Io(_) | Error::Other(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        // Never leak file-system paths or OS error text to a phone.
        let message = match &e {
            Error::Io(_) | Error::Other(_) => "internal error".to_string(),
            other => other.to_string(),
        };
        let mut body = serde_json::json!({ "code": e.code(), "message": message });
        let mut resp_headers: Vec<(HeaderName, HeaderValue)> = Vec::new();
        if let Error::OffsetMismatch(n) = &e {
            body["offset"] = (*n).into();
            if let Ok(v) = HeaderValue::from_str(&n.to_string()) {
                resp_headers.push((HeaderName::from_static("upload-offset"), v));
            }
        }
        if matches!(e, Error::RateLimited) {
            resp_headers.push((header::RETRY_AFTER, HeaderValue::from_static("30")));
        }
        let mut resp = (status, Json(body)).into_response();
        for (k, v) in resp_headers {
            resp.headers_mut().insert(k, v);
        }
        resp
    }
}

/// JSON body extractor that reports problems in the API's own error format.
pub struct ApiJson<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for ApiJson<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(v)) => Ok(ApiJson(v)),
            Err(e) => Err(ApiError(Error::invalid(e.body_text()))),
        }
    }
}

// ---- authentication extractor -----------------------------------------------

pub fn cookie_value(parts: &Parts) -> Option<String> {
    for h in parts.headers.get_all(header::COOKIE) {
        let Ok(s) = h.to_str() else { continue };
        for pair in s.split(';') {
            let pair = pair.trim();
            if let Some(v) = pair.strip_prefix(COOKIE_NAME).and_then(|r| r.strip_prefix('=')) {
                if !v.is_empty() && v.len() <= 256 {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

pub fn peer_ip(parts: &Parts) -> IpAddr {
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED))
}

/// An authenticated phone session.
pub struct Session(pub SessionInfo);

impl FromRequestParts<Arc<Ctx>> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, ctx: &Arc<Ctx>) -> Result<Self, ApiError> {
        let ip = peer_ip(parts);
        ctx.core.auth.check_rate(ip)?;
        match cookie_value(parts).and_then(|t| ctx.core.auth.authenticate(&t)) {
            Some(s) => Ok(Session(s)),
            None => {
                ctx.core.auth.record_fail(ip);
                Err(ApiError(Error::Unauthorized))
            }
        }
    }
}

// ---- guard middleware ---------------------------------------------------------

const CSP: &str = "default-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; \
style-src 'self' 'unsafe-inline'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; \
base-uri 'none'; form-action 'self'; object-src 'none'";

async fn guard(State(ctx): State<Arc<Ctx>>, req: Request, next: Next) -> Response {
    if let Some(ip) = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip()) {
        ctx.note_peer(ip);
    }

    let host = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| h.to_ascii_lowercase());
    let Some(host) = host.filter(|h| ctx.hosts.iter().any(|a| a == h)) else {
        return plain(StatusCode::MISDIRECTED_REQUEST, "unknown host");
    };

    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let ok = origin
            .to_str()
            .ok()
            .and_then(|o| o.strip_prefix("http://"))
            .is_some_and(|o| o.eq_ignore_ascii_case(&host));
        if !ok {
            return plain(StatusCode::FORBIDDEN, "origin not allowed");
        }
    }

    let method = req.method().clone();
    if method == Method::OPTIONS {
        return plain(StatusCode::METHOD_NOT_ALLOWED, "method not allowed");
    }
    let is_api = req.uri().path().starts_with("/api/");
    let safe = method == Method::GET || method == Method::HEAD;
    if !safe && req.headers().get("x-veydrop").and_then(|v| v.to_str().ok()) != Some("1") {
        return plain(StatusCode::FORBIDDEN, "missing x-veydrop header");
    }

    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    // Downloads of shared files set their own (sandboxing) CSP.
    if !h.contains_key(header::CONTENT_SECURITY_POLICY) {
        h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    }
    if is_api {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

fn plain(status: StatusCode, msg: &'static str) -> Response {
    (status, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], msg).into_response()
}

// ---- router -----------------------------------------------------------------

async fn info() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "app": "veydrop", "version": env!("CARGO_PKG_VERSION"), "apiVersion": 1 }))
}

async fn api_not_found() -> ApiError {
    ApiError(Error::NotFound)
}

pub fn router(ctx: Arc<Ctx>) -> Router {
    let api = Router::new()
        .route("/info", get(info))
        .route("/session", get(api_pair::session_status))
        .route("/logout", post(api_pair::logout))
        .route("/pair/request", post(api_pair::request))
        .route("/pair/claim", post(api_pair::claim))
        .route("/events", get(api_events::events))
        .route("/offers", post(api_upload::create_offer))
        .route("/offers/{id}", get(api_upload::offer_status))
        .route("/transfers", get(api_upload::snapshot))
        .route(
            "/uploads/{tid}",
            get(api_upload::upload_status).put(api_upload::put_chunk).delete(api_upload::cancel),
        )
        .route("/uploads/{tid}/state", post(api_upload::set_state))
        .route("/uploads/{tid}/complete", post(api_upload::complete))
        .route("/share", get(api_download::share))
        .route("/share/items/{id}/download", get(api_download::download))
        .route("/share/zip", get(api_download::zip))
        .fallback(api_not_found)
        // JSON bodies are small; chunk PUTs stream and are bounded in `incoming`.
        .layer(axum::extract::DefaultBodyLimit::max(4 * 1024 * 1024));

    Router::new()
        .nest("/api", api)
        .fallback(static_files::serve)
        .layer(middleware::from_fn_with_state(ctx.clone(), guard))
        .with_state(ctx)
}

/// Runs the server until `ctx.shutdown` fires.
pub async fn serve(listener: tokio::net::TcpListener, ctx: Arc<Ctx>) {
    let token = ctx.shutdown.clone();
    let app = router(ctx).into_make_service_with_connect_info::<SocketAddr>();
    let res = axum::serve(listener, app).with_graceful_shutdown(async move { token.cancelled().await }).await;
    if let Err(e) = res {
        tracing::warn!("server stopped with an error: {e}");
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    #[test]
    fn only_the_first_external_request_is_reported_as_first() {
        let r = Reach::default();
        assert_eq!(r.snapshot().requests, 0);
        assert!(r.snapshot().first_at.is_none());
        let phone: IpAddr = "192.0.2.44".parse().unwrap();
        assert!(r.record(phone));
        assert!(!r.record(phone));
        let s = r.snapshot();
        assert_eq!(s.requests, 2);
        assert_eq!(s.last_ip.as_deref(), Some("192.0.2.44"));
        assert!(s.first_at.is_some() && s.last_at >= s.first_at);
    }
}
