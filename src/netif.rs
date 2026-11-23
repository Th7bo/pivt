use std::process::Command;

/// Best-guess local IP for LHOST. Prefers a VPN tun (HTB/THM), then common
/// private interfaces, then anything non-loopback.
pub fn detect_lhost() -> Option<String> {
    let out = Command::new("ip").args(["-4", "-o", "addr", "show"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);

    // (iface, ip)
    let mut ifaces: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        // e.g. "3: tun0    inet 10.10.14.5/23 scope global tun0"
        let mut parts = line.split_whitespace();
        let _idx = parts.next();
        let name = match parts.next() {
            Some(n) => n.to_string(),
            None => continue,
        };
        // find token after "inet"
        let mut found = None;
        let toks: Vec<&str> = line.split_whitespace().collect();
        for (i, t) in toks.iter().enumerate() {
            if *t == "inet" {
                if let Some(cidr) = toks.get(i + 1) {
                    found = cidr.split('/').next().map(|s| s.to_string());
                }
            }
        }
        if let Some(ip) = found {
            if ip == "127.0.0.1" {
                continue;
            }
            ifaces.push((name, ip));
        }
    }

    // priority order
    let pick = |pred: &dyn Fn(&str) -> bool| ifaces.iter().find(|(n, _)| pred(n)).map(|(_, ip)| ip.clone());

    pick(&|n| n.starts_with("tun"))
        .or_else(|| pick(&|n| n.starts_with("tap")))
        .or_else(|| pick(&|n| n.starts_with("wg")))
        .or_else(|| pick(&|n| n == "tailscale0"))
        .or_else(|| pick(&|n| n.starts_with("wl")))
        .or_else(|| pick(&|n| n.starts_with("en") || n.starts_with("eth")))
        .or_else(|| ifaces.first().map(|(_, ip)| ip.clone()))
}
