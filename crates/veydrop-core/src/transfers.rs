//! Registry of live and recently finished transfers.
//!
//! Progress is *event driven*: byte counters are written by the transfer
//! tasks, and a single flusher task coalesces them into at most one
//! `Transfers` event every 250 ms — and only while something is active.
//! When idle the flusher is parked on a `Notify`, so idle CPU use is zero.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::auth::random_token;
use crate::bus::Bus;
use crate::error::Error;
use crate::history::HistoryStore;
use crate::model::{
    now_ms, EngineEvent, HistoryEntry, Stats, TransferDirection, TransferInfo, TransferState,
};

const FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const SNAPSHOT_CAP: usize = 500;

pub fn new_id() -> String {
    random_token(9)
}

/// Exponentially weighted moving average of throughput.
pub struct SpeedMeter {
    ewma: f64,
    seeded: bool,
    last_at: Instant,
    last_bytes: u64,
}

impl SpeedMeter {
    pub fn new(bytes: u64) -> Self {
        Self { ewma: 0.0, seeded: false, last_at: Instant::now(), last_bytes: bytes }
    }

    pub fn sample(&mut self, bytes: u64, now: Instant) -> f64 {
        let dt = now.duration_since(self.last_at).as_secs_f64();
        if dt < 0.1 {
            return self.ewma;
        }
        let delta = bytes.saturating_sub(self.last_bytes) as f64;
        let inst = delta / dt;
        self.ewma = if self.seeded { 0.35 * inst + 0.65 * self.ewma } else { inst };
        self.seeded = true;
        self.last_at = now;
        self.last_bytes = bytes;
        self.ewma
    }

    pub fn reset(&mut self, bytes: u64) {
        self.ewma = 0.0;
        self.seeded = false;
        self.last_at = Instant::now();
        self.last_bytes = bytes;
    }
}

struct Entry {
    info: TransferInfo,
    meter: SpeedMeter,
    cancel: CancellationToken,
    dirty: bool,
    last_activity: Instant,
}

pub struct Transfers {
    inner: Mutex<HashMap<String, Entry>>,
    notify: Arc<Notify>,
    bus: Bus,
    history: Arc<HistoryStore>,
    stats: Mutex<Stats>,
    closed: AtomicBool,
}

impl Drop for Transfers {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }
}

impl Transfers {
    /// Must be called inside a Tokio runtime (spawns the flusher).
    pub fn new(bus: Bus, history: Arc<HistoryStore>) -> Arc<Self> {
        let t = Arc::new(Self {
            inner: Mutex::new(HashMap::new()),
            notify: Arc::new(Notify::new()),
            bus,
            history,
            stats: Mutex::new(Stats::default()),
            closed: AtomicBool::new(false),
        });
        tokio::spawn(flusher(Arc::downgrade(&t), t.notify.clone()));
        t
    }

    fn touch(&self) {
        self.notify.notify_one();
    }

    /// Registers a transfer. `info.id` must be unique.
    pub fn create(&self, info: TransferInfo) -> CancellationToken {
        let token = CancellationToken::new();
        let transferred = info.transferred;
        self.inner.lock().insert(
            info.id.clone(),
            Entry {
                info,
                meter: SpeedMeter::new(transferred),
                cancel: token.clone(),
                dirty: true,
                last_activity: Instant::now(),
            },
        );
        self.touch();
        token
    }

    /// Convenience constructor with sensible defaults.
    #[allow(clippy::too_many_arguments)]
    pub fn blank(
        direction: TransferDirection,
        name: &str,
        rel_path: Option<String>,
        size: u64,
        state: TransferState,
        session_id: Option<String>,
        device_name: &str,
        offer_id: Option<String>,
    ) -> TransferInfo {
        TransferInfo {
            id: new_id(),
            direction,
            name: name.to_string(),
            rel_path,
            size,
            transferred: 0,
            state,
            error: None,
            speed_bps: 0.0,
            eta_secs: None,
            session_id,
            device_name: device_name.to_string(),
            offer_id,
            started_at: now_ms(),
            finished_at: None,
            saved_path: None,
            sha256: None,
        }
    }

    pub fn get(&self, id: &str) -> Option<TransferInfo> {
        self.inner.lock().get(id).map(|e| e.info.clone())
    }

    pub fn cancel_token(&self, id: &str) -> Option<CancellationToken> {
        self.inner.lock().get(id).map(|e| e.cancel.clone())
    }

    /// Newest first, capped.
    pub fn list(&self) -> Vec<TransferInfo> {
        let mut v: Vec<TransferInfo> = self.inner.lock().values().map(|e| e.info.clone()).collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v.truncate(SNAPSHOT_CAP);
        v
    }

    pub fn list_for_session(&self, session_id: &str) -> Vec<TransferInfo> {
        let mut v: Vec<TransferInfo> = self
            .inner
            .lock()
            .values()
            .filter(|e| e.info.session_id.as_deref() == Some(session_id))
            .map(|e| e.info.clone())
            .collect();
        v.sort_by(|a, b| b.started_at.cmp(&a.started_at));
        v
    }

    pub fn ids_where(&self, f: impl Fn(&TransferInfo) -> bool) -> Vec<String> {
        self.inner.lock().values().filter(|e| f(&e.info)).map(|e| e.info.id.clone()).collect()
    }

    pub fn active_count(&self) -> usize {
        self.inner.lock().values().filter(|e| !e.info.state.is_terminal()).count()
    }

    pub fn stats(&self) -> Stats {
        self.stats.lock().clone()
    }

    /// Records the byte counter. Ignored once the transfer is terminal.
    pub fn set_progress(&self, id: &str, transferred: u64) {
        {
            let mut g = self.inner.lock();
            let Some(e) = g.get_mut(id) else { return };
            if e.info.state.is_terminal() {
                return;
            }
            e.info.transferred = transferred;
            e.dirty = true;
            e.last_activity = Instant::now();
        }
        self.touch();
    }

    /// Non-terminal state change (Queued/Active/Paused/Interrupted/Finalizing).
    pub fn set_state(&self, id: &str, state: TransferState) {
        debug_assert!(!state.is_terminal(), "use finish() for terminal states");
        {
            let mut g = self.inner.lock();
            let Some(e) = g.get_mut(id) else { return };
            if e.info.state.is_terminal() || e.info.state == state {
                return;
            }
            e.info.state = state;
            if matches!(state, TransferState::Paused | TransferState::Interrupted | TransferState::Queued) {
                e.info.speed_bps = 0.0;
                e.info.eta_secs = None;
            }
            let bytes = e.info.transferred;
            e.meter.reset(bytes);
            e.dirty = true;
            e.last_activity = Instant::now();
        }
        self.touch();
    }

    /// Rolls the counter back after a failed chunk and marks the transfer
    /// resumable.
    pub fn interrupt(&self, id: &str, committed: u64) {
        {
            let mut g = self.inner.lock();
            let Some(e) = g.get_mut(id) else { return };
            if e.info.state.is_terminal() {
                return;
            }
            e.info.transferred = committed;
            e.info.state = TransferState::Interrupted;
            e.info.speed_bps = 0.0;
            e.info.eta_secs = None;
            e.meter.reset(committed);
            e.dirty = true;
        }
        self.touch();
    }

    /// Moves a transfer to a terminal state exactly once, records history and
    /// statistics, and notifies the UI. Returns `None` if it was already
    /// terminal (so callers can't double count).
    pub fn finish(
        &self,
        id: &str,
        state: TransferState,
        error: Option<&Error>,
        saved_path: Option<String>,
        sha256: Option<String>,
    ) -> Option<TransferInfo> {
        debug_assert!(state.is_terminal());
        let (info, token) = {
            let mut g = self.inner.lock();
            let e = g.get_mut(id)?;
            if e.info.state.is_terminal() {
                return None;
            }
            e.info.state = state;
            e.info.error = error.map(|er| er.code().to_string());
            e.info.finished_at = Some(now_ms());
            e.info.speed_bps = 0.0;
            e.info.eta_secs = None;
            if state == TransferState::Completed {
                e.info.transferred = e.info.size;
            }
            if saved_path.is_some() {
                e.info.saved_path = saved_path;
            }
            if sha256.is_some() {
                e.info.sha256 = sha256;
            }
            e.dirty = false;
            (e.info.clone(), e.cancel.clone())
        };
        if matches!(state, TransferState::Cancelled | TransferState::Failed | TransferState::Rejected) {
            token.cancel();
        }
        {
            let mut s = self.stats.lock();
            match (state, info.direction) {
                (TransferState::Completed, TransferDirection::Outgoing) => {
                    s.sent_bytes += info.size;
                    s.sent_files += 1;
                }
                (TransferState::Completed, TransferDirection::Incoming) => {
                    s.received_bytes += info.size;
                    s.received_files += 1;
                }
                (TransferState::Failed, _) => s.failed += 1,
                _ => {}
            }
        }
        let finished = info.finished_at.unwrap_or_else(now_ms);
        // Offers that were rejected before any byte moved aren't "activity".
        if state != TransferState::Rejected {
            self.history.add(HistoryEntry {
                id: info.id.clone(),
                direction: info.direction,
                name: info.name.clone(),
                rel_path: info.rel_path.clone(),
                size: info.size,
                transferred: info.transferred,
                device_name: info.device_name.clone(),
                state,
                error: info.error.clone(),
                started_at: info.started_at,
                finished_at: finished,
                duration_ms: finished.saturating_sub(info.started_at),
                saved_path: info.saved_path.clone(),
                sha256: info.sha256.clone(),
            });
            self.bus.emit(EngineEvent::HistoryChanged);
        }
        self.bus.emit(EngineEvent::TransferDone(info.clone()));
        self.bus.emit(EngineEvent::Stats(self.stats()));
        Some(info)
    }

    /// Cancels a transfer (idempotent). Callers remove any partial file.
    pub fn cancel(&self, id: &str) -> Option<TransferInfo> {
        self.finish(id, TransferState::Cancelled, Some(&Error::Cancelled), None, None)
    }

    /// Active transfers with no byte movement for `idle` — their connection is
    /// presumably gone. Returns the ids that were flipped to Interrupted.
    pub fn mark_stalled(&self, idle: Duration) -> Vec<String> {
        let now = Instant::now();
        let mut hit = Vec::new();
        {
            let mut g = self.inner.lock();
            for e in g.values_mut() {
                if e.info.state == TransferState::Active && now.duration_since(e.last_activity) > idle {
                    e.info.state = TransferState::Interrupted;
                    e.info.speed_bps = 0.0;
                    e.info.eta_secs = None;
                    e.dirty = true;
                    hit.push(e.info.id.clone());
                }
            }
        }
        if !hit.is_empty() {
            self.touch();
        }
        hit
    }

    /// Drops finished transfers older than `age` from the live list (they
    /// remain in history).
    pub fn prune(&self, age: Duration) {
        let cutoff = now_ms().saturating_sub(age.as_millis() as u64);
        self.inner.lock().retain(|_, e| match e.info.finished_at {
            Some(t) if e.info.state.is_terminal() => t > cutoff,
            _ => true,
        });
    }

    /// Delta snapshot: transfers changed since the previous call.
    fn drain_dirty(&self) -> (Vec<TransferInfo>, bool) {
        let now = Instant::now();
        let mut g = self.inner.lock();
        let mut out = Vec::new();
        let mut any_active = false;
        for e in g.values_mut() {
            if e.info.state == TransferState::Active {
                any_active = true;
                let speed = e.meter.sample(e.info.transferred, now);
                e.info.speed_bps = speed;
                e.info.eta_secs = if speed > 1.0 && e.info.size > e.info.transferred {
                    Some(((e.info.size - e.info.transferred) as f64 / speed).ceil() as u64)
                } else {
                    None
                };
                e.dirty = true;
            }
            if e.dirty {
                e.dirty = false;
                out.push(e.info.clone());
            }
        }
        (out, any_active)
    }
}

async fn flusher(weak: Weak<Transfers>, notify: Arc<Notify>) {
    loop {
        notify.notified().await;
        loop {
            tokio::time::sleep(FLUSH_INTERVAL).await;
            let Some(t) = weak.upgrade() else { return };
            if t.closed.load(Ordering::SeqCst) {
                return;
            }
            let (batch, any_active) = t.drain_dirty();
            if !batch.is_empty() {
                t.bus.emit(EngineEvent::Transfers(batch));
            }
            if !any_active {
                break;
            }
        }
        if weak.upgrade().is_none() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk() -> (Arc<Transfers>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let hist = HistoryStore::load(dir.path());
        (Transfers::new(Bus::new(), hist), dir)
    }

    #[tokio::test]
    async fn terminal_state_is_final_and_counted_once() {
        let (t, _d) = mk();
        let info = Transfers::blank(
            TransferDirection::Incoming, "a.bin", None, 10, TransferState::Active, None, "Phone", None,
        );
        let id = info.id.clone();
        t.create(info);
        assert!(t.finish(&id, TransferState::Completed, None, None, None).is_some());
        assert!(t.finish(&id, TransferState::Failed, Some(&Error::HashMismatch), None, None).is_none());
        let s = t.stats();
        assert_eq!((s.received_files, s.received_bytes, s.failed), (1, 10, 0));
        assert_eq!(t.get(&id).unwrap().state, TransferState::Completed);
    }

    #[tokio::test]
    async fn failed_is_never_reported_as_completed() {
        let (t, _d) = mk();
        let info = Transfers::blank(
            TransferDirection::Outgoing, "a.bin", None, 10, TransferState::Active, None, "Phone", None,
        );
        let id = info.id.clone();
        t.create(info);
        t.set_progress(&id, 4);
        t.finish(&id, TransferState::Failed, Some(&Error::Io(std::io::Error::other("x"))), None, None);
        let i = t.get(&id).unwrap();
        assert_eq!(i.state, TransferState::Failed);
        assert_eq!(i.transferred, 4);
        assert_eq!(t.stats().sent_files, 0);
        assert_eq!(t.stats().failed, 1);
    }

    #[tokio::test]
    async fn stalled_active_transfers_become_interrupted() {
        let (t, _d) = mk();
        let info = Transfers::blank(
            TransferDirection::Incoming, "a", None, 10, TransferState::Active, None, "P", None,
        );
        let id = info.id.clone();
        t.create(info);
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(t.mark_stalled(Duration::from_millis(5)), vec![id.clone()]);
        assert_eq!(t.get(&id).unwrap().state, TransferState::Interrupted);
    }

    #[test]
    fn speed_meter_tracks_rate() {
        let mut m = SpeedMeter::new(0);
        let t0 = m.last_at;
        let s = m.sample(1_000_000, t0 + Duration::from_secs(1));
        assert!((s - 1_000_000.0).abs() < 1.0);
    }
}
