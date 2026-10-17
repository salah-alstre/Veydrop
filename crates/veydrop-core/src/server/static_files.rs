//! Serves the bundled phone web app. Nothing outside `mobile_dir` is ever
//! reachable, and the shell itself exposes no data (every API call needs a
//! session).

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::header::{self, HeaderValue};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use super::Ctx;

const MISSING_PAGE: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Veydrop</title><body style=\"font-family:system-ui;background:#050505;color:#ddd;padding:2rem\"><h1>Veydrop</h1><p>The phone interface is not installed on this PC. Reinstall Veydrop.</p>";

fn resolve(root: &Path, uri_path: &str) -> Option<PathBuf> {
    let decoded = percent_encoding::percent_decode_str(uri_path).decode_utf8().ok()?;
    if decoded.contains('\\') || decoded.contains(':') || decoded.contains('\0') {
        return None;
    }
    let mut out = root.to_path_buf();
    for comp in Path::new(decoded.trim_start_matches('/')).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(out)
}

fn cache_header(path: &Path) -> &'static str {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let in_assets = path.components().any(|c| c.as_os_str() == "assets");
    if in_assets {
        // Vite fingerprints everything in /assets.
        "public, max-age=31536000, immutable"
    } else if name == "index.html" || name == "sw.js" {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

pub async fn serve(State(ctx): State<Arc<Ctx>>, method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let root = &ctx.mobile_dir;
    let Some(mut path) = resolve(root, uri.path()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let wants_asset = Path::new(uri.path()).extension().is_some();
    if path.is_dir() || (!wants_asset && !path.is_file()) {
        // SPA route (or directory): the app shell.
        path = root.join("index.html");
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            let mut resp = Response::new(if method == Method::HEAD { Body::empty() } else { Body::from(bytes.clone()) });
            let h = resp.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_str(mime.as_ref()).expect("mime is ascii"));
            h.insert(header::CONTENT_LENGTH, HeaderValue::from(bytes.len()));
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache_header(&path)));
            resp
        }
        Err(_) if wants_asset => StatusCode::NOT_FOUND.into_response(),
        Err(_) => {
            let mut resp = (StatusCode::SERVICE_UNAVAILABLE, MISSING_PAGE).into_response();
            resp.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
            resp
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_blocks_traversal() {
        let root = Path::new("C:/site");
        assert!(resolve(root, "/a/../b").is_none());
        assert!(resolve(root, "/%2e%2e/secret").is_none());
        assert!(resolve(root, "/a%5cb").is_none());
        assert!(resolve(root, "/C:/windows").is_none());
        assert_eq!(resolve(root, "/assets/app.js").unwrap(), root.join("assets").join("app.js"));
        assert_eq!(resolve(root, "/").unwrap(), root.to_path_buf());
    }
}
