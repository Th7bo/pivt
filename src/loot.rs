use crate::model::config_dir;
use crate::room::Room;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A captured credential / secret found during a box.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cred {
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub pass: String,
    #[serde(default)]
    pub hash: String,
    #[serde(default)]
    pub note: String,
}

impl Cred {
    pub fn is_empty(&self) -> bool {
        self.user.is_empty() && self.pass.is_empty() && self.hash.is_empty() && self.note.is_empty()
    }
    /// One-line summary for the list.
    pub fn label(&self) -> String {
        let mut s = String::new();
        if !self.user.is_empty() {
            s.push_str(&self.user);
        }
        if !self.pass.is_empty() {
            s.push(':');
            s.push_str(&self.pass);
        }
        if !self.hash.is_empty() {
            if !s.is_empty() {
                s.push(' ');
            }
            let h = if self.hash.len() > 16 {
                format!("{}…", &self.hash[..16])
            } else {
                self.hash.clone()
            };
            s.push_str(&format!("#{h}"));
        }
        if s.is_empty() {
            s.push_str("(empty)");
        }
        s
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LootFile {
    #[serde(default, rename = "cred")]
    creds: Vec<Cred>,
}

/// Loot lives in the active room (loot.toml) or ~/.config/pivt/loot/<target>.toml.
pub fn loot_path(active_room: Option<&Room>, target: &str) -> PathBuf {
    if let Some(r) = active_room {
        return r.path.join("loot.toml");
    }
    let base = config_dir().join("loot");
    let name = if target.is_empty() {
        "global".to_string()
    } else {
        target
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
            .collect()
    };
    base.join(format!("{name}.toml"))
}

pub fn load(path: &PathBuf) -> Vec<Cred> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| toml::from_str::<LootFile>(&t).ok())
        .map(|f| f.creds)
        .unwrap_or_default()
}

pub fn save(path: &PathBuf, creds: &[Cred]) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let f = LootFile { creds: creds.to_vec() };
    if let Ok(txt) = toml::to_string_pretty(&f) {
        let _ = std::fs::write(path, txt);
    }
}
