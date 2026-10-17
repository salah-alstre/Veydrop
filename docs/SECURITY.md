# Veydrop security

Veydrop opens a small web server on your PC so a phone can exchange files with it. This document says what is protected, how, and what is **not** protected.

## The most important limitation: no encryption on the LAN

The phone talks to the PC over **plain HTTP**. Nothing on the wire is encrypted or authenticated at the transport level. Anyone who can observe traffic on the same network (for example on an open or shared Wi-Fi network, or from a compromised router) can read the files and the session cookie and could hijack a session.

Veydrop does not hide this: the Connect Device page and Settings > About both say so. Mitigations are operational: use a network you trust (home Wi-Fi with a good password, or your own phone hotspot), keep connections short, and disconnect when done. A browser cannot be given a trusted TLS certificate for a private IP address without installing a certificate on the phone, so HTTPS is not offered in 1.0.

## Threat model

| In scope | Out of scope |
| --- | --- |
| A stranger on the same network who finds the server's port and tries to use it | A passive or active attacker who can read or modify LAN traffic (plain HTTP) |
| A web page in some other browser tab trying to reach the server (CSRF, DNS rebinding) | A compromised PC or phone |
| A paired phone sending malicious file names or huge/odd ZIP requests | Malware inside files you choose to accept (Veydrop does not scan files) |
| Guessing the pairing token or session token | Someone physically holding an unlocked, paired phone |

## What Veydrop does

**Network exposure**
- The server starts only when you press *Start connection* and stops when you stop it or quit. It binds **only to the IP of the interface you choose**, not to all interfaces.
- Port order: your preferred port, then 47821–47900, then any free port.
- Veydrop never creates firewall rules, never turns Windows Defender Firewall off, and never changes network profiles. If Windows asks, you decide.

**Pairing**
- The QR code holds a one-time token (16 random bytes) in the URL *fragment*, which browsers do not send to servers or put in logs. The PC stores only its SHA-256 hash. Default lifetime 180 seconds (configurable 30–900), single use.
- Presenting the token creates a *pending request*. A person must approve it on the PC, after comparing a 4-digit code shown on both screens. Requests time out after 90 seconds. At most 3 requests may be pending and at most 8 sessions active.
- The 4-digit code is always ASCII `0-9` (leading zeros kept) in every language and on both screens. It is bound to one pairing attempt: the PC's approval dialog and the phone's waiting screen show the code of the same request id, decisions resolve strictly by that id, and rejecting or expiring one attempt can never approve another. Approve only if the numbers match.
- After 10 bad token guesses in total, the current QR token is destroyed.
- Approval creates a session token (32 random bytes). Only its hash is kept on the PC. It reaches the phone once, as a cookie.

**Sessions**
- Cookie `vd_s`: `HttpOnly` (page scripts cannot read it) and `SameSite=Strict`. No `Secure` flag, because the transport is HTTP.
- Sessions expire (default 240 minutes, configurable). Stopping the server or disconnecting a phone revokes sessions immediately and the phone is told.
- Optional "remember approved phones" lets a trusted phone reconnect without a fresh code comparison. Turn it off in Settings if you do not want that.

**Language**
- The language is chosen on the PC only. It reaches the phone as one of two fixed values (`en` or `ar`) in the pairing/session/event responses and is ignored by the phone if it is anything else. The phone cannot set, report or influence it, so it adds no input the PC trusts.

**Request checks**
- The `Host` header must match the address Veydrop is serving (wrong host: HTTP 421). This blocks DNS-rebinding.
- `Origin` is checked, and every state-changing request must carry a custom header (`x-veydrop: 1`), so a foreign web page cannot trigger actions.
- Responses carry a strict Content-Security-Policy (`default-src 'self'`, `frame-ancestors 'none'`, and related directives).
- Rate limiting per remote IP (40 requests per minute on the pairing and authenticated endpoints); 5 requests with a missing or invalid session cookie within two minutes lock that IP out for two minutes.

**File safety**
- Incoming names are sanitized on the PC: path separators, `..`, absolute and drive-letter paths, reserved Windows names (CON, NUL, COM1…), trailing dots/spaces and invalid characters are neutralized, and the final path is verified to stay inside the destination folder. Existing files are never overwritten; duplicates get a numbered name.
- Incoming data is written to a temporary partial file, hashed with SHA-256 while streaming, and only moved into place if the hash the phone declared matches. A mismatch deletes the data and reports failure.
- Free disk space is checked before accepting a batch; an optional maximum file size is enforced; chunks are capped at 32 MiB; an offer may list at most 10,000 files and a session may have at most 5 pending offers.
- The phone can only see items you explicitly put on the *Send Files* list. File paths are never exposed to the phone, only opaque ids. If a shared file changes on disk after it was added, the download fails rather than serving inconsistent data.
- ZIP downloads are streamed, limited to 5,000 items per archive and under 4 GiB in total (no ZIP64; larger selections are refused and must be downloaded file by file), and entry names are sanitized.

**Privacy**
- Nothing leaves your network. No telemetry, no accounts, no analytics, no update pings.
- Tokens and cookies are never written to logs.
- Transfer history is stored locally in the app data folder and can be cleared in Activity or Settings.

## Test status

The rules above are covered by 68 Rust unit tests and 22 real-HTTP integration tests (unauthorized access, Host/Origin/header guards, lockout, one-time and expired tokens, revocation, traversal, hash mismatch, cross-session isolation, ZIP, changed files, concurrent pairing attempts, language sync) plus Playwright end-to-end tests. This is not an independent security audit. No penetration test has been done.

## Installer is unsigned

`Veydrop_1.0.0_x64-setup.exe` is not code signed, so Windows SmartScreen shows an "unknown publisher" warning. If you obtained it from anywhere other than your own build, do not run it. Build from source and compare hashes yourself.

## Reporting a problem

If you find a vulnerability, please report it privately to the project maintainer instead of opening a public issue.
