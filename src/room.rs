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

/// Root holding all rooms: $CTF_ROOT or ~/TryHackMe (matches the shell kit).
pub fn ctf_root() -> PathBuf {
    if let Some(r) = std::env::var_os("CTF_ROOT") {
        return PathBuf::from(r);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TryHackMe")
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
    /// First few non-empty note lines, for a preview.
    pub fn notes_preview(&self, max: usize) -> Vec<String> {
        std::fs::read_to_string(self.notes_file())
            .map(|s| s.lines().map(|l| l.to_string()).take(max).collect())
            .unwrap_or_default()
    }
}

/// All rooms (dirs under CTF_ROOT marked with .ctfroom), newest first.
pub fn list_rooms() -> Vec<Room> {
    let root = ctf_root();
    let mut rooms: Vec<(std::time::SystemTime, Room)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&root) {
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

/// Scaffold a new room (dirs + notes.md + cheatsheet copy), like `ctf new`.
pub fn scaffold(name: &str, ip: &str, host: &str) -> std::io::Result<Room> {
    let dir = ctf_root().join(name);
    for sub in ["nmap", "loot", "exploits"] {
        std::fs::create_dir_all(dir.join(sub))?;
    }
    std::fs::write(dir.join(".ctfroom"), "")?;

    // copy the user's cheatsheet if it exists
    if let Some(home) = dirs::home_dir() {
        let sheet = home.join(".config/zsh/ctf-cheatsheet.md");
        if sheet.exists() {
            let _ = std::fs::copy(&sheet, dir.join("CHEATSHEET.md"));
        }
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
