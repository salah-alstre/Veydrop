//! Local network discovery: which addresses can a phone reach, which one is
//! the best guess, and an event-driven (no polling) address-change signal.

use std::net::{IpAddr, Ipv4Addr};
use std::sync::OnceLock;

use tokio::sync::broadcast;

use crate::model::NetIface;

const VIRTUAL_HINTS: &[&str] = &[
    "vethernet", "vmware", "virtualbox", "vbox", "hyper-v", "wsl", "docker", "vmnet", "loopback",
    "bluetooth", "pseudo", "host-only", "npcap",
];
const VPN_HINTS: &[&str] = &[
    "tailscale", "zerotier", "tap-", "tap ", "tun", "wireguard", "openvpn", "nordlynx", "vpn",
    "proton", "hamachi", "cisco", "anyconnect", "fortinet", "globalprotect",
];
const WIFI_HINTS: &[&str] = &["wi-fi", "wifi", "wlan", "wireless", "802.11"];

fn is_cgnat(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 100 && (64..=127).contains(&o[1])
}

fn is_private_v4(ip: Ipv4Addr) -> bool {
    ip.is_private()
}

/// Scores one adapter. Higher is better. Pure so it can be unit-tested.
pub fn classify(name: &str, ip: Ipv4Addr) -> NetIface {
    let lname = name.to_lowercase();
    let o = ip.octets();
    let is_virtual = VIRTUAL_HINTS.iter().any(|h| lname.contains(h));
    let is_vpn = VPN_HINTS.iter().any(|h| lname.contains(h)) || is_cgnat(ip);
    let is_wifi = WIFI_HINTS.iter().any(|h| lname.contains(h));
    let hotspot = o[0] == 192 && o[1] == 168 && o[2] == 137;
    let private = is_private_v4(ip);

    let kind = if hotspot {
        "hotspot"
    } else if is_vpn {
        "vpn"
    } else if is_virtual {
        "virtual"
    } else if is_wifi {
        "wifi"
    } else if lname.contains("ethernet") || lname.starts_with("eth") || lname.starts_with("en") {
        "ethernet"
    } else {
        "other"
    };

    let mut score = 0i32;
    if private {
        score += 50;
        if o[0] == 192 && o[1] == 168 {
            score += 20;
        } else if o[0] == 10 {
            score += 12;
        } else {
            score += 6; // 172.16/12 — also what Hyper-V/WSL use, so least preferred
        }
    }
    if hotspot {
        score += 4;
    }
    if is_wifi {
        score += 6;
    }
    if kind == "ethernet" {
        score += 5;
    }
    if is_virtual {
        score -= 45;
    }
    if is_vpn {
        score -= 45;
    }
    if is_cgnat(ip) {
        score -= 20;
    }
    NetIface {
        name: name.to_string(),
        ip: ip.to_string(),
        kind: kind.to_string(),
        is_private: private,
        is_virtual: is_virtual || is_vpn,
        score,
        recommended: false,
    }
}

/// Lists usable IPv4 interfaces, best first, with exactly one `recommended`.
/// `preferred` (a user override) wins if it is still present.
pub fn list_interfaces(preferred: Option<&str>) -> Vec<NetIface> {
    let mut out: Vec<NetIface> = match if_addrs::get_if_addrs() {
        Ok(v) => v
            .into_iter()
            .filter(|i| !i.is_loopback())
            .filter_map(|i| match i.addr.ip() {
                IpAddr::V4(ip) if !ip.is_link_local() && !ip.is_unspecified() => Some(classify(&i.name, ip)),
                _ => None,
            })
            .collect(),
        Err(e) => {
            tracing::warn!("could not enumerate network interfaces: {e}");
            vec![]
        }
    };
    out.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.ip.cmp(&b.ip)));
    out.dedup_by(|a, b| a.ip == b.ip);
    let pick = preferred
        .and_then(|p| out.iter().position(|i| i.ip == p))
        .or_else(|| if out.is_empty() { None } else { Some(0) });
    if let Some(i) = pick {
        out[i].recommended = true;
    }
    out
}

/// Every IP address assigned to this PC (loopback included). A request whose
/// peer address is in this set came from the PC itself, which says nothing
/// about whether another device can reach the server.
pub fn local_ips() -> std::collections::HashSet<IpAddr> {
    match if_addrs::get_if_addrs() {
        Ok(v) => v.into_iter().map(|i| i.addr.ip()).collect(),
        Err(e) => {
            tracing::warn!("could not enumerate network interfaces: {e}");
            Default::default()
        }
    }
}

pub fn ip_present(ip: &str) -> bool {
    list_interfaces(None).iter().any(|i| i.ip == ip)
}

static CHANGES: OnceLock<broadcast::Sender<()>> = OnceLock::new();

/// Subscribes to "some IP address changed on this PC". Backed by a single
/// process-wide thread blocked in the OS (`NotifyAddrChange`) so it costs no
/// CPU while nothing changes. On non-Windows builds it never fires.
pub fn watch_changes() -> broadcast::Receiver<()> {
    CHANGES
        .get_or_init(|| {
            let (tx, _rx) = broadcast::channel(8);
            #[cfg(windows)]
            {
                let tx2 = tx.clone();
                let _ = std::thread::Builder::new().name("veydrop-addr-watch".into()).spawn(move || {
                    use windows_sys::Win32::NetworkManagement::IpHelper::NotifyAddrChange;
                    loop {
                        // SAFETY: null handle + null overlapped = synchronous wait.
                        let rc = unsafe { NotifyAddrChange(std::ptr::null_mut(), std::ptr::null()) };
                        if rc != 0 {
                            // API unavailable/failed: stop rather than spin.
                            tracing::warn!("NotifyAddrChange failed ({rc}); address-change detection disabled");
                            break;
                        }
                        // Let the stack settle, then notify (send errors just mean no listeners).
                        std::thread::sleep(std::time::Duration::from_millis(400));
                        let _ = tx2.send(());
                    }
                });
            }
            tx
        })
        .subscribe()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(a: u8, b: u8, c: u8, d: u8) -> Ipv4Addr {
        Ipv4Addr::new(a, b, c, d)
    }

    #[test]
    fn prefers_real_lan_over_virtual() {
        let wifi = classify("Wi-Fi", ip(192, 168, 1, 20));
        let hv = classify("vEthernet (Default Switch)", ip(172, 25, 0, 1));
        let vb = classify("VirtualBox Host-Only Network", ip(192, 168, 56, 1));
        let ts = classify("Tailscale", ip(100, 101, 2, 3));
        assert!(wifi.score > hv.score);
        assert!(wifi.score > vb.score);
        assert!(wifi.score > ts.score);
        assert!(hv.is_virtual && vb.is_virtual && ts.is_virtual);
        assert!(!wifi.is_virtual);
    }

    #[test]
    fn detects_hotspot() {
        assert_eq!(classify("Local Area Connection* 10", ip(192, 168, 137, 1)).kind, "hotspot");
    }

    #[test]
    fn listing_marks_exactly_one_recommended_when_non_empty() {
        let l = list_interfaces(None);
        if !l.is_empty() {
            assert_eq!(l.iter().filter(|i| i.recommended).count(), 1);
        }
    }
}
