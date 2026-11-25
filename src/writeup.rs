use crate::model::{config_dir, create_private_dir, private_options, write_private};
use crate::room::{now, Room};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where the running log goes: the active room's dir if set, else
/// ~/.config/pivt/writeups/<target|date>.md.
pub fn log_path(active_room: Option<&Room>, target: &str) -> PathBuf {
    if let Some(r) = active_room {
        return r.log_file();
    }
    let base = config_dir().join("writeups");
    let _ = create_private_dir(&base);
    let name = if target.is_empty() {
        now("%Y-%m-%d")
    } else {
        sanitize(target)
    };
    base.join(format!("{name}.md"))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

fn ensure_header(path: &Path, target: &str) {
    if path.exists() {
        return;
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let tgt = if target.is_empty() { "(no target)" } else { target };
    let header = format!(
        "# pivt log — {tgt}\n\n_started {}_\n\nCommands run and notes, newest at the bottom.\n",
        now("%Y-%m-%d %H:%M")
    );
    // the log records filled commands, passwords included
    let _ = write_private(path, header);
}

fn append(path: &Path, text: &str) {
    if let Ok(mut f) = private_options().append(true).open(path) {
        let _ = f.write_all(text.as_bytes());
    }
}

/// Record a copied command. Returns the log path on success.
pub fn log_command(
    active_room: Option<&Room>,
    target: &str,
    name: &str,
    category: &str,
    filled: &str,
) -> PathBuf {
    let path = log_path(active_room, target);
    ensure_header(&path, target);
    let ts = now("%H:%M:%S");
    let entry = format!("\n### `{name}`  ·  {category}  ·  {ts}\n```bash\n{filled}\n```\n");
    append(&path, &entry);
    path
}

/// Record a freeform note/thought.
pub fn log_note(active_room: Option<&Room>, target: &str, text: &str) -> PathBuf {
    let path = log_path(active_room, target);
    ensure_header(&path, target);
    let ts = now("%H:%M:%S");
    append(&path, &format!("\n> **{ts}** {text}\n"));
    path
}

/// Record a captured flag: logs it and, if a room is active, updates the
/// `| user |` / `| root |` row in notes.md's Flags table.
pub fn set_flag(active_room: Option<&Room>, target: &str, which: &str, value: &str) -> PathBuf {
    let log = log_path(active_room, target);
    ensure_header(&log, target);
    let ts = now("%H:%M:%S");
    append(&log, &format!("\n> **{ts}** 🚩 {which} flag: `{value}`\n"));

    if let Some(room) = active_room {
        let notes = room.notes_file();
        if let Ok(body) = std::fs::read_to_string(&notes) {
            let mut found = false;
            let updated: String = body
                .lines()
                .map(|line| {
                    let t = line.trim_start();
                    if t.starts_with(&format!("| {which} ")) || t.starts_with(&format!("|{which}")) {
                        found = true;
                        format!("| {which} | {value} |")
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            let out = if found {
                updated
            } else {
                format!("{body}\n\n## Flags\n| flag | value |\n|---|---|\n| {which} | {value} |\n")
            };
            let _ = std::fs::write(&notes, out);
        }
    }
    log
}

/// Build a clean structured writeup from the room's notes, loot and command log.
pub fn export(
    active_room: Option<&Room>,
    target: &str,
    domain: &str,
    lhost: &str,
    loot: &[crate::loot::Cred],
) -> std::io::Result<PathBuf> {
    let out_path = match active_room {
        Some(r) => r.path.join("writeup.md"),
        None => {
            let base = config_dir().join("writeups");
            create_private_dir(&base)?;
            let name = if target.is_empty() { now("%Y-%m-%d") } else { sanitize(target) };
            base.join(format!("{name}-writeup.md"))
        }
    };

    let title = active_room
        .map(|r| r.name.clone())
        .unwrap_or_else(|| if target.is_empty() { "box".into() } else { target.to_string() });

    let mut s = String::new();
    s.push_str(&format!("# {title} — writeup\n\n"));
    s.push_str(&format!("- **Target:** {}\n", if target.is_empty() { "—" } else { target }));
    if !domain.is_empty() {
        s.push_str(&format!("- **Domain/host:** {domain}\n"));
    }
    if !lhost.is_empty() {
        s.push_str(&format!("- **Attacker IP:** {lhost}\n"));
    }
    s.push_str(&format!("- **Exported:** {}\n\n", now("%Y-%m-%d %H:%M")));

    if !loot.is_empty() {
        s.push_str("## Credentials\n\n| user | pass | hash | note |\n|---|---|---|---|\n");
        for c in loot {
            s.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                md_cell(&c.user),
                md_cell(&c.pass),
                md_cell(&c.hash),
                md_cell(&c.note)
            ));
        }
        s.push('\n');
    }

    if let Some(room) = active_room {
        if let Ok(notes) = std::fs::read_to_string(room.notes_file()) {
            s.push_str("## Notes\n\n");
            s.push_str(&notes);
            s.push_str("\n\n");
        }
    }

    let log = log_path(active_room, target);
    if let Ok(logtxt) = std::fs::read_to_string(&log) {
        s.push_str("## Command log\n\n");
        // skip the log's own H1 header line
        let body: String = logtxt.lines().skip_while(|l| l.starts_with("# ")).collect::<Vec<_>>().join("\n");
        s.push_str(body.trim_start());
        s.push('\n');
    }

    // carries the credentials table
    write_private(&out_path, s)?;
    Ok(out_path)
}

fn md_cell(s: &str) -> String {
    if s.is_empty() {
        " ".to_string()
    } else {
        s.replace('|', "\\|")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::Room;

    #[test]
    fn logs_command_into_room() {
        let dir = std::env::temp_dir().join(format!("pivt-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let room = Room { name: "t".into(), path: dir.clone() };
        let p = log_command(Some(&room), "10.0.0.1", "Quick scan", "nmap", "nmap -sC 10.0.0.1");
        let body = std::fs::read_to_string(&p).unwrap();
        assert!(body.contains("# pivt log"));
        assert!(body.contains("nmap -sC 10.0.0.1"));
        assert!(body.contains("Quick scan"));
        let p2 = log_note(Some(&room), "10.0.0.1", "got a shell");
        assert_eq!(p, p2);
        assert!(std::fs::read_to_string(&p2).unwrap().contains("got a shell"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn flag_updates_notes_and_export() {
        let dir = std::env::temp_dir().join(format!("pivt-we-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let room = Room { name: "Codify".into(), path: dir.clone() };
        std::fs::write(room.notes_file(),
            "# Codify\n\n## Flags\n| flag | value |\n|---|---|\n| user | |\n| root | |\n").unwrap();
        // capture a flag
        set_flag(Some(&room), "10.0.0.1", "user", "FLAG123");
        let notes = std::fs::read_to_string(room.notes_file()).unwrap();
        assert!(notes.contains("| user | FLAG123 |"), "notes:\n{notes}");
        // a logged command so the export has a command section
        log_command(Some(&room), "10.0.0.1", "scan", "nmap", "nmap 10.0.0.1");
        let creds = vec![crate::loot::Cred { user: "svc".into(), pass: "pw".into(), hash: String::new(), note: "x".into() }];
        let out = export(Some(&room), "10.0.0.1", "codify.htb", "10.10.14.7", &creds).unwrap();
        let w = std::fs::read_to_string(&out).unwrap();
        assert!(w.contains("# Codify — writeup"));
        assert!(w.contains("## Credentials"));
        assert!(w.contains("| svc | pw |"));
        assert!(w.contains("## Command log"));
        assert!(w.contains("nmap 10.0.0.1"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
