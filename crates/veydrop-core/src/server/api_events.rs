//! Server-sent events: the phone's live channel (share changes, offer
//! decisions, revocation). No polling.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use tokio::sync::broadcast::error::RecvError;

use super::{Ctx, Session};
use crate::model::EngineEvent;

const SESSION_CHECK: Duration = Duration::from_secs(30);

/// Marks the session online while the stream exists.
struct OnlineGuard {
    ctx: Arc<Ctx>,
    id: String,
}

impl OnlineGuard {
    fn new(ctx: Arc<Ctx>, id: String) -> Self {
        if ctx.core.auth.set_online(&id, 1).is_some() {
            ctx.core.bus.emit(EngineEvent::Sessions(ctx.core.auth.sessions()));
        }
        Self { ctx, id }
    }
}

impl Drop for OnlineGuard {
    fn drop(&mut self) {
        if self.ctx.core.auth.set_online(&self.id, -1).is_some() {
            self.ctx.core.bus.emit(EngineEvent::Sessions(self.ctx.core.auth.sessions()));
        }
    }
}

enum Next {
    Phone(crate::model::PhoneEvent),
    Lagged,
    Closed,
    Tick,
}

fn ev(kind: &str, data: &serde_json::Value) -> Event {
    Event::default().event(kind).data(data.to_string())
}

/// `GET /api/events`
pub async fn events(State(ctx): State<Arc<Ctx>>, Session(s): Session) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let guard = OnlineGuard::new(ctx.clone(), s.id.clone());
    let mut rx = ctx.core.bus.subscribe_phones();
    let sid = s.id.clone();

    let stream = async_stream::stream! {
        let _guard = guard;
        let hello = serde_json::json!({
            "session": ctx.core.auth.session(&sid),
            "pcName": ctx.core.settings.get().device_name,
            "language": ctx.core.settings.effective_language(),
            "share": ctx.core.shares.phone_info(),
            "transfers": ctx.core.incoming.session_snapshot(&sid),
        });
        yield Ok(ev("hello", &hello));

        let mut check = tokio::time::interval(SESSION_CHECK);
        check.tick().await;
        loop {
            let next = tokio::select! {
                r = rx.recv() => match r {
                    Ok(e) => Next::Phone(e),
                    Err(RecvError::Lagged(_)) => Next::Lagged,
                    Err(RecvError::Closed) => Next::Closed,
                },
                _ = check.tick() => Next::Tick,
                _ = ctx.shutdown.cancelled() => Next::Closed,
            };
            match next {
                Next::Phone(e) => {
                    if e.target.is_none() || e.target.as_deref() == Some(sid.as_str()) {
                        yield Ok(ev(e.kind, &e.data));
                        if e.kind == "revoked" {
                            return;
                        }
                    }
                }
                Next::Lagged => {
                    // Missed some events: tell the page to reload its state.
                    yield Ok(ev("resync", &serde_json::json!({})));
                }
                Next::Closed => return,
                Next::Tick => {
                    if ctx.core.auth.session(&sid).is_none() {
                        yield Ok(ev("revoked", &serde_json::json!({})));
                        return;
                    }
                }
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("ka"))
}
