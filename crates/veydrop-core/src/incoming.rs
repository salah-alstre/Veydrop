//! Phone -> PC transfers: offers, user decision, resumable chunked uploads and
//! verified finalisation.
//!
//! Life of an upload:
//!
//! 1. The phone posts an *offer* (names, sizes). Names and folder paths are
//!    sanitised here, before anything touches the file system.
//! 2. The user accepts or rejects on the PC (or the offer is auto-accepted when
//!    "ask before receiving" is off). Nothing is written before that.
//! 3. For every file an [`UploadEntry`] and a `Queued` transfer are created.
//!    The phone streams chunks with `put_chunk` into `<name>.<id>.veydrop-part`.
//!    A chunk only counts once it is fully written; the incremental SHA-256 is
//!    committed together with the byte offset, so a dropped connection can be
//!    resumed from the last committed byte.
//! 4. `complete` compares the SHA-256 the phone computed over the original file
//!    with the one computed here over the received bytes, fsyncs, and only then
//!    moves the file to its final, collision-free name. A mismatch deletes the
//!    partial file and the transfer is reported as failed.

use std::collections::HashMap;
use std::fmt::Display;
use std::io::SeekFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use parking_lot::Mutex;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::bus::Bus;
use crate::error::{Error, Result};
use crate::fsutil;
use crate::model::{
    now_ms, EngineEvent, Ms, OfferFile, OfferInfo, OfferState, TransferDirection, TransferInfo,
    TransferState,
};
use crate::partials::{Partials, PART_SUFFIX};
use crate::settings::SettingsStore;
use crate::transfers::Transfers;

const MAX_FILES_PER_OFFER: usize = 10_000;
const MAX_PENDING_OFFERS_PER_SESSION: usize = 5;
const MAX_CLIENT_ID_LEN: usize = 64;
const MAX_CHUNK_BYTES: u64 = 32 * 1024 * 1024;
const LOCK_WAIT: Duration = Duration::from_secs(5);
const OFFER_TTL: Duration = Duration::from_secs(10 * 60);
const RESOLVED_KEEP: Duration = Duration::from_secs(30 * 60);
const ENTRY_KEEP: Duration = Duration::from_secs(10 * 60);
const SPACE_MARGIN: u64 = 32 * 1024 * 1024;

// ---- public data returned to the HTTP layer -------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadRef {
    pub client_id: String,
    pub transfer_id: String,
}

/// What a phone is allowed to know about its offer (no PC paths).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneOffer {
    pub id: String,
    pub state: OfferState,
    pub error: Option<String>,
    pub uploads: Vec<UploadRef>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadStatus {
    pub transfer_id: String,
    /// Next byte the server expects.
    pub received: u64,
    pub size: u64,
    pub state: TransferState,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneUpload {
    pub transfer_id: String,
    pub client_id: String,
    pub offer_id: String,
    pub name: String,
    pub size: u64,
    pub transferred: u64,
    pub state: TransferState,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhoneSnapshot {
    pub offers: Vec<PhoneOffer>,
    pub uploads: Vec<PhoneUpload>,
}

// ---- internals --------------------------------------------------------------

struct StagedFile {
    client_id: String,
    name: String,
    dirs: Vec<String>,
    size: u64,
}

struct Offer {
    info: OfferInfo,
    files: Vec<StagedFile>,
    uploads: Vec<UploadRef>,
    deciding: bool,
    resolved_at: Option<Ms>,
}

struct UploadState {
    received: u64,
    hasher: Sha256,
    created: bool,
    done: bool,
}

struct UploadEntry {
    tid: String,
    offer_id: String,
    session_id: String,
    client_id: String,
    name: String,
    dir: PathBuf,
    part_path: PathBuf,
    size: u64,
    cancel: CancellationToken,
    /// Mirror of `UploadState::received` readable without the upload lock.
    committed: AtomicU64,
    state: tokio::sync::Mutex<UploadState>,
}

struct ChunkSlot<'a>(&'a AtomicUsize);

impl<'a> ChunkSlot<'a> {
    fn acquire(counter: &'a AtomicUsize, max: usize) -> Option<Self> {
        if counter.fetch_add(1, Ordering::SeqCst) >= max {
            counter.fetch_sub(1, Ordering::SeqCst);
            return None;
        }
        Some(Self(counter))
    }
}

impl Drop for ChunkSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Marks the transfer interrupted (and rolls its counter back) unless the
/// chunk committed. Also fires when the request future is dropped because the
/// phone disconnected.
struct InterruptGuard<'a> {
    transfers: &'a Transfers,
    tid: &'a str,
    committed: u64,
    armed: bool,
}

impl Drop for InterruptGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.transfers.interrupt(self.tid, self.committed);
        }
    }
}

struct ChunkErr {
    err: Error,
    /// A fatal error (disk problem) fails the whole transfer; otherwise the
    /// phone can resume.
    fatal: bool,
}

impl ChunkErr {
    fn soft(err: Error) -> Self {
        Self { err, fatal: false }
    }
    fn fatal(err: Error) -> Self {
        Self { err, fatal: true }
    }
}

fn map_io(e: std::io::Error) -> Error {
    let full = e.kind() == std::io::ErrorKind::StorageFull || matches!(e.raw_os_error(), Some(112 | 39 | 28));
    if full {
        Error::InsufficientSpace
    } else {
        Error::Io(e)
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn finalize_file(dir: &Path, part: &Path, name: &str, size: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let f = std::fs::OpenOptions::new().write(true).create(true).truncate(false).open(part)?;
    if f.metadata()?.len() != size {
        return Err(std::io::Error::other("received file has an unexpected size"));
    }
    f.sync_all()?;
    drop(f);
    let target = fsutil::reserve_unique_file(dir, name)?;
    if let Err(e) = std::fs::rename(part, &target) {
        let _ = std::fs::remove_file(&target);
        return Err(e);
    }
    Ok(target)
}

pub struct Incoming {
    transfers: Arc<Transfers>,
    bus: Bus,
    settings: Arc<SettingsStore>,
    partials: Arc<Partials>,
    offers: Mutex<HashMap<String, Offer>>,
    uploads: Mutex<HashMap<String, Arc<UploadEntry>>>,
    active_chunks: AtomicUsize,
}

impl Incoming {
    pub fn new(
        transfers: Arc<Transfers>,
        bus: Bus,
        settings: Arc<SettingsStore>,
        data_dir: &Path,
    ) -> Arc<Self> {
        Arc::new(Self {
            transfers,
            bus,
            settings,
            partials: Partials::load(data_dir),
            offers: Mutex::new(HashMap::new()),
            uploads: Mutex::new(HashMap::new()),
            active_chunks: AtomicUsize::new(0),
        })
    }

    // ---- offers -----------------------------------------------------------

    pub fn create_offer(&self, session_id: &str, device_name: &str, files: Vec<OfferFile>) -> Result<PhoneOffer> {
        if files.is_empty() || files.len() > MAX_FILES_PER_OFFER {
            return Err(Error::invalid("file count"));
        }
        let max_size = self.settings.get().max_file_size_bytes();
        let mut seen = std::collections::HashSet::new();
        let mut staged = Vec::with_capacity(files.len());
        let mut shown = Vec::with_capacity(files.len());
        let mut total: u64 = 0;
        for f in files {
            if f.client_id.is_empty() || f.client_id.len() > MAX_CLIENT_ID_LEN || !seen.insert(f.client_id.clone()) {
                return Err(Error::invalid("clientId"));
            }
            if max_size.is_some_and(|m| f.size > m) {
                return Err(Error::TooLarge);
            }
            let comps = f.rel_path.as_deref().map(fsutil::sanitize_rel_path).unwrap_or_default();
            let (name, dirs) = match comps.split_last() {
                Some((last, dirs)) => (last.clone(), dirs.to_vec()),
                None => (fsutil::sanitize_component(&f.name), Vec::new()),
            };
            let rel_path = (!dirs.is_empty()).then(|| format!("{}/{}", dirs.join("/"), name));
            total = total.saturating_add(f.size);
            shown.push(OfferFile { client_id: f.client_id.clone(), name: name.clone(), rel_path, size: f.size });
            staged.push(StagedFile { client_id: f.client_id, name, dirs, size: f.size });
        }
        let id = crate::transfers::new_id();
        let info = OfferInfo {
            id: id.clone(),
            session_id: session_id.to_string(),
            device_name: device_name.to_string(),
            files: shown,
            total_size: total,
            created_at: now_ms(),
            state: OfferState::Pending,
            error: None,
            dest_dir: None,
        };
        {
            let mut g = self.offers.lock();
            let pending = g
                .values()
                .filter(|o| o.info.session_id == session_id && o.info.state == OfferState::Pending)
                .count();
            if pending >= MAX_PENDING_OFFERS_PER_SESSION {
                return Err(Error::RateLimited);
            }
            g.insert(
                id.clone(),
                Offer { info: info.clone(), files: staged, uploads: Vec::new(), deciding: false, resolved_at: None },
            );
        }
        if self.settings.get().ask_before_receive {
            self.bus.emit(EngineEvent::Offer(info));
        } else if let Err(e) = self.decide(&id, true, None) {
            self.fail_offer(&id, &e);
        }
        self.phone_offer(session_id, &id)
    }

    fn fail_offer(&self, offer_id: &str, err: &Error) {
        let session = {
            let mut g = self.offers.lock();
            let Some(o) = g.get_mut(offer_id) else { return };
            if o.info.state != OfferState::Pending {
                return;
            }
            o.deciding = false;
            o.info.state = OfferState::Failed;
            o.info.error = Some(err.code().to_string());
            o.resolved_at = Some(now_ms());
            o.info.session_id.clone()
        };
        self.announce_offer(&session, offer_id);
    }

    fn announce_offer(&self, session_id: &str, offer_id: &str) {
        self.bus.emit(EngineEvent::OfferResolved { id: offer_id.to_string() });
        if let Ok(po) = self.phone_offer(session_id, offer_id) {
            self.bus.phone(Some(session_id), "offer", serde_json::to_value(po).unwrap_or_default());
        }
    }

    pub fn phone_offer(&self, session_id: &str, offer_id: &str) -> Result<PhoneOffer> {
        let g = self.offers.lock();
        let o = g.get(offer_id).filter(|o| o.info.session_id == session_id).ok_or(Error::NotFound)?;
        Ok(PhoneOffer {
            id: o.info.id.clone(),
            state: o.info.state,
            error: o.info.error.clone(),
            uploads: o.uploads.clone(),
        })
    }

    pub fn pending_offers(&self) -> Vec<OfferInfo> {
        let mut v: Vec<OfferInfo> = self
            .offers
            .lock()
            .values()
            .filter(|o| o.info.state == OfferState::Pending)
            .map(|o| o.info.clone())
            .collect();
        v.sort_by_key(|o| o.created_at);
        v
    }

    /// Accept (into `dest`, default: the configured download folder) or
    /// reject. A failed accept (e.g. not enough space) leaves the offer
    /// pending so the user can choose another folder.
    pub fn decide(&self, offer_id: &str, accept: bool, dest: Option<&Path>) -> Result<()> {
        let (session_id, device_name, staged) = {
            let mut g = self.offers.lock();
            let o = g.get_mut(offer_id).ok_or(Error::NotFound)?;
            if o.info.state != OfferState::Pending || o.deciding {
                return Err(Error::Conflict("offer already decided".into()));
            }
            if !accept {
                o.info.state = OfferState::Rejected;
                o.resolved_at = Some(now_ms());
                let s = o.info.session_id.clone();
                drop(g);
                self.announce_offer(&s, offer_id);
                return Ok(());
            }
            o.deciding = true;
            let staged: Vec<(String, String, Vec<String>, u64)> =
                o.files.iter().map(|f| (f.client_id.clone(), f.name.clone(), f.dirs.clone(), f.size)).collect();
            (o.info.session_id.clone(), o.info.device_name.clone(), staged)
        };
        let dest_dir = dest
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(self.settings.get().download_dir));
        let total: u64 = staged.iter().map(|s| s.3).sum();
        let prepared = self.prepare_destination(&dest_dir, &staged, total);
        let mapped = match prepared {
            Ok(m) => m,
            Err(e) => {
                if let Some(o) = self.offers.lock().get_mut(offer_id) {
                    o.deciding = false;
                }
                return Err(e);
            }
        };
        let mut refs = Vec::with_capacity(staged.len());
        for (client_id, name, dirs, size) in staged {
            let dir = match dirs.split_first() {
                Some((top, rest)) => {
                    let mut p = dest_dir.join(mapped.get(top).map(String::as_str).unwrap_or(top));
                    for r in rest {
                        p.push(r);
                    }
                    p
                }
                None => dest_dir.clone(),
            };
            let rel_path = (!dirs.is_empty()).then(|| format!("{}/{}", dirs.join("/"), name));
            let mut info = Transfers::blank(
                TransferDirection::Incoming,
                &name,
                rel_path,
                size,
                TransferState::Queued,
                Some(session_id.clone()),
                &device_name,
                Some(offer_id.to_string()),
            );
            let tid = info.id.clone();
            info.saved_path = None;
            let cancel = self.transfers.create(info);
            let tid8: String = tid.chars().take(8).collect();
            let part_path = dir.join(format!("{name}.{tid8}{PART_SUFFIX}"));
            let entry = Arc::new(UploadEntry {
                tid: tid.clone(),
                offer_id: offer_id.to_string(),
                session_id: session_id.clone(),
                client_id: client_id.clone(),
                name,
                dir,
                part_path,
                size,
                cancel,
                committed: AtomicU64::new(0),
                state: tokio::sync::Mutex::new(UploadState {
                    received: 0,
                    hasher: Sha256::new(),
                    created: false,
                    done: false,
                }),
            });
            self.uploads.lock().insert(tid.clone(), entry);
            refs.push(UploadRef { client_id, transfer_id: tid });
        }
        {
            let mut g = self.offers.lock();
            if let Some(o) = g.get_mut(offer_id) {
                o.deciding = false;
                o.info.state = OfferState::Accepted;
                o.info.dest_dir = Some(dest_dir.to_string_lossy().into_owned());
                o.uploads = refs;
                o.resolved_at = Some(now_ms());
            }
        }
        self.announce_offer(&session_id, offer_id);
        Ok(())
    }

    /// Creates the destination, proves it is writable, checks free space and
    /// reserves one fresh folder per top-level folder of the offer.
    fn prepare_destination(
        &self,
        dest: &Path,
        staged: &[(String, String, Vec<String>, u64)],
        total: u64,
    ) -> Result<HashMap<String, String>> {
        std::fs::create_dir_all(dest)?;
        let probe = dest.join(format!(".veydrop-probe-{}", crate::auth::random_token(6)));
        std::fs::File::create(&probe)?;
        let _ = std::fs::remove_file(&probe);
        if let Some(free) = fsutil::available_space(dest) {
            if free < total.saturating_add(SPACE_MARGIN) {
                return Err(Error::InsufficientSpace);
            }
        }
        let mut mapped: HashMap<String, String> = HashMap::new();
        let mut created: Vec<PathBuf> = Vec::new();
        for (_, _, dirs, _) in staged {
            let Some(top) = dirs.first() else { continue };
            if mapped.contains_key(top) {
                continue;
            }
            match fsutil::reserve_unique_dir(dest, top) {
                Ok(p) => {
                    let actual = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| top.clone());
                    mapped.insert(top.clone(), actual);
                    created.push(p);
                }
                Err(e) => {
                    for c in created {
                        let _ = std::fs::remove_dir(c);
                    }
                    return Err(Error::Io(e));
                }
            }
        }
        Ok(mapped)
    }

    // ---- uploads ----------------------------------------------------------

    fn entry(&self, session_id: &str, tid: &str) -> Result<Arc<UploadEntry>> {
        self.uploads.lock().get(tid).filter(|e| e.session_id == session_id).cloned().ok_or(Error::NotFound)
    }

    fn remove_part(&self, entry: &UploadEntry) {
        match std::fs::remove_file(&entry.part_path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!("could not remove partial file: {e}"),
        }
        self.partials.remove(&entry.tid);
    }

    fn fail(&self, entry: &UploadEntry, err: &Error) {
        self.transfers.finish(&entry.tid, TransferState::Failed, Some(err), None, None);
        entry.cancel.cancel();
        self.remove_part(entry);
    }

    pub fn upload_status(&self, session_id: &str, tid: &str) -> Result<UploadStatus> {
        let entry = self.entry(session_id, tid)?;
        let info = self.transfers.get(tid).ok_or(Error::NotFound)?;
        let received = if info.state == TransferState::Completed {
            entry.size
        } else {
            entry.committed.load(Ordering::SeqCst)
        };
        Ok(UploadStatus {
            transfer_id: tid.to_string(),
            received,
            size: entry.size,
            state: info.state,
            error: info.error,
        })
    }

    /// Appends one chunk at `offset`. Returns the new committed offset.
    pub async fn put_chunk<S, E>(&self, session_id: &str, tid: &str, offset: u64, mut body: S) -> Result<u64>
    where
        S: Stream<Item = std::result::Result<Bytes, E>> + Unpin,
        E: Display,
    {
        let entry = self.entry(session_id, tid)?;
        match self.transfers.get(tid).map(|i| i.state) {
            None => return Err(Error::NotFound),
            Some(TransferState::Paused) => return Err(Error::Conflict("paused".into())),
            Some(TransferState::Cancelled) => return Err(Error::Cancelled),
            Some(s) if s.is_terminal() || s == TransferState::Finalizing => {
                return Err(Error::Conflict("transfer finished".into()))
            }
            Some(_) => {}
        }
        let max_slots = (self.settings.get().max_parallel_uploads as usize * 2).max(2);
        let _slot = ChunkSlot::acquire(&self.active_chunks, max_slots).ok_or(Error::RateLimited)?;
        let mut st = tokio::time::timeout(LOCK_WAIT, entry.state.lock())
            .await
            .map_err(|_| Error::Conflict("upload busy".into()))?;
        if entry.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if st.done {
            return Err(Error::Conflict("transfer finished".into()));
        }
        if offset != st.received {
            return Err(Error::OffsetMismatch(st.received));
        }
        let mut guard = InterruptGuard { transfers: &self.transfers, tid, committed: st.received, armed: true };
        self.transfers.set_state(tid, TransferState::Active);

        let result = self.write_body(&entry, &mut st, &mut body).await;
        if entry.cancel.is_cancelled() {
            // Cancelled while a chunk was in flight: make sure nothing lingers.
            guard.armed = false;
            self.remove_part(&entry);
            return Err(Error::Cancelled);
        }
        match result {
            Ok(new_received) => {
                guard.armed = false;
                entry.committed.store(new_received, Ordering::SeqCst);
                self.transfers.set_progress(tid, new_received);
                Ok(new_received)
            }
            Err(ce) if ce.fatal => {
                guard.armed = false;
                drop(st);
                self.fail(&entry, &ce.err);
                Err(ce.err)
            }
            Err(ce) => Err(ce.err),
        }
    }

    async fn write_body<S, E>(
        &self,
        entry: &UploadEntry,
        st: &mut UploadState,
        body: &mut S,
    ) -> std::result::Result<u64, ChunkErr>
    where
        S: Stream<Item = std::result::Result<Bytes, E>> + Unpin,
        E: Display,
    {
        if !st.created {
            tokio::fs::create_dir_all(&entry.dir).await.map_err(|e| ChunkErr::fatal(map_io(e)))?;
        }
        let mut f = tokio::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&entry.part_path)
            .await
            .map_err(|e| ChunkErr::fatal(map_io(e)))?;
        if !st.created {
            st.created = true;
            self.partials.add(&entry.tid, &entry.part_path);
        }
        let on_disk = f.metadata().await.map_err(|e| ChunkErr::fatal(map_io(e)))?.len();
        if on_disk < st.received {
            return Err(ChunkErr::fatal(Error::Io(std::io::Error::other("partial file went missing"))));
        }
        f.set_len(st.received).await.map_err(|e| ChunkErr::fatal(map_io(e)))?;
        f.seek(SeekFrom::Start(st.received)).await.map_err(|e| ChunkErr::fatal(map_io(e)))?;

        let mut hasher = st.hasher.clone();
        let mut n: u64 = 0;
        let outcome: std::result::Result<(), ChunkErr> = loop {
            let frame = tokio::select! {
                biased;
                _ = entry.cancel.cancelled() => break Err(ChunkErr::soft(Error::Cancelled)),
                frame = body.next() => frame,
            };
            match frame {
                None => break Ok(()),
                Some(Err(e)) => {
                    break Err(ChunkErr::soft(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionAborted,
                        e.to_string(),
                    ))))
                }
                Some(Ok(b)) => {
                    if b.is_empty() {
                        continue;
                    }
                    n += b.len() as u64;
                    if n > MAX_CHUNK_BYTES {
                        break Err(ChunkErr::soft(Error::invalid("chunk too large")));
                    }
                    if st.received + n > entry.size {
                        break Err(ChunkErr::soft(Error::invalid("data exceeds the announced file size")));
                    }
                    if let Err(e) = f.write_all(&b).await {
                        break Err(ChunkErr::fatal(map_io(e)));
                    }
                    hasher.update(&b);
                    self.transfers.set_progress(&entry.tid, st.received + n);
                }
            }
        };
        // Always flush: a later chunk truncates/seeks this file and must not
        // race with writes still queued on the blocking pool.
        let flushed = f.flush().await;
        outcome?;
        flushed.map_err(|e| ChunkErr::fatal(map_io(e)))?;
        st.received += n;
        st.hasher = hasher;
        Ok(st.received)
    }

    /// Verifies and publishes a fully received file. Idempotent.
    pub async fn complete(&self, session_id: &str, tid: &str, client_sha256: &str) -> Result<TransferInfo> {
        let entry = self.entry(session_id, tid)?;
        let info = self.transfers.get(tid).ok_or(Error::NotFound)?;
        match info.state {
            TransferState::Completed => return Ok(info),
            TransferState::Cancelled => return Err(Error::Cancelled),
            s if s.is_terminal() => return Err(Error::Conflict("transfer finished".into())),
            _ => {}
        }
        if !is_hex64(client_sha256) {
            return Err(Error::invalid("sha256"));
        }
        let st = tokio::time::timeout(LOCK_WAIT, entry.state.lock())
            .await
            .map_err(|_| Error::Conflict("upload busy".into()))?;
        // Another `complete` may have finished while we waited.
        if let Some(i) = self.transfers.get(tid) {
            if i.state == TransferState::Completed {
                return Ok(i);
            }
            if i.state.is_terminal() {
                return Err(Error::Conflict("transfer finished".into()));
            }
        }
        if entry.cancel.is_cancelled() {
            return Err(Error::Cancelled);
        }
        if st.received != entry.size {
            return Err(Error::OffsetMismatch(st.received));
        }
        self.transfers.set_state(tid, TransferState::Finalizing);
        let digest = hex::encode(st.hasher.clone().finalize());
        if !digest.eq_ignore_ascii_case(client_sha256) {
            drop(st);
            self.fail(&entry, &Error::HashMismatch);
            return Err(Error::HashMismatch);
        }
        let (dir, part, name, size) = (entry.dir.clone(), entry.part_path.clone(), entry.name.clone(), entry.size);
        let moved = tokio::task::spawn_blocking(move || finalize_file(&dir, &part, &name, size))
            .await
            .map_err(|e| Error::Other(e.to_string()))
            .and_then(|r| r.map_err(map_io));
        let mut st = st;
        match moved {
            Ok(path) => {
                st.done = true;
                self.partials.remove(tid);
                let saved = path.to_string_lossy().into_owned();
                self.transfers
                    .finish(tid, TransferState::Completed, None, Some(saved), Some(digest))
                    .or_else(|| self.transfers.get(tid))
                    .ok_or(Error::NotFound)
            }
            Err(e) => {
                drop(st);
                self.fail(&entry, &e);
                Err(e)
            }
        }
    }

    /// Pause / resume an upload. Paused uploads reject chunks until resumed.
    pub fn set_paused(&self, session_id: Option<&str>, tid: &str, paused: bool) -> Result<TransferInfo> {
        let entry = match session_id {
            Some(s) => self.entry(s, tid)?,
            None => self.uploads.lock().get(tid).cloned().ok_or(Error::NotFound)?,
        };
        let info = self.transfers.get(tid).ok_or(Error::NotFound)?;
        if info.state.is_terminal() || info.state == TransferState::Finalizing {
            return Err(Error::Conflict("transfer finished".into()));
        }
        self.transfers.set_state(tid, if paused { TransferState::Paused } else { TransferState::Queued });
        let now = self.transfers.get(tid).ok_or(Error::NotFound)?;
        self.bus.phone(
            Some(&entry.session_id),
            "transfer",
            serde_json::json!({ "transferId": tid, "state": now.state }),
        );
        Ok(now)
    }

    /// Cancels an upload (from the phone with `Some(session)`, or the PC with
    /// `None`) and deletes its partial file immediately.
    pub fn cancel(&self, session_id: Option<&str>, tid: &str) -> Result<()> {
        let entry = match session_id {
            Some(s) => self.entry(s, tid)?,
            None => self.uploads.lock().get(tid).cloned().ok_or(Error::NotFound)?,
        };
        self.transfers.cancel(tid);
        entry.cancel.cancel();
        self.remove_part(&entry);
        self.bus.phone(Some(&entry.session_id), "cancelled", serde_json::json!({ "transferId": tid }));
        Ok(())
    }

    pub fn session_snapshot(&self, session_id: &str) -> PhoneSnapshot {
        let mut offers: Vec<(Ms, PhoneOffer)> = self
            .offers
            .lock()
            .values()
            .filter(|o| o.info.session_id == session_id)
            .map(|o| {
                (
                    o.info.created_at,
                    PhoneOffer {
                        id: o.info.id.clone(),
                        state: o.info.state,
                        error: o.info.error.clone(),
                        uploads: o.uploads.clone(),
                    },
                )
            })
            .collect();
        offers.sort_by_key(|o| o.0);
        let entries: Vec<Arc<UploadEntry>> =
            self.uploads.lock().values().filter(|e| e.session_id == session_id).cloned().collect();
        let uploads = entries
            .into_iter()
            .filter_map(|e| {
                let i = self.transfers.get(&e.tid)?;
                Some(PhoneUpload {
                    transfer_id: e.tid.clone(),
                    client_id: e.client_id.clone(),
                    offer_id: e.offer_id.clone(),
                    name: e.name.clone(),
                    size: e.size,
                    transferred: if i.state == TransferState::Completed {
                        e.size
                    } else {
                        e.committed.load(Ordering::SeqCst)
                    },
                    state: i.state,
                    error: i.error,
                })
            })
            .collect();
        PhoneSnapshot { offers: offers.into_iter().map(|o| o.1).collect(), uploads }
    }

    // ---- lifecycle --------------------------------------------------------

    /// Session revoked/expired: reject its offers, cancel its uploads, delete
    /// their partial files.
    pub fn on_session_gone(&self, session_id: &str) {
        let pending: Vec<String> = self
            .offers
            .lock()
            .values()
            .filter(|o| o.info.session_id == session_id && o.info.state == OfferState::Pending)
            .map(|o| o.info.id.clone())
            .collect();
        for id in pending {
            if let Some(o) = self.offers.lock().get_mut(&id) {
                o.info.state = OfferState::Rejected;
                o.resolved_at = Some(now_ms());
            }
            self.bus.emit(EngineEvent::OfferResolved { id });
        }
        let entries: Vec<Arc<UploadEntry>> =
            self.uploads.lock().values().filter(|e| e.session_id == session_id).cloned().collect();
        for e in entries {
            self.discard_if_unfinished(&e);
        }
    }

    /// Server stopping: nothing can be resumed afterwards.
    pub fn on_server_stop(&self) {
        let ids: Vec<String> = self
            .offers
            .lock()
            .values()
            .filter(|o| o.info.state == OfferState::Pending)
            .map(|o| o.info.id.clone())
            .collect();
        for id in ids {
            if let Some(o) = self.offers.lock().get_mut(&id) {
                o.info.state = OfferState::Rejected;
                o.resolved_at = Some(now_ms());
            }
            self.bus.emit(EngineEvent::OfferResolved { id });
        }
        let entries: Vec<Arc<UploadEntry>> = self.uploads.lock().values().cloned().collect();
        for e in entries {
            self.discard_if_unfinished(&e);
        }
        self.partials.flush();
    }

    fn discard_if_unfinished(&self, e: &UploadEntry) {
        match self.transfers.get(&e.tid) {
            Some(i) if !i.state.is_terminal() => {
                self.transfers.cancel(&e.tid);
            }
            _ => {}
        }
        if self.transfers.get(&e.tid).is_none_or(|i| i.state != TransferState::Completed) {
            e.cancel.cancel();
            self.remove_part(e);
        }
    }

    /// Periodic housekeeping (called by the engine's janitor while serving).
    pub fn tick(&self) {
        let now = now_ms();
        // Offers nobody answered.
        let expired: Vec<(String, String)> = self
            .offers
            .lock()
            .values()
            .filter(|o| {
                o.info.state == OfferState::Pending
                    && !o.deciding
                    && now.saturating_sub(o.info.created_at) > OFFER_TTL.as_millis() as u64
            })
            .map(|o| (o.info.id.clone(), o.info.session_id.clone()))
            .collect();
        for (id, session) in expired {
            self.fail_offer(&id, &Error::PairingExpired);
            if let Some(o) = self.offers.lock().get_mut(&id) {
                o.info.error = Some("expired".into());
            }
            let _ = session;
        }
        self.offers.lock().retain(|_, o| match o.resolved_at {
            Some(t) => now.saturating_sub(t) < RESOLVED_KEEP.as_millis() as u64,
            None => true,
        });
        // Upload entries whose transfers finished long ago.
        let keep_ms = ENTRY_KEEP.as_millis() as u64;
        self.uploads.lock().retain(|tid, _| match self.transfers.get(tid) {
            None => false,
            Some(i) => match (i.state.is_terminal(), i.finished_at) {
                (true, Some(t)) => now.saturating_sub(t) < keep_ms,
                _ => true,
            },
        });
        self.sweep_partials();
    }

    /// Removes partial files older than the retention period (also run once at
    /// start-up to clean up after a crash). Active transfers are never touched.
    pub fn sweep_partials(&self) {
        let hours = self.settings.get().partial_retention_hours.max(1) as u64;
        for (tid, path) in self.partials.older_than(Duration::from_secs(hours * 3600)) {
            let entry = self.uploads.lock().get(&tid).cloned();
            match entry {
                None => self.partials.purge(&tid, &path),
                Some(e) => {
                    let state = self.transfers.get(&tid).map(|i| i.state);
                    if matches!(state, None | Some(TransferState::Interrupted | TransferState::Queued)) {
                        self.discard_if_unfinished(&e);
                    } else if state.is_some_and(|s| s.is_terminal()) {
                        self.partials.purge(&tid, &path);
                    }
                }
            }
        }
    }

    pub fn flush(&self) {
        self.partials.flush();
    }

    pub fn partial_count(&self) -> usize {
        self.partials.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HistoryStore;
    use futures_util::stream;

    struct Fx {
        inc: Arc<Incoming>,
        transfers: Arc<Transfers>,
        dest: PathBuf,
        _data: tempfile::TempDir,
        _dest: tempfile::TempDir,
    }

    fn fx() -> Fx {
        let data = tempfile::tempdir().unwrap();
        let dest_dir = tempfile::tempdir().unwrap();
        let bus = Bus::new();
        let settings = Arc::new(SettingsStore::load(data.path()));
        settings
            .patch(serde_json::json!({ "downloadDir": dest_dir.path().to_string_lossy() }))
            .unwrap();
        let transfers = Transfers::new(bus.clone(), HistoryStore::load(data.path()));
        let inc = Incoming::new(transfers.clone(), bus, settings, data.path());
        Fx { inc, transfers, dest: dest_dir.path().to_path_buf(), _data: data, _dest: dest_dir }
    }

    fn sha(data: &[u8]) -> String {
        hex::encode(Sha256::digest(data))
    }

    fn file(id: &str, name: &str, rel: Option<&str>, size: u64) -> OfferFile {
        OfferFile { client_id: id.into(), name: name.into(), rel_path: rel.map(String::from), size }
    }

    fn chunk(data: &[u8]) -> impl Stream<Item = std::result::Result<Bytes, std::io::Error>> + Unpin {
        stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(data))])
    }

    fn accepted(fx: &Fx, files: Vec<OfferFile>) -> PhoneOffer {
        let o = fx.inc.create_offer("s1", "Phone", files).unwrap();
        fx.inc.decide(&o.id, true, None).unwrap();
        fx.inc.phone_offer("s1", &o.id).unwrap()
    }

    #[tokio::test]
    async fn offer_waits_for_user_and_writes_nothing_before() {
        let fx = fx();
        let o = fx.inc.create_offer("s1", "Phone", vec![file("a", "a.txt", None, 3)]).unwrap();
        assert_eq!(o.state, OfferState::Pending);
        assert!(o.uploads.is_empty());
        assert_eq!(fx.inc.pending_offers().len(), 1);
        assert_eq!(std::fs::read_dir(&fx.dest).unwrap().count(), 0);
        assert!(matches!(
            fx.inc.put_chunk("s1", "nope", 0, chunk(b"abc")).await,
            Err(Error::NotFound)
        ));
    }

    #[tokio::test]
    async fn upload_roundtrip_verifies_and_places_file() {
        let fx = fx();
        let data = b"hello veydrop".to_vec();
        let po = accepted(&fx, vec![file("a", "hello.txt", None, data.len() as u64)]);
        let tid = po.uploads[0].transfer_id.clone();
        assert_eq!(fx.inc.put_chunk("s1", &tid, 0, chunk(&data[..5])).await.unwrap(), 5);
        assert_eq!(fx.inc.upload_status("s1", &tid).unwrap().received, 5);
        assert_eq!(fx.inc.put_chunk("s1", &tid, 5, chunk(&data[5..])).await.unwrap(), data.len() as u64);
        // part file exists while incomplete / unfinalised, final file does not
        assert!(!fx.dest.join("hello.txt").exists());
        let info = fx.inc.complete("s1", &tid, &sha(&data)).await.unwrap();
        assert_eq!(info.state, TransferState::Completed);
        assert_eq!(std::fs::read(fx.dest.join("hello.txt")).unwrap(), data);
        assert!(std::fs::read_dir(&fx.dest).unwrap().all(|e| !e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(PART_SUFFIX)));
        // idempotent
        assert_eq!(fx.inc.complete("s1", &tid, &sha(&data)).await.unwrap().state, TransferState::Completed);
        assert_eq!(fx.transfers.stats().received_files, 1);
    }

    #[tokio::test]
    async fn wrong_offset_reports_resume_point() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "a.bin", None, 10)]);
        let tid = &po.uploads[0].transfer_id;
        fx.inc.put_chunk("s1", tid, 0, chunk(b"12345")).await.unwrap();
        match fx.inc.put_chunk("s1", tid, 7, chunk(b"x")).await {
            Err(Error::OffsetMismatch(5)) => {}
            other => panic!("unexpected {other:?}"),
        }
        match fx.inc.put_chunk("s1", tid, 0, chunk(b"x")).await {
            Err(Error::OffsetMismatch(5)) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn hash_mismatch_fails_and_deletes_partial() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "a.bin", None, 4)]);
        let tid = po.uploads[0].transfer_id.clone();
        fx.inc.put_chunk("s1", &tid, 0, chunk(b"abcd")).await.unwrap();
        let err = fx.inc.complete("s1", &tid, &sha(b"different")).await.unwrap_err();
        assert!(matches!(err, Error::HashMismatch));
        let info = fx.transfers.get(&tid).unwrap();
        assert_eq!(info.state, TransferState::Failed);
        assert_eq!(info.error.as_deref(), Some("hash_mismatch"));
        assert_eq!(std::fs::read_dir(&fx.dest).unwrap().count(), 0, "nothing left behind");
        assert_eq!(fx.transfers.stats().received_files, 0);
    }

    #[tokio::test]
    async fn incomplete_upload_cannot_be_completed() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "a.bin", None, 10)]);
        let tid = &po.uploads[0].transfer_id;
        fx.inc.put_chunk("s1", tid, 0, chunk(b"abc")).await.unwrap();
        assert!(matches!(
            fx.inc.complete("s1", tid, &sha(b"abc")).await,
            Err(Error::OffsetMismatch(3))
        ));
    }

    #[tokio::test]
    async fn oversized_data_is_refused() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "a.bin", None, 4)]);
        let tid = &po.uploads[0].transfer_id;
        assert!(matches!(fx.inc.put_chunk("s1", tid, 0, chunk(b"too long")).await, Err(Error::Invalid(_))));
        // still resumable from 0
        assert_eq!(fx.inc.upload_status("s1", tid).unwrap().received, 0);
        assert_eq!(fx.inc.put_chunk("s1", tid, 0, chunk(b"fine")).await.unwrap(), 4);
    }

    #[tokio::test]
    async fn interrupted_chunk_is_rolled_back_and_resumable() {
        let fx = fx();
        let data: Vec<u8> = (0..100u8).collect();
        let po = accepted(&fx, vec![file("a", "r.bin", None, 100)]);
        let tid = po.uploads[0].transfer_id.clone();
        fx.inc.put_chunk("s1", &tid, 0, chunk(&data[..40])).await.unwrap();
        // connection drops after 20 more bytes of the next chunk
        let broken = stream::iter(vec![
            Ok::<Bytes, std::io::Error>(Bytes::copy_from_slice(&data[40..60])),
            Err(std::io::Error::other("reset")),
        ]);
        assert!(fx.inc.put_chunk("s1", &tid, 40, broken).await.is_err());
        let info = fx.transfers.get(&tid).unwrap();
        assert_eq!(info.state, TransferState::Interrupted);
        assert_eq!(info.transferred, 40);
        let st = fx.inc.upload_status("s1", &tid).unwrap();
        assert_eq!(st.received, 40);
        fx.inc.put_chunk("s1", &tid, 40, chunk(&data[40..])).await.unwrap();
        fx.inc.complete("s1", &tid, &sha(&data)).await.unwrap();
        assert_eq!(std::fs::read(fx.dest.join("r.bin")).unwrap(), data);
    }

    #[tokio::test]
    async fn cancel_deletes_partial_file() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "c.bin", None, 10)]);
        let tid = po.uploads[0].transfer_id.clone();
        fx.inc.put_chunk("s1", &tid, 0, chunk(b"12345")).await.unwrap();
        assert_eq!(std::fs::read_dir(&fx.dest).unwrap().count(), 1);
        fx.inc.cancel(Some("s1"), &tid).unwrap();
        assert_eq!(fx.transfers.get(&tid).unwrap().state, TransferState::Cancelled);
        assert_eq!(std::fs::read_dir(&fx.dest).unwrap().count(), 0);
        assert!(matches!(fx.inc.put_chunk("s1", &tid, 5, chunk(b"x")).await, Err(Error::Cancelled)));
    }

    #[tokio::test]
    async fn pause_blocks_chunks_until_resumed() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "p.bin", None, 6)]);
        let tid = &po.uploads[0].transfer_id;
        fx.inc.put_chunk("s1", tid, 0, chunk(b"abc")).await.unwrap();
        fx.inc.set_paused(Some("s1"), tid, true).unwrap();
        assert!(matches!(fx.inc.put_chunk("s1", tid, 3, chunk(b"def")).await, Err(Error::Conflict(_))));
        fx.inc.set_paused(Some("s1"), tid, false).unwrap();
        assert_eq!(fx.inc.put_chunk("s1", tid, 3, chunk(b"def")).await.unwrap(), 6);
    }

    #[tokio::test]
    async fn rejected_offer_creates_nothing() {
        let fx = fx();
        let o = fx.inc.create_offer("s1", "Phone", vec![file("a", "a.txt", None, 3)]).unwrap();
        fx.inc.decide(&o.id, false, None).unwrap();
        let po = fx.inc.phone_offer("s1", &o.id).unwrap();
        assert_eq!(po.state, OfferState::Rejected);
        assert!(po.uploads.is_empty());
        assert!(fx.inc.decide(&o.id, true, None).is_err(), "decision is final");
        assert_eq!(std::fs::read_dir(&fx.dest).unwrap().count(), 0);
    }

    #[tokio::test]
    async fn other_sessions_cannot_touch_an_upload() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "a.bin", None, 3)]);
        let tid = &po.uploads[0].transfer_id;
        assert!(matches!(fx.inc.put_chunk("s2", tid, 0, chunk(b"abc")).await, Err(Error::NotFound)));
        assert!(matches!(fx.inc.upload_status("s2", tid), Err(Error::NotFound)));
        assert!(matches!(fx.inc.cancel(Some("s2"), tid), Err(Error::NotFound)));
        assert!(matches!(fx.inc.phone_offer("s2", &po.id), Err(Error::NotFound)));
    }

    #[tokio::test]
    async fn duplicate_names_and_folders_get_unique_names() {
        let fx = fx();
        std::fs::write(fx.dest.join("a.txt"), b"existing").unwrap();
        std::fs::create_dir(fx.dest.join("Pics")).unwrap();
        let po = accepted(
            &fx,
            vec![
                file("1", "a.txt", None, 1),
                file("2", "x.jpg", Some("Pics/sub/x.jpg"), 1),
                file("3", "y.jpg", Some("Pics/y.jpg"), 1),
            ],
        );
        for (i, r) in po.uploads.iter().enumerate() {
            fx.inc.put_chunk("s1", &r.transfer_id, 0, chunk(b"z")).await.unwrap();
            let info = fx.inc.complete("s1", &r.transfer_id, &sha(b"z")).await.unwrap();
            assert_eq!(info.state, TransferState::Completed, "file {i}");
        }
        assert_eq!(std::fs::read(fx.dest.join("a.txt")).unwrap(), b"existing");
        assert_eq!(std::fs::read(fx.dest.join("a (1).txt")).unwrap(), b"z");
        assert_eq!(std::fs::read(fx.dest.join("Pics (1)").join("sub").join("x.jpg")).unwrap(), b"z");
        assert_eq!(std::fs::read(fx.dest.join("Pics (1)").join("y.jpg")).unwrap(), b"z");
        assert!(fx.dest.join("Pics").read_dir().unwrap().next().is_none(), "existing folder untouched");
    }

    #[tokio::test]
    async fn traversal_in_names_cannot_escape_destination() {
        let fx = fx();
        let po = accepted(
            &fx,
            vec![
                file("1", "..\\..\\evil.exe", None, 1),
                file("2", "z", Some("../../outside/../z.txt"), 1),
            ],
        );
        for r in &po.uploads {
            fx.inc.put_chunk("s1", &r.transfer_id, 0, chunk(b"q")).await.unwrap();
            fx.inc.complete("s1", &r.transfer_id, &sha(b"q")).await.unwrap();
        }
        let parent = fx.dest.parent().unwrap();
        assert!(!parent.join("evil.exe").exists());
        assert!(!parent.join("z.txt").exists());
        for t in fx.transfers.list() {
            let saved = PathBuf::from(t.saved_path.unwrap());
            assert!(saved.starts_with(&fx.dest), "{saved:?} escaped");
        }
    }

    #[tokio::test]
    async fn zero_byte_files_complete() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "empty.txt", None, 0)]);
        let tid = &po.uploads[0].transfer_id;
        let info = fx.inc.complete("s1", tid, &sha(b"")).await.unwrap();
        assert_eq!(info.state, TransferState::Completed);
        assert_eq!(std::fs::read(fx.dest.join("empty.txt")).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn invalid_offers_are_refused() {
        let fx = fx();
        assert!(fx.inc.create_offer("s1", "P", vec![]).is_err());
        assert!(fx.inc.create_offer("s1", "P", vec![file("", "a", None, 1)]).is_err());
        assert!(fx.inc.create_offer("s1", "P", vec![file("a", "a", None, 1), file("a", "b", None, 1)]).is_err());
        for _ in 0..MAX_PENDING_OFFERS_PER_SESSION {
            fx.inc.create_offer("s1", "P", vec![file("a", "a", None, 1)]).unwrap();
        }
        assert!(matches!(
            fx.inc.create_offer("s1", "P", vec![file("a", "a", None, 1)]),
            Err(Error::RateLimited)
        ));
    }

    #[tokio::test]
    async fn session_gone_cleans_up_everything() {
        let fx = fx();
        let po = accepted(&fx, vec![file("a", "g.bin", None, 10)]);
        let tid = po.uploads[0].transfer_id.clone();
        fx.inc.put_chunk("s1", &tid, 0, chunk(b"12345")).await.unwrap();
        let pending = fx.inc.create_offer("s1", "P", vec![file("b", "b", None, 1)]).unwrap();
        fx.inc.on_session_gone("s1");
        assert_eq!(fx.transfers.get(&tid).unwrap().state, TransferState::Cancelled);
        assert_eq!(fx.inc.phone_offer("s1", &pending.id).unwrap().state, OfferState::Rejected);
        assert_eq!(std::fs::read_dir(&fx.dest).unwrap().count(), 0);
        assert_eq!(fx.inc.partial_count(), 0);
    }
}
