use std::process::Command;

/// Local IPv4 addresses as (interface, ip), loopback excluded, from `ip -4 -o addr`.
pub fn list_ifaces() -> Vec<(String, String)> {
    let Ok(out) = Command::new("ip").args(["-4", "-o", "addr", "show"]).output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    parse_ip_addr(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `ip -4 -o addr show` lines, e.g.
/// "3: tun0    inet 10.10.14.5/23 scope global tun0".
pub fn parse_ip_addr(text: &str) -> Vec<(String, String)> {
    let mut ifaces = Vec::new();
    for line in text.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let Some(name) = toks.get(1) else { continue };
        let name = name.split('@').next().unwrap_or(name);
        let ip = toks
            .iter()
            .position(|t| *t == "inet")
            .and_then(|i| toks.get(i + 1))
            .and_then(|cidr| cidr.split('/').next());
        if let Some(ip) = ip {
            if !ip.starts_with("127.") {
                ifaces.push((name.to_string(), ip.to_string()));
            }
        }
    }
    ifaces
}

/// A VPN-style interface (HTB/THM OpenVPN, WireGuard).
pub fn is_vpn(name: &str) -> bool {
    name.starts_with("tun") || name.starts_with("tap") || name.starts_with("wg")
}

/// Best-guess LHOST: a VPN tun first, then common private interfaces, then
/// anything non-loopback.
pub fn pick_lhost(ifaces: &[(String, String)]) -> Option<(String, String)> {
    let pick = |pred: &dyn Fn(&str) -> bool| ifaces.iter().find(|(n, _)| pred(n)).cloned();
    pick(&|n| n.starts_with("tun"))
        .or_else(|| pick(&|n| n.starts_with("tap")))
        .or_else(|| pick(&|n| n.starts_with("wg")))
        .or_else(|| pick(&|n| n == "tailscale0"))
        .or_else(|| pick(&|n| n.starts_with("wl")))
        .or_else(|| pick(&|n| n.starts_with("en") || n.starts_with("eth")))
        .or_else(|| ifaces.first().cloned())
}

pub fn detect_lhost() -> Option<String> {
    pick_lhost(&list_ifaces()).map(|(_, ip)| ip)
}

/// What to do with the saved LHOST at startup.
#[derive(Debug, PartialEq, Eq)]
pub enum LhostCheck {
    /// Saved value is fine (still on a local interface), or nothing to go on.
    Keep,
    /// Nothing saved: use this (iface, ip).
    Fill(String, String),
    /// Saved IP is gone from every interface and a VPN interface is up:
    /// switch to it (iface, ip).
    Replace(String, String),
    /// Saved IP is gone and there's no VPN to switch to: warn.
    Stale,
}

/// Decide about a saved LHOST. A manually chosen address that is still
/// assigned to some interface is never overridden.
pub fn check_lhost(saved: &str, ifaces: &[(String, String)]) -> LhostCheck {
    let saved = saved.trim();
    if saved.is_empty() {
        return match pick_lhost(ifaces) {
            Some((n, ip)) => LhostCheck::Fill(n, ip),
            None => LhostCheck::Keep,
        };
    }
    if ifaces.is_empty() || ifaces.iter().any(|(_, ip)| ip == saved) {
        // no interface info at all (no `ip` binary?): don't guess
        return LhostCheck::Keep;
    }
    match ifaces.iter().find(|(n, _)| is_vpn(n)) {
        Some((n, ip)) => LhostCheck::Replace(n.clone(), ip.clone()),
        None => LhostCheck::Stale,
    }
}

/// Whether `ip` is assigned to a local interface (None if unknown).
pub fn is_local(ip: &str, ifaces: &[(String, String)]) -> Option<bool> {
    (!ifaces.is_empty()).then(|| ifaces.iter().any(|(_, x)| x == ip))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ifs(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn parses_ip_output() {
        let text = "1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever\n\
                    2: wlp3s0    inet 192.168.1.20/24 brd 192.168.1.255 scope global dynamic wlp3s0\n\
                    5: tun0    inet 10.10.14.5/23 scope global tun0\n\
                    7: veth1@if6    inet 172.17.0.1/16 scope global veth1\n";
        assert_eq!(
            parse_ip_addr(text),
            ifs(&[("wlp3s0", "192.168.1.20"), ("tun0", "10.10.14.5"), ("veth1", "172.17.0.1")])
        );
    }

    #[test]
    fn lhost_decisions() {
        let vpn = ifs(&[("wlp3s0", "192.168.1.20"), ("tun0", "10.10.14.9")]);
        let novpn = ifs(&[("wlp3s0", "192.168.1.20")]);
        // nothing saved: fill with the VPN address
        assert_eq!(check_lhost("", &vpn), LhostCheck::Fill("tun0".into(), "10.10.14.9".into()));
        // still the VPN address: keep
        assert_eq!(check_lhost("10.10.14.9", &vpn), LhostCheck::Keep);
        // a manual pick that is still live (wifi) is never clobbered by the VPN
        assert_eq!(check_lhost("192.168.1.20", &vpn), LhostCheck::Keep);
        // the old VPN lease is gone and a new tun is up: switch
        assert_eq!(check_lhost("10.10.14.2", &vpn), LhostCheck::Replace("tun0".into(), "10.10.14.9".into()));
        // gone, but no VPN up yet: only warn
        assert_eq!(check_lhost("10.10.14.2", &novpn), LhostCheck::Stale);
        // no interface data at all: leave it
        assert_eq!(check_lhost("10.10.14.2", &[]), LhostCheck::Keep);
        assert_eq!(check_lhost("", &[]), LhostCheck::Keep);
        assert_eq!(is_local("10.10.14.2", &novpn), Some(false));
        assert_eq!(is_local("10.10.14.2", &[]), None);
    }
}
