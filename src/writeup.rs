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

/// Where job output is teed: `<room>/logs`, else ~/.local/share/pivt/logs.
pub fn logs_dir(active_room: Option<&Room>) -> PathBuf {
    match active_room {
        Some(r) => r.path.join("logs"),
        None => dirs::data_dir().unwrap_or_else(config_dir).join("pivt").join("logs"),
    }
}

/// A fresh `<YYYYmmdd-HHMMSS>-<slug>.log` path for a job titled `title`.
pub fn job_log_path(active_room: Option<&Room>, title: &str) -> PathBuf {
    let dir = logs_dir(active_room);
    let base = format!("{}-{}", now("%Y%m%d-%H%M%S"), slug(title));
    let mut p = dir.join(format!("{base}.log"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{base}-{n}.log"));
        n += 1;
    }
    p
}

/// "nmap Quick top-ports [2]" → "nmap-quick-top-ports-2" (max 48 chars).
fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(48).collect();
    if out.is_empty() {
        "job".into()
    } else {
        out.trim_end_matches('-').to_string()
    }
}

/// How a job log is referenced from the room's markdown: relative to the
/// room when it's inside it, else the full path.
fn log_ref(active_room: Option<&Room>, log: &Path) -> String {
    active_room
        .and_then(|r| log.strip_prefix(&r.path).ok())
        .unwrap_or(log)
        .display()
        .to_string()
}

/// Record a copied or run command (`job_log` = where a run's output goes).
/// Returns the log path.
pub fn log_command(
    active_room: Option<&Room>,
    target: &str,
    name: &str,
    category: &str,
    filled: &str,
    job_log: Option<&Path>,
) -> PathBuf {
    let path = log_path(active_room, target);
    ensure_header(&path, target);
    let ts = now("%H:%M:%S");
    let mut entry = format!("\n### `{name}`  ·  {category}  ·  {ts}\n```bash\n{filled}\n```\n");
    if let Some(l) = job_log {
        let r = log_ref(active_room, l);
        entry.push_str(&format!("output: [{r}]({r})\n"));
    }
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

    // every job's full output, oldest first (names start with the timestamp)
    if let Some(room) = active_room {
        let mut logs: Vec<PathBuf> = std::fs::read_dir(logs_dir(Some(room)))
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "log")).collect())
            .unwrap_or_default();
        logs.sort();
        if !logs.is_empty() {
            s.push_str("\n## Job output logs\n\nFull output of every command run from pivt (`less -R` keeps the colors).\n\n");
            for l in &logs {
                let r = log_ref(Some(room), l);
                let first = std::fs::read_to_string(l)
                    .ok()
                    .and_then(|t| t.lines().nth(1).map(|c| c.trim_start_matches("$ ").to_string()))
                    .unwrap_or_default();
                let first: String = first.chars().take(100).collect();
                s.push_str(&format!("- [{r}]({r}) — `{}`\n", first.replace('`', "'")));
            }
        }
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
        let p = log_command(Some(&room), "10.0.0.1", "Quick scan", "nmap", "nmap -sC 10.0.0.1", None);
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
        let job_log = job_log_path(Some(&room), "scan [1]");
        assert!(job_log.starts_with(dir.join("logs")) && job_log.to_string_lossy().ends_with("-scan-1.log"), "{job_log:?}");
        std::fs::create_dir_all(job_log.parent().unwrap()).unwrap();
        std::fs::write(&job_log, "── pivt job · scan ──\n$ nmap 10.0.0.1\n").unwrap();
        assert_ne!(job_log_path(Some(&room), "scan [1]"), job_log, "names don't collide");
        log_command(Some(&room), "10.0.0.1", "scan", "nmap", "nmap 10.0.0.1", Some(&job_log));
        let creds = vec![crate::loot::Cred { user: "svc".into(), pass: "pw".into(), hash: String::new(), note: "x".into() }];
        let out = export(Some(&room), "10.0.0.1", "codify.htb", "10.10.14.7", &creds).unwrap();
        let w = std::fs::read_to_string(&out).unwrap();
        assert!(w.contains("# Codify — writeup"));
        assert!(w.contains("## Credentials"));
        assert!(w.contains("| svc | pw |"));
        assert!(w.contains("## Command log"));
        assert!(w.contains("nmap 10.0.0.1"));
        let rel = job_log.strip_prefix(&dir).unwrap().display().to_string();
        assert!(w.contains(&format!("output: [{rel}]({rel})")), "per-command link missing:\n{w}");
        assert!(w.contains("## Job output logs") && w.contains(&format!("- [{rel}]({rel}) — `nmap 10.0.0.1`")), "{w}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
