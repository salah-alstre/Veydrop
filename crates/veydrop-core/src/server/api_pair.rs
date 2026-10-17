//! Pairing endpoints: token -> pending request -> desktop approval -> cookie.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{ConnectInfo, State};
use axum::http::header::{HeaderValue, SET_COOKIE};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{cookie_value, ApiJson, ApiResult, Ctx, Session, COOKIE_NAME};
use crate::auth::{ClaimStatus, PairArgs};
use crate::error::Error;
use crate::model::{now_ms, EngineEvent, SessionInfo};

const LONG_POLL: Duration = Duration::from_secs(20);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairRequestBody {
    token: String,
    device_id: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    platform: String,
}

#[derive(Deserialize)]
pub struct ClaimBody {
    id: String,
    claim: String,
}

fn clean_label(s: &str, max: usize) -> String {
    let cleaned: String = s.chars().filter(|c| !c.is_control()).collect();
    cleaned.trim().chars().take(max).collect()
}

fn session_cookie(token: &str, session: &SessionInfo) -> HeaderValue {
    let max_age = match session.expires_at {
        Some(t) => (t.saturating_sub(now_ms()) / 1000).max(1),
        None => 31_536_000,
    };
    let v = format!("{COOKIE_NAME}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}");
    HeaderValue::from_str(&v).expect("cookie is ascii")
}

fn clear_cookie() -> HeaderValue {
    HeaderValue::from_str(&format!("{COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0"))
        .expect("cookie is ascii")
}

/// `GET /api/session` - public and soft: never counts as an auth failure.
pub async fn session_status(State(ctx): State<Arc<Ctx>>, parts: Parts) -> Json<serde_json::Value> {
    let found = cookie_value(&parts).and_then(|t| ctx.core.auth.authenticate(&t));
    match found {
        Some(s) => Json(json!({
            "paired": true,
            "session": s,
            "pcName": ctx.core.settings.get().device_name,
            "language": ctx.core.settings.effective_language(),
        })),
        None => Json(json!({ "paired": false })),
    }
}

/// `POST /api/pair/request`
pub async fn request(
    State(ctx): State<Arc<Ctx>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    ApiJson(body): ApiJson<PairRequestBody>,
) -> ApiResult<Json<serde_json::Value>> {
    let ip: IpAddr = addr.ip();
    let device_id = body.device_id.trim();
    if body.token.len() > 128
        || device_id.len() < 8
        || device_id.len() > 64
        || !device_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::invalid("bad pairing request").into());
    }
    let mut label = clean_label(&body.label, 48);
    if label.is_empty() {
        label = "Phone".into();
    }
    let platform = clean_label(&body.platform, 24);
    let s = ctx.core.settings.get();
    let trusted = s.trust_known_devices && ctx.core.settings.is_trusted(device_id);

    let start = ctx.core.auth.request_pair(
        ip,
        PairArgs { token: &body.token, device_id, label: &label, platform: &platform },
        trusted,
    )?;
    let id = start.info.id.clone();
    ctx.core.bus.emit(EngineEvent::PairRequest(start.info.clone()));
    if trusted {
        // Known device: it still needed the fresh QR token, so approve at once.
        ctx.core.decide_pair(&id, true)?;
    }
    Ok(Json(json!({
        "id": start.info.id,
        "code": start.info.code,
        "claim": start.claim,
        "expiresAt": start.info.expires_at,
        "trusted": trusted,
        "pcName": s.device_name,
        "language": ctx.core.settings.effective_language(),
    })))
}

/// `POST /api/pair/claim` - long-polls for the desktop user's decision.
pub async fn claim(
    State(ctx): State<Arc<Ctx>>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    ApiJson(body): ApiJson<ClaimBody>,
) -> ApiResult<Response> {
    if body.id.len() > 64 || body.claim.len() > 128 {
        return Err(Error::invalid("bad claim").into());
    }
    let ip = addr.ip();
    let deadline = tokio::time::sleep(LONG_POLL);
    tokio::pin!(deadline);
    loop {
        // Register interest *before* looking at the state so a decision made
        // in between cannot be missed.
        let notified = ctx.core.auth.changed.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        match ctx.core.auth.claim(ip, &body.id, &body.claim)? {
            ClaimStatus::Pending => {
                tokio::select! {
                    _ = &mut notified => {}
                    _ = &mut deadline => return Ok(Json(json!({ "status": "pending" })).into_response()),
                    _ = ctx.shutdown.cancelled() => return Ok(Json(json!({ "status": "pending" })).into_response()),
                }
            }
            ClaimStatus::Rejected => return Ok(Json(json!({ "status": "rejected" })).into_response()),
            ClaimStatus::Expired => return Ok(Json(json!({ "status": "expired" })).into_response()),
            ClaimStatus::Approved { token, session } => {
                let mut resp = Json(json!({
                    "status": "approved",
                    "session": session,
                    "pcName": ctx.core.settings.get().device_name,
                    "language": ctx.core.settings.effective_language(),
                }))
                .into_response();
                resp.headers_mut().append(SET_COOKIE, session_cookie(&token, &session));
                return Ok(resp);
            }
        }
    }
}

/// `POST /api/logout`
pub async fn logout(State(ctx): State<Arc<Ctx>>, Session(s): Session) -> Response {
    ctx.core.end_session(&s.id);
    let mut resp = Json(json!({ "ok": true })).into_response();
    resp.headers_mut().append(SET_COOKIE, clear_cookie());
    resp
}
