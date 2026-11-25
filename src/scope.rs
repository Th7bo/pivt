//! Per-room scope: the hosts a room's commands are allowed to touch.
//!
//! A room's scope is a list of entries (IPv4/IPv6 addresses, CIDRs,
//! hostnames, `*.domain` wildcards) stored in its `.ctfroom` marker. Without
//! one, the room's `.target` (ip + hostname, plus `*.hostname`) and the HTB
//! machine IP (when the room is that machine) are the scope. Before a command
//! runs, the hosts in its *filled* line are extracted ([`extract`]) and
//! checked against it; local addresses (LHOST, loopback, this machine's
//! interfaces, 0.0.0.0) always pass.

use std::collections::BTreeMap;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

/// What happens when a command targets something outside the scope.
/// Config `scope_mode = "warn" | "confirm" | "strict"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Run, but say so in the status line.
    Warn,
    /// Ask first: run anyway / add to scope / cancel.
    #[default]
    Confirm,
    /// Refuse outright.
    Strict,
}

impl Mode {
    /// Unknown or empty → the default (confirm), so config parsing never fails.
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "warn" => Mode::Warn,
            "strict" => Mode::Strict,
            _ => Mode::Confirm,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Warn => "warn",
            Mode::Confirm => "confirm",
            Mode::Strict => "strict",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Mode::Warn => Mode::Confirm,
            Mode::Confirm => Mode::Strict,
            Mode::Strict => Mode::Warn,
        }
    }
}

/// A scope entry, or a host found in a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Addr {
    /// An address with its prefix length (32 / 128 for a single host).
    Net(IpAddr, u8),
    /// A lowercase hostname; as a scope entry it may start with `*.`.
    Host(String),
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Addr::Net(ip, p) if *p == full_prefix(ip) => write!(f, "{ip}"),
            Addr::Net(ip, p) => write!(f, "{ip}/{p}"),
            Addr::Host(h) => f.write_str(h),
        }
    }
}

fn full_prefix(ip: &IpAddr) -> u8 {
    if ip.is_ipv4() {
        32
    } else {
        128
    }
}

impl Addr {
    /// Parse a scope entry: `10.10.10.5`, `10.10.10.0/24`, `dead:beef::/64`,
    /// `[::1]`, `dc-2`, `box.htb`, `*.htb`. Case and a trailing dot are ignored.
    pub fn parse(s: &str) -> Option<Addr> {
        let s = s.trim().trim_end_matches('.').to_ascii_lowercase();
        if s.is_empty() {
            return None;
        }
        let (ip_part, prefix) = match s.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (s.as_str(), None),
        };
        let ip_part = ip_part.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = ip_part.parse::<IpAddr>() {
            let full = full_prefix(&ip);
            let p = match prefix {
                None => full,
                Some(p) => p.parse::<u8>().ok().filter(|p| *p <= full)?,
            };
            return Some(Addr::Net(mask(ip, p), p));
        }
        if prefix.is_some() {
            return None;
        }
        let bare = s.strip_prefix("*.").unwrap_or(&s);
        // a dotted all-numeric name is a broken IP, not a host
        if valid_host(bare) && !(bare.contains('.') && bare.chars().all(|c| c.is_ascii_digit() || c == '.')) {
            return Some(Addr::Host(s));
        }
        None
    }

    /// Whether this (scope entry) covers `t`.
    pub fn contains(&self, t: &Addr) -> bool {
        match (self, t) {
            (Addr::Net(a, ap), Addr::Net(b, bp)) => {
                a.is_ipv4() == b.is_ipv4() && bp >= ap && mask(*a, *ap) == mask(*b, *ap)
            }
            (Addr::Host(p), Addr::Host(h)) => match p.strip_prefix('*') {
                Some(suffix) => h.len() > suffix.len() && h.ends_with(suffix),
                None => p == h,
            },
            _ => false,
        }
    }
}

/// Zero the host bits of `ip` beyond `prefix`.
fn mask(ip: IpAddr, prefix: u8) -> IpAddr {
    match ip {
        IpAddr::V4(v) => {
            let m = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix as u32) };
            IpAddr::V4(Ipv4Addr::from(u32::from(v) & m))
        }
        IpAddr::V6(v) => {
            let m = if prefix == 0 { 0 } else { u128::MAX << (128 - prefix as u32) };
            IpAddr::V6(Ipv6Addr::from(u128::from(v) & m))
        }
    }
}

/// Hostname labels: `[a-z0-9_-]`, 1–63 chars, not starting/ending with `-`;
/// the last label isn't all digits.
fn valid_host(s: &str) -> bool {
    if s.is_empty() || s.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = s.split('.').collect();
    let ok = labels.iter().all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    });
    ok && !labels.last().is_some_and(|l| l.chars().all(|c| c.is_ascii_digit()))
}

/// The effective scope of a room.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub entries: Vec<Addr>,
    /// Set in the room's `.ctfroom` (else derived from `.target` / HTB).
    pub explicit: bool,
}

impl Scope {
    pub fn from_strings(list: &[String], explicit: bool) -> Self {
        let mut entries: Vec<Addr> = Vec::new();
        for e in list.iter().filter_map(|s| Addr::parse(s)) {
            if !entries.contains(&e) {
                entries.push(e);
            }
        }
        Scope { entries, explicit }
    }
    /// Entries as they're stored (`10.10.10.0/24`, `*.htb`).
    pub fn strings(&self) -> Vec<String> {
        self.entries.iter().map(|e| e.to_string()).collect()
    }
    /// `t` is covered by an entry, directly or (for a hostname) through an
    /// `/etc/hosts` line pointing it at an in-scope address.
    pub fn allows(&self, t: &Addr, hosts: &EtcHosts) -> bool {
        if self.entries.iter().any(|e| e.contains(t)) {
            return true;
        }
        match t {
            Addr::Host(h) => hosts
                .resolve(h)
                .iter()
                .any(|ip| self.entries.iter().any(|e| e.contains(&Addr::Net(*ip, full_prefix(ip))))),
            _ => false,
        }
    }
    /// "a, b, c, +2" for prompts; "none set" when empty.
    pub fn summary(&self, max: usize) -> String {
        if self.entries.is_empty() {
            return "none set".into();
        }
        let mut s: Vec<String> = self.entries.iter().take(max).map(|e| e.to_string()).collect();
        if self.entries.len() > max {
            s.push(format!("+{}", self.entries.len() - max));
        }
        s.join(", ")
    }
}

/// Default scope of a room without an explicit one: its `.target` ip and
/// hostname (plus `*.hostname` for vhosts) and the HTB machine IP.
pub fn defaults(target: Option<(String, Option<String>)>, htb_ip: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if Addr::parse(&s).is_some() && !out.contains(&s) {
            out.push(s);
        }
    };
    if let Some((ip, host)) = target {
        push(ip);
        if let Some(h) = host.filter(|h| !h.is_empty()) {
            let h = h.to_ascii_lowercase();
            if h.contains('.') {
                push(format!("*.{h}"));
            }
            push(h);
        }
    }
    if let Some(ip) = htb_ip.filter(|i| !i.is_empty()) {
        push(ip.to_string());
    }
    // keep the order natural: addresses / host first, wildcard after
    out.sort_by_key(|s| s.starts_with("*."));
    out
}

/// Public reference sites tool-download / lookup commands fetch from; never
/// an engagement target.
const REFERENCE_HOSTS: &[&str] = &[
    "github.com", "*.github.com", "*.github.io", "raw.githubusercontent.com", "*.githubusercontent.com",
    "gitlab.com", "pypi.org", "files.pythonhosted.org", "crates.io", "static.crates.io",
    "*.hackthebox.com", "tryhackme.com", "*.tryhackme.com",
];

/// Addresses that are this machine (always allowed).
#[derive(Debug, Clone, Default)]
pub struct Local {
    pub lhost: String,
    /// This machine's interface addresses.
    pub ips: Vec<String>,
}

impl Local {
    pub fn covers(&self, t: &Addr) -> bool {
        match t {
            Addr::Net(ip, p) => {
                if ip.is_loopback() && (*p >= 8 || ip.is_ipv6()) {
                    return true; // 127.0.0.0/8 and ::1
                }
                if *p != full_prefix(ip) {
                    return false;
                }
                let s = ip.to_string();
                ip.is_unspecified() || s == self.lhost || self.ips.contains(&s)
            }
            Addr::Host(h) => {
                matches!(h.as_str(), "localhost" | "localhost.localdomain" | "ip6-localhost" | "ip6-loopback")
                    || h.eq_ignore_ascii_case(&self.lhost)
                    || REFERENCE_HOSTS.iter().any(|r| Addr::Host((*r).into()).contains(t))
            }
        }
    }
}

/// `/etc/hosts` name → address mappings.
#[derive(Debug, Clone, Default)]
pub struct EtcHosts(Vec<(IpAddr, Vec<String>)>);

impl EtcHosts {
    pub fn parse(text: &str) -> Self {
        let mut v = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("");
            let mut it = line.split_whitespace();
            let Some(ip) = it.next().and_then(|i| i.parse::<IpAddr>().ok()) else { continue };
            let names: Vec<String> = it.map(|n| n.trim_end_matches('.').to_ascii_lowercase()).collect();
            if !names.is_empty() {
                v.push((ip, names));
            }
        }
        EtcHosts(v)
    }
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path).map(|t| Self::parse(&t)).unwrap_or_default()
    }
    pub fn resolve(&self, name: &str) -> Vec<IpAddr> {
        self.0.iter().filter(|(_, n)| n.iter().any(|x| x == name)).map(|(ip, _)| *ip).collect()
    }
}

/// Variables whose values are targets: checked whenever they appear in a line.
const HOST_VARS: &[&str] = &[
    "RHOST", "RHOSTS", "DOMAIN", "HOST", "VHOST", "DC", "DC_IP", "DCIP", "TARGET", "FQDN", "SUBNET", "URL",
];

/// Variable names (substrings) holding credentials / data, never hosts.
const SECRET_VARS: &[&str] = &["PASS", "HASH", "SECRET", "TOKEN", "KEY", "USER", "DATA"];

/// URL schemes that aren't network locations (`php://filter/…`).
const NON_NET_SCHEMES: &[&str] = &[
    "php", "file", "data", "jar", "expect", "zip", "phar", "glob", "javascript", "mailto", "zlib", "compress.zlib",
];

/// TLDs a bare dotted word must end in to count as a hostname (keeps
/// `http.server`, `rockyou.txt` and module names out).
const TLDS: &[&str] = &[
    "htb", "thm", "local", "lan", "corp", "internal", "intra", "home", "localdomain", "test", "example", "arpa",
    "com", "net", "org", "io", "dev", "edu", "gov", "mil", "int", "info", "biz", "xyz", "app", "cloud", "ai",
    "me", "co", "uk", "de", "fr", "nl", "be", "eu", "us", "ca", "au", "ch", "se", "no", "fi", "dk", "pl",
    "it", "es", "ru", "cn", "jp", "in", "br", "tv", "cc", "site", "online", "tech",
];

/// File extensions that make `name.ext:123` a file, not `host:port`.
const FILE_EXTS: &[&str] = &[
    "txt", "py", "php", "sh", "rb", "pl", "js", "json", "xml", "html", "htm", "md", "log", "lst", "list", "conf",
    "cfg", "ini", "nmap", "gnmap", "zip", "gz", "tar", "exe", "dll", "ps1", "bat", "c", "so", "elf", "bin", "key",
    "pem", "crt", "pcap", "out", "csv", "db", "sql", "kirbi", "ccache", "hash", "jar", "war", "asp", "aspx", "jsp",
];

/// Every host a filled command line points at: IPv4 / CIDR / IPv6 literals,
/// URL hosts, `user@host`, `host:port`, `//host/share`, dotted hostnames
/// with a known TLD, plus the values of host variables ([`HOST_VARS`]) that
/// appear in the line. Deduplicated, in order of appearance.
pub fn extract(line: &str, vars: &BTreeMap<String, String>) -> Vec<Addr> {
    let mut out: Vec<Addr> = Vec::new();
    // a password like `p@ss` (also inside `user%p@ss`, `-pp@ss`) isn't
    // `user@host`: cut credential values with an @ out of the words first
    let secrets: Vec<&str> = vars
        .iter()
        .filter(|(k, v)| v.contains('@') && SECRET_VARS.iter().any(|s| k.contains(s)))
        .map(|(_, v)| v.as_str())
        .collect();
    for mut word in shell_words(line) {
        for sec in &secrets {
            word = word.replace(sec, "\u{0}");
        }
        for piece in word.split(|c: char| c.is_whitespace() || c == '\u{0}' || "=,;|&()<>`\"'".contains(c)) {
            if !piece.is_empty() {
                scan_piece(piece, &mut out);
            }
        }
    }
    for k in HOST_VARS {
        let Some(v) = vars.get(*k).map(|v| v.trim()).filter(|v| !v.is_empty()) else { continue };
        if !contains_bounded(line, v) {
            continue;
        }
        match Addr::parse(v) {
            Some(a) if *k != "URL" => push(&mut out, a),
            _ => scan_piece(v, &mut out),
        }
    }
    out
}

fn push(out: &mut Vec<Addr>, a: Addr) {
    if !out.contains(&a) {
        out.push(a);
    }
}

/// `needle` occurs in `hay` not glued to other host characters.
fn contains_bounded(hay: &str, needle: &str) -> bool {
    let glue = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    let mut start = 0;
    while let Some(i) = hay[start..].find(needle) {
        let at = start + i;
        let before = hay[..at].chars().next_back();
        let after = hay[at + needle.len()..].chars().next();
        if !before.is_some_and(glue) && !after.is_some_and(glue) {
            return true;
        }
        start = at + needle.len().max(1);
        if start > hay.len() {
            break;
        }
    }
    false
}

/// Split a line into shell words (quotes removed, backslash escapes).
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                has = true;
                for d in chars.by_ref() {
                    if d == '\'' {
                        break;
                    }
                    cur.push(d);
                }
            }
            '"' => {
                has = true;
                while let Some(d) = chars.next() {
                    match d {
                        '"' => break,
                        '\\' => {
                            if let Some(e) = chars.next() {
                                cur.push(e);
                            }
                        }
                        d => cur.push(d),
                    }
                }
            }
            '\\' => {
                has = true;
                if let Some(e) = chars.next() {
                    cur.push(e);
                }
            }
            c if c.is_whitespace() => {
                if has {
                    words.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                has = true;
                cur.push(c);
            }
        }
    }
    if has {
        words.push(cur);
    }
    words
}

/// A host taken from a structural position (URL, `user@`, `//share`): any
/// valid hostname or address.
fn host_token(h: &str, out: &mut Vec<Addr>) {
    let h = h.trim_start_matches('[').trim_end_matches(']');
    if h.is_empty() || h.contains('<') {
        return;
    }
    if let Some(a) = Addr::parse(h).filter(|a| !matches!(a, Addr::Host(s) if s.starts_with('*'))) {
        push(out, a);
    }
}

fn scan_piece(p: &str, out: &mut Vec<Addr>) {
    // scheme://[user@]host[:port]/… — only the authority counts (a path may
    // carry SSRF payloads aimed from the target, not from us)
    if let Some((scheme, rest)) = p.split_once("://") {
        let scheme = scheme.to_ascii_lowercase();
        let scheme_ok = !scheme.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if scheme_ok {
            if !NON_NET_SCHEMES.contains(&scheme.as_str()) {
                let auth = rest.split(['/', '?', '#']).next().unwrap_or("");
                let auth = auth.rsplit_once('@').map(|(_, h)| h).unwrap_or(auth);
                host_token(strip_port(auth), out);
            }
            return;
        }
    }
    // //host/share, \\host\share
    if let Some(rest) = p.strip_prefix("//").or_else(|| p.strip_prefix("\\\\")) {
        host_token(rest.split(['/', '\\']).next().unwrap_or(""), out);
        return;
    }
    // user@host, domain/user:pass@host, @dns-server, user@host:path (scp)
    if let Some((_, h)) = p.rsplit_once('@') {
        let h = if h.starts_with('[') {
            h.split_inclusive(']').next().unwrap_or(h)
        } else {
            h.split(':').next().unwrap_or(h)
        };
        host_token(h, out);
        return;
    }
    // bare IPv6 (with optional /prefix or [..]:port)
    if p.matches(':').count() >= 2 {
        let v6 = match p.strip_prefix('[') {
            Some(rest) => rest.split(']').next().unwrap_or(""),
            None => p,
        };
        if let Some(a) = Addr::parse(v6) {
            if matches!(a, Addr::Net(IpAddr::V6(_), _)) {
                push(out, a);
                return;
            }
        }
    }
    scan_ipv4(p, out);
    // host.tld:port
    if let Some((h, port)) = p.rsplit_once(':') {
        let h = h.to_ascii_lowercase();
        let port = port.split('/').next().unwrap_or("");
        let port_ok = !port.is_empty() && port.len() <= 5 && port.chars().all(|c| c.is_ascii_digit()) && port.parse::<u32>().is_ok_and(|n| (1..=65535).contains(&n));
        if port_ok && h.contains('.') && valid_host(&h) && !is_file_ext(&h) && !is_all_numeric(&h) {
            push(out, Addr::Host(h));
        }
        return;
    }
    // bare box.htb / dc01.corp.local
    let h = p.trim_end_matches('.').to_ascii_lowercase();
    if h.contains('.') && !h.contains('/') && valid_host(&h) && !is_all_numeric(&h) {
        let tld = h.rsplit('.').next().unwrap_or("");
        if TLDS.contains(&tld) {
            push(out, Addr::Host(h));
        }
    }
}

fn is_all_numeric(h: &str) -> bool {
    h.chars().all(|c| c.is_ascii_digit() || c == '.')
}

fn is_file_ext(h: &str) -> bool {
    h.rsplit('.').next().is_some_and(|e| FILE_EXTS.contains(&e))
}

/// `host:port` → host (IPv6 in brackets kept whole).
fn strip_port(auth: &str) -> &str {
    if auth.starts_with('[') {
        return auth.split(']').next().map(|h| &h[1..]).unwrap_or(auth);
    }
    match auth.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.contains(':') => h,
        _ => auth,
    }
}

/// Dotted-quad IPv4 addresses (with an optional `/nn`) inside `p`. Runs of
/// digits and dots with exactly four parts only, so versions like `7.0.0` or
/// `2.4.10` never match; runs glued to letters (`v1.2.3.4`) are skipped.
fn scan_ipv4(p: &str, out: &mut Vec<Addr>) {
    let b = p.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
            i += 1;
        }
        // glued to a word: `v1.2.3.4`, `build-2.4.10.1` (but `-10.0.0.1` is fine)
        let glued_before = start > 0
            && (b[start - 1].is_ascii_alphabetic()
                || b[start - 1] == b'_'
                || (b[start - 1] == b'-' && start > 1 && b[start - 2].is_ascii_alphanumeric()));
        let run = p[start..i].trim_end_matches('.');
        let mut end = start + run.len();
        let parts: Vec<&str> = run.split('.').collect();
        let is_ip = parts.len() == 4
            && parts.iter().all(|x| !x.is_empty() && x.len() <= 3 && x.parse::<u16>().is_ok_and(|n| n <= 255));
        if !is_ip || glued_before {
            continue;
        }
        // optional /nn (not a /24abc path segment)
        let mut prefix = None;
        if end < b.len() && b[end] == b'/' {
            let mut j = end + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let digits = &p[end + 1..j];
            let after_ok = j >= b.len() || !(b[j].is_ascii_alphanumeric() || b[j] == b'.' || b[j] == b'_');
            if !digits.is_empty() && digits.len() <= 2 && after_ok {
                if let Ok(n) = digits.parse::<u8>() {
                    if n <= 32 {
                        prefix = Some(n);
                        end = j;
                    }
                }
            }
        }
        let glued_after = end < b.len() && (b[end].is_ascii_alphabetic() || b[end] == b'_');
        if glued_after {
            continue;
        }
        if let Ok(ip) = run.parse::<Ipv4Addr>() {
            let p = prefix.unwrap_or(32);
            push(out, Addr::Net(mask(IpAddr::V4(ip), p), p));
        }
        i = end.max(i);
    }
}

/// Hosts in `lines` that are neither local nor in `scope`, deduplicated.
pub fn out_of_scope(
    lines: &[String],
    vars: &BTreeMap<String, String>,
    scope: &Scope,
    local: &Local,
    hosts: &EtcHosts,
) -> Vec<Addr> {
    let mut out: Vec<Addr> = Vec::new();
    for l in lines {
        for a in extract(l, vars) {
            if !local.covers(&a) && !scope.allows(&a, hosts) {
                push(&mut out, a);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(line: &str) -> Vec<String> {
        extract(line, &BTreeMap::new()).iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn extracts_hosts_from_filled_commands() {
        assert_eq!(ex("nmap -sC -sV -p 7744 10.10.11.42 -oN nmap/quick.txt"), ["10.10.11.42"]);
        assert_eq!(ex("nmap -sn 10.10.10.0/24"), ["10.10.10.0/24"]);
        assert_eq!(ex("ffuf -u http://10.10.11.42:8080/FUZZ -w /usr/share/wordlists/x.txt"), ["10.10.11.42"]);
        assert_eq!(ex("feroxbuster -u https://dev.box.htb/ -x php"), ["dev.box.htb"]);
        assert_eq!(ex("ssh svc@192.168.56.102"), ["192.168.56.102"]);
        assert_eq!(ex("ssh -p 2222 root@dc-2"), ["dc-2"]);
        assert_eq!(ex("impacket-secretsdump corp/admin:'P@ss:w0rd'@10.10.10.5"), ["10.10.10.5"]);
        assert_eq!(ex("scp alice@10.10.10.9:/etc/passwd ."), ["10.10.10.9"]);
        assert_eq!(ex("dig axfr box.htb @10.10.10.3"), ["box.htb", "10.10.10.3"]);
        assert_eq!(ex("smbclient //10.10.10.4/share -N"), ["10.10.10.4"]);
        assert_eq!(ex("evil-winrm -i dc01.corp.local -u x"), ["dc01.corp.local"]);
        assert_eq!(ex("curl box.htb:8443/api"), ["box.htb"]);
        assert_eq!(ex("nc -nv target.htb:4444"), ["target.htb"]);
        assert_eq!(ex("curl http://[dead:beef::1]:80/"), ["dead:beef::1"]);
        assert_eq!(ex("ping6 dead:beef::5"), ["dead:beef::5"]);
        assert_eq!(ex("hydra -l u -P p.txt ssh://10.10.11.7 -t 4"), ["10.10.11.7"]);
        assert_eq!(ex("nmap --script-args http.useragent=x 10.0.0.1,10.0.0.2"), ["10.0.0.1", "10.0.0.2"]);
        assert_eq!(ex("ssh -L 8080:127.0.0.1:80 u@10.1.1.1"), ["127.0.0.1", "10.1.1.1"]);
        assert_eq!(ex("nmap 10.10.10.1-254"), ["10.10.10.1"], "an nmap range still names its base");
        assert_eq!(ex("ffuf -H 'Host: FUZZ.box.htb' -u http://10.10.10.3"), ["fuzz.box.htb", "10.10.10.3"]);
    }

    #[test]
    fn no_false_positives() {
        for line in [
            "searchsploit apache 2.4.10",
            "searchsploit openssh 7.0.0",
            "python3 -m http.server 80",
            "nc -lvnp 4444",
            "hashcat -m 1000 hash.txt rockyou.txt",
            "john --format=nt --wordlist=/usr/share/wordlists/rockyou.txt hashes.lst",
            "php -r 'echo file_get_contents(\"php://filter/convert.base64-encode/resource=index.php\");'",
            "impacket-wmiexec -hashes :aad3b435b51404eeaad3b435b51404ee:31d6cfe0d16ae931b73c59d7e0c089c0 x",
            "echo v1.2.3.4 build-2.4.10.1-rc",
            "cat nmap/quick.txt:12",
            "sleep 5; date +%H:%M:%S",
        ] {
            assert_eq!(ex(line), Vec::<String>::new(), "{line}");
        }
    }

    #[test]
    fn host_vars_count_when_they_appear() {
        let mut vars = BTreeMap::new();
        vars.insert("RHOST".to_string(), "dc-2".to_string());
        vars.insert("DOMAIN".to_string(), "corp".to_string());
        vars.insert("URL".to_string(), "http://web.box.htb".to_string());
        let got: Vec<String> = extract("crackmapexec smb dc-2 -d corp", &vars).iter().map(|a| a.to_string()).collect();
        assert_eq!(got, ["dc-2", "corp"]);
        // a value that isn't in the line doesn't count, nor one glued into a word
        assert!(extract("nmap 10.0.0.1 -oN dc-2x.txt", &vars).iter().all(|a| a.to_string() == "10.0.0.1"));
        let got: Vec<String> = extract("whatweb http://web.box.htb", &vars).iter().map(|a| a.to_string()).collect();
        assert_eq!(got, ["web.box.htb"]);
        // a password with an @ is not user@host
        vars.insert("PASS".to_string(), "Summer@2024".to_string());
        for line in ["nxc smb 10.0.0.9 -u bob -p 'Summer@2024'", "rpcclient -U bob%Summer@2024 10.0.0.9", "mysql -h 10.0.0.9 -pSummer@2024"] {
            let got: Vec<String> = extract(line, &vars).iter().map(|a| a.to_string()).collect();
            assert_eq!(got, ["10.0.0.9"], "{line}");
        }
    }

    #[test]
    fn cidr_and_wildcard_matching() {
        let e = |s: &str| Addr::parse(s).unwrap();
        assert!(e("10.10.10.0/24").contains(&e("10.10.10.77")));
        assert!(!e("10.10.10.0/24").contains(&e("10.10.11.1")));
        assert!(e("10.10.10.0/24").contains(&e("10.10.10.128/25")));
        assert!(!e("10.10.10.0/24").contains(&e("10.10.0.0/16")), "a wider net isn't inside");
        assert!(!e("10.10.10.5").contains(&e("10.10.10.0/24")));
        assert_eq!(e("10.10.10.77/24").to_string(), "10.10.10.0/24", "host bits masked");
        assert!(e("*.htb").contains(&e("box.htb")) && e("*.htb").contains(&e("a.b.htb")));
        assert!(!e("*.htb").contains(&e("htb")) && !e("*.htb").contains(&e("xhtb")));
        assert!(e("DC-2.").contains(&e("dc-2")));
        assert!(e("dead:beef::/64").contains(&e("dead:beef::42")));
        assert!(!e("dead:beef::/64").contains(&e("10.0.0.1")));
        for bad in ["", "10.0.0.1/33", "1.2.3", "-bad", "a..b", "box.htb/24"] {
            assert!(Addr::parse(bad).is_none(), "{bad}");
        }
        // /etc/hosts maps a vhost onto an in-scope address
        let scope = Scope::from_strings(&["10.10.10.3".into()], true);
        let hosts = EtcHosts::parse("127.0.0.1 localhost\n10.10.10.3 lame.htb dev.lame.htb # box\n");
        assert!(scope.allows(&e("dev.lame.htb"), &hosts));
        assert!(!scope.allows(&e("other.htb"), &hosts));
    }

    #[test]
    fn default_scope_from_target_and_htb() {
        assert_eq!(defaults(None, None), Vec::<String>::new());
        assert_eq!(
            defaults(Some(("10.10.10.3".into(), Some("Lame.htb".into()))), Some("10.10.10.3")),
            ["10.10.10.3", "lame.htb", "*.lame.htb"]
        );
        assert_eq!(defaults(Some(("192.168.56.102".into(), Some("dc-2".into()))), Some("10.129.1.1")), ["192.168.56.102", "dc-2", "10.129.1.1"]);
    }

    #[test]
    fn local_addresses_always_pass() {
        let local = Local { lhost: "10.10.14.7".into(), ips: vec!["192.168.1.20".into()] };
        let scope = Scope::from_strings(&["10.10.10.3".into()], false);
        let vars = BTreeMap::from([("LHOST".to_string(), "10.10.14.7".to_string())]);
        let lines = |s: &str| vec![s.to_string()];
        let oos = |s: &str| out_of_scope(&lines(s), &vars, &scope, &local, &EtcHosts::default());
        assert!(oos("python3 -m http.server 80 --bind 10.10.14.7").is_empty());
        assert!(oos("nc -lvnp 4444 -s 0.0.0.0").is_empty());
        assert!(oos("curl http://127.0.0.1:8000/ http://localhost/ http://[::1]/").is_empty());
        assert!(oos("wget https://github.com/carlospolop/PEASS-ng/releases/latest/download/linpeas.sh").is_empty());
        assert!(oos("ping -c1 192.168.1.20").is_empty());
        assert!(oos("nmap 10.10.10.3").is_empty());
        assert_eq!(oos("nmap 10.10.10.4 10.10.10.3 10.10.10.4").iter().map(|a| a.to_string()).collect::<Vec<_>>(), ["10.10.10.4"]);
        assert_eq!(oos("nmap 0.0.0.0/0").len(), 1, "everything is not local");
    }

    #[test]
    fn mode_parse_falls_back() {
        assert_eq!(Mode::parse("warn"), Mode::Warn);
        assert_eq!(Mode::parse(" STRICT "), Mode::Strict);
        assert_eq!(Mode::parse(""), Mode::Confirm);
        assert_eq!(Mode::parse("yolo"), Mode::Confirm);
        assert_eq!(Mode::Strict.next(), Mode::Warn);
    }
}
