//! Registry of partial (`*.veydrop-part`) files on disk.
//!
//! Every incomplete upload registers here so that files left behind by a crash
//! or an abandoned session can be found and removed after the retention
//! period. The registry is persisted (`partials.json`) with a ~500 ms debounce.
//!
//! Only paths ending in [`PART_SUFFIX`] are ever deleted, so a tampered
//! `partials.json` cannot be used to delete arbitrary files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::fsutil;
use crate::model::{now_ms, Ms};

pub const PART_SUFFIX: &str = ".veydrop-part";
const FLUSH_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Record {
    tid: String,
    path: String,
    created_at: Ms,
}

pub struct Partials {
    file: PathBuf,
    map: Mutex<HashMap<String, Record>>,
    scheduled: AtomicBool,
}

impl Partials {
    pub fn load(data_dir: &Path) -> Arc<Self> {
        let file = data_dir.join("partials.json");
        let records: Vec<Record> =
            std::fs::read(&file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let map = records.into_iter().map(|r| (r.tid.clone(), r)).collect();
        Arc::new(Self { file, map: Mutex::new(map), scheduled: AtomicBool::new(false) })
    }

    pub fn add(self: &Arc<Self>, tid: &str, path: &Path) {
        self.map.lock().insert(
            tid.to_string(),
            Record { tid: tid.to_string(), path: path.to_string_lossy().into_owned(), created_at: now_ms() },
        );
        self.schedule();
    }

    pub fn remove(self: &Arc<Self>, tid: &str) {
        if self.map.lock().remove(tid).is_some() {
            self.schedule();
        }
    }

    pub fn len(&self) -> usize {
        self.map.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Records older than `max_age`: `(transfer id, path)`.
    pub fn older_than(&self, max_age: Duration) -> Vec<(String, PathBuf)> {
        let cutoff = now_ms().saturating_sub(max_age.as_millis() as u64);
        self.map
            .lock()
            .values()
            .filter(|r| r.created_at <= cutoff)
            .map(|r| (r.tid.clone(), PathBuf::from(&r.path)))
            .collect()
    }

    /// Deletes the file behind a record (only if it really is a part file) and
    /// forgets the record.
    pub fn purge(self: &Arc<Self>, tid: &str, path: &Path) {
        let is_part = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(PART_SUFFIX));
        if is_part {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => tracing::warn!("could not remove stale partial file: {e}"),
            }
        }
        self.remove(tid);
    }

    /// Startup / periodic cleanup of everything older than `max_age` that is
    /// not owned by a live upload (`live(tid)`).
    pub fn purge_stale(self: &Arc<Self>, max_age: Duration, live: impl Fn(&str) -> bool) -> usize {
        let mut n = 0;
        for (tid, path) in self.older_than(max_age) {
            if !live(&tid) {
                self.purge(&tid, &path);
                n += 1;
            }
        }
        n
    }

    fn schedule(self: &Arc<Self>) {
        if self.scheduled.swap(true, Ordering::SeqCst) {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(h) => {
                let me = self.clone();
                h.spawn(async move {
                    tokio::time::sleep(FLUSH_DELAY).await;
                    me.scheduled.store(false, Ordering::SeqCst);
                    let me2 = me.clone();
                    let _ = tokio::task::spawn_blocking(move || me2.flush()).await;
                });
            }
            Err(_) => {
                self.scheduled.store(false, Ordering::SeqCst);
                self.flush();
            }
        }
    }

    /// Writes the registry now (also called at engine shutdown).
    pub fn flush(&self) {
        let records: Vec<Record> = self.map.lock().values().cloned().collect();
        match serde_json::to_vec(&records) {
            Ok(bytes) => {
                if let Err(e) = fsutil::write_atomic(&self.file, &bytes) {
                    tracing::warn!("could not persist partials registry: {e}");
                }
            }
            Err(e) => tracing::warn!("could not serialise partials registry: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stale_part_files_are_removed_and_others_are_not() {
        let d = tempfile::tempdir().unwrap();
        let part = d.path().join("a.bin.12345678.veydrop-part");
        let other = d.path().join("important.docx");
        std::fs::write(&part, b"x").unwrap();
        std::fs::write(&other, b"y").unwrap();
        let p = Partials::load(d.path());
        p.add("t1", &part);
        p.add("t2", &other); // tampered registry entry: must never be deleted
        p.flush();
        let p = Partials::load(d.path());
        assert_eq!(p.len(), 2);
        let n = p.purge_stale(Duration::ZERO, |_| false);
        assert_eq!(n, 2);
        assert!(!part.exists());
        assert!(other.exists());
        assert!(p.is_empty());
    }

    #[tokio::test]
    async fn live_entries_are_kept() {
        let d = tempfile::tempdir().unwrap();
        let part = d.path().join("a.1.veydrop-part");
        std::fs::write(&part, b"x").unwrap();
        let p = Partials::load(d.path());
        p.add("t1", &part);
        assert_eq!(p.purge_stale(Duration::ZERO, |t| t == "t1"), 0);
        assert!(part.exists());
    }
}
