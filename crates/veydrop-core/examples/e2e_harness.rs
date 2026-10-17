//! Headless harness for the browser end-to-end tests (Playwright).
//!
//! It starts the REAL engine and the REAL server on a real network interface and then plays the
//! role of the person at the PC through stdin commands, so a browser can drive the phone side of
//! every flow. It is test tooling only: it is an example target, never part of the installer.
//!
//! Usage: e2e_harness <mobile_dir> <work_dir> [share_file ...]
//!
//! stdout (one JSON object per line): {"event":"ready", ...}, {"event":"pair-request", ...},
//! {"event":"offer", ...}, {"event":"transfer-done", ...}, {"event":"server", ...}.
//! stdin commands: approve auto|reject|never, decide <id> approve|reject, offers accept|reject, share-on, share-off, revoke, stop,
//! lang en|ar (the PC user changes the app language), regen, quit.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;

use serde_json::json;
use tokio::io::{AsyncBufReadExt, BufReader};
use veydrop_core::engine::{Engine, EngineConfig};
use veydrop_core::model::EngineEvent;

fn say(v: serde_json::Value) {
    println!("{v}");
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: e2e_harness <mobile_dir> <work_dir> [share_file ...]");
        std::process::exit(2);
    }
    let mobile_dir = PathBuf::from(&args[0]);
    let work = PathBuf::from(&args[1]);
    let share_files: Vec<PathBuf> = args[2..].iter().map(PathBuf::from).collect();
    let downloads = work.join("downloads");
    std::fs::create_dir_all(&downloads).unwrap();

    let engine = Engine::new(EngineConfig { data_dir: work.join("data"), mobile_dir }).await.unwrap();
    engine
        .update_settings(json!({ "downloadDir": downloads.to_string_lossy(), "askBeforeReceive": true }))
        .unwrap();
    let st = match engine.start_server().await {
        Ok(s) => s,
        Err(e) => {
            say(json!({ "event": "error", "message": e.to_string() }));
            std::process::exit(3);
        }
    };
    if !share_files.is_empty() {
        let report = engine.add_share_paths(share_files).await.unwrap();
        say(json!({ "event": "shared", "items": report.share.items.len(), "skipped": report.skipped }));
        engine.set_share_active(true).unwrap();
    }

    // 1 = approve automatically, 2 = reject automatically, 0 = leave pending
    let approve = Arc::new(AtomicU8::new(1));
    let accept = Arc::new(AtomicBool::new(true));

    let pairing = engine.pairing().expect("pairing");
    say(json!({
        "event": "ready",
        "ip": st.ip.map(|i| i.to_string()),
        "port": st.port,
        "pairUrl": pairing.url,
        "downloads": downloads.to_string_lossy(),
    }));

    // The "person at the PC": reacts to events from the engine.
    {
        let engine = engine.clone();
        let approve = approve.clone();
        let accept = accept.clone();
        let mut rx = engine.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(EngineEvent::PairRequest(r)) => {
                        say(json!({ "event": "pair-request", "id": r.id, "code": r.code, "label": r.device_label, "trusted": r.trusted }));
                        match approve.load(Ordering::SeqCst) {
                            1 => {
                                let _ = engine.decide_pair(&r.id, true);
                            }
                            2 => {
                                let _ = engine.decide_pair(&r.id, false);
                            }
                            _ => {}
                        }
                    }
                    Ok(EngineEvent::Offer(o)) => {
                        say(json!({ "event": "offer", "id": o.id, "files": o.files.len(), "total": o.total_size }));
                        let r = engine.decide_offer(&o.id, accept.load(Ordering::SeqCst), None);
                        say(json!({ "event": "offer-decided", "id": o.id, "ok": r.is_ok(), "error": r.err().map(|e| e.to_string()) }));
                    }
                    Ok(EngineEvent::TransferDone(t)) => {
                        say(json!({ "event": "transfer-done", "id": t.id, "name": t.name, "state": t.state, "error": t.error }));
                    }
                    Ok(EngineEvent::Server(s)) => {
                        say(json!({ "event": "server", "running": s.running }));
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        });
    }

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut it = line.split_whitespace();
        match (it.next(), it.next()) {
            (Some("approve"), Some(m)) => {
                approve.store(match m { "auto" => 1, "reject" => 2, _ => 0 }, Ordering::SeqCst)
            }
            (Some("offers"), Some(m)) => accept.store(m == "accept", Ordering::SeqCst),
            (Some("decide"), Some(id)) => {
                let approve_it = it.next() == Some("approve");
                let _ = engine.decide_pair(id, approve_it);
            }
            (Some("share-on"), _) => {
                let _ = engine.set_share_active(true);
            }
            (Some("share-off"), _) => {
                let _ = engine.set_share_active(false);
            }
            (Some("lang"), Some(l)) => {
                let _ = engine.update_settings(json!({ "language": l }));
            }
            (Some("revoke"), _) => engine.disconnect_all(),
            (Some("regen"), _) => {
                if let Ok(p) = engine.regenerate_pairing().await {
                    say(json!({ "event": "pairing", "pairUrl": p.url }));
                }
            }
            (Some("stop"), _) => engine.stop_server().await,
            (Some("quit"), _) => break,
            _ => {}
        }
    }
    engine.shutdown().await;
    say(json!({ "event": "bye" }));
}
