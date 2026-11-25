use std::path::{Path, PathBuf};
use std::process::Command;

/// Current date/time via the `date` command (keeps us free of a time crate).
pub fn now(fmt: &str) -> String {
    Command::new("date")
        .arg(format!("+{fmt}"))
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// A public CTF platform a room belongs to. Only rooms known to be on one
/// get their name and link shown in the safe Discord presence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Htb,
    Thm,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Htb => "HTB",
            Platform::Thm => "THM",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Platform::Htb => "htb",
            Platform::Thm => "thm",
        }
    }
    /// Public page for a box/room of this name.
    pub fn url(self, name: &str) -> String {
        let slug: String = name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .collect();
        match self {
            Platform::Htb => format!("https://app.hackthebox.com/machines/{slug}"),
            Platform::Thm => format!("https://tryhackme.com/room/{}", slug.to_ascii_lowercase()),
        }
    }
}

/// `platform = "htb"` (or thm) from a `.ctfroom` marker's contents.
pub fn parse_platform(marker: &str) -> Option<Platform> {
    marker.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        if k.trim() != "platform" {
            return None;
        }
        match v.trim().trim_matches('"').to_ascii_lowercase().as_str() {
            "htb" | "hackthebox" => Some(Platform::Htb),
            "thm" | "tryhackme" => Some(Platform::Thm),
            _ => None,
        }
    })
}

#[derive(Debug, Clone)]
pub struct Room {
    pub name: String,
    pub path: PathBuf,
}

impl Room {
    pub fn target_file(&self) -> PathBuf {
        self.path.join(".target")
    }
    pub fn notes_file(&self) -> PathBuf {
        self.path.join("notes.md")
    }
    pub fn log_file(&self) -> PathBuf {
        self.path.join("pivt-log.md")
    }
    /// (ip, host) from the room's .target, if present.
    pub fn read_target(&self) -> Option<(String, Option<String>)> {
        let s = std::fs::read_to_string(self.target_file()).ok()?;
        let mut it = s.split_whitespace();
        let ip = it.next()?.to_string();
        let host = it.next().map(|h| h.to_string());
        Some((ip, host))
    }
    pub fn write_target(&self, ip: &str, host: &str) {
        let line = if host.is_empty() {
            format!("{ip}\n")
        } else {
            format!("{ip} {host}\n")
        };
        let _ = std::fs::write(self.target_file(), line);
    }
    /// Platform recorded in the room's `.ctfroom` marker, if any.
    pub fn platform(&self) -> Option<Platform> {
        std::fs::read_to_string(self.path.join(".ctfroom")).ok().as_deref().and_then(parse_platform)
    }
    /// Record (or clear) the room's platform in its `.ctfroom` marker.
    pub fn set_platform(&self, p: Option<Platform>) -> std::io::Result<()> {
        let body = p.map(|p| format!("platform = \"{}\"\n", p.key())).unwrap_or_default();
        std::fs::write(self.path.join(".ctfroom"), body)
    }
    /// First few non-empty note lines, for a preview.
    pub fn notes_preview(&self, max: usize) -> Vec<String> {
        std::fs::read_to_string(self.notes_file())
            .map(|s| s.lines().map(|l| l.to_string()).take(max).collect())
            .unwrap_or_default()
    }
}

/// All rooms (dirs under `root` marked with .ctfroom), newest first.
pub fn list_rooms(root: &Path) -> Vec<Room> {
    let mut rooms: Vec<(std::time::SystemTime, Room)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let p = e.path();
            if p.join(".ctfroom").exists() {
                let mtime = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);
                let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                rooms.push((mtime, Room { name, path: p }));
            }
        }
    }
    rooms.sort_by(|a, b| b.0.cmp(&a.0));
    rooms.into_iter().map(|(_, r)| r).collect()
}

/// Walk up from `start` looking for a `.ctfroom` marker.
pub fn find_room(start: &Path) -> Option<Room> {
    let mut d = start.to_path_buf();
    loop {
        if d.join(".ctfroom").exists() {
            let name = d.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
            return Some(Room { name, path: d });
        }
        if !d.pop() {
            return None;
        }
    }
}

pub fn find_room_cwd() -> Option<Room> {
    std::env::current_dir().ok().and_then(|d| find_room(&d))
}

/// Scaffold a new room under `root` (dirs + notes.md + optional template
/// copy), like `ctf new`.
pub fn scaffold(root: &Path, template: Option<&Path>, name: &str, ip: &str, host: &str) -> std::io::Result<Room> {
    if !root.exists() {
        // a fresh rooms dir will hold loot and notes: owner-only
        crate::model::create_private_dir(root)?;
    }
    let dir = root.join(name);
    for sub in ["nmap", "loot", "exploits"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    std::fs::write(dir.join(".ctfroom"), "")?;

    if let Some(t) = template.filter(|t| t.is_file()) {
        let _ = std::fs::copy(t, dir.join(template_dest(t)));
    }

    let notes = dir.join("notes.md");
    if !notes.exists() {
        let ipd = if ip.is_empty() { "?" } else { ip };
        let hostd = if host.is_empty() { "?" } else { host };
        let started = now("%Y-%m-%d %H:%M");
        let body = format!(
            "# {name}\n\n- **IP:** {ipd}  **Host:** {hostd}\n- **Started:** {started}\n\n\
             ## Recon\n- [ ] nmap initial / full\n- [ ] web: ferox / fuzz\n\n\
             ## Foothold\n\n## PrivEsc\n\n## Flags\n| flag | value |\n|---|---|\n| user | |\n| root | |\n\n\
             ## Creds / loot\n"
        );
        std::fs::write(notes, body)?;
    }

    let room = Room {
        name: name.to_string(),
        path: dir,
    };
    if !ip.is_empty() {
        room.write_target(ip, host);
    }
    Ok(room)
}

/// File name a template gets inside the room: the legacy zsh cheatsheet keeps
/// its old `CHEATSHEET.md` name, anything else keeps its own.
fn template_dest(t: &Path) -> String {
    match t.file_name().and_then(|n| n.to_str()) {
        Some("ctf-cheatsheet.md") | None => "CHEATSHEET.md".to_string(),
        Some(n) => n.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaffold_copies_template_only_when_given() {
        let base = std::env::temp_dir().join(format!("pivt-scaffold-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("rooms");
        let tpl = base.join("my-notes.md");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(&tpl, "# tpl").unwrap();
        let a = scaffold(&root, None, "a", "", "").unwrap();
        assert!(a.path.join(".ctfroom").exists() && a.notes_file().exists());
        assert_eq!(std::fs::read_dir(&a.path).unwrap().count(), 5, "nmap loot exploits .ctfroom notes.md only");
        let b = scaffold(&root, Some(&tpl), "b", "10.0.0.2", "b.htb").unwrap();
        assert_eq!(std::fs::read_to_string(b.path.join("my-notes.md")).unwrap(), "# tpl");
        assert_eq!(b.read_target(), Some(("10.0.0.2".into(), Some("b.htb".into()))));
        assert_eq!(template_dest(Path::new("/x/ctf-cheatsheet.md")), "CHEATSHEET.md");
        let names: Vec<_> = list_rooms(&root).into_iter().map(|r| r.name).collect();
        assert!(names.contains(&"a".to_string()) && names.contains(&"b".to_string()));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn platform_marker_roundtrip() {
        assert_eq!(parse_platform(""), None);
        assert_eq!(parse_platform("platform = \"htb\"\n"), Some(Platform::Htb));
        assert_eq!(parse_platform("platform=THM"), Some(Platform::Thm));
        assert_eq!(parse_platform("platform = \"corp\""), None);
        let dir = std::env::temp_dir().join(format!("pivt-plat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let r = Room { name: "Blue".into(), path: dir.clone() };
        r.set_platform(Some(Platform::Thm)).unwrap();
        assert_eq!(r.platform(), Some(Platform::Thm));
        r.set_platform(None).unwrap();
        assert_eq!(r.platform(), None);
        assert_eq!(Platform::Thm.url("Blue"), "https://tryhackme.com/room/blue");
        assert_eq!(Platform::Htb.url("Lame"), "https://app.hackthebox.com/machines/Lame");
        assert_eq!(Platform::Htb.url("a b/../c"), "https://app.hackthebox.com/machines/ab..c");
        std::fs::remove_dir_all(&dir).ok();
    }
}
