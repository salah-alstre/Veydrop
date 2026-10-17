# Manual real-device test checklist

Veydrop has **not** been tested on physical phones. Use this checklist before trusting a release. Record device model, OS/browser version, result, and notes for every row.

Test environment: Windows 10/11 x64 PC with Veydrop installed from `Veydrop_1.0.0_x64-setup.exe`, a home Wi-Fi network, and each phone on the same network.

Devices to cover at minimum: one iPhone (Safari, iOS 16.4+), one Android phone (Chrome 111+). Also try an older or alternative browser (Firefox, Samsung Internet) if you want to confirm the documented limits.

## Installer and desktop
- [ ] SmartScreen shows the expected unsigned warning; install completes without admin rights.
- [ ] App starts, splash then Dashboard; no server is listening yet (`netstat -ano` shows no Veydrop port).
- [ ] The phone has no language picker; Settings > Language only states the language chosen on the PC.
- [ ] With the PC set to Arabic, a freshly scanned phone shows Arabic/RTL on the waiting screen and after pairing, even if the phone's own language is English (and the reverse).
- [ ] While paired, switch the PC language English -> Arabic -> English: the phone changes live, with no QR rescan, no reload and no disconnect. Repeat during a running upload: it must still finish.
- [ ] Close and reopen Safari/Chrome tab, or lock/unlock the phone: it comes back in the PC's current language.
- [ ] The 4-digit pairing code is the same Western-digit number (0-9) on the PC dialog and the phone, in English and in Arabic. Try several attempts; reject one and confirm the other still shows its own code.
- [ ] Theme: System, Dark, Light all render correctly.
- [ ] Closing the window hides to tray; tray menu Open Veydrop / Stop sharing / Quit Veydrop work.
- [ ] Quit while connected stops the server and the phone shows "Connection ended".
- [ ] Uninstall removes the app and leaves received files.

## Connect
- [ ] Start connection; Windows Firewall prompt appears (it should; Veydrop does not alter rules). Allow on Private. If no prompt appeared and the network is Public, expect the phone to time out until you add the scoped rule shown under Connection diagnostics (copy-only; you run it as administrator).
- [ ] Phone on the same Wi-Fi, mobile data off: open `http://<PC address>:<port>` from the Connect screen in Safari. Then on the PC click **Run diagnostics**: "Reached by <phone IP>" must appear. If Safari fails, the PC must still say "No other device has reached the server yet" (no false success).
- [ ] Phone camera scans the QR and opens the page. Typing the printed address also works.
- [ ] The 4-digit code matches on both screens. Reject on the PC shows a clear message on the phone.
- [ ] Approve on the PC; the phone shows Connected.
- [ ] Reusing the same QR after pairing fails ("already used"). An expired QR fails cleanly. New code works.
- [ ] A second phone can pair; both listed under Connected phones. Disconnect one from the PC; only that phone ends.
- [ ] Reload the phone page: session persists. Close the tab and reopen the address without the QR: asks to pair again (or resumes only if cookie still valid).
- [ ] Phone on mobile data cannot connect. Phone on a different Wi-Fi network cannot connect.

## Phone to PC
- [ ] Send one small photo; PC prompts Accept; file arrives in the chosen folder with correct size and opens correctly.
- [ ] Send several files (mixed photos, video, PDF); progress is real and the final state matches what is on disk.
- [ ] Send a file larger than 1 GB: completes, SHA-256 of source and destination match (`Get-FileHash`).
- [ ] Send a folder (where the browser supports it): structure preserved.
- [ ] Reject an offer on the PC: the phone shows rejected; nothing is written.
- [ ] Cancel from the phone and from the PC mid-transfer: no partial file remains in the destination.
- [ ] Pause and resume; lock the phone screen briefly and unlock: transfer recovers or fails visibly, never shows success falsely.
- [ ] Turn Wi-Fi off on the phone mid-transfer and back on: behavior is clear and honest.
- [ ] Send two files with the same name: both kept (numbered).
- [ ] Send with a file name containing Arabic, emoji, spaces, very long names: saved safely.
- [ ] Fill the destination drive (or set a max file size): clear error.

## PC to phone
- [ ] Add files and a folder in Send Files; they appear on the phone's Receive list, nothing else does.
- [ ] Download a single file: it saves and opens; compare hash with the source.
- [ ] Download several as ZIP: archive opens, contents match.
- [ ] Remove an item on the PC: it disappears from the phone.
- [ ] Stop sharing: the phone shows it stopped.
- [ ] Modify a shared file on the PC while the phone is downloading: download fails visibly or the original is served, never silent corruption.
- [ ] Where do downloads land on iOS and Android? Note the location and whether it is clear to the user.

## Security checks
- [ ] From another device on the LAN without a session, open `http://<pc-ip>:<port>/api/share`: 401.
- [ ] Open the address using a different Host name or the PC's other IP: rejected (421) or no access.
- [ ] Revoke a phone on the PC: the phone loses access at once.
- [ ] With a packet capture on the LAN, confirm traffic is plain HTTP (expected, documented).
- [ ] Windows Defender Firewall rules list: Veydrop created none on its own.

## Resilience
- [ ] PC sleeps and wakes, or Wi-Fi switches: phone shows connection lost; reconnect works after a new QR.
- [ ] Keep the app open for several hours: no memory growth, session expires at the configured time.
- [ ] Windows 10 and Windows 11, 100% and 150% display scaling, high-contrast mode, keyboard-only navigation of the desktop app.

Sign-off: tester ______ date ______ app version 1.0.0 build ______
