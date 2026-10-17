//! Phone -> PC: offers and chunked, resumable uploads.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use super::{ApiJson, ApiResult, Ctx, Session};
use crate::error::Error;
use crate::model::{OfferFile, TransferInfo};

#[derive(Deserialize)]
pub struct OfferBody {
    files: Vec<OfferFile>,
}

#[derive(Deserialize)]
pub struct StateBody {
    paused: bool,
}

#[derive(Deserialize)]
pub struct CompleteBody {
    #[serde(default)]
    sha256: String,
}

/// What a phone may learn about a transfer: never the PC-side path.
fn trim(t: &TransferInfo) -> serde_json::Value {
    json!({
        "transferId": t.id,
        "state": t.state,
        "transferred": t.transferred,
        "size": t.size,
        "error": t.error,
        "sha256": t.sha256,
    })
}

fn check_id(id: &str) -> ApiResult<()> {
    if id.is_empty() || id.len() > 64 {
        return Err(Error::invalid("bad id").into());
    }
    Ok(())
}

/// `POST /api/offers`
pub async fn create_offer(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    ApiJson(body): ApiJson<OfferBody>,
) -> ApiResult<Json<crate::incoming::PhoneOffer>> {
    let offer = ctx.core.incoming.create_offer(&s.id, &s.device_label, body.files)?;
    Ok(Json(offer))
}

/// `GET /api/offers/{id}`
pub async fn offer_status(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(id): Path<String>,
) -> ApiResult<Json<crate::incoming::PhoneOffer>> {
    check_id(&id)?;
    Ok(Json(ctx.core.incoming.phone_offer(&s.id, &id)?))
}

/// `GET /api/transfers` - this phone's offers and uploads (used to recover after a reload).
pub async fn snapshot(State(ctx): State<Arc<Ctx>>, Session(s): Session) -> Json<crate::incoming::PhoneSnapshot> {
    Json(ctx.core.incoming.session_snapshot(&s.id))
}

/// `GET /api/uploads/{tid}` - where to resume from.
pub async fn upload_status(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(tid): Path<String>,
) -> ApiResult<Response> {
    check_id(&tid)?;
    let st = ctx.core.incoming.upload_status(&s.id, &tid)?;
    let off = HeaderValue::from_str(&st.received.to_string()).expect("digits");
    let mut resp = Json(st).into_response();
    resp.headers_mut().insert(HeaderName::from_static("upload-offset"), off);
    Ok(resp)
}

/// `PUT /api/uploads/{tid}` with `Upload-Offset`.
pub async fn put_chunk(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(tid): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Response> {
    check_id(&tid)?;
    let offset: u64 = headers
        .get("upload-offset")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| Error::invalid("missing Upload-Offset"))?;
    let received = ctx.core.incoming.put_chunk(&s.id, &tid, offset, body.into_data_stream()).await?;
    let mut resp = Json(json!({ "transferId": tid, "received": received })).into_response();
    resp.headers_mut().insert(
        HeaderName::from_static("upload-offset"),
        HeaderValue::from_str(&received.to_string()).expect("digits"),
    );
    Ok(resp)
}

/// `POST /api/uploads/{tid}/state`
pub async fn set_state(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(tid): Path<String>,
    ApiJson(body): ApiJson<StateBody>,
) -> ApiResult<Json<serde_json::Value>> {
    check_id(&tid)?;
    let t = ctx.core.incoming.set_paused(Some(&s.id), &tid, body.paused)?;
    Ok(Json(trim(&t)))
}

/// `POST /api/uploads/{tid}/complete`
pub async fn complete(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(tid): Path<String>,
    ApiJson(body): ApiJson<CompleteBody>,
) -> ApiResult<Json<serde_json::Value>> {
    check_id(&tid)?;
    if body.sha256.len() != 64 {
        return Err(Error::invalid("sha256 must be 64 hex characters").into());
    }
    let t = ctx.core.incoming.complete(&s.id, &tid, &body.sha256).await?;
    Ok(Json(trim(&t)))
}

/// `DELETE /api/uploads/{tid}`
pub async fn cancel(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(tid): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    check_id(&tid)?;
    ctx.core.incoming.cancel(Some(&s.id), &tid)?;
    Ok(Json(json!({ "ok": true })))
}
