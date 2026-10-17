//! Read-only network diagnostics.
//!
//! Nothing here changes Windows settings. Two different questions are kept
//! apart on purpose:
//!
//! * *Can this PC reach its own server?* ([`local_http_check`]) — useful to
//!   prove the server is alive and speaking HTTP, but it never crosses the
//!   firewall, so it says nothing about a phone.
//! * *Did another device reach it?* — only the server's own request log can
//!   answer that (see `server::Reach`).
//!
//! The firewall/profile report is a reading of the rules Windows reports; it
//! is labelled as such and never presented as proof.

use std::net::{IpAddr, SocketAddr};
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ---- local HTTP self-check ---------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LocalCheck {
    /// The server answered a real HTTP request on its bound address.
    pub ok: bool,
    pub url: String,
    pub status: Option<u16>,
    pub millis: Option<u64>,
    /// The phone web app (`/`) was served as HTML.
    pub page_ok: bool,
    pub error: Option<String>,
}

async fn http_get(addr: SocketAddr, path: &str) -> Result<(u16, String), String> {
    let mut s = tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(addr))
        .await
        .map_err(|_| "timed out connecting".to_string())?
        .map_err(|e| e.to_string())?;
    let req = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nAccept: */*\r\n\r\n");
    s.write_all(req.as_bytes()).await.map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let read = async {
        let mut chunk = [0u8; 4096];
        loop {
            let n = s.read(&mut chunk).await?;
            if n == 0 || buf.len() > 64 * 1024 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        std::io::Result::Ok(())
    };
    tokio::time::timeout(Duration::from_secs(3), read)
        .await
        .map_err(|_| "timed out waiting for a response".to_string())?
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let status = text
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(|| "the reply was not valid HTTP".to_string())?;
    Ok((status, text))
}

/// Opens a real TCP connection to the server's own bound address and checks it
/// answers HTTP. A success only proves the server process is alive.
pub async fn local_http_check(ip: IpAddr, port: u16) -> LocalCheck {
    let addr = SocketAddr::new(ip, port);
    let started = Instant::now();
    let mut out = LocalCheck { url: format!("http://{addr}/"), ..Default::default() };
    match http_get(addr, "/api/info").await {
        Ok((status, body)) => {
            out.status = Some(status);
            out.millis = Some(started.elapsed().as_millis() as u64);
            out.ok = status == 200 && body.contains('{');
            if !out.ok {
                out.error = Some(format!("unexpected HTTP status {status}"));
            }
        }
        Err(e) => out.error = Some(e),
    }
    if out.ok {
        out.page_ok = matches!(http_get(addr, "/").await, Ok((200, b)) if {
            let b = b.to_ascii_lowercase();
            b.contains("<html") || b.contains("<!doctype html")
        });
    }
    out
}

// ---- Windows network profile and firewall ------------------------------------

fn one_or_many<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Option::<V>::deserialize(d)? {
        None => vec![],
        Some(V::One(s)) => vec![s],
        Some(V::Many(v)) => v,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FwRule {
    pub name: String,
    pub action: String,
    pub profile: String,
    pub program: String,
    pub protocol: String,
    #[serde(deserialize_with = "one_or_many")]
    pub local_port: Vec<String>,
    #[serde(deserialize_with = "one_or_many")]
    pub remote_address: Vec<String>,
}

/// Raw facts from Windows, as reported by a read-only PowerShell query.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WinNet {
    pub alias: Option<String>,
    /// "Public" | "Private" | "DomainAuthenticated"
    pub category: Option<String>,
    /// "True" | "False" | "NotConfigured" for the profile that applies.
    pub fw_enabled: Option<String>,
    /// "Allow" | "Block" | "NotConfigured"
    pub default_inbound: Option<String>,
    #[serde(deserialize_with = "rules_vec")]
    pub rules: Vec<FwRule>,
    pub rules_ok: bool,
    pub error: Option<String>,
}

fn rules_vec<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FwRule>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum V {
        One(FwRule),
        Many(Vec<FwRule>),
    }
    Ok(match Option::<V>::deserialize(d)? {
        None => vec![],
        Some(V::One(r)) => vec![r],
        Some(V::Many(v)) => v,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FirewallReport {
    /// "allowed" | "blocked" | "no_rule" | "off" | "unknown" — a reading of
    /// Windows' rules, not proof that a phone can connect.
    pub state: String,
    pub profile: Option<String>,
    pub matching_allow: Vec<String>,
    pub matching_block: Vec<String>,
    pub exe: Option<String>,
    /// Shown to the user to copy; Veydrop never runs it.
    pub suggested_command: Option<String>,
}

fn profile_name(category: &str) -> &'static str {
    match category {
        "Public" => "Public",
        "Private" => "Private",
        _ => "Domain",
    }
}

fn port_matches(list: &[String], port: u16) -> bool {
    if list.is_empty() {
        return true;
    }
    list.iter().any(|e| {
        let e = e.trim();
        if e.eq_ignore_ascii_case("any") {
            return true;
        }
        match e.split_once('-') {
            Some((a, b)) => {
                matches!((a.trim().parse::<u16>(), b.trim().parse::<u16>()), (Ok(a), Ok(b)) if a <= port && port <= b)
            }
            None => e.parse::<u16>().is_ok_and(|p| p == port),
        }
    })
}

fn protocol_is_tcp(p: &str) -> bool {
    let p = p.trim();
    p.is_empty() || p.eq_ignore_ascii_case("any") || p.eq_ignore_ascii_case("tcp") || p == "6"
}

fn profile_matches(rule_profile: &str, profile: &str) -> bool {
    let r = rule_profile.trim().to_ascii_lowercase();
    r.is_empty() || r.contains("any") || r.contains(&profile.to_ascii_lowercase())
}

fn norm_path(p: &str) -> String {
    p.trim().trim_matches('"').replace('/', "\\").to_ascii_lowercase()
}

/// The command a user could run, as Administrator, to allow Veydrop in. It is
/// scoped to this program, this port range, the local subnet and the one
/// profile in use.
pub fn suggested_rule(exe: &str, port: u16, profile: &str) -> String {
    let ports = if (47821..=47900).contains(&port) { "47821-47900".to_string() } else { port.to_string() };
    format!(
        "New-NetFirewallRule -DisplayName \"Veydrop (local transfers)\" -Direction Inbound -Action Allow -Protocol TCP -LocalPort {ports} -Program \"{exe}\" -RemoteAddress LocalSubnet -Profile {profile}"
    )
}

/// Reads Windows' reported rules for this program/port/profile.
pub fn evaluate(win: &WinNet, exe: Option<&str>, port: u16) -> FirewallReport {
    let profile = win.category.as_deref().map(profile_name);
    let mut rep = FirewallReport {
        state: "unknown".into(),
        profile: profile.map(str::to_string),
        matching_allow: vec![],
        matching_block: vec![],
        exe: exe.map(str::to_string),
        suggested_command: None,
    };
    let (Some(profile), true) = (profile, win.rules_ok) else { return rep };

    if win.fw_enabled.as_deref() == Some("False") {
        rep.state = "off".into();
        return rep;
    }
    for r in &win.rules {
        let program_ok = r.program.trim().is_empty()
            || r.program.eq_ignore_ascii_case("any")
            || exe.is_some_and(|e| norm_path(&r.program) == norm_path(e));
        if !(program_ok
            && protocol_is_tcp(&r.protocol)
            && port_matches(&r.local_port, port)
            && profile_matches(&r.profile, profile))
        {
            continue;
        }
        if r.action.eq_ignore_ascii_case("block") {
            rep.matching_block.push(r.name.clone());
        } else if r.action.eq_ignore_ascii_case("allow") {
            rep.matching_allow.push(r.name.clone());
        }
    }
    rep.state = if !rep.matching_block.is_empty() {
        "blocked"
    } else if !rep.matching_allow.is_empty() || win.default_inbound.as_deref() == Some("Allow") {
        "allowed"
    } else {
        "no_rule"
    }
    .into();
    if let (Some(exe), "no_rule") = (exe, rep.state.as_str()) {
        rep.suggested_command = Some(suggested_rule(exe, port, profile));
    }
    rep
}

#[cfg(windows)]
const QUERY: &str = r#"
$ErrorActionPreference = 'Stop'
$ip = $env:VD_IP; $exe = $env:VD_EXE
$r = [ordered]@{ rulesOk = $false }
try {
  $idx = (Get-NetIPAddress -IPAddress $ip -AddressFamily IPv4 | Select-Object -First 1).InterfaceIndex
  $p = Get-NetConnectionProfile -InterfaceIndex $idx | Select-Object -First 1
  $r.alias = [string]$p.InterfaceAlias
  $r.category = [string]$p.NetworkCategory
} catch { $r.error = $_.Exception.Message }
try {
  $fname = switch ($r.category) { 'Public' { 'Public' } 'Private' { 'Private' } default { 'Domain' } }
  $fp = Get-NetFirewallProfile -Name $fname -PolicyStore ActiveStore
  $r.fwEnabled = [string]$fp.Enabled
  $r.defaultInbound = [string]$fp.DefaultInboundAction
  $app = @{}; Get-NetFirewallApplicationFilter -PolicyStore ActiveStore | ForEach-Object { $app[$_.InstanceID] = $_ }
  $prt = @{}; Get-NetFirewallPortFilter -PolicyStore ActiveStore | ForEach-Object { $prt[$_.InstanceID] = $_ }
  $adr = @{}; Get-NetFirewallAddressFilter -PolicyStore ActiveStore | ForEach-Object { $adr[$_.InstanceID] = $_ }
  $rules = foreach ($x in (Get-NetFirewallRule -Direction Inbound -Enabled True -PolicyStore ActiveStore)) {
    $a = $app[$x.InstanceID]; $q = $prt[$x.InstanceID]; $d = $adr[$x.InstanceID]
    if ($a.Package -and [string]$a.Package -ne 'Any') { continue }
    $prog = [string]$a.Program
    if ($prog -ne 'Any' -and $prog -ine $exe) { continue }
    $lp = @($q.LocalPort | ForEach-Object { [string]$_ })
    if ($prog -eq 'Any' -and -not ($lp -contains 'Any' -or ($lp | Where-Object { $_ -match '^\d' }))) { continue }
    [pscustomobject]@{ name = $x.DisplayName; action = [string]$x.Action; profile = [string]$x.Profile; program = $prog; protocol = [string]$q.Protocol; localPort = $lp; remoteAddress = @($d.RemoteAddress | ForEach-Object { [string]$_ }) }
  }
  $r.rules = @($rules)
  $r.rulesOk = $true
} catch { $r.error = $_.Exception.Message }
$r | ConvertTo-Json -Depth 5 -Compress
"#;

/// Best-effort, strictly read-only query of the network profile of the adapter
/// that owns `ip`, plus the inbound firewall rules that could affect Veydrop.
pub async fn query_windows(ip: IpAddr, port: u16, exe: Option<&str>) -> WinNet {
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("powershell.exe");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", QUERY])
            .env("VD_IP", ip.to_string())
            .env("VD_PORT", port.to_string())
            .env("VD_EXE", exe.unwrap_or(""))
            .creation_flags(0x0800_0000)
            .kill_on_drop(true);
        let out = match tokio::time::timeout(Duration::from_secs(30), cmd.output()).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return WinNet { error: Some(e.to_string()), ..Default::default() },
            Err(_) => return WinNet { error: Some("the Windows query timed out".into()), ..Default::default() },
        };
        let text = String::from_utf8_lossy(&out.stdout);
        match serde_json::from_str::<WinNet>(text.trim().trim_start_matches('\u{feff}')) {
            Ok(w) => w,
            Err(e) => WinNet { error: Some(format!("unreadable Windows reply: {e}")), ..Default::default() },
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (ip, port, exe);
        WinNet { error: Some("not Windows".into()), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(name: &str, action: &str, profile: &str, program: &str, ports: &[&str]) -> FwRule {
        FwRule {
            name: name.into(),
            action: action.into(),
            profile: profile.into(),
            program: program.into(),
            protocol: "TCP".into(),
            local_port: ports.iter().map(|s| s.to_string()).collect(),
            remote_address: vec![],
        }
    }

    fn win(category: &str, rules: Vec<FwRule>) -> WinNet {
        WinNet {
            category: Some(category.into()),
            fw_enabled: Some("True".into()),
            default_inbound: Some("Block".into()),
            rules,
            rules_ok: true,
            ..Default::default()
        }
    }

    const EXE: &str = r"C:\Program Files\Veydrop\veydrop.exe";

    #[test]
    fn no_rule_on_public_suggests_scoped_command() {
        let r = evaluate(&win("Public", vec![]), Some(EXE), 47821);
        assert_eq!(r.state, "no_rule");
        let cmd = r.suggested_command.unwrap();
        assert!(cmd.contains("-RemoteAddress LocalSubnet"));
        assert!(cmd.contains("-LocalPort 47821-47900"));
        assert!(cmd.contains("-Profile Public"));
        assert!(cmd.contains(EXE));
        assert!(!cmd.contains("-Profile Any"));
    }

    #[test]
    fn allow_rule_for_other_profile_does_not_count() {
        let w = win("Public", vec![rule("v", "Allow", "Private", EXE, &["Any"])]);
        assert_eq!(evaluate(&w, Some(EXE), 47821).state, "no_rule");
        let w = win("Private", vec![rule("v", "Allow", "Private, Public", &EXE.to_lowercase(), &["Any"])]);
        assert_eq!(evaluate(&w, Some(EXE), 47821).state, "allowed");
    }

    #[test]
    fn block_overrides_allow() {
        let w = win(
            "Private",
            vec![rule("a", "Allow", "Any", EXE, &["Any"]), rule("b", "Block", "Any", EXE, &["Any"])],
        );
        let r = evaluate(&w, Some(EXE), 47821);
        assert_eq!(r.state, "blocked");
        assert_eq!(r.matching_block, vec!["b"]);
        assert!(r.suggested_command.is_none());
    }

    #[test]
    fn port_ranges_and_other_programs() {
        let w = win("Private", vec![rule("a", "Allow", "Any", "Any", &["47000-47900"])]);
        assert_eq!(evaluate(&w, Some(EXE), 47821).state, "allowed");
        assert_eq!(evaluate(&w, Some(EXE), 5000).state, "no_rule");
        let w = win("Private", vec![rule("a", "Allow", "Any", r"C:\other.exe", &["Any"])]);
        assert_eq!(evaluate(&w, Some(EXE), 47821).state, "no_rule");
    }

    #[test]
    fn firewall_off_and_failed_query() {
        let mut w = win("Public", vec![]);
        w.fw_enabled = Some("False".into());
        assert_eq!(evaluate(&w, Some(EXE), 47821).state, "off");
        let w = WinNet { error: Some("x".into()), ..Default::default() };
        assert_eq!(evaluate(&w, Some(EXE), 47821).state, "unknown");
    }

    #[test]
    fn parses_powershell_json_with_collapsed_arrays() {
        let j = r#"{"rulesOk":true,"category":"Public","rules":{"name":"n","action":"Allow","profile":"Any","program":"Any","protocol":"TCP","localPort":"80","remoteAddress":null}}"#;
        let w: WinNet = serde_json::from_str(j).unwrap();
        assert_eq!(w.rules.len(), 1);
        assert_eq!(w.rules[0].local_port, vec!["80"]);
        assert!(w.rules[0].remote_address.is_empty());
        let w: WinNet = serde_json::from_str(r#"{"rulesOk":true,"rules":null}"#).unwrap();
        assert!(w.rules.is_empty());
    }

    #[tokio::test]
    async fn local_check_reports_a_dead_port() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        drop(l);
        let c = local_http_check("127.0.0.1".parse().unwrap(), port).await;
        assert!(!c.ok);
        assert!(c.error.is_some());
    }
}
