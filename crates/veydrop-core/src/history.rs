//! Transfer history persisted to `history.json`. Contains metadata only;
//! never tokens, cookies or session secrets.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::fsutil;
use crate::model::{now_ms, HistoryEntry, TransferDirection, TransferState};

const MAX_ENTRIES: usize = 5_000;

pub struct HistoryStore {
    path: PathBuf,
    entries: Mutex<Vec<HistoryEntry>>,
    dirty: Notify,
    flusher_started: Mutex<bool>,
}

#[derive(Default, Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryQuery {
    pub search: Option<String>,
    /// "incoming" | "outgoing"
    pub direction: Option<String>,
    /// "completed" | "failed" | "cancelled"
    pub status: Option<String>,
    pub limit: Option<usize>,
}

impl HistoryStore {
    pub fn load(data_dir: &Path) -> Arc<Self> {
        let path = data_dir.join("history.json");
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<HistoryEntry>>(&b).ok())
            .unwrap_or_default();
        Arc::new(HistoryStore {
            path,
            entries: Mutex::new(entries),
            dirty: Notify::new(),
            flusher_started: Mutex::new(false),
        })
    }

    /// Starts the debounced writer (needs a runtime). Idempotent. Idle cost
    /// is zero: the task parks on a `Notify`.
    pub fn start_flusher(self: &Arc<Self>) {
        let mut started = self.flusher_started.lock();
        if *started {
            return;
        }
        *started = true;
        let this = self.clone();
        tokio::spawn(async move {
            loop {
                this.dirty.notified().await;
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let me = this.clone();
                let _ = tokio::task::spawn_blocking(move || me.flush()).await;
            }
        });
    }

    pub fn flush(&self) {
        let bytes = {
            let guard = self.entries.lock();
            serde_json::to_vec(&*guard)
        };
        match bytes {
            Ok(b) => {
                if let Err(e) = fsutil::write_atomic(&self.path, &b) {
                    tracing::error!("could not save history: {e}");
                }
            }
            Err(e) => tracing::error!("could not serialise history: {e}"),
        }
    }

    pub fn add(&self, entry: HistoryEntry) {
        {
            let mut g = self.entries.lock();
            g.push(entry);
            let len = g.len();
            if len > MAX_ENTRIES {
                g.drain(0..len - MAX_ENTRIES);
            }
        }
        self.dirty.notify_one();
    }

    pub fn list(&self, q: &HistoryQuery) -> Vec<HistoryEntry> {
        let needle = q.search.as_ref().map(|s| s.to_lowercase()).filter(|s| !s.is_empty());
        let g = self.entries.lock();
        let mut out: Vec<HistoryEntry> = g
            .iter()
            .rev()
            .filter(|e| match q.direction.as_deref() {
                Some("incoming") => e.direction == TransferDirection::Incoming,
                Some("outgoing") => e.direction == TransferDirection::Outgoing,
                _ => true,
            })
            .filter(|e| match q.status.as_deref() {
                Some("completed") => e.state == TransferState::Completed,
                Some("failed") => e.state == TransferState::Failed,
                Some("cancelled") => matches!(e.state, TransferState::Cancelled | TransferState::Rejected),
                _ => true,
            })
            .filter(|e| match &needle {
                Some(n) => {
                    e.name.to_lowercase().contains(n)
                        || e.device_name.to_lowercase().contains(n)
                        || e.rel_path.as_deref().unwrap_or("").to_lowercase().contains(n)
                }
                None => true,
            })
            .cloned()
            .collect();
        if let Some(l) = q.limit {
            out.truncate(l);
        }
        out
    }

    pub fn remove(&self, id: &str) {
        self.entries.lock().retain(|e| e.id != id);
        self.dirty.notify_one();
    }

    pub fn clear(&self) {
        self.entries.lock().clear();
        self.dirty.notify_one();
    }

    pub fn prune(&self, retention_days: u32) {
        if retention_days == 0 {
            return;
        }
        let cutoff = now_ms().saturating_sub(retention_days as u64 * 86_400_000);
        let mut g = self.entries.lock();
        let before = g.len();
        g.retain(|e| e.finished_at >= cutoff);
        let changed = g.len() != before;
        drop(g);
        if changed {
            self.dirty.notify_one();
        }
    }
}
