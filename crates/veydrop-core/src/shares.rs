//! The PC → phone side: the set of files the user has explicitly staged.
//!
//! Only items listed here can ever be served. Phones refer to them by random
//! item ids, never by path. A background indexer computes SHA-256 + CRC-32 so
//! the phone can verify downloads it controls and ZIPs can carry an exact
//! length.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::auth::random_token;
use crate::bus::Bus;
use crate::error::{Error, Result};
use crate::fsutil;
use crate::model::{now_ms, EngineEvent, Ms, ShareInfo, ShareItemInfo};
use crate::zipstream::ZipEntry;

/// Upper bound on staged files (protects the UI and the phone's JSON).
pub const MAX_ITEMS: usize = 20_000;
const EMIT_EVERY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct ShareItem {
    pub id: String,
    pub name: String,
    pub rel_path: String,
    pub abs_path: PathBuf,
    pub size: u64,
    pub mtime_ms: Ms,
    pub sha256: Option<String>,
    pub crc32: Option<u32>,
    pub missing: bool,
}

impl ShareItem {
    pub fn mtime(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_millis(self.mtime_ms)
    }
}

struct ShareState {
    id: String,
    created_at: Ms,
    active: bool,
    items: Vec<ShareItem>,
    indexed: bool,
    index_cancel: CancellationToken,
    generation: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddReport {
    pub share: ShareInfo,
    /// Files that could not be read, exceeded the item cap, or were symlinks.
    pub skipped: usize,
}

pub enum ZipSel {
    All,
    /// A staged folder: everything under `rel_path/`.
    Prefix(String),
    Ids(Vec<String>),
}

pub struct Shares {
    state: Mutex<Option<ShareState>>,
    bus: Bus,
}

fn mtime_ms(meta: &std::fs::Metadata) -> Ms {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn recompute_indexed(items: &[ShareItem]) -> bool {
    items.iter().all(|i| i.sha256.is_some() || i.missing)
}

fn to_info(st: &ShareState) -> ShareInfo {
    ShareInfo {
        id: st.id.clone(),
        created_at: st.created_at,
        active: st.active,
        total_size: st.items.iter().filter(|i| !i.missing).map(|i| i.size).sum(),
        indexed: st.indexed,
        items: st
            .items
            .iter()
            .map(|i| ShareItemInfo {
                id: i.id.clone(),
                name: i.name.clone(),
                rel_path: i.rel_path.clone(),
                size: i.size,
                sha256: i.sha256.clone(),
                ready: i.sha256.is_some(),
                missing: i.missing,
            })
            .collect(),
    }
}

/// Inserts " (n)" before the extension of the last path segment.
fn uniquified(rel: &str, n: u32) -> String {
    let (dir, file) = match rel.rsplit_once('/') {
        Some((d, f)) => (Some(d), f),
        None => (None, rel),
    };
    let (stem, ext) = match file.rfind('.') {
        Some(i) if i > 0 => (&file[..i], &file[i..]),
        _ => (file, ""),
    };
    let f = format!("{stem} ({n}){ext}");
    match dir {
        Some(d) => format!("{d}/{f}"),
        None => f,
    }
}

struct Found {
    abs: PathBuf,
    rel: String,
    size: u64,
    mtime: Ms,
}

/// Blocking: walks the chosen paths and stats every regular file.
fn collect(paths: Vec<PathBuf>) -> (Vec<Found>, usize) {
    let mut found = Vec::new();
    let mut skipped = 0usize;
    for p in paths {
        if found.len() >= MAX_ITEMS {
            skipped += 1;
            continue;
        }
        let meta = match std::fs::symlink_metadata(&p) {
            Ok(m) => m,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        if meta.file_type().is_symlink() {
            skipped += 1;
        } else if meta.is_file() {
            let name = fsutil::sanitize_component(&p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            found.push(Found { abs: p, rel: name, size: meta.len(), mtime: mtime_ms(&meta) });
        } else if meta.is_dir() {
            let root = fsutil::sanitize_component(
                &p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "Folder".into()),
            );
            for entry in walkdir::WalkDir::new(&p).follow_links(false).into_iter() {
                let Ok(entry) = entry else {
                    skipped += 1;
                    continue;
                };
                if !entry.file_type().is_file() {
                    continue;
                }
                if found.len() >= MAX_ITEMS {
                    skipped += 1;
                    continue;
                }
                let Ok(m) = entry.metadata() else {
                    skipped += 1;
                    continue;
                };
                let Ok(sub) = entry.path().strip_prefix(&p) else { continue };
                let joined = sub
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                let parts = fsutil::sanitize_rel_path(&format!("{root}/{joined}"));
                if parts.is_empty() {
                    skipped += 1;
                    continue;
                }
                found.push(Found {
                    abs: entry.path().to_path_buf(),
                    rel: parts.join("/"),
                    size: m.len(),
                    mtime: mtime_ms(&m),
                });
            }
        } else {
            skipped += 1;
        }
    }
    (found, skipped)
}

enum Hashed {
    Done(String, u32),
    Changed,
    Cancelled,
}

fn hash_file(path: &Path, size: u64, mtime: Ms, cancel: &CancellationToken) -> std::io::Result<Hashed> {
    let mut f = std::fs::File::open(path)?;
    let mut sha = Sha256::new();
    let mut crc = crc32fast::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        if cancel.is_cancelled() {
            return Ok(Hashed::Cancelled);
        }
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
        crc.update(&buf[..n]);
        total += n as u64;
    }
    let meta = std::fs::metadata(path)?;
    if total != size || meta.len() != size || mtime_ms(&meta) != mtime {
        return Ok(Hashed::Changed);
    }
    Ok(Hashed::Done(hex::encode(sha.finalize()), crc.finalize()))
}

impl Shares {
    pub fn new(bus: Bus) -> Arc<Self> {
        Arc::new(Self { state: Mutex::new(None), bus })
    }

    fn announce(&self) {
        self.bus.emit(EngineEvent::Share(self.info()));
        self.bus.phone(None, "share", serde_json::json!({}));
    }

    pub fn info(&self) -> Option<ShareInfo> {
        self.state.lock().as_ref().map(to_info)
    }

    /// What a phone may see: nothing unless the user is actively sharing.
    pub fn phone_info(&self) -> Option<ShareInfo> {
        self.state.lock().as_ref().filter(|s| s.active).map(to_info)
    }

    pub fn is_active(&self) -> bool {
        self.state.lock().as_ref().is_some_and(|s| s.active)
    }

    pub fn item(&self, id: &str) -> Option<ShareItem> {
        self.state.lock().as_ref()?.items.iter().find(|i| i.id == id).cloned()
    }

    /// Like [`item`], but only while the share is active and the file still
    /// matches what was staged.
    pub fn servable_item(&self, id: &str) -> Result<ShareItem> {
        let g = self.state.lock();
        let st = g.as_ref().filter(|s| s.active).ok_or(Error::NotFound)?;
        let item = st.items.iter().find(|i| i.id == id).ok_or(Error::NotFound)?;
        if item.missing {
            return Err(Error::Conflict("file_changed".into()));
        }
        Ok(item.clone())
    }

    /// Stages files and folders. Existing items are kept.
    pub async fn add_paths(self: &Arc<Self>, paths: Vec<PathBuf>) -> Result<AddReport> {
        if paths.is_empty() {
            return Err(Error::invalid("no paths"));
        }
        let (found, mut skipped) = tokio::task::spawn_blocking(move || collect(paths))
            .await
            .map_err(|e| Error::Other(e.to_string()))?;

        {
            let mut g = self.state.lock();
            let st = g.get_or_insert_with(|| ShareState {
                id: random_token(9),
                created_at: now_ms(),
                active: false,
                items: Vec::new(),
                indexed: true,
                index_cancel: CancellationToken::new(),
                generation: 0,
            });
            let mut taken: HashSet<String> = st.items.iter().map(|i| i.rel_path.to_lowercase()).collect();
            let mut known: HashSet<PathBuf> = st.items.iter().map(|i| i.abs_path.clone()).collect();
            for f in found {
                if !known.insert(f.abs.clone()) {
                    continue; // same file picked twice
                }
                if st.items.len() >= MAX_ITEMS {
                    skipped += 1;
                    continue;
                }
                let mut rel = f.rel.clone();
                let mut n = 1;
                while taken.contains(&rel.to_lowercase()) {
                    n += 1;
                    rel = uniquified(&f.rel, n);
                }
                taken.insert(rel.to_lowercase());
                let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
                st.items.push(ShareItem {
                    id: random_token(9),
                    name,
                    rel_path: rel,
                    abs_path: f.abs,
                    size: f.size,
                    mtime_ms: f.mtime,
                    sha256: None,
                    crc32: None,
                    missing: false,
                });
            }
            st.indexed = recompute_indexed(&st.items);
            if st.items.is_empty() {
                *g = None;
            }
        }
        self.start_indexer();
        self.announce();
        let share = self.info().ok_or_else(|| Error::invalid("nothing could be added"))?;
        Ok(AddReport { share, skipped })
    }

    pub fn remove_item(&self, id: &str) -> Result<()> {
        {
            let mut g = self.state.lock();
            let st = g.as_mut().ok_or(Error::NotFound)?;
            let before = st.items.len();
            st.items.retain(|i| i.id != id);
            if st.items.len() == before {
                return Err(Error::NotFound);
            }
            st.indexed = recompute_indexed(&st.items);
            if st.items.is_empty() {
                st.index_cancel.cancel();
                *g = None;
            }
        }
        self.announce();
        Ok(())
    }

    pub fn clear(&self) {
        {
            let mut g = self.state.lock();
            if let Some(st) = g.take() {
                st.index_cancel.cancel();
            }
        }
        self.announce();
    }

    /// Starts/stops serving. Staged items are kept either way.
    pub fn set_active(&self, active: bool) -> Result<()> {
        {
            let mut g = self.state.lock();
            let st = g.as_mut().ok_or(Error::NotFound)?;
            if st.active == active {
                return Ok(());
            }
            st.active = active;
        }
        self.announce();
        Ok(())
    }

    /// Entries for a ZIP download. Requires an active, fully indexed share so
    /// each entry carries a verified size + CRC.
    pub fn items_for_zip(&self, sel: &ZipSel) -> Result<Vec<ZipEntry>> {
        let g = self.state.lock();
        let st = g.as_ref().filter(|s| s.active).ok_or(Error::NotFound)?;
        if !st.indexed {
            return Err(Error::Conflict("share_indexing".into()));
        }
        let ids: HashSet<&str> = match sel {
            ZipSel::Ids(v) => v.iter().map(String::as_str).collect(),
            _ => HashSet::new(),
        };
        let entries: Vec<ZipEntry> = st
            .items
            .iter()
            .filter(|i| !i.missing)
            .filter(|i| match sel {
                ZipSel::All => true,
                ZipSel::Prefix(p) => i.rel_path.starts_with(&format!("{}/", p.trim_end_matches('/'))),
                ZipSel::Ids(_) => ids.contains(i.id.as_str()),
            })
            .filter_map(|i| {
                Some(ZipEntry {
                    name: i.rel_path.clone(),
                    path: i.abs_path.clone(),
                    size: i.size,
                    crc32: i.crc32?,
                    mtime: i.mtime(),
                })
            })
            .collect();
        if entries.is_empty() {
            return Err(Error::NotFound);
        }
        Ok(entries)
    }

    /// Fails with `Conflict("file_changed")` if the file no longer matches
    /// what was staged (so we never serve something the user didn't pick).
    pub fn verify_fresh(item: &ShareItem) -> Result<()> {
        let meta = std::fs::metadata(&item.abs_path).map_err(|_| Error::Conflict("file_changed".into()))?;
        if meta.len() != item.size || mtime_ms(&meta) != item.mtime_ms {
            return Err(Error::Conflict("file_changed".into()));
        }
        Ok(())
    }

    fn start_indexer(self: &Arc<Self>) {
        let (gen, token) = {
            let mut g = self.state.lock();
            let Some(st) = g.as_mut() else { return };
            st.index_cancel.cancel();
            st.index_cancel = CancellationToken::new();
            st.generation += 1;
            if st.indexed {
                return;
            }
            (st.generation, st.index_cancel.clone())
        };
        let me = self.clone();
        tokio::spawn(async move { me.run_indexer(gen, token).await });
    }

    async fn run_indexer(self: Arc<Self>, gen: u64, token: CancellationToken) {
        let mut last_emit = Instant::now();
        loop {
            if token.is_cancelled() {
                return;
            }
            let next = {
                let g = self.state.lock();
                let Some(st) = g.as_ref().filter(|s| s.generation == gen) else { return };
                st.items
                    .iter()
                    .find(|i| i.sha256.is_none() && !i.missing)
                    .map(|i| (i.id.clone(), i.abs_path.clone(), i.size, i.mtime_ms))
            };
            let Some((id, path, size, mtime)) = next else { break };
            let t = token.clone();
            let res = tokio::task::spawn_blocking(move || hash_file(&path, size, mtime, &t)).await;
            let outcome = match res {
                Ok(Ok(Hashed::Cancelled)) => return,
                Ok(Ok(h)) => h,
                Ok(Err(e)) => {
                    tracing::debug!("indexing failed: {e}");
                    Hashed::Changed
                }
                Err(_) => return,
            };
            {
                let mut g = self.state.lock();
                let Some(st) = g.as_mut().filter(|s| s.generation == gen) else { return };
                if let Some(item) = st.items.iter_mut().find(|i| i.id == id) {
                    match outcome {
                        Hashed::Done(sha, crc) => {
                            item.sha256 = Some(sha);
                            item.crc32 = Some(crc);
                        }
                        _ => item.missing = true,
                    }
                }
            }
            if last_emit.elapsed() >= EMIT_EVERY {
                last_emit = Instant::now();
                self.bus.emit(EngineEvent::Share(self.info()));
            }
        }
        {
            let mut g = self.state.lock();
            let Some(st) = g.as_mut().filter(|s| s.generation == gen) else { return };
            st.indexed = recompute_indexed(&st.items);
        }
        self.announce();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_indexed(s: &Shares) {
        for _ in 0..200 {
            if s.info().is_some_and(|i| i.indexed) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("indexing never finished");
    }

    #[tokio::test]
    async fn stages_files_and_folders_and_hashes_them() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), b"abc").unwrap();
        let folder = d.path().join("Photos");
        std::fs::create_dir_all(folder.join("2024")).unwrap();
        std::fs::write(folder.join("x.jpg"), b"1").unwrap();
        std::fs::write(folder.join("2024").join("y.jpg"), b"22").unwrap();

        let s = Shares::new(Bus::new());
        let rep = s.add_paths(vec![d.path().join("a.txt"), folder]).await.unwrap();
        assert_eq!(rep.skipped, 0);
        let mut rels: Vec<_> = rep.share.items.iter().map(|i| i.rel_path.clone()).collect();
        rels.sort();
        assert_eq!(rels, ["Photos/2024/y.jpg", "Photos/x.jpg", "a.txt"]);
        assert!(!rep.share.active, "staging must not start sharing");
        assert!(s.phone_info().is_none(), "phones see nothing until sharing starts");

        wait_indexed(&s).await;
        let a = s.info().unwrap().items.into_iter().find(|i| i.name == "a.txt").unwrap();
        assert_eq!(a.sha256.unwrap(), hex::encode(Sha256::digest(b"abc")));
    }

    #[tokio::test]
    async fn duplicate_names_are_made_unique_and_repeats_ignored() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("one")).unwrap();
        std::fs::create_dir_all(d.path().join("two")).unwrap();
        let p1 = d.path().join("one").join("same.txt");
        let p2 = d.path().join("two").join("same.txt");
        std::fs::write(&p1, b"1").unwrap();
        std::fs::write(&p2, b"2").unwrap();
        let s = Shares::new(Bus::new());
        s.add_paths(vec![p1.clone(), p2, p1]).await.unwrap();
        let mut rels: Vec<_> = s.info().unwrap().items.into_iter().map(|i| i.rel_path).collect();
        rels.sort();
        assert_eq!(rels, ["same (2).txt", "same.txt"]);
    }

    #[tokio::test]
    async fn modified_files_are_flagged_and_never_served() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.bin");
        std::fs::write(&p, b"hello").unwrap();
        let s = Shares::new(Bus::new());
        s.add_paths(vec![p.clone()]).await.unwrap();
        wait_indexed(&s).await;
        s.set_active(true).unwrap();
        let id = s.info().unwrap().items[0].id.clone();
        assert!(s.servable_item(&id).is_ok());
        let item = s.item(&id).unwrap();
        std::fs::write(&p, b"hello, but longer now").unwrap();
        assert!(matches!(Shares::verify_fresh(&item), Err(Error::Conflict(_))));
    }

    #[tokio::test]
    async fn stop_hides_items_from_phones_but_keeps_them_staged() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.bin");
        std::fs::write(&p, b"x").unwrap();
        let s = Shares::new(Bus::new());
        s.add_paths(vec![p]).await.unwrap();
        wait_indexed(&s).await;
        let id = s.info().unwrap().items[0].id.clone();
        s.set_active(true).unwrap();
        assert!(s.phone_info().is_some());
        s.set_active(false).unwrap();
        assert!(s.phone_info().is_none());
        assert!(matches!(s.servable_item(&id), Err(Error::NotFound)));
        assert_eq!(s.info().unwrap().items.len(), 1);
        s.remove_item(&id).unwrap();
        assert!(s.info().is_none());
    }

    #[tokio::test]
    async fn zip_selection_filters_by_prefix_and_requires_index() {
        let d = tempfile::tempdir().unwrap();
        let folder = d.path().join("Docs");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.txt"), b"a").unwrap();
        std::fs::write(d.path().join("b.txt"), b"b").unwrap();
        let s = Shares::new(Bus::new());
        s.add_paths(vec![folder, d.path().join("b.txt")]).await.unwrap();
        s.set_active(true).unwrap();
        wait_indexed(&s).await;
        let all = s.items_for_zip(&ZipSel::All).unwrap();
        assert_eq!(all.len(), 2);
        let docs = s.items_for_zip(&ZipSel::Prefix("Docs".into())).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].name, "Docs/a.txt");
        assert!(matches!(s.items_for_zip(&ZipSel::Ids(vec!["nope".into()])), Err(Error::NotFound)));
    }
}
