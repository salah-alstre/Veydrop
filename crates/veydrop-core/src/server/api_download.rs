//! PC -> phone: the shared file list, single-file downloads (with Range
//! support) and streamed ZIP archives.

// `Guard::done` is read by its Drop impl; rustc cannot see that through the
// `stream!` macro and flags the final assignments.
#![allow(unused_assignments)]

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::header::{self, HeaderMap, HeaderName, HeaderValue};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures_util::StreamExt;
use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::{ApiResult, Ctx, Session};
use crate::error::Error;
use crate::model::{TransferDirection, TransferState};
use crate::shares::{Shares, ZipSel};
use crate::transfers::Transfers;

const READ_CHUNK: usize = 256 * 1024;
const MAX_ZIP_IDS: usize = 5000;

/// RFC 5987 `attr-char`: everything else is percent-encoded.
const ATTR_CHAR: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'!')
    .remove(b'#')
    .remove(b'$')
    .remove(b'&')
    .remove(b'+')
    .remove(b'-')
    .remove(b'.')
    .remove(b'^')
    .remove(b'_')
    .remove(b'`')
    .remove(b'|')
    .remove(b'~');

#[derive(Deserialize)]
pub struct DownloadQuery {
    /// `1` = let the browser show images/video/audio/PDF instead of saving.
    #[serde(default)]
    inline: Option<String>,
}

#[derive(Deserialize)]
pub struct ZipQuery {
    /// Comma separated share item ids.
    #[serde(default)]
    ids: Option<String>,
    /// A staged folder (`rel_path` of its root).
    #[serde(default)]
    prefix: Option<String>,
}

/// `GET /api/share` - what the PC is currently sharing (or `null`).
pub async fn share(State(ctx): State<Arc<Ctx>>, _s: Session) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "share": ctx.core.shares.phone_info() }))
}

// ---- helpers ------------------------------------------------------------------

fn content_disposition(kind: &str, name: &str) -> HeaderValue {
    let fallback: String = name
        .chars()
        .map(|c| if c.is_ascii() && !c.is_ascii_control() && c != '"' && c != '\\' && c != '%' { c } else { '_' })
        .collect();
    let encoded = utf8_percent_encode(name, ATTR_CHAR);
    HeaderValue::from_str(&format!("{kind}; filename=\"{fallback}\"; filename*=UTF-8''{encoded}"))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

fn is_inline_safe(mime: &str) -> bool {
    mime.starts_with("image/") && mime != "image/svg+xml"
        || mime.starts_with("video/")
        || mime.starts_with("audio/")
        || mime == "application/pdf"
}

enum Range {
    Full,
    Part(u64, u64),
    Unsatisfiable,
}

/// Single `bytes=` ranges only; anything fancier gets the whole file.
fn parse_range(headers: &HeaderMap, size: u64) -> Range {
    if headers.contains_key(header::IF_RANGE) {
        // We keep no validators, so a conditional range is answered in full.
        return Range::Full;
    }
    let Some(v) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) else { return Range::Full };
    let Some(spec) = v.trim().strip_prefix("bytes=") else { return Range::Full };
    if spec.contains(',') {
        return Range::Full;
    }
    let Some((a, b)) = spec.trim().split_once('-') else { return Range::Full };
    let (a, b) = (a.trim(), b.trim());
    if size == 0 {
        return Range::Full;
    }
    if a.is_empty() {
        // suffix: last n bytes
        let Ok(n) = b.parse::<u64>() else { return Range::Full };
        if n == 0 {
            return Range::Unsatisfiable;
        }
        let n = n.min(size);
        return Range::Part(size - n, size - 1);
    }
    let Ok(start) = a.parse::<u64>() else { return Range::Full };
    if start >= size {
        return Range::Unsatisfiable;
    }
    let end = if b.is_empty() {
        size - 1
    } else {
        match b.parse::<u64>() {
            Ok(e) if e >= start => e.min(size - 1),
            _ => return Range::Full,
        }
    };
    Range::Part(start, end)
}

/// Keeps the transfer list honest when a response body is dropped early
/// (phone went away, screen locked, user cancelled).
struct Guard {
    transfers: Arc<Transfers>,
    tid: Option<String>,
    committed: u64,
    resumable: bool,
    done: bool,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if self.done {
            return;
        }
        let Some(tid) = &self.tid else { return };
        if self.resumable {
            self.transfers.interrupt(tid, self.committed);
        } else {
            self.transfers.cancel(tid);
        }
    }
}

// ---- single file ----------------------------------------------------------------

/// `GET /api/share/items/{id}/download`
pub async fn download(
    State(ctx): State<Arc<Ctx>>,
    Session(s): Session,
    Path(id): Path<String>,
    Query(q): Query<DownloadQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    if id.len() > 64 {
        return Err(Error::NotFound.into());
    }
    let item = ctx.core.shares.servable_item(&id)?;
    Shares::verify_fresh(&item)?;
    let size = item.size;

    let (start, end, partial) = match parse_range(&headers, size) {
        Range::Full => (0, size.saturating_sub(1), false),
        Range::Part(a, b) => (a, b, true),
        Range::Unsatisfiable => {
            let mut resp = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
            resp.headers_mut().insert(
                header::CONTENT_RANGE,
                HeaderValue::from_str(&format!("bytes */{size}")).expect("ascii"),
            );
            return Ok(resp);
        }
    };
    let length = if size == 0 { 0 } else { end - start + 1 };

    let mut file = tokio::fs::File::open(&item.abs_path).await.map_err(|_| Error::Conflict("file_changed".into()))?;
    if start > 0 {
        file.seek(std::io::SeekFrom::Start(start)).await?;
    }

    let mime = mime_guess::from_path(&item.name).first_or_octet_stream();
    let mime_str = mime.essence_str().to_string();
    let inline = q.inline.as_deref() == Some("1") && is_inline_safe(&mime_str);
    let content_type = if inline { mime_str.clone() } else { "application/octet-stream".to_string() };

    // Only a request that runs to the end of the file is a "download" worth
    // showing in the transfer list; probes and partial previews are not.
    let tracked = size == 0 || end + 1 == size;
    let transfers = ctx.core.transfers.clone();
    let mut tid: Option<String> = None;
    let mut cancel = None;
    if tracked {
        let existing = transfers
            .ids_where(|t| {
                t.direction == TransferDirection::Outgoing
                    && t.state == TransferState::Interrupted
                    && t.session_id.as_deref() == Some(s.id.as_str())
                    && t.offer_id.as_deref() == Some(item.id.as_str())
            })
            .into_iter()
            .next();
        match existing {
            Some(eid) => {
                transfers.set_progress(&eid, start);
                transfers.set_state(&eid, TransferState::Active);
                cancel = transfers.cancel_token(&eid);
                tid = Some(eid);
            }
            None => {
                let mut info = Transfers::blank(
                    TransferDirection::Outgoing,
                    &item.name,
                    Some(item.rel_path.clone()),
                    size,
                    TransferState::Active,
                    Some(s.id.clone()),
                    &s.device_label,
                    Some(item.id.clone()),
                );
                info.transferred = start;
                info.sha256 = item.sha256.clone();
                tid = Some(info.id.clone());
                cancel = Some(transfers.create(info));
            }
        }
    }
    if size == 0 {
        if let Some(t) = &tid {
            transfers.finish(t, TransferState::Completed, None, None, item.sha256.clone());
        }
        tid = None;
    }

    let sha = item.sha256.clone();
    let mut guard = Guard { transfers: transfers.clone(), tid: tid.clone(), committed: start, resumable: true, done: size == 0 };
    let body_stream = async_stream::stream! {
        let mut remaining = length;
        let mut buf = vec![0u8; READ_CHUNK];
        let mut sent = start;
        while remaining > 0 {
            let want = remaining.min(READ_CHUNK as u64) as usize;
            let read = match &cancel {
                Some(c) => {
                    tokio::select! {
                        r = file.read(&mut buf[..want]) => Some(r),
                        _ = c.cancelled() => None,
                    }
                }
                None => Some(file.read(&mut buf[..want]).await),
            };
            let Some(read) = read else {
                guard.done = true; // already terminal (cancelled from the desktop)
                yield Err(std::io::Error::new(std::io::ErrorKind::Interrupted, "cancelled"));
                return;
            };
            match read {
                Ok(0) => {
                    // The file shrank under us: fail loudly instead of serving a short body.
                    if let Some(t) = &guard.tid {
                        guard.transfers.finish(t, TransferState::Failed, Some(&Error::Conflict("file_changed".into())), None, None);
                    }
                    guard.done = true;
                    yield Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "file changed"));
                    return;
                }
                Ok(n) => {
                    remaining -= n as u64;
                    sent += n as u64;
                    guard.committed = sent;
                    if let Some(t) = &guard.tid {
                        guard.transfers.set_progress(t, sent);
                    }
                    yield Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&buf[..n]));
                }
                Err(e) => {
                    if let Some(t) = &guard.tid {
                        guard.transfers.finish(t, TransferState::Failed, Some(&Error::Io(std::io::Error::new(e.kind(), "read failed"))), None, None);
                    }
                    guard.done = true;
                    yield Err(e);
                    return;
                }
            }
        }
        // Everything has been handed to the connection (the consumer polled
        // again after the last chunk).
        if let Some(t) = &guard.tid {
            guard.transfers.finish(t, TransferState::Completed, None, None, sha.clone());
        }
        guard.done = true;
    };

    let mut resp = Response::new(Body::from_stream(body_stream));
    *resp.status_mut() = if partial { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK };
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&content_type).expect("ascii"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if partial {
        h.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {start}-{end}/{size}")).expect("ascii"),
        );
    }
    h.insert(header::CONTENT_DISPOSITION, content_disposition(if inline { "inline" } else { "attachment" }, &item.name));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
    if let Some(sha) = item.sha256.as_deref().and_then(|s| HeaderValue::from_str(s).ok()) {
        h.insert(HeaderName::from_static("x-veydrop-sha256"), sha);
    }
    Ok(resp)
}

// ---- ZIP --------------------------------------------------------------------------

fn zip_name(sel: &ZipSel) -> String {
    match sel {
        ZipSel::Prefix(p) => {
            let last = p.trim_end_matches('/').rsplit('/').next().unwrap_or("");
            if last.is_empty() {
                "Veydrop.zip".into()
            } else {
                format!("{last}.zip")
            }
        }
        _ => "Veydrop.zip".into(),
    }
}

/// `GET /api/share/zip?ids=a,b,c` or `?prefix=Folder`
pub async fn zip(State(ctx): State<Arc<Ctx>>, Session(s): Session, Query(q): Query<ZipQuery>) -> ApiResult<Response> {
    let sel = match (q.ids.as_deref(), q.prefix.as_deref()) {
        (Some(ids), None) => {
            let v: Vec<String> =
                ids.split(',').map(str::trim).filter(|x| !x.is_empty() && x.len() <= 64).map(str::to_string).collect();
            if v.is_empty() || v.len() > MAX_ZIP_IDS {
                return Err(Error::invalid("bad ids").into());
            }
            ZipSel::Ids(v)
        }
        (None, Some(p)) => {
            if p.is_empty() || p.len() > 1024 {
                return Err(Error::invalid("bad prefix").into());
            }
            ZipSel::Prefix(p.to_string())
        }
        (None, None) => ZipSel::All,
        _ => return Err(Error::invalid("use either ids or prefix").into()),
    };
    let entries = ctx.core.shares.items_for_zip(&sel)?;
    for e in &entries {
        // A file that changed since indexing would corrupt the archive.
        let meta = std::fs::metadata(&e.path).map_err(|_| Error::Conflict("file_changed".into()))?;
        if meta.len() != e.size {
            return Err(Error::Conflict("file_changed".into()).into());
        }
    }
    let name = zip_name(&sel);
    let total = crate::zipstream::content_length(&entries)?;

    let transfers = ctx.core.transfers.clone();
    let info = Transfers::blank(
        TransferDirection::Outgoing,
        &name,
        None,
        total,
        TransferState::Active,
        Some(s.id.clone()),
        &s.device_label,
        None,
    );
    let tid = info.id.clone();
    let cancel = transfers.create(info);

    let produced = Arc::new(AtomicU64::new(0));
    let (progress_t, progress_id, progress_n) = (transfers.clone(), tid.clone(), produced.clone());
    let on_bytes = move |n: u64| {
        let total = progress_n.fetch_add(n, Ordering::Relaxed) + n;
        progress_t.set_progress(&progress_id, total);
    };
    let (len, inner) = match crate::zipstream::stream(entries, on_bytes, cancel) {
        Ok(v) => v,
        Err(e) => {
            transfers.finish(&tid, TransferState::Failed, Some(&e), None, None);
            return Err(e.into());
        }
    };

    let mut guard = Guard { transfers: transfers.clone(), tid: Some(tid.clone()), committed: 0, resumable: false, done: false };
    let body_stream = async_stream::stream! {
        let mut inner = Box::pin(inner);
        while let Some(item) = inner.next().await {
            match item {
                Ok(b) => yield Ok::<Bytes, std::io::Error>(b),
                Err(e) => {
                    if let Some(t) = &guard.tid {
                        // `cancel()` already made it terminal; this is a no-op then.
                        guard.transfers.finish(t, TransferState::Failed, Some(&Error::Other("zip".into())), None, None);
                    }
                    guard.done = true;
                    yield Err(e);
                    return;
                }
            }
        }
        if let Some(t) = &guard.tid {
            guard.transfers.finish(t, TransferState::Completed, None, None, None);
        }
        guard.done = true;
    };

    let mut resp = Response::new(Body::from_stream(body_stream));
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/zip"));
    h.insert(header::CONTENT_LENGTH, HeaderValue::from(len));
    h.insert(header::CONTENT_DISPOSITION, content_disposition("attachment", &name));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: &str) -> HeaderMap {
        let mut m = HeaderMap::new();
        m.insert(header::RANGE, HeaderValue::from_str(v).unwrap());
        m
    }

    #[test]
    fn ranges() {
        assert!(matches!(parse_range(&HeaderMap::new(), 100), Range::Full));
        assert!(matches!(parse_range(&h("bytes=0-9"), 100), Range::Part(0, 9)));
        assert!(matches!(parse_range(&h("bytes=90-"), 100), Range::Part(90, 99)));
        assert!(matches!(parse_range(&h("bytes=-10"), 100), Range::Part(90, 99)));
        assert!(matches!(parse_range(&h("bytes=-500"), 100), Range::Part(0, 99)));
        assert!(matches!(parse_range(&h("bytes=50-5000"), 100), Range::Part(50, 99)));
        assert!(matches!(parse_range(&h("bytes=100-"), 100), Range::Unsatisfiable));
        assert!(matches!(parse_range(&h("bytes=0-1,5-6"), 100), Range::Full));
        assert!(matches!(parse_range(&h("items=0-1"), 100), Range::Full));
        assert!(matches!(parse_range(&h("bytes=9-3"), 100), Range::Full));
    }

    #[test]
    fn disposition_is_header_safe() {
        let v = content_disposition("attachment", "ü\"\r\nx.txt");
        let s = v.to_str().unwrap();
        assert!(!s.contains('\n') && !s.contains('\r'));
        assert!(s.contains("filename*=UTF-8''%C3%BC"));
    }

    #[test]
    fn inline_only_for_media() {
        assert!(is_inline_safe("image/png"));
        assert!(!is_inline_safe("image/svg+xml"));
        assert!(!is_inline_safe("text/html"));
        assert!(is_inline_safe("application/pdf"));
    }
}
