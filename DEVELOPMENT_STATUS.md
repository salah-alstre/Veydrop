# Veydrop development status

_Last updated: 2026-10-17_

Veydrop 1.0.0 is feature-complete against the specification, and an unsigned Windows installer has been built and inspected. It is **not fully release-validated**:

- It has **not** been verified on a physical iPhone or Android phone.
- The installer has **not** been run through an install/uninstall cycle.
- One intermittent end-to-end failure on the emulated WebKit profile is **unresolved** (see [Known issues](#known-issues)).

## What was verified, and how

| Category | What it covers | Status |
| --- | --- | --- |
| Automated tests | Rust unit and real-HTTP integration tests, desktop and mobile unit tests, type-checks, mobile production build | Passing (table below) |
| Browser emulation | Playwright runs of the phone web app against the real Rust engine: Pixel 7 on Chromium, iPhone 14 on Playwright's WebKit build. **Emulation, not real devices; WebKit is not iOS Safari.** | 77/78 per full run, 3 runs (see below) |
| Local Windows run | The release `veydrop.exe` was launched from a scratch folder with isolated data (`VEYDROP_DATA_DIR`) and driven over the WebView2 DevTools port to capture the README desktop screenshots, with an emulated Pixel 7 as the phone | Done once, with sample data |
| Physical devices | Real iPhone / Android over a real LAN | **Not tested** |
| Installer | Built, size/hash/version/signature inspected | Built and inspected; **not installed or uninstalled** |

## Test results (latest, 2026-10-17)

| Suite | Result |
| --- | --- |
| Rust `veydrop-core` (`cargo test -p veydrop-core`) | 68 unit + 22 real-HTTP integration tests passing |
| Desktop vitest + `tsc --noEmit` | 34/34, type-check clean |
| Mobile vitest + `tsc` + production build | 68/68, type-check clean, build OK |
| Playwright e2e, full suite (39 tests x 2 profiles = 78) | **77/78 in each of 3 full runs** (not a clean pass; see below) |
| `send.spec.ts` repeated on WebKit alone | 12 runs, 84/84 |
| `receive.spec.ts` repeated on WebKit alone | 10 runs, 70/70 |
| Localization parity | Covered by vitest: every phone key exists in English and Arabic (123/123) and every desktop key (319/319); unit tests cover both languages |

Pair-code consistency (ASCII digits, LTR, isolated, leading zeros, bound to its pairing attempt, never in Arabic-Indic digits) and desktop-to-phone language sync (including a live change and an Arabic desktop) are covered in the Rust integration tests, the vitest suites and the Playwright suite.

The Playwright tests run the phone app against the real Rust engine through `crates/veydrop-core/examples/e2e_harness.rs`, test tooling that stands in for the person at the PC. It is not part of the shipped app.

Full-run logs kept for reference: `e2e/final-run.log` (run 2) and `e2e/final-run3.log` (run 3).

## Installer

- Path: `target/release/bundle/nsis/Veydrop_1.0.0_x64-setup.exe`
- Size: 3,407,392 bytes (about 3.25 MiB)
- Built: 2026-10-17 02:17:26
- SHA-256: `BC4FEDB6192B6EC600960DA67C5B00EBB97881C435BC3E0E232031AC90AF5436`
- Product version 1.0.0; Authenticode status **NotSigned** (SmartScreen will warn).
- Not installed or uninstalled in isolation; no silent install was done on the development machine.

## Known issues

- **Unresolved intermittent WebKit-profile e2e failure.** In three full-suite runs exactly one WebKit test failed each time, and it was not always the same test: `send.spec.ts:70` once ("Connection ended" shown in the page) and `receive.spec.ts:62` twice (the "Download selected as ZIP" test; the page showed "Connection to the PC was lost. Retrying." and `waitForEvent("download")` timed out). Chromium never failed. Repeating either file on its own on WebKit passed every time (12x and 10x). In the run-3 trace the failing page's `/api/events` stream was aborted and the page then re-requested `/api/session`. The trace bundle also contained several extra pages, but their timestamps show they belong to earlier tests in the same worker, so they are not an explanation. The root cause was **not found**. It is unknown whether it is a Veydrop bug or a quirk of the emulated WebKit build (one unverified suspicion is how that build handles a navigation that turns into a download while an event stream is open). It is not masked with retries, and the suite is not claimed to be green. The physical-device checklist covers the same download path on a real iPhone.
- Playwright WebKit reports the session cookie as `SameSite=None`; `SameSite=Strict` is asserted in the Rust tests.
- See [LIMITATIONS](docs/LIMITATIONS.md) for the platform limits (plain HTTP, no silent saves, background throttling, stored ZIP only, minimum browser versions, unsigned installer).

## iPhone connection investigation (2026-10-17)

Report: Safari on a real iPhone shows "server stopped responding" while the site works on the PC (PC 10.0.0.25, iPhone 10.0.0.4).

Findings on the dev PC (read-only inspection):
- The server was healthy: listening on 10.0.0.25:47821 only (not loopback), `curl` to that address returned 200 with valid HTML, and no panics or exits were found. The chosen adapter was correct.
- Windows network profile is **Public**, the firewall is on, and **no inbound allow rule** exists for Veydrop or the port. A Public profile with no allow rule drops incoming SYNs from other devices, which is the classic cause of Safari's timeout-style error. A PC reaching its own LAN address bypasses the firewall, so the earlier local success could not show this.
- This is the most probable cause, but it is **not confirmed on a physical iPhone**.

Changes:
- Reach tracker: only requests from another address count as "a device reached the server".
- Diagnostics (`engine.diagnostics`, `diag.rs`, `DiagnosticsPanel.tsx`) with a "Run diagnostics" button: server state, bound address:port, adapter, real local HTTP check, Windows profile, firewall evaluation (read-only), pairing status, external reach.
- Copy-only, narrowly scoped firewall command (program + TCP 47821-47900 + LocalSubnet + current profile). Never executed by the app; Veydrop never changes Windows Firewall.
- Server re-binds when the PC's address changes; "Restart server" and manual adapter selection added.

## Completed

- **Engine (`crates/veydrop-core`)**: pairing with one-time tokens and PC approval, sessions, rate limiting and lockout, Axum server with Host/Origin/header guards and CSP, chunked resumable SHA-256-verified uploads, range downloads and streamed (stored) ZIP, shares, history, settings, network detection and diagnostics.
- **Language sync**: the PC is the single source of truth (`system|en|ar`, resolved to `en|ar`). The phone receives the language at pairing, in the session response and via a live `language` event, and has no language selector of its own.
- **Pairing code**: always ASCII digits, left-to-right, isolated from surrounding RTL text, tabular, leading zeros kept, and tied to a single pairing request id so approval always applies to the request being shown.
- **Desktop shell (`desktop/src-tauri`)**: Tauri 2 commands, tray, close-to-tray dialog, notifications, autostart, single instance, graceful shutdown, NSIS configuration, original icons.
- **Desktop UI (`desktop/src`)**: Dashboard, Connect Device, Send Files, Receive Files, Activity, Settings; English and Arabic with RTL; dark, light and system themes.
- **Phone web app (`mobile/`)**: Send to PC, Receive from PC, safe-area support, RTL, built into `desktop/src-tauri/resources/mobile` and served by Rust.
- **Docs**: [README](README.md) (English only), [SECURITY](docs/SECURITY.md), [LIMITATIONS](docs/LIMITATIONS.md), [TROUBLESHOOTING](docs/TROUBLESHOOTING.md), [DEVICE_TEST_CHECKLIST](docs/DEVICE_TEST_CHECKLIST.md).
- **README assets**: `docs/screenshots/` holds nine desktop captures (real release build, sample data, LAN address and token masked) and six phone captures (browser emulation); `docs/demo/` holds `pairing-flow.gif` and `language-sync.gif`, built from those captures.

## Not done

- **No physical-device testing.** Use [docs/DEVICE_TEST_CHECKLIST.md](docs/DEVICE_TEST_CHECKLIST.md).
- The installer has not been installed, run or uninstalled in isolation.
- The WebKit e2e flake above is unresolved.
- Not code signed; no independent security audit; no `LICENSE` file has been added.
- Minor cleanup: unused `pub fn weak()` in `engine.rs`.

## Commands

```bash
cargo test -p veydrop-core
(cd desktop && npm test && npx tsc --noEmit -p .)
(cd mobile && npm test && npm run build)
(cd e2e && npm test)
(cd desktop && npx tauri build)
```

Rebuild the mobile bundle (`npm run build` in `mobile/`) after any mobile source change, because the e2e tests serve the built bundle. Do not run cargo and the e2e suite at the same time as a build.
