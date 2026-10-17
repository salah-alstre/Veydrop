//! Real end-to-end tests: a real Engine, a real HTTP server on a real network
//! interface, and a plain HTTP client playing the phone. Nothing is mocked.
//!
//! These need a non-loopback IPv4 interface (any LAN, Wi-Fi or virtual
//! adapter). Without one the tests report that and pass trivially, which is
//! the only honest option on an offline CI box.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tempfile::TempDir;
use veydrop_core::engine::{Engine, EngineConfig};
use veydrop_core::model::{TransferInfo, TransferState};

struct Env {
    engine: Engine,
    base: String,
    host: String,
    dl: PathBuf,
    http: reqwest::Client,
    _dirs: Vec<TempDir>,
}

fn sha(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed).wrapping_add((i >> 8) as u8)).collect()
}

async fn env() -> Option<Env> {
    let data = tempfile::tempdir().unwrap();
    let mobile = tempfile::tempdir().unwrap();
    let dl = tempfile::tempdir().unwrap();
    std::fs::write(mobile.path().join("index.html"), "<!doctype html><title>vd</title>hello").unwrap();
    let engine = Engine::new(EngineConfig {
        data_dir: data.path().to_path_buf(),
        mobile_dir: mobile.path().to_path_buf(),
    })
    .await
    .unwrap();
    engine
        .update_settings(json!({ "downloadDir": dl.path().to_string_lossy(), "askBeforeReceive": true }))
        .unwrap();
    match engine.start_server().await {
        Ok(st) => {
            let host = format!("{}:{}", st.ip.unwrap(), st.port.unwrap());
            let mut h = reqwest::header::HeaderMap::new();
            h.insert("x-veydrop", "1".parse().unwrap());
            let http = reqwest::Client::builder().default_headers(h).timeout(Duration::from_secs(60)).build().unwrap();
            Some(Env {
                engine,
                base: format!("http://{host}"),
                host,
                dl: dl.path().to_path_buf(),
                http,
                _dirs: vec![data, mobile, dl],
            })
        }
        Err(e) => {
            eprintln!("SKIPPED: no usable network interface ({e})");
            None
        }
    }
}

impl Env {
    fn token(&self) -> String {
        let url = self.engine.pairing().expect("pairing").url;
        url.split("#t=").nth(1).unwrap().to_string()
    }

    async fn pair_request(&self, token: &str, device: &str) -> reqwest::Response {
        self.http
            .post(format!("{}/api/pair/request", self.base))
            .json(&json!({ "token": token, "deviceId": device, "label": "Test phone", "platform": "Android" }))
            .send()
            .await
            .unwrap()
    }

    /// Full pairing as a phone would do it. Returns the session cookie.
    async fn pair(&self, device: &str) -> String {
        let r = self.pair_request(&self.token(), device).await;
        assert_eq!(r.status(), 200, "pair request");
        let j: Value = r.json().await.unwrap();
        let (id, claim) = (j["id"].as_str().unwrap().to_string(), j["claim"].as_str().unwrap().to_string());
        let http = self.http.clone();
        let url = format!("{}/api/pair/claim", self.base);
        let (id2, claim2) = (id.clone(), claim.clone());
        let waiter = tokio::spawn(async move {
            http.post(url).json(&json!({ "id": id2, "claim": claim2 })).send().await.unwrap()
        });
        tokio::time::sleep(Duration::from_millis(150)).await;
        if !j["trusted"].as_bool().unwrap_or(false) {
            self.engine.decide_pair(&id, true).unwrap();
        }
        let resp = waiter.await.unwrap();
        assert_eq!(resp.status(), 200);
        let set = resp.headers().get("set-cookie").expect("cookie").to_str().unwrap().to_string();
        assert!(set.contains("HttpOnly") && set.contains("SameSite=Strict"), "cookie flags: {set}");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "approved");
        set.split(';').next().unwrap().to_string()
    }

    fn get(&self, path: &str, cookie: &str) -> reqwest::RequestBuilder {
        self.http.get(format!("{}{}", self.base, path)).header("cookie", cookie)
    }

    fn post(&self, path: &str, cookie: &str) -> reqwest::RequestBuilder {
        self.http.post(format!("{}{}", self.base, path)).header("cookie", cookie)
    }
}

/// Creates an offer, decides it on the "desktop", returns (offer id, [(clientId, transferId)]).
async fn make_offer(env: &Env, cookie: &str, files: Value, accept: bool) -> (String, Vec<(String, String)>) {
    let r = env.post("/api/offers", cookie).json(&json!({ "files": files })).send().await.unwrap();
    let status = r.status();
    let j: Value = r.json().await.unwrap();
    assert_eq!(status, 200, "offer failed: {j}");
    let id = j["id"].as_str().unwrap().to_string();
    let pending = env.engine.pending_offers();
    assert!(pending.iter().any(|o| o.id == id), "the desktop must see the offer and nothing is written before approval");
    assert!(std::fs::read_dir(&env.dl).unwrap().next().is_none(), "nothing may be written before approval");
    env.engine.decide_offer(&id, accept, Some(env.dl.clone())).unwrap();
    let j: Value = env.get(&format!("/api/offers/{id}"), cookie).send().await.unwrap().json().await.unwrap();
    assert_eq!(j["state"], if accept { "accepted" } else { "rejected" }, "{j}");
    let uploads: Vec<(String, String)> = j["uploads"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| (u["clientId"].as_str().unwrap().to_string(), u["transferId"].as_str().unwrap().to_string()))
        .collect();
    if accept {
        assert_eq!(uploads.len(), j["uploads"].as_array().unwrap().len());
    }
    (id, uploads)
}

async fn put_all(env: &Env, cookie: &str, tid: &str, data: &[u8], chunk: usize) {
    let mut off = 0usize;
    while off < data.len() {
        let end = (off + chunk).min(data.len());
        let r = env
            .http
            .put(format!("{}/api/uploads/{tid}", env.base))
            .header("cookie", cookie)
            .header("upload-offset", off.to_string())
            .body(data[off..end].to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "chunk at {off}");
        off = end;
    }
}

async fn complete(env: &Env, cookie: &str, tid: &str, sha256: &str) -> reqwest::Response {
    env.post(&format!("/api/uploads/{tid}/complete"), cookie).json(&json!({ "sha256": sha256 })).send().await.unwrap()
}

fn transfer(env: &Env, tid: &str) -> TransferInfo {
    env.engine.transfers().into_iter().find(|t| t.id == tid).expect("transfer exists")
}

// ---------------------------------------------------------------------------

#[tokio::test]
async fn unauthenticated_requests_are_rejected_and_host_is_validated() {
    let Some(env) = env().await else { return };
    // The static site is public (it contains no data).
    let r = env.http.get(format!("{}/", env.base)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(r.headers().get("content-security-policy").is_some());
    assert_eq!(r.headers().get("x-content-type-options").unwrap(), "nosniff");
    // Everything with data requires a session.
    for p in ["/api/share", "/api/transfers", "/api/events", "/api/share/zip"] {
        let r = env.http.get(format!("{}{p}", env.base)).send().await.unwrap();
        assert_eq!(r.status(), 401, "{p} must need a session");
    }
    // A bogus cookie is not a session.
    // (This is the 5th failure; it is still answered 401 but locks the address out.)
    let r = env.get("/api/share", "vd_s=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = env.get("/api/share", "vd_s=BBBB").send().await.unwrap();
    assert_eq!(r.status(), 429, "repeated failures must be rate limited");
    // DNS-rebinding style Host header.
    let r = env.http.get(format!("{}/", env.base)).header("host", "evil.example").send().await.unwrap();
    assert_eq!(r.status(), 421);
    // Cross-origin requests are refused and no CORS headers are ever offered.
    let r = env.http.get(format!("{}/api/info", env.base)).header("origin", "http://evil.example").send().await.unwrap();
    assert_eq!(r.status(), 403);
    let r = env.http.get(format!("{}/api/info", env.base)).send().await.unwrap();
    assert!(r.headers().get("access-control-allow-origin").is_none());
    // State-changing requests need the custom header (CSRF).
    let bare = reqwest::Client::new();
    let r = bare.post(format!("{}/api/logout", env.base)).send().await.unwrap();
    assert_eq!(r.status(), 403);
    let r = bare.request(reqwest::Method::OPTIONS, format!("{}/api/offers", env.base)).send().await.unwrap();
    assert_eq!(r.status(), 405);
    // Unknown API paths and static traversal.
    let r = env.http.get(format!("{}/api/nope", env.base)).send().await.unwrap();
    assert_eq!(r.status(), 404);
    let r = env.http.get(format!("{}/..%2f..%2fCargo.toml", env.base)).send().await.unwrap();
    assert!(r.status() == 404 || r.status() == 400 || r.status() == 200);
    if r.status() == 200 {
        assert!(!r.text().await.unwrap().contains("[package]"));
    }
    env.engine.shutdown().await;
}

#[tokio::test]
async fn pairing_needs_approval_token_is_single_use_and_can_be_regenerated() {
    let Some(env) = env().await else { return };
    // Wrong token.
    let r = env.pair_request("not-the-token", "device-aaaa-1111").await;
    assert!(r.status() == 401 || r.status() == 403 || r.status() == 410, "got {}", r.status());

    // Request, then reject on the desktop.
    let token = env.token();
    let j: Value = env.pair_request(&token, "device-aaaa-1111").await.json().await.unwrap();
    let (id, claim) = (j["id"].as_str().unwrap().to_string(), j["claim"].as_str().unwrap().to_string());
    assert_eq!(j["code"].as_str().unwrap().len(), 4);
    // A wrong claim secret learns nothing.
    let r = env.http.post(format!("{}/api/pair/claim", env.base)).json(&json!({"id": id, "claim": "wrong"})).send().await.unwrap();
    assert!(r.status().is_client_error());
    // Pending until the desktop decides: nothing is granted automatically.
    assert_eq!(env.engine.snapshot().pair_requests.len(), 1);
    assert!(env.engine.sessions().is_empty());
    env.engine.decide_pair(&id, false).unwrap();
    let r: Value = env
        .http
        .post(format!("{}/api/pair/claim", env.base))
        .json(&json!({"id": id, "claim": claim}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["status"], "rejected");
    assert!(env.engine.sessions().is_empty());

    // The token has been consumed by that request: replaying it must fail.
    let r = env.pair_request(&token, "device-bbbb-2222").await;
    assert!(r.status().is_client_error(), "a used token must not work again, got {}", r.status());

    // Regenerate gives a new working token and kills the old one.
    let old = env.token();
    let fresh = env.engine.regenerate_pairing().await.unwrap();
    assert_ne!(fresh.url, format!("http://{}/#t={old}", env.host));
    let r = env.pair_request(&old, "device-cccc-3333").await;
    assert!(r.status().is_client_error());
    let cookie = env.pair("device-dddd-4444").await;
    let r = env.get("/api/session", &cookie).send().await.unwrap();
    let s: Value = r.json().await.unwrap();
    assert_eq!(s["paired"], true);
    assert_eq!(env.engine.sessions().len(), 1);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn revoking_a_session_locks_the_phone_out_immediately() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-revoke-1").await;
    assert_eq!(env.get("/api/transfers", &cookie).send().await.unwrap().status(), 200);
    let sid = env.engine.sessions()[0].id.clone();
    assert!(env.engine.revoke_session(&sid));
    assert_eq!(env.get("/api/transfers", &cookie).send().await.unwrap().status(), 401);
    assert_eq!(env.get("/api/share", &cookie).send().await.unwrap().status(), 401);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn upload_is_chunked_verified_and_placed_with_unique_names() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-upload-1").await;
    let a = pattern(300_000, 1);
    let b = pattern(12_345, 2);
    let files = json!([
        { "clientId": "a", "name": "photo.jpg", "relPath": null, "size": a.len() },
        { "clientId": "b", "name": "photo.jpg", "relPath": null, "size": b.len() },
        { "clientId": "c", "name": "../../evil.txt", "relPath": null, "size": 4 },
        { "clientId": "d", "name": "empty.bin", "relPath": null, "size": 0 },
    ]);
    let (_oid, ups) = make_offer(&env, &cookie, files, true).await;
    let tid = |c: &str| ups.iter().find(|u| u.0 == c).unwrap().1.clone();

    put_all(&env, &cookie, &tid("a"), &a, 64 * 1024).await;
    assert_eq!(complete(&env, &cookie, &tid("a"), &sha(&a)).await.status(), 200);
    put_all(&env, &cookie, &tid("b"), &b, 5000).await;
    assert_eq!(complete(&env, &cookie, &tid("b"), &sha(&b)).await.status(), 200);
    put_all(&env, &cookie, &tid("c"), b"evil", 100).await;
    assert_eq!(complete(&env, &cookie, &tid("c"), &sha(b"evil")).await.status(), 200);
    assert_eq!(complete(&env, &cookie, &tid("d"), &sha(b"")).await.status(), 200);

    // Everything landed inside the destination, duplicates did not overwrite.
    let mut names: Vec<String> =
        std::fs::read_dir(&env.dl).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    names.sort();
    assert_eq!(names.len(), 4, "{names:?}");
    assert!(names.iter().any(|n| n == "photo.jpg"));
    assert!(names.iter().any(|n| n.starts_with("photo") && n != "photo.jpg"), "duplicate gets a new name: {names:?}");
    assert!(names.iter().any(|n| n.contains("evil")), "{names:?}");
    assert!(!env.dl.parent().unwrap().join("evil.txt").exists());
    assert!(!env.dl.parent().unwrap().parent().unwrap().join("evil.txt").exists());
    let saved_a = transfer(&env, &tid("a")).saved_path.unwrap();
    assert_eq!(std::fs::read(&saved_a).unwrap(), a);
    assert!(
        !std::fs::read_dir(&env.dl).unwrap().any(|e| e.unwrap().file_name().to_string_lossy().ends_with("veydrop-part")),
        "no part files may remain after success"
    );
    // The phone is never told the PC path.
    let snap: Value = env.get("/api/transfers", &cookie).send().await.unwrap().json().await.unwrap();
    assert!(!snap.to_string().contains(&env.dl.to_string_lossy().replace('\\', "\\\\")));
    // History records it, without any token.
    let hist = env.engine.history(&Default::default());
    assert!(hist.len() >= 4);
    assert!(!serde_json::to_string(&hist).unwrap().to_lowercase().contains("vd_s"));
    assert_eq!(env.engine.snapshot().stats.received_files, 4);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn rejected_offers_and_bad_hashes_never_produce_files() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-reject-01").await;
    // Rejected by the user.
    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "x", "name": "no.txt", "size": 3 }]), false).await;
    assert!(ups.is_empty(), "a rejected offer must not hand out upload slots");
    let r = env
        .http
        .put(format!("{}/api/uploads/{}", env.base, "a".repeat(16)))
        .header("cookie", &cookie)
        .header("upload-offset", "0")
        .body("abc")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_client_error(), "uploading a rejected file must fail, got {}", r.status());
    assert!(std::fs::read_dir(&env.dl).unwrap().next().is_none());

    // Accepted, but the phone claims the wrong hash.
    let data = pattern(70_000, 9);
    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "y", "name": "bad.bin", "size": data.len() }]), true).await;
    put_all(&env, &cookie, &ups[0].1, &data, 32 * 1024).await;
    let r = complete(&env, &cookie, &ups[0].1, &sha(b"something else")).await;
    assert_eq!(r.status(), 422);
    assert_eq!(transfer(&env, &ups[0].1).state, TransferState::Failed);
    assert!(std::fs::read_dir(&env.dl).unwrap().next().is_none(), "a corrupt file must not be kept");

    // More data than announced is refused.
    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "z", "name": "z.bin", "size": 10 }]), true).await;
    let r = env
        .http
        .put(format!("{}/api/uploads/{}", env.base, ups[0].1))
        .header("cookie", &cookie)
        .header("upload-offset", "0")
        .body(vec![0u8; 11])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    // Completing an incomplete file is refused and never reported as success.
    let r = complete(&env, &cookie, &ups[0].1, &sha(&[0u8; 10])).await;
    assert!(r.status().is_client_error());
    assert_ne!(transfer(&env, &ups[0].1).state, TransferState::Completed);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn interrupted_upload_resumes_from_the_committed_offset() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-resume-01").await;
    let data = pattern(500_000, 7);
    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "r", "name": "big.bin", "size": data.len() }]), true).await;
    let tid = ups[0].1.clone();
    put_all(&env, &cookie, &tid, &data[..200_000], 100_000).await;

    // A wrong offset reports where to resume instead of corrupting the file.
    let r = env
        .http
        .put(format!("{}/api/uploads/{tid}", env.base))
        .header("cookie", &cookie)
        .header("upload-offset", "123")
        .body(vec![1u8; 10])
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let j: Value = r.json().await.unwrap();
    assert_eq!(j["offset"], 200_000);

    // The phone asks where to continue (e.g. after a reload / Wi-Fi drop).
    let st: Value = env.get(&format!("/api/uploads/{tid}"), &cookie).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["received"], 200_000);

    // Pause blocks chunks; resume re-enables them.
    let r = env.post(&format!("/api/uploads/{tid}/state"), &cookie).json(&json!({"paused": true})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let blocked = env
        .http
        .put(format!("{}/api/uploads/{tid}", env.base))
        .header("cookie", &cookie)
        .header("upload-offset", "200000")
        .body(data[200_000..210_000].to_vec())
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), 409);
    env.post(&format!("/api/uploads/{tid}/state"), &cookie).json(&json!({"paused": false})).send().await.unwrap();

    put_all_from(&env, &cookie, &tid, &data, 200_000, 150_000).await;
    assert_eq!(complete(&env, &cookie, &tid, &sha(&data)).await.status(), 200);
    assert_eq!(std::fs::read(transfer(&env, &tid).saved_path.unwrap()).unwrap(), data);
    env.engine.shutdown().await;
}

async fn put_all_from(env: &Env, cookie: &str, tid: &str, data: &[u8], from: usize, chunk: usize) {
    let mut off = from;
    while off < data.len() {
        let end = (off + chunk).min(data.len());
        let r = env
            .http
            .put(format!("{}/api/uploads/{tid}", env.base))
            .header("cookie", cookie)
            .header("upload-offset", off.to_string())
            .body(data[off..end].to_vec())
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        off = end;
    }
}

#[tokio::test]
async fn cancelled_upload_is_removed_and_never_succeeds() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-cancel-01").await;
    let data = pattern(200_000, 3);
    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "k", "name": "gone.bin", "size": data.len() }]), true).await;
    let tid = ups[0].1.clone();
    put_all(&env, &cookie, &tid, &data[..100_000], 50_000).await;
    // Phone cancels.
    let r = env.http.delete(format!("{}/api/uploads/{tid}", env.base)).header("cookie", &cookie).send().await.unwrap();
    assert!(r.status().is_success());
    assert_eq!(transfer(&env, &tid).state, TransferState::Cancelled);
    let r = complete(&env, &cookie, &tid, &sha(&data)).await;
    assert!(r.status().is_client_error());
    assert_eq!(transfer(&env, &tid).state, TransferState::Cancelled);
    assert!(std::fs::read_dir(&env.dl).unwrap().next().is_none(), "partial data must be deleted on cancel");

    // Desktop cancels an in-progress transfer.
    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "k2", "name": "gone2.bin", "size": data.len() }]), true).await;
    let tid = ups[0].1.clone();
    put_all(&env, &cookie, &tid, &data[..50_000], 50_000).await;
    env.engine.cancel_transfer(&tid).unwrap();
    let r = env
        .http
        .put(format!("{}/api/uploads/{tid}", env.base))
        .header("cookie", &cookie)
        .header("upload-offset", "50000")
        .body(data[50_000..60_000].to_vec())
        .send()
        .await
        .unwrap();
    assert!(r.status().is_client_error());
    assert!(std::fs::read_dir(&env.dl).unwrap().next().is_none());
    env.engine.shutdown().await;
}

#[tokio::test]
async fn one_phone_cannot_touch_another_phones_transfer() {
    let Some(env) = env().await else { return };
    let c1 = env.pair("device-owner-001").await;
    // Second phone needs a fresh token (single use).
    env.engine.regenerate_pairing().await.unwrap();
    let c2 = env.pair("device-other-002").await;
    let (_o, ups) = make_offer(&env, &c1, json!([{ "clientId": "m", "name": "mine.bin", "size": 5 }]), true).await;
    let tid = ups[0].1.clone();
    let r = env
        .http
        .put(format!("{}/api/uploads/{tid}", env.base))
        .header("cookie", &c2)
        .header("upload-offset", "0")
        .body("hello")
        .send()
        .await
        .unwrap();
    assert!(r.status().is_client_error());
    assert!(env.get(&format!("/api/uploads/{tid}"), &c2).send().await.unwrap().status().is_client_error());
    assert!(env.http.delete(format!("{}/api/uploads/{tid}", env.base)).header("cookie", &c2).send().await.unwrap().status().is_client_error());
    assert_ne!(transfer(&env, &tid).state, TransferState::Cancelled);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn shared_files_download_with_ranges_zip_and_are_hidden_when_stopped() {
    let Some(env) = env().await else { return };
    let src = tempfile::tempdir().unwrap();
    let f1 = src.path().join("one.txt");
    let f2 = src.path().join("two.bin");
    let folder = src.path().join("album");
    std::fs::create_dir_all(folder.join("sub")).unwrap();
    std::fs::write(&f1, b"hello veydrop").unwrap();
    let two = pattern(400_000, 5);
    std::fs::write(&f2, &two).unwrap();
    std::fs::write(folder.join("a.txt"), b"AAA").unwrap();
    std::fs::write(folder.join("sub").join("b.txt"), b"BBBB").unwrap();

    let cookie = env.pair("device-download-1").await;
    // Nothing shared yet.
    let j: Value = env.get("/api/share", &cookie).send().await.unwrap().json().await.unwrap();
    assert!(j["share"].is_null());

    let rep = env.engine.add_share_paths(vec![f1.clone(), f2.clone(), folder.clone()]).await.unwrap();
    assert_eq!(rep.skipped, 0);
    // Staged but not started: the phone still sees nothing.
    let j: Value = env.get("/api/share", &cookie).send().await.unwrap().json().await.unwrap();
    assert!(j["share"].is_null(), "an inactive share must stay invisible");
    env.engine.set_share_active(true).unwrap();
    let j: Value = env.get("/api/share", &cookie).send().await.unwrap().json().await.unwrap();
    let items = j["share"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    let find = |n: &str| items.iter().find(|i| i["name"] == n).unwrap().clone();
    let id2 = find("two.bin")["id"].as_str().unwrap().to_string();
    let rels: Vec<String> = items.iter().map(|i| i["relPath"].as_str().unwrap().replace('\\', "/")).collect();
    assert!(rels.iter().any(|r| r == "album/sub/b.txt"), "folder structure preserved: {rels:?}");
    // The phone is never shown PC paths.
    assert!(!j.to_string().contains(&src.path().to_string_lossy().replace('\\', "\\\\")));

    // Full download + hash.
    let r = env.get(&format!("/api/share/items/{id2}/download"), &cookie).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers().get("accept-ranges").unwrap(), "bytes");
    assert!(r.headers().get("content-disposition").unwrap().to_str().unwrap().starts_with("attachment"));
    let body = r.bytes().await.unwrap();
    assert_eq!(sha(&body), sha(&two));
    // Range.
    let r = env.get(&format!("/api/share/items/{id2}/download"), &cookie).header("range", "bytes=1000-1999").send().await.unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers().get("content-range").unwrap(), &format!("bytes 1000-1999/{}", two.len()));
    assert_eq!(r.bytes().await.unwrap().as_ref(), &two[1000..2000]);
    let r = env.get(&format!("/api/share/items/{id2}/download"), &cookie).header("range", "bytes=999999999-").send().await.unwrap();
    assert_eq!(r.status(), 416);
    // Unknown id / traversal-ish id.
    assert_eq!(env.get("/api/share/items/nope/download", &cookie).send().await.unwrap().status(), 404);
    assert_eq!(env.get("/api/share/items/..%2f..%2fx/download", &cookie).send().await.unwrap().status(), 404);

    // ZIP of everything is a valid archive with the right content and names.
    let r = env.get("/api/share/zip", &cookie).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let bytes = r.bytes().await.unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut seen = Vec::new();
    for i in 0..z.len() {
        let mut e = z.by_index(i).unwrap();
        assert!(!e.name().contains(".."), "{}", e.name());
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut e, &mut buf).unwrap();
        seen.push((e.name().to_string(), buf));
    }
    assert!(seen.iter().any(|(n, b)| n == "one.txt" && b == b"hello veydrop"));
    assert!(seen.iter().any(|(n, b)| n == "two.bin" && b == &two));
    assert!(seen.iter().any(|(n, b)| n == "album/sub/b.txt" && b == b"BBBB"));

    // Stopping the share hides everything from the phone at once.
    env.engine.set_share_active(false).unwrap();
    assert_eq!(env.get(&format!("/api/share/items/{id2}/download"), &cookie).send().await.unwrap().status(), 404);
    let j: Value = env.get("/api/share", &cookie).send().await.unwrap().json().await.unwrap();
    assert!(j["share"].is_null());
    env.engine.shutdown().await;
}

#[tokio::test]
async fn a_file_changed_after_selection_is_not_served() {
    let Some(env) = env().await else { return };
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("edit.txt");
    std::fs::write(&f, b"original").unwrap();
    let cookie = env.pair("device-changed-01").await;
    env.engine.add_share_paths(vec![f.clone()]).await.unwrap();
    env.engine.set_share_active(true).unwrap();
    let id = env.engine.share().unwrap().items[0].id.clone();
    std::fs::write(&f, b"modified and longer now").unwrap();
    let r = env.get(&format!("/api/share/items/{id}/download"), &cookie).send().await.unwrap();
    assert_eq!(r.status(), 409);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn large_file_streams_both_ways_with_correct_hashes() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-large-0001").await;
    let size = 96 * 1024 * 1024usize;
    let data = pattern(size, 11);
    let expect = sha(&data);

    let (_o, ups) = make_offer(&env, &cookie, json!([{ "clientId": "L", "name": "large.bin", "size": size }]), true).await;
    let tid = ups[0].1.clone();
    put_all(&env, &cookie, &tid, &data, 8 * 1024 * 1024).await;
    assert_eq!(complete(&env, &cookie, &tid, &expect).await.status(), 200);
    let saved = PathBuf::from(transfer(&env, &tid).saved_path.unwrap());
    assert_eq!(std::fs::metadata(&saved).unwrap().len() as usize, size);
    assert_eq!(sha(&std::fs::read(&saved).unwrap()), expect);

    // And back down, consumed as a stream.
    env.engine.add_share_paths(vec![saved.clone()]).await.unwrap();
    env.engine.set_share_active(true).unwrap();
    let id = env.engine.share().unwrap().items[0].id.clone();
    let mut r = env.get(&format!("/api/share/items/{id}/download"), &cookie).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let mut h = Sha256::new();
    let mut total = 0usize;
    while let Some(c) = r.chunk().await.unwrap() {
        total += c.len();
        h.update(&c);
    }
    assert_eq!(total, size);
    assert_eq!(hex::encode(h.finalize()), expect);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn desktop_cancel_stops_a_running_download() {
    let Some(env) = env().await else { return };
    let src = tempfile::tempdir().unwrap();
    let f = src.path().join("slow.bin");
    std::fs::write(&f, pattern(64 * 1024 * 1024, 4)).unwrap();
    let cookie = env.pair("device-dlcancel-01").await;
    env.engine.add_share_paths(vec![f]).await.unwrap();
    env.engine.set_share_active(true).unwrap();
    let id = env.engine.share().unwrap().items[0].id.clone();
    let mut r = env.get(&format!("/api/share/items/{id}/download"), &cookie).send().await.unwrap();
    let first = r.chunk().await.unwrap();
    assert!(first.is_some());
    // The transfer is visible to the desktop while it runs.
    let t = env.engine.transfers().into_iter().find(|t| t.name == "slow.bin").expect("outgoing transfer tracked");
    env.engine.cancel_transfer(&t.id).unwrap();
    let mut got = first.unwrap().len();
    let mut ended_early = false;
    loop {
        match r.chunk().await {
            Ok(Some(c)) => got += c.len(),
            Ok(None) => break,
            Err(_) => {
                ended_early = true;
                break;
            }
        }
    }
    assert!(ended_early || got < 64 * 1024 * 1024, "a cancelled download must not run to completion");
    assert_eq!(transfer(&env, &t.id).state, TransferState::Cancelled);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn trusted_devices_skip_approval_but_still_need_a_fresh_qr_token() {
    let Some(env) = env().await else { return };
    env.engine.update_settings(json!({ "trustKnownDevices": true })).unwrap();
    let _ = env.pair("device-trusted-001").await;
    assert_eq!(env.engine.settings().trusted_devices.len(), 1);
    env.engine.regenerate_pairing().await.unwrap();
    // Same device again: approved without a click, but only with the new token.
    let r = env.pair_request("stale-or-forged-token", "device-trusted-001").await;
    assert!(r.status().is_client_error());
    let j: Value = env.pair_request(&env.token(), "device-trusted-001").await.json().await.unwrap();
    assert_eq!(j["trusted"], true);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn shutdown_stops_listening_revokes_sessions_and_restart_works() {
    let Some(env) = env().await else { return };
    let cookie = env.pair("device-shutdown-01").await;
    assert_eq!(env.get("/api/transfers", &cookie).send().await.unwrap().status(), 200);
    assert!(env.engine.is_running());
    env.engine.stop_server().await;
    assert!(!env.engine.is_running());
    assert!(env.engine.sessions().is_empty());
    assert!(env.engine.pairing().is_none());
    assert!(!env.engine.server_status().running);
    let res = env.http.get(format!("{}/api/info", env.base)).timeout(Duration::from_secs(3)).send().await;
    assert!(res.is_err(), "the port must be closed after stopping");

    // Starting again yields a fresh server; the old cookie is worthless.
    let st = env.engine.start_server().await.unwrap();
    assert!(st.running);
    let base2 = format!("http://{}:{}", st.ip.unwrap(), st.port.unwrap());
    let r = env.http.get(format!("{base2}/api/transfers")).header("cookie", &cookie).send().await.unwrap();
    assert_eq!(r.status(), 401);
    env.engine.shutdown().await;
    assert!(!env.engine.is_running());
}

#[tokio::test]
async fn settings_persist_and_events_are_published() {
    let Some(env) = env().await else { return };
    let mut rx = env.engine.subscribe();
    let s = env.engine.update_settings(json!({ "deviceName": "Studio PC", "language": "ar", "theme": "light" })).unwrap();
    assert_eq!(s.device_name, "Studio PC");
    // Out-of-range values are clamped to safe bounds, wrongly typed ones are refused.
    assert_eq!(env.engine.update_settings(json!({ "pairingSeconds": 1 })).unwrap().pairing_seconds, 30);
    assert_eq!(env.engine.update_settings(json!({ "chunkSizeMib": 100000 })).unwrap().chunk_size_mib, 16);
    assert!(env.engine.update_settings(json!({ "language": 5 })).is_err());
    let mut saw_settings = false;
    while let Ok(ev) = tokio::time::timeout(Duration::from_millis(300), rx.recv()).await {
        if let Ok(veydrop_core::model::EngineEvent::Settings(_)) = ev {
            saw_settings = true;
            break;
        }
    }
    assert!(saw_settings);
    let snap = serde_json::to_string(&env.engine.snapshot()).unwrap();
    assert!(snap.contains("Studio PC"));
    env.engine.shutdown().await;
}

#[tokio::test]
async fn requests_from_this_pc_never_count_as_a_phone_reaching_the_server() {
    let Some(env) = env().await else { return };
    // This PC talks to its own LAN address: Windows skips the firewall for that,
    // so it must not be reported as proof that a phone can connect.
    let info = env.http.get(format!("{}/api/info", env.base)).send().await.unwrap();
    assert_eq!(info.status(), 200);
    let _cookie = env.pair("device-selfcheck-01").await;

    let d = env.engine.diagnostics(false).await;
    assert!(d.running);
    assert_eq!(d.bind.as_deref(), Some(env.host.as_str()));
    let local = d.local.expect("local check present while running");
    assert!(local.ok && local.page_ok, "local check should see the page: {local:?}");
    assert!(!d.external.reached, "self-traffic must not count: {:?}", d.external);
    assert_eq!(d.external.requests, 0);
    assert!(!d.phone_seen);
    assert!(d.warnings.iter().any(|w| w == "not_reached"), "must warn until a device really reaches the PC: {:?}", d.warnings);
    let st = env.engine.server_status();
    assert!(!st.phone_seen && st.remote_ip.is_none() && st.reached_at.is_none());
    env.engine.shutdown().await;
}

#[tokio::test]
async fn server_stays_responsive_and_restarts_cleanly() {
    let Some(env) = env().await else { return };
    let _ = env.pair_request(&env.token(), "device-restart-01").await;
    // Pairing is pending; the server must keep answering other requests.
    for _ in 0..20 {
        let r = env.http.get(format!("{}/api/info", env.base)).send().await.unwrap();
        assert_eq!(r.status(), 200);
    }
    let st = env.engine.restart_server().await.unwrap();
    assert!(st.running && st.ip.is_some() && st.port.is_some());
    let base = format!("http://{}:{}", st.ip.unwrap(), st.port.unwrap());
    let r = env.http.get(format!("{base}/api/info")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert!(env.engine.pairing().is_some(), "a fresh QR must be issued for the restarted server");
    env.engine.shutdown().await;
}

// ---- language synchronisation (the PC decides the phone's language) -------

/// Reads an SSE stream until `needle` appears; returns everything read so far.
async fn sse_until(resp: &mut reqwest::Response, needle: &str) -> String {
    let mut buf = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while !buf.contains(needle) {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(left, resp.chunk()).await {
            Ok(Ok(Some(c))) => buf.push_str(&String::from_utf8_lossy(&c)),
            other => panic!("stream ended or timed out waiting for {needle:?}; got {buf:?} ({other:?})"),
        }
    }
    buf
}

#[tokio::test]
async fn phone_gets_the_desktop_language_at_pairing_session_check_and_event_stream_connect() {
    let Some(env) = env().await else { return };
    env.engine.update_settings(json!({ "language": "ar" })).unwrap();

    // pair/request and pair/claim both carry it
    let j: Value = env.pair_request(&env.token(), "device-lang-0001").await.json().await.unwrap();
    assert_eq!(j["language"], "ar");
    let (id, claim) = (j["id"].as_str().unwrap().to_string(), j["claim"].as_str().unwrap().to_string());
    env.engine.decide_pair(&id, true).unwrap();
    let r = env
        .http
        .post(format!("{}/api/pair/claim", env.base))
        .json(&json!({"id": id, "claim": claim}))
        .send()
        .await
        .unwrap();
    let cookie = r.headers().get("set-cookie").unwrap().to_str().unwrap().split(';').next().unwrap().to_string();
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["status"], "approved");
    assert_eq!(body["language"], "ar");

    // a reconnecting phone restores the desktop language from /api/session ...
    let s: Value = env.get("/api/session", &cookie).send().await.unwrap().json().await.unwrap();
    assert_eq!(s["language"], "ar");
    // ... and from the hello of every (re)connected event stream
    let mut sse = env.get("/api/events", &cookie).send().await.unwrap();
    let got = sse_until(&mut sse, "\"language\"").await;
    assert!(got.contains("event: hello") && got.contains("\"language\":\"ar\""), "{got}");
    env.engine.shutdown().await;
}

#[tokio::test]
async fn changing_the_desktop_language_reaches_a_connected_phone_live_in_both_directions() {
    let Some(env) = env().await else { return };
    env.engine.update_settings(json!({ "language": "en" })).unwrap();
    let cookie = env.pair("device-lang-0002").await;
    let mut sse = env.get("/api/events", &cookie).send().await.unwrap();
    let hello = sse_until(&mut sse, "\"language\"").await;
    assert!(hello.contains("\"language\":\"en\""), "{hello}");

    // EN -> AR on the same connection and session, no re-pairing
    env.engine.update_settings(json!({ "language": "ar" })).unwrap();
    let got = sse_until(&mut sse, "event: language").await;
    assert!(got.contains("\"language\":\"ar\""), "{got}");

    // AR -> EN
    env.engine.update_settings(json!({ "language": "en" })).unwrap();
    let got = sse_until(&mut sse, "\"language\":\"en\"").await;
    assert!(got.contains("event: language"), "{got}");

    // The session was never touched and still works.
    assert_eq!(env.engine.sessions().len(), 1);
    assert_eq!(env.get("/api/transfers", &cookie).send().await.unwrap().status(), 200);
    env.engine.shutdown().await;
}

#[tokio::test]
async fn unchanged_language_does_not_broadcast_and_system_language_follows_the_desktop_ui() {
    let Some(env) = env().await else { return };
    env.engine.update_settings(json!({ "language": "ar" })).unwrap();
    let cookie = env.pair("device-lang-0003").await;
    let mut sse = env.get("/api/events", &cookie).send().await.unwrap();
    sse_until(&mut sse, "\"language\"").await;

    // Re-saving the same language sends nothing.
    env.engine.update_settings(json!({ "language": "ar" })).unwrap();
    // "system" while the desktop UI resolves to Arabic is still Arabic: no event.
    env.engine.set_system_language("ar");
    env.engine.update_settings(json!({ "language": "system" })).unwrap();
    // A real change (OS language moves to an unsupported one -> English) is the first event.
    env.engine.set_system_language("de");
    let got = sse_until(&mut sse, "event: language").await;
    assert!(got.contains("\"language\":\"en\""), "the only language event is the change to English: {got}");
    assert_eq!(got.matches("event: language").count(), 1, "{got}");
    assert_eq!(env.engine.language(), "en");

    // With "system" selected, a later desktop OS language change is mirrored too.
    env.engine.set_system_language("ar");
    let got = sse_until(&mut sse, "\"language\":\"ar\"").await;
    assert!(got.contains("event: language"));
    env.engine.shutdown().await;
}

#[tokio::test]
async fn desktop_language_survives_an_app_restart() {
    let data = TempDir::new().unwrap();
    let mobile = TempDir::new().unwrap();
    let cfg = || EngineConfig { data_dir: data.path().to_path_buf(), mobile_dir: mobile.path().to_path_buf() };
    {
        let engine = Engine::new(cfg()).await.unwrap();
        engine.update_settings(json!({ "language": "ar" })).unwrap();
        assert_eq!(engine.language(), "ar");
        engine.shutdown().await;
    }
    let engine = Engine::new(cfg()).await.unwrap();
    assert_eq!(engine.settings().language, "ar");
    assert_eq!(engine.language(), "ar");
    engine.update_settings(json!({ "language": "en" })).unwrap();
    engine.shutdown().await;
    let engine = Engine::new(cfg()).await.unwrap();
    assert_eq!(engine.language(), "en");
    engine.shutdown().await;
}

// ---- pairing-code consistency ------------------------------------------------

#[tokio::test]
async fn concurrent_pairing_attempts_stay_bound_to_their_own_phone_and_code() {
    let Some(env) = env().await else { return };
    // Phone A and phone B each scan a (different) fresh QR and wait for approval.
    let ja: Value = env.pair_request(&env.token(), "device-conc-aaaa").await.json().await.unwrap();
    env.engine.regenerate_pairing().await.unwrap();
    let jb: Value = env.pair_request(&env.token(), "device-conc-bbbb").await.json().await.unwrap();

    for j in [&ja, &jb] {
        let code = j["code"].as_str().unwrap();
        assert!(code.len() == 4 && code.bytes().all(|b| b.is_ascii_digit()), "code {code:?}");
    }
    let (ida, claima) = (ja["id"].as_str().unwrap().to_string(), ja["claim"].as_str().unwrap().to_string());
    let (idb, claimb) = (jb["id"].as_str().unwrap().to_string(), jb["claim"].as_str().unwrap().to_string());
    assert_ne!(ida, idb);

    // The desktop shows two distinct pending requests, each with exactly the code its phone got.
    let snap = env.engine.snapshot();
    assert_eq!(snap.pair_requests.len(), 2);
    for (id, j) in [(&ida, &ja), (&idb, &jb)] {
        let req = snap.pair_requests.iter().find(|r| &r.id == id).expect("pending request");
        assert_eq!(req.code, j["code"].as_str().unwrap());
    }

    let claim = |id: &str, secret: &str| {
        let url = format!("{}/api/pair/claim", env.base);
        let http = env.http.clone();
        let body = json!({"id": id, "claim": secret});
        async move { http.post(url).json(&body).send().await.unwrap() }
    };

    // Rejecting A does not approve B, and B's phone keeps waiting.
    env.engine.decide_pair(&ida, false).unwrap();
    assert!(env.engine.sessions().is_empty());
    let ra: Value = claim(&ida, &claima).await.json().await.unwrap();
    assert_eq!(ra["status"], "rejected");
    let rb: Value = claim(&idb, &claimb).await.json().await.unwrap();
    assert_eq!(rb["status"], "pending", "B must still be waiting after A was rejected");

    // A's id with B's secret (and vice versa) learns nothing.
    assert!(claim(&ida, &claimb).await.status().is_client_error());
    assert!(claim(&idb, &claima).await.status().is_client_error());

    // A rejected attempt can never be approved afterwards.
    assert!(env.engine.decide_pair(&ida, true).is_err());
    assert!(env.engine.sessions().is_empty());

    // Approving B creates exactly one session, delivered only to B.
    env.engine.decide_pair(&idb, true).unwrap();
    assert_eq!(env.engine.sessions().len(), 1);
    let rb = claim(&idb, &claimb).await;
    assert!(rb.headers().get("set-cookie").is_some());
    let rb: Value = rb.json().await.unwrap();
    assert_eq!(rb["status"], "approved");
    let ra: Value = claim(&ida, &claima).await.json().await.unwrap();
    assert_eq!(ra["status"], "rejected");
    assert!(ra.get("session").is_none());
    env.engine.shutdown().await;
}
