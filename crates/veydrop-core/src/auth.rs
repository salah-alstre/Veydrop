//! Pairing and session authentication.
//!
//! * A **pairing token** is 16 random bytes, shown only inside the QR URL
//!   fragment, stored here only as a SHA-256 hash, valid once, and short-lived.
//! * Presenting it creates a **pending pair request** that a human must approve
//!   on the PC (a 4-digit code is shown on both screens).
//! * Approval mints a **session token** (32 random bytes) delivered to the phone
//!   once as an HttpOnly cookie; only its hash is kept.
//! * Pairing endpoints are rate limited per remote IP.
//!
//! Nothing in this module logs or serialises a secret.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use parking_lot::Mutex;
use rand::RngCore;
use sha2::{Digest, Sha256};
use tokio::sync::Notify;

use crate::error::{Error, Result};
use crate::model::{now_ms, Ms, PairRequestInfo, SessionInfo};

pub const MAX_PENDING: usize = 3;
pub const MAX_SESSIONS: usize = 8;
/// How long the PC user has to approve a request.
pub const APPROVAL_MS: u64 = 90_000;
/// How long an already-decided request stays claimable (lost-response recovery).
const DECIDED_KEEP_MS: u64 = 120_000;
/// Total bad token guesses after which the current QR token is destroyed.
const MAX_BAD_TOKEN_ATTEMPTS: u32 = 10;

const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_MAX_REQUESTS: u32 = 40;
const FAIL_WINDOW: Duration = Duration::from_secs(120);
const FAIL_MAX: u32 = 5;
const LOCKOUT: Duration = Duration::from_secs(120);

/// `n` bytes from the OS-seeded CSPRNG, base64url (no padding).
pub fn random_token(n: usize) -> String {
    let mut buf = vec![0u8; n];
    rand::rng().fill_bytes(&mut buf);
    URL_SAFE_NO_PAD.encode(buf)
}

pub fn sha256_hex(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}

fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn clean_label(raw: &str, platform: &str) -> String {
    let s: String = raw.chars().filter(|c| !c.is_control()).collect();
    let s: String = s.trim().chars().take(40).collect();
    if s.is_empty() {
        match platform {
            "ios" => "iPhone".into(),
            "android" => "Android phone".into(),
            _ => "Phone".into(),
        }
    } else {
        s
    }
}

fn clean_platform(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "ios" => "ios".into(),
        "android" => "android".into(),
        "desktop" => "desktop".into(),
        _ => "other".into(),
    }
}

fn valid_device_id(id: &str) -> bool {
    (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

struct PairingTok {
    hash: String,
    created_at: Ms,
    expires_at: Ms,
    used: bool,
    bad_attempts: u32,
    expiry_reported: bool,
}

enum Outcome {
    Waiting,
    Approved { token: String, session_id: String },
    Rejected,
    Expired,
}

struct Pending {
    info: PairRequestInfo,
    claim_hash: String,
    device_id: String,
    outcome: Outcome,
    /// When the entry can be dropped.
    purge_at: Ms,
}

struct Session {
    info: SessionInfo,
    token_hash: String,
    device_id: String,
    online: usize,
}

#[derive(Default)]
struct Bucket {
    window_start: Option<Instant>,
    count: u32,
    fail_start: Option<Instant>,
    fails: u32,
    locked_until: Option<Instant>,
    last_used: Option<Instant>,
}

#[derive(Default)]
struct State {
    pairing: Option<PairingTok>,
    pending: HashMap<String, Pending>,
    sessions: HashMap<String, Session>,
    by_token: HashMap<String, String>,
    limiter: HashMap<IpAddr, Bucket>,
}

pub struct PairArgs<'a> {
    pub token: &'a str,
    pub device_id: &'a str,
    pub label: &'a str,
    pub platform: &'a str,
}

pub struct PairStart {
    pub info: PairRequestInfo,
    /// Secret the phone uses to poll for the decision. Never shown on the PC.
    pub claim: String,
}

pub enum ClaimStatus {
    Pending,
    Approved { token: String, session: SessionInfo },
    Rejected,
    Expired,
}

pub struct Decision {
    pub session: Option<SessionInfo>,
    pub device_id: String,
    pub device_label: String,
    /// Older sessions of the same device that were replaced.
    pub replaced: Vec<String>,
}

#[derive(Default, Debug)]
pub struct TickReport {
    pub expired_sessions: Vec<String>,
    pub expired_pending: Vec<String>,
    pub pairing_expired: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PairingMeta {
    pub created_at: Ms,
    pub expires_at: Ms,
    pub used: bool,
}

pub struct Auth {
    state: Mutex<State>,
    /// Pulsed whenever a pending request is decided (wakes long-polling phones).
    pub changed: Notify,
}

impl Default for Auth {
    fn default() -> Self {
        Self::new()
    }
}

impl Auth {
    pub fn new() -> Self {
        Auth { state: Mutex::new(State::default()), changed: Notify::new() }
    }

    // ---- pairing token ---------------------------------------------------

    /// Replaces any existing token. Returns the plaintext (for the QR URL).
    pub fn new_pairing(&self, ttl: Duration) -> String {
        let token = random_token(16);
        let now = now_ms();
        self.state.lock().pairing = Some(PairingTok {
            hash: sha256_hex(&token),
            created_at: now,
            expires_at: now + ttl.as_millis() as u64,
            used: false,
            bad_attempts: 0,
            expiry_reported: false,
        });
        token
    }

    pub fn clear_pairing(&self) {
        self.state.lock().pairing = None;
    }

    pub fn pairing_meta(&self) -> Option<PairingMeta> {
        self.state
            .lock()
            .pairing
            .as_ref()
            .map(|p| PairingMeta { created_at: p.created_at, expires_at: p.expires_at, used: p.used })
    }

    // ---- rate limiting ---------------------------------------------------

    /// Counts one request from `ip`; errors when locked out or over budget.
    pub fn check_rate(&self, ip: IpAddr) -> Result<()> {
        let mut st = self.state.lock();
        Self::check_rate_locked(&mut st, ip)
    }

    fn check_rate_locked(st: &mut State, ip: IpAddr) -> Result<()> {
        let now = Instant::now();
        let b = st.limiter.entry(ip).or_default();
        b.last_used = Some(now);
        if let Some(until) = b.locked_until {
            if now < until {
                return Err(Error::RateLimited);
            }
            b.locked_until = None;
            b.fails = 0;
            b.fail_start = None;
        }
        match b.window_start {
            Some(s) if now.duration_since(s) < RATE_WINDOW => b.count += 1,
            _ => {
                b.window_start = Some(now);
                b.count = 1;
            }
        }
        if b.count > RATE_MAX_REQUESTS {
            return Err(Error::RateLimited);
        }
        Ok(())
    }

    /// Records a failed authentication attempt; repeated failures lock the IP out.
    pub fn record_fail(&self, ip: IpAddr) {
        let mut st = self.state.lock();
        Self::record_fail_locked(&mut st, ip);
    }

    fn record_fail_locked(st: &mut State, ip: IpAddr) {
        let now = Instant::now();
        let b = st.limiter.entry(ip).or_default();
        match b.fail_start {
            Some(s) if now.duration_since(s) < FAIL_WINDOW => b.fails += 1,
            _ => {
                b.fail_start = Some(now);
                b.fails = 1;
            }
        }
        if b.fails >= FAIL_MAX {
            b.locked_until = Some(now + LOCKOUT);
        }
    }

    // ---- pairing flow ----------------------------------------------------

    pub fn request_pair(&self, ip: IpAddr, args: PairArgs<'_>, trusted: bool) -> Result<PairStart> {
        let mut st = self.state.lock();
        Self::check_rate_locked(&mut st, ip)?;

        if args.token.len() > 64 || !valid_device_id(args.device_id) {
            return Err(Error::invalid("pairing request"));
        }
        let now = now_ms();

        let Some(p) = st.pairing.as_mut() else {
            return Err(Error::PairingExpired);
        };
        if !ct_eq(&p.hash, &sha256_hex(args.token)) {
            p.bad_attempts += 1;
            if p.bad_attempts >= MAX_BAD_TOKEN_ATTEMPTS {
                st.pairing = None;
            }
            Self::record_fail_locked(&mut st, ip);
            return Err(Error::Unauthorized);
        }
        if p.used {
            return Err(Error::PairingUsed);
        }
        if now >= p.expires_at {
            return Err(Error::PairingExpired);
        }

        let waiting = st.pending.values().filter(|x| matches!(x.outcome, Outcome::Waiting)).count();
        if waiting >= MAX_PENDING {
            return Err(Error::RateLimited);
        }
        let other_devices = st.sessions.values().filter(|s| s.device_id != args.device_id).count();
        if other_devices >= MAX_SESSIONS {
            return Err(Error::Conflict("too many connected devices".into()));
        }

        if let Some(p) = st.pairing.as_mut() {
            p.used = true; // one-time: consumed even if the PC user later rejects
        }

        let platform = clean_platform(args.platform);
        let id = random_token(9);
        let claim = random_token(16);
        let mut code_bytes = [0u8; 4];
        rand::rng().fill_bytes(&mut code_bytes);
        let code = format!("{:04}", u32::from_le_bytes(code_bytes) % 10_000);
        let info = PairRequestInfo {
            id: id.clone(),
            code,
            device_label: clean_label(args.label, &platform),
            platform,
            remote_ip: ip.to_string(),
            created_at: now,
            expires_at: now + APPROVAL_MS,
            trusted,
        };
        st.pending.insert(
            id,
            Pending {
                info: info.clone(),
                claim_hash: sha256_hex(&claim),
                device_id: args.device_id.to_string(),
                outcome: Outcome::Waiting,
                purge_at: now + APPROVAL_MS + DECIDED_KEEP_MS,
            },
        );
        Ok(PairStart { info, claim })
    }

    /// Approve or reject. `session_minutes == 0` means no time limit.
    pub fn decide(&self, id: &str, approve: bool, session_minutes: u32) -> Result<Decision> {
        let mut st = self.state.lock();
        let now = now_ms();
        let (device_id, label, platform, remote_ip, trusted) = {
            let p = st.pending.get_mut(id).ok_or(Error::NotFound)?;
            if !matches!(p.outcome, Outcome::Waiting) {
                return Err(Error::Conflict("already decided".into()));
            }
            if now >= p.info.expires_at {
                p.outcome = Outcome::Expired;
                drop(st);
                self.changed.notify_waiters();
                return Err(Error::PairingExpired);
            }
            (
                p.device_id.clone(),
                p.info.device_label.clone(),
                p.info.platform.clone(),
                p.info.remote_ip.clone(),
                p.info.trusted,
            )
        };

        if !approve {
            if let Some(p) = st.pending.get_mut(id) {
                p.outcome = Outcome::Rejected;
                p.purge_at = now + DECIDED_KEEP_MS;
            }
            drop(st);
            self.changed.notify_waiters();
            return Ok(Decision { session: None, device_id, device_label: label, replaced: vec![] });
        }

        // A device that pairs again replaces its previous session.
        let old: Vec<String> =
            st.sessions.iter().filter(|(_, s)| s.device_id == device_id).map(|(k, _)| k.clone()).collect();
        for k in &old {
            if let Some(s) = st.sessions.remove(k) {
                st.by_token.remove(&s.token_hash);
            }
        }

        let token = random_token(32);
        let session_id = random_token(9);
        let info = SessionInfo {
            id: session_id.clone(),
            device_label: label.clone(),
            platform,
            remote_ip,
            connected_at: now,
            last_seen: now,
            expires_at: (session_minutes > 0).then(|| now + session_minutes as u64 * 60_000),
            online: false,
            trusted,
        };
        let token_hash = sha256_hex(&token);
        st.by_token.insert(token_hash.clone(), session_id.clone());
        st.sessions.insert(
            session_id.clone(),
            Session { info: info.clone(), token_hash, device_id: device_id.clone(), online: 0 },
        );
        if let Some(p) = st.pending.get_mut(id) {
            p.outcome = Outcome::Approved { token, session_id };
            p.purge_at = now + DECIDED_KEEP_MS;
        }
        drop(st);
        self.changed.notify_waiters();
        Ok(Decision { session: Some(info), device_id, device_label: label, replaced: old })
    }

    /// Phone polls for the decision. Idempotent for `DECIDED_KEEP_MS`.
    pub fn claim(&self, ip: IpAddr, id: &str, claim_secret: &str) -> Result<ClaimStatus> {
        let mut st = self.state.lock();
        Self::check_rate_locked(&mut st, ip)?;
        let now = now_ms();
        let ok = match st.pending.get(id) {
            Some(p) => ct_eq(&p.claim_hash, &sha256_hex(claim_secret)),
            None => false,
        };
        if !ok {
            Self::record_fail_locked(&mut st, ip);
            return Err(Error::NotFound);
        }
        let session_lookup = |st: &State, sid: &str| st.sessions.get(sid).map(|s| s.info.clone());
        let p = st.pending.get_mut(id).ok_or(Error::NotFound)?;
        if matches!(p.outcome, Outcome::Waiting) && now >= p.info.expires_at {
            p.outcome = Outcome::Expired;
        }
        match &p.outcome {
            Outcome::Waiting => Ok(ClaimStatus::Pending),
            Outcome::Rejected => Ok(ClaimStatus::Rejected),
            Outcome::Expired => Ok(ClaimStatus::Expired),
            Outcome::Approved { token, session_id } => {
                let (token, sid) = (token.clone(), session_id.clone());
                match session_lookup(&st, &sid) {
                    Some(session) => Ok(ClaimStatus::Approved { token, session }),
                    // Approved but revoked/expired before the phone collected it.
                    None => Ok(ClaimStatus::Expired),
                }
            }
        }
    }

    // ---- sessions --------------------------------------------------------

    /// Resolves a session token to its session. Removes expired sessions.
    pub fn authenticate(&self, token: &str) -> Option<SessionInfo> {
        if token.is_empty() || token.len() > 128 {
            return None;
        }
        let hash = sha256_hex(token);
        let mut st = self.state.lock();
        let sid = st.by_token.get(&hash)?.clone();
        let now = now_ms();
        let expired = st.sessions.get(&sid).map(|s| s.info.expires_at.is_some_and(|e| now >= e)).unwrap_or(true);
        if expired {
            if let Some(s) = st.sessions.remove(&sid) {
                st.by_token.remove(&s.token_hash);
            }
            return None;
        }
        let s = st.sessions.get_mut(&sid)?;
        s.info.last_seen = now;
        Some(s.info.clone())
    }

    /// Adjusts the live-connection count. Returns the session when its
    /// online/offline state flipped.
    pub fn set_online(&self, id: &str, delta: i32) -> Option<SessionInfo> {
        let mut st = self.state.lock();
        let s = st.sessions.get_mut(id)?;
        let was = s.online > 0;
        s.online = if delta >= 0 { s.online + delta as usize } else { s.online.saturating_sub((-delta) as usize) };
        s.info.online = s.online > 0;
        s.info.last_seen = now_ms();
        (was != s.info.online).then(|| s.info.clone())
    }

    pub fn session(&self, id: &str) -> Option<SessionInfo> {
        self.state.lock().sessions.get(id).map(|s| s.info.clone())
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        let mut v: Vec<SessionInfo> = self.state.lock().sessions.values().map(|s| s.info.clone()).collect();
        v.sort_by_key(|s| s.connected_at);
        v
    }

    pub fn pending(&self) -> Vec<PairRequestInfo> {
        let mut v: Vec<PairRequestInfo> = self
            .state
            .lock()
            .pending
            .values()
            .filter(|p| matches!(p.outcome, Outcome::Waiting))
            .map(|p| p.info.clone())
            .collect();
        v.sort_by_key(|p| p.created_at);
        v
    }

    pub fn revoke(&self, id: &str) -> bool {
        let mut st = self.state.lock();
        match st.sessions.remove(id) {
            Some(s) => {
                st.by_token.remove(&s.token_hash);
                true
            }
            None => false,
        }
    }

    /// Drops every session and pending request and the QR token.
    pub fn revoke_all(&self) -> Vec<String> {
        let mut st = self.state.lock();
        let ids: Vec<String> = st.sessions.keys().cloned().collect();
        st.sessions.clear();
        st.by_token.clear();
        st.pending.clear();
        st.pairing = None;
        drop(st);
        self.changed.notify_waiters();
        ids
    }

    // ---- housekeeping ----------------------------------------------------

    pub fn tick(&self) -> TickReport {
        let mut st = self.state.lock();
        let now = now_ms();
        let mut rep = TickReport::default();

        let dead: Vec<String> = st
            .sessions
            .iter()
            .filter(|(_, s)| s.info.expires_at.is_some_and(|e| now >= e))
            .map(|(k, _)| k.clone())
            .collect();
        for k in dead {
            if let Some(s) = st.sessions.remove(&k) {
                st.by_token.remove(&s.token_hash);
            }
            rep.expired_sessions.push(k);
        }

        for (id, p) in st.pending.iter_mut() {
            if matches!(p.outcome, Outcome::Waiting) && now >= p.info.expires_at {
                p.outcome = Outcome::Expired;
                rep.expired_pending.push(id.clone());
            }
        }
        st.pending.retain(|_, p| now < p.purge_at);

        if let Some(p) = st.pairing.as_mut() {
            if !p.used && now >= p.expires_at && !p.expiry_reported {
                p.expiry_reported = true;
                rep.pairing_expired = true;
            }
        }

        let now_i = Instant::now();
        st.limiter.retain(|_, b| {
            b.locked_until.is_some_and(|u| now_i < u)
                || b.last_used.is_some_and(|t| now_i.duration_since(t) < Duration::from_secs(600))
        });
        drop(st);
        if !rep.expired_pending.is_empty() {
            self.changed.notify_waiters();
        }
        rep
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(n: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, n])
    }

    fn args<'a>(token: &'a str, dev: &'a str) -> PairArgs<'a> {
        PairArgs { token, device_id: dev, label: "Test phone", platform: "ios" }
    }

    #[test]
    fn tokens_are_random_and_long_enough() {
        let a = random_token(16);
        let b = random_token(16);
        assert_ne!(a, b);
        assert_eq!(a.len(), 22);
    }

    #[test]
    fn full_pairing_flow() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let start = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        assert_eq!(start.info.code.len(), 4);
        assert!(matches!(a.claim(ip(5), &start.info.id, &start.claim).unwrap(), ClaimStatus::Pending));
        assert_eq!(a.pending().len(), 1);

        let d = a.decide(&start.info.id, true, 30).unwrap();
        assert!(d.session.is_some());
        assert!(a.pending().is_empty());

        let ClaimStatus::Approved { token, session } = a.claim(ip(5), &start.info.id, &start.claim).unwrap() else {
            panic!("expected approval");
        };
        // idempotent for lost-response recovery
        assert!(matches!(a.claim(ip(5), &start.info.id, &start.claim).unwrap(), ClaimStatus::Approved { .. }));
        assert_eq!(a.authenticate(&token).unwrap().id, session.id);
        assert!(a.authenticate("not-a-token").is_none());
    }

    #[test]
    fn pairing_token_is_one_time() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        let again = a.request_pair(ip(6), args(&tok, "device-0002"), false);
        assert!(matches!(again, Err(Error::PairingUsed)));
    }

    #[test]
    fn pairing_token_expires() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_millis(30));
        std::thread::sleep(Duration::from_millis(60));
        assert!(matches!(a.request_pair(ip(5), args(&tok, "device-0001"), false), Err(Error::PairingExpired)));
        assert!(a.tick().pairing_expired);
        assert!(!a.tick().pairing_expired, "reported once");
    }

    #[test]
    fn wrong_token_is_rejected_and_eventually_burns_the_token() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        for i in 0..MAX_BAD_TOKEN_ATTEMPTS {
            // rotate IPs so the per-IP lockout does not interfere
            let r = a.request_pair(ip(10 + i as u8), args("wrong-token", "device-0001"), false);
            assert!(matches!(r, Err(Error::Unauthorized)));
        }
        let r = a.request_pair(ip(99), args(&tok, "device-0001"), false);
        assert!(matches!(r, Err(Error::PairingExpired)), "token destroyed after too many guesses");
    }

    #[test]
    fn rate_limiter_locks_out_repeated_failures() {
        let a = Auth::new();
        a.new_pairing(Duration::from_secs(60));
        for _ in 0..FAIL_MAX {
            let _ = a.request_pair(ip(7), args("bad", "device-0001"), false);
        }
        assert!(matches!(a.request_pair(ip(7), args("bad", "device-0001"), false), Err(Error::RateLimited)));
        // a different address is unaffected
        assert!(matches!(a.request_pair(ip(8), args("bad", "device-0001"), false), Err(Error::Unauthorized)));
    }

    #[test]
    fn rejection_and_revocation() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        a.decide(&s.info.id, false, 30).unwrap();
        assert!(matches!(a.claim(ip(5), &s.info.id, &s.claim).unwrap(), ClaimStatus::Rejected));
        assert!(a.decide(&s.info.id, true, 30).is_err(), "cannot approve after rejecting");

        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        let d = a.decide(&s.info.id, true, 30).unwrap();
        let ClaimStatus::Approved { token, .. } = a.claim(ip(5), &s.info.id, &s.claim).unwrap() else { panic!() };
        assert!(a.authenticate(&token).is_some());
        assert!(a.revoke(&d.session.unwrap().id));
        assert!(a.authenticate(&token).is_none());
    }

    #[test]
    fn claim_requires_the_claim_secret() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        a.decide(&s.info.id, true, 30).unwrap();
        assert!(a.claim(ip(6), &s.info.id, "guess").is_err());
    }

    #[test]
    fn sessions_expire_and_pairing_again_replaces_the_device_session() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        let first = a.decide(&s.info.id, true, 30).unwrap().session.unwrap();

        let tok = a.new_pairing(Duration::from_secs(60));
        let s2 = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        let d2 = a.decide(&s2.info.id, true, 30).unwrap();
        assert_eq!(d2.replaced, vec![first.id.clone()]);
        assert_eq!(a.sessions().len(), 1);

        // expiry: backdate through a 0-minute-limit session is "no limit"; use tick on a forced short one
        {
            let mut st = a.state.lock();
            let sid = st.sessions.keys().next().unwrap().clone();
            st.sessions.get_mut(&sid).unwrap().info.expires_at = Some(now_ms() - 1);
        }
        assert_eq!(a.tick().expired_sessions.len(), 1);
        assert!(a.sessions().is_empty());
    }

    #[test]
    fn pairing_codes_are_four_ascii_digits_with_leading_zeros_preserved() {
        let mut leading_zero = 0;
        for i in 0..3000u32 {
            let a = Auth::new();
            let tok = a.new_pairing(Duration::from_secs(60));
            let s = a.request_pair(ip(5), args(&tok, &format!("device-{i:05}")), false).unwrap();
            let c = &s.info.code;
            assert_eq!(c.len(), 4, "code {c:?}");
            assert!(c.bytes().all(|b| b.is_ascii_digit()), "code {c:?} must be ASCII 0-9 only");
            if c.starts_with('0') {
                leading_zero += 1;
            }
        }
        // ~10% of codes start with 0; if padding were lost these would be shorter and caught above.
        assert!(leading_zero > 0, "expected some zero-prefixed codes in 3000 samples");
    }

    #[test]
    fn concurrent_requests_are_independent_attempts() {
        let a = Auth::new();
        let tok1 = a.new_pairing(Duration::from_secs(60));
        let r1 = a.request_pair(ip(5), args(&tok1, "device-aaaa-1"), false).unwrap();
        let tok2 = a.new_pairing(Duration::from_secs(60));
        let r2 = a.request_pair(ip(6), args(&tok2, "device-bbbb-2"), false).unwrap();
        assert_ne!(r1.info.id, r2.info.id);
        assert_ne!(r1.claim, r2.claim);
        assert_eq!(a.pending().len(), 2);

        // Rejecting one attempt never approves (or even touches) the other.
        a.decide(&r1.info.id, false, 30).unwrap();
        assert!(a.sessions().is_empty());
        assert!(matches!(a.claim(ip(5), &r1.info.id, &r1.claim).unwrap(), ClaimStatus::Rejected));
        assert!(matches!(a.claim(ip(6), &r2.info.id, &r2.claim).unwrap(), ClaimStatus::Pending));

        // One attempt's secret cannot read another attempt's outcome.
        assert!(a.claim(ip(6), &r2.info.id, &r1.claim).is_err());
        assert!(a.claim(ip(5), &r1.info.id, &r2.claim).is_err());

        // Approving the other yields exactly one session, bound to that device.
        let d = a.decide(&r2.info.id, true, 30).unwrap();
        assert_eq!(d.device_id, "device-bbbb-2");
        assert_eq!(a.sessions().len(), 1);
        assert!(matches!(a.claim(ip(5), &r1.info.id, &r1.claim).unwrap(), ClaimStatus::Rejected));
        assert!(a.decide(&r1.info.id, true, 30).is_err(), "a rejected attempt can never be approved later");
    }

    #[test]
    fn an_expired_attempt_cannot_be_approved_or_claimed() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        a.state.lock().pending.get_mut(&s.info.id).unwrap().info.expires_at = now_ms() - 1;
        assert!(matches!(a.decide(&s.info.id, true, 30), Err(Error::PairingExpired)));
        assert!(a.sessions().is_empty());
        assert!(matches!(a.claim(ip(5), &s.info.id, &s.claim).unwrap(), ClaimStatus::Expired));
        assert!(a.decide(&s.info.id, true, 30).is_err(), "still no approval after expiry");
        assert!(a.sessions().is_empty());
    }

    #[test]
    fn online_flag_follows_connection_count() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        let sid = a.decide(&s.info.id, true, 0).unwrap().session.unwrap().id;
        assert!(a.set_online(&sid, 1).is_some());
        assert!(a.set_online(&sid, 1).is_none());
        assert!(a.set_online(&sid, -1).is_none());
        assert!(a.set_online(&sid, -1).is_some());
        assert!(!a.session(&sid).unwrap().online);
    }

    #[test]
    fn revoke_all_clears_everything() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        let s = a.request_pair(ip(5), args(&tok, "device-0001"), false).unwrap();
        a.decide(&s.info.id, true, 0).unwrap();
        assert_eq!(a.revoke_all().len(), 1);
        assert!(a.sessions().is_empty() && a.pairing_meta().is_none());
    }

    #[test]
    fn rejects_malformed_device_ids_and_sanitises_labels() {
        let a = Auth::new();
        let tok = a.new_pairing(Duration::from_secs(60));
        assert!(a.request_pair(ip(5), args(&tok, "x"), false).is_err());
        assert!(a.request_pair(ip(5), args(&tok, "bad id with spaces!"), false).is_err());
        let s = a
            .request_pair(
                ip(5),
                PairArgs { token: &tok, device_id: "device-0001", label: "\u{0007}  Eve\n", platform: "weird" },
                false,
            )
            .unwrap();
        assert_eq!(s.info.device_label, "Eve");
        assert_eq!(s.info.platform, "other");
    }
}
