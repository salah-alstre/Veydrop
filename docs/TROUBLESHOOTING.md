# Troubleshooting

## The phone cannot open the address

Start with **Connect Device > Connection diagnostics > Run diagnostics**. It separates two very different things:

- **Local check** (the PC requests its own page). This only proves the server is running and returning the phone web app. Windows never applies the firewall to a PC talking to itself, so it says nothing about whether a phone can connect.
- **Reached by another device.** This turns green only after a request really arrives from another address (your phone). Until then Veydrop shows "No other device has reached the server yet". It is never faked.

Typical symptom: Safari says "the server stopped responding" or times out, while the page works on the PC. Almost always Windows is silently dropping the phone's connection because the network is *Public* and no inbound rule allows Veydrop. Diagnostics shows the profile, whether the firewall is on, and whether an inbound allow rule matches Veydrop or the port (read-only queries; Veydrop never changes Windows settings).

1. **Same network?** The phone must be on the same Wi-Fi as the PC, not on mobile data. Turn mobile data off to be sure. Both addresses should share a subnet (for example 10.0.0.25 and 10.0.0.4).
2. **Firewall.** If no rule matches, diagnostics shows a copy-only PowerShell command that allows only this program, only TCP ports 47821-47900, only from the local subnet, only on your current network profile. Run it yourself in an *administrator* PowerShell if you agree. Veydrop never runs it and never disables the firewall. Alternatively use *Windows Security > Firewall & network protection > Allow an app through firewall*.
3. **Network profile.** If Windows treats your Wi-Fi/Ethernet as *Public*, it blocks incoming connections. Setting it to *Private* is your decision and only appropriate on a network you trust.
4. **Wrong adapter.** On Connect Device, pick the network the phone can reach (usually Wi-Fi). VPN, Hyper-V, WSL, and VirtualBox adapters are not reachable from a phone. The page warns when the selected adapter is virtual or not in a private range.
5. **Isolation.** Guest Wi-Fi and some routers ("AP isolation", "client isolation") stop devices talking to each other. Use the main network or a phone hotspot.
6. **VPN.** Turn off VPNs on the phone and the PC while transferring.
7. **Typing the address.** Use exactly what Connect Device shows, including `http://` and the port.

## "Pairing code expired" or "already used"

The QR token is single use and short lived (default 3 minutes). Press **New code** on the PC and scan again.

## The phone says "Waiting for approval" and nothing happens

Approve on the PC (Veydrop shows a dialog and, if enabled, a notification). Requests time out after 90 seconds. Compare the 4-digit code on both screens first.

## "Too many attempts"

The PC rate-limits repeated bad requests from one device and locks it out for two minutes. Wait and try again.

## "Connection ended"

The session was revoked, expired (default 240 minutes), or the PC stopped the connection or changed network address. Scan a fresh QR code.

## Port already in use

Set a different preferred port in Settings (or 0 for automatic), then start the connection again.

## Transfer stalls or is slow

- Keep the phone screen on and the browser in the foreground. iOS and Android pause background pages.
- Move closer to the router or use 5 GHz Wi-Fi.
- In Settings, try a smaller upload chunk size on weak Wi-Fi, a larger one on good Wi-Fi.

## "The file arrived damaged" (checksum mismatch)

The data changed or was corrupted in transit, so Veydrop discarded it rather than saving a bad file. Retry the transfer. If it repeats, check the network and whether the source file is still being modified.

## "Not enough free disk space" / "File larger than the limit"

Free space on the destination drive, choose another folder, or raise the maximum file size in Settings.

## Download on the phone did not save where expected

The phone's browser decides where downloads go (see [Limitations](LIMITATIONS.md)). On iOS check Safari's Downloads and the Files app.

## The ZIP download is refused

Selections of 4 GiB or more, or more than 5,000 items, cannot be zipped. Download files individually.

## SmartScreen warns about the installer

The installer is unsigned. Click *More info > Run anyway* only if you built it yourself or trust where you got it.

## Where is my data?

Settings, history and unfinished temporary files live in Veydrop's app data folder (shown in Settings > About). Received files go to the download location from Settings. Uninstalling does not delete received files.
