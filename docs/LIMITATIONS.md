# Known limitations

Veydrop 1.0.0 is honest about what it cannot do. These come from the web platform, from the design, or from what has and has not been tested.

## Testing coverage

- **No physical iPhone or Android device has been used.** All phone-side testing was done with Playwright emulation: a Pixel 7 profile on Chromium and an iPhone 14 profile on Playwright's WebKit build. Those are not real devices, and Playwright WebKit is not iOS Safari. Run the [device checklist](DEVICE_TEST_CHECKLIST.md).
- The installer has been **built and inspected** (size, hash, version metadata, unsigned) but **not run through an install/uninstall cycle** in an isolated environment. The desktop screenshots in the README are real captures of the release `veydrop.exe` running from a scratch folder with sample data; the phone screenshots are browser emulation, not a physical phone.
- **One intermittent end-to-end failure** has been seen on the emulated WebKit profile only, in full-suite runs (77/78 in each of 3 runs): `send.spec.ts` failed once and `receive.spec.ts` (the ZIP download test) twice; the page showed "Connection ended" or "Connection to the PC was lost". Repeating `send.spec.ts` 12 times and `receive.spec.ts` 10 times on WebKit on their own passed every run. The cause has not been found, so it is not known whether it is a bug in Veydrop or an artifact of the emulated WebKit build. It is deliberately not hidden with retries. See [DEVELOPMENT_STATUS](../DEVELOPMENT_STATUS.md) for the exact counts.
- The e2e harness that approves pairing and accepts offers is test tooling only and is not part of the shipped app.
- Not an independent security audit.

## Network

- **Plain HTTP, no encryption** on the LAN. See [SECURITY.md](SECURITY.md).
- Phone and PC must be on the same reachable network. Guest Wi-Fi, "AP/client isolation", some corporate or public networks, and VPNs can block it. There is no relay or internet mode.
- IPv4 only for pairing URLs.
- If the PC's IP address changes (sleep, roaming, new network), the QR code and any phone session break and you must reconnect.
- Veydrop does not create firewall rules. If Windows blocks incoming connections, you must allow Veydrop yourself.

## Phone browser

- Needs a modern browser: roughly **iOS/Safari 16.4+** or **Chrome 111+** (Tailwind 4 requirements). Older browsers may render incorrectly or not at all.
- **No silent saves.** A browser decides where downloads go; Veydrop cannot save into a chosen folder or the gallery. On iOS, files appear in Safari's downloads / Files; saving photos to the camera roll needs an extra step.
- **Web Share API with files requires a secure context**, so it is unavailable over plain HTTP. Veydrop uses ordinary downloads.
- **Background throttling:** iOS and Android suspend pages and network access when the browser is backgrounded or the screen locks. Keep the page open and the screen on during large transfers; an interrupted transfer resumes when the page returns.
- **Resume works only within a live session and open page.** Closing the tab loses the phone's file handles (browsers do not allow re-reading previously picked files); you must pick the files again.
- **Downloads cannot be verified on the phone.** The browser handles the download; the PC can only report that it served the bytes. Upload (phone to PC) is SHA-256 verified; download is not verified client-side.
- Folder selection (`webkitdirectory`) varies by browser; on some phones only individual files can be chosen.
- Very large single files depend on the phone's memory and browser limits.

## Transfers

- Uploads from the phone are limited by the optional maximum file size and by free disk space.
- ZIP downloads are "stored" (uncompressed), capped at 5,000 items and below 4 GiB total; bigger selections must be downloaded file by file.
- Files that change on the PC after you added them to the Send list fail rather than being served inconsistently.
- Up to 8 phones can be signed in at once; transfers are not bandwidth-throttled.
- Unfinished uploads are kept for a configurable time (default 24 hours) and then deleted.

## Desktop

- Windows 10/11 x64 only. The installer is **unsigned** (SmartScreen warning) and installs for the current user only.
- Closing the window hides Veydrop to the tray by default (configurable); Quit stops the server and removes temporary files.
- Languages: English and Arabic.
