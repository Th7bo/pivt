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

/// What replaces a flag or credential value in a redacted export.
pub const REDACTED: &str = "‹redacted›";

/// How to export. The default (secrets out, no active box) is the safe one.
#[derive(Debug, Clone, Default)]
pub struct ExportOpts {
    /// Keep flags and credential values in. Off unless the user says yes.
    pub include_secrets: bool,
    /// Name of a not-yet-retired HTB machine the panel knows about: the
    /// export is marked PRIVATE (file name + header).
    pub active_box: Option<String>,
    /// Extra values to scrub (PASS/HASH-style variables).
    pub secrets: Vec<String>,
    /// The room's copy/run history (newest first): runs become the
    /// "Commands run" table.
    pub history: Vec<crate::history::Entry>,
}

/// Variables whose values are credentials (PASS, HASH, WPSCAN_TOKEN, …).
pub fn is_secret_var(name: &str) -> bool {
    ["PASS", "HASH", "TOKEN", "SECRET", "KEY"].iter().any(|k| name.contains(k))
}

/// The output file: `writeup.md` in the room (or `<target>-writeup.md` in
/// the config dir), with `-PRIVATE` before `.md` for an active box.
pub fn export_path(active_room: Option<&Room>, target: &str, private: bool) -> PathBuf {
    let suffix = if private { "-PRIVATE" } else { "" };
    match active_room {
        Some(r) => r.path.join(format!("writeup{suffix}.md")),
        None => {
            let name = if target.is_empty() { now("%Y-%m-%d") } else { sanitize(target) };
            config_dir().join("writeups").join(format!("{name}-writeup{suffix}.md"))
        }
    }
}

/// Build a clean structured writeup from the room's notes, loot and command
/// log. Unless `opts.include_secrets`, flags and credential values are
/// replaced with [`REDACTED`] and the structure is kept.
pub fn export(
    active_room: Option<&Room>,
    target: &str,
    domain: &str,
    lhost: &str,
    loot: &[crate::loot::Cred],
    opts: &ExportOpts,
) -> std::io::Result<PathBuf> {
    let out_path = export_path(active_room, target, opts.active_box.is_some());
    if let Some(dir) = out_path.parent() {
        if active_room.is_none() {
            create_private_dir(dir)?;
        }
    }
    let mut secrets: Vec<String> = loot.iter().flat_map(|c| [c.pass.clone(), c.hash.clone()]).collect();
    secrets.extend(opts.secrets.iter().cloned());
    let scrub = |text: &str| if opts.include_secrets { text.to_string() } else { redact(text, &secrets) };

    let title = active_room
        .map(|r| r.name.clone())
        .unwrap_or_else(|| if target.is_empty() { "box".into() } else { target.to_string() });

    let mut s = String::new();
    s.push_str(&format!("# {title} — writeup\n\n"));
    if let Some(m) = &opts.active_box {
        s.push_str(&format!(
            "> **PRIVATE — active box.** {m} is an active Hack The Box machine: sharing \
             solutions, flags or walkthroughs breaks HTB's rules. Keep this file local \
             until the machine retires.\n\n"
        ));
    }
    if opts.include_secrets {
        s.push_str("> **Contains flags and plaintext credentials.** Keep it private.\n\n");
    } else {
        s.push_str(&format!(
            "> Flags and credential values are replaced with `{REDACTED}`. Linked job \
             logs are raw and not redacted. Review before sharing.\n\n"
        ));
    }
    s.push_str(&format!("- **Target:** {}\n", if target.is_empty() { "—" } else { target }));
    if !domain.is_empty() {
        s.push_str(&format!("- **Domain/host:** {domain}\n"));
    }
    if !lhost.is_empty() {
        s.push_str(&format!("- **Attacker IP:** {lhost}\n"));
    }
    s.push_str(&format!("- **Exported:** {}\n\n", now("%Y-%m-%d %H:%M")));

    if !loot.is_empty() {
        let hide = |v: &str| if opts.include_secrets || v.is_empty() { v.to_string() } else { REDACTED.to_string() };
        s.push_str("## Credentials\n\n| user | pass | hash | note |\n|---|---|---|---|\n");
        for c in loot {
            s.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                md_cell(&c.user),
                md_cell(&hide(&c.pass)),
                md_cell(&hide(&c.hash)),
                md_cell(&scrub(&c.note))
            ));
        }
        s.push('\n');
    }

    if let Some(room) = active_room {
        if let Ok(notes) = std::fs::read_to_string(room.notes_file()) {
            s.push_str("## Notes\n\n");
            s.push_str(&scrub(&notes));
            s.push_str("\n\n");
        }
    }

    // runs from the history, oldest first, with their exit status
    let runs: Vec<&crate::history::Entry> =
        opts.history.iter().rev().filter(|e| e.action == crate::history::Action::Run).collect();
    if !runs.is_empty() {
        s.push_str("## Commands run\n\n| when | command | exit |\n|---|---|---|\n");
        for e in runs {
            let exit = match e.exit {
                Some(0) => "0".to_string(),
                Some(-1) => "killed".to_string(),
                Some(c) => c.to_string(),
                None => "?".to_string(),
            };
            let line = scrub(&e.cmd).replace('`', "'");
            s.push_str(&format!(
                "| {} | **{}** `{}` | {} |\n",
                md_cell(&e.short_time()),
                md_cell(&e.name),
                md_cell(&line),
                exit
            ));
        }
        s.push('\n');
    }

    let log = log_path(active_room, target);
    if let Ok(logtxt) = std::fs::read_to_string(&log) {
        s.push_str("## Command log\n\n");
        // skip the log's own H1 header line
        let body: String = logtxt.lines().skip_while(|l| l.starts_with("# ")).collect::<Vec<_>>().join("\n");
        s.push_str(&scrub(body.trim_start()));
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
                let first: String = scrub(&first).chars().take(100).collect();
                s.push_str(&format!("- [{r}]({r}) — `{}`\n", first.replace('`', "'")));
            }
        }
    }

    // may carry the credentials table
    write_private(&out_path, s)?;
    Ok(out_path)
}

/// Scrub flags and credential values from markdown, keeping its structure:
/// every known secret value, the value cells of a "Flags" table, backticked
/// values on pivt's flag / answer / cracked log lines, and flag-shaped tokens
/// (`HTB{…}`, `THM{…}`, `flag{…}`, bare 32-hex strings).
pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut secrets: Vec<&str> = secrets.iter().map(|s| s.trim()).filter(|s| s.chars().count() >= 3).collect();
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    secrets.dedup();
    let mut in_flags = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            in_flags = t.to_ascii_lowercase().contains("flag");
        }
        let mut l = line.to_string();
        for sec in &secrets {
            l = l.replace(sec, REDACTED);
        }
        if in_flags && t.starts_with('|') {
            l = redact_flag_row(&l);
        }
        if ["🚩", "🧩", "🔓"].iter().any(|m| l.contains(m)) {
            l = redact_backticks(&l);
        }
        out.push(redact_tokens(&l));
    }
    let mut r = out.join("\n");
    if text.ends_with('\n') {
        r.push('\n');
    }
    r
}

/// `| user | HTB{x} |` → `| user | ‹redacted› |`; header, separator and
/// empty cells stay.
fn redact_flag_row(line: &str) -> String {
    let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
    if cells.len() < 2 || cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':'))) {
        return line.to_string();
    }
    if cells[0].eq_ignore_ascii_case("flag") || cells[1].eq_ignore_ascii_case("value") {
        return line.to_string();
    }
    let rest: Vec<&str> = cells[1..].iter().map(|c| if c.is_empty() { "" } else { REDACTED }).collect();
    format!("| {} | {} |", cells[0], rest.join(" | "))
}

/// Replace every `` `span` `` with `` `‹redacted›` ``.
fn redact_backticks(line: &str) -> String {
    let mut out = String::new();
    let mut parts = line.split('`');
    if let Some(first) = parts.next() {
        out.push_str(first);
    }
    let rest: Vec<&str> = parts.collect();
    for (i, p) in rest.iter().enumerate() {
        out.push('`');
        // odd pieces are inside backticks, unless it's an unclosed trailing one
        if i % 2 == 0 && i + 1 < rest.len() {
            out.push_str(REDACTED);
        } else {
            out.push_str(p);
        }
    }
    out
}

/// Scrub flag-shaped tokens: `HTB{…}` / `THM{…}` / `flag{…}` / `CTF{…}` and
/// standalone 32-hex strings (HTB user/root flags, NT hashes).
fn redact_tokens(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if boundary {
            // PREFIX{...}
            let word_end = (i..chars.len()).find(|&j| !chars[j].is_ascii_alphanumeric()).unwrap_or(chars.len());
            let word: String = chars[i..word_end].iter().collect::<String>().to_ascii_lowercase();
            if matches!(word.as_str(), "htb" | "thm" | "flag" | "ctf") && chars.get(word_end) == Some(&'{') {
                if let Some(close) = (word_end + 1..chars.len()).find(|&j| chars[j] == '}') {
                    out.push_str(REDACTED);
                    i = close + 1;
                    continue;
                }
            }
            // bare 32 hex chars
            let hex_end = (i..chars.len()).find(|&j| !chars[j].is_ascii_hexdigit()).unwrap_or(chars.len());
            let after_ok = hex_end == chars.len() || !chars[hex_end].is_ascii_alphanumeric();
            if hex_end - i == 32 && after_ok {
                out.push_str(REDACTED);
                i = hex_end;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
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
    fn export_lists_history_runs_redacted() {
        use crate::history::{Action, Entry};
        let dir = std::env::temp_dir().join(format!("pivt-test-hist-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let room = Room { name: "Box".into(), path: dir.clone() };
        let mut run = Entry::new(Action::Run, "web/hydra ssh", "hydra ssh", "hydra -l admin -p 'S3cret!' ssh://10.0.0.1");
        run.exit = Some(0);
        let copy = Entry::new(Action::Copy, "web/curl verbose", "curl verbose", "curl -v http://x");
        let opts = ExportOpts { secrets: vec!["S3cret!".into()], history: vec![copy, run], ..ExportOpts::default() };
        let out = export(Some(&room), "10.0.0.1", "", "", &[], &opts).unwrap();
        let w = std::fs::read_to_string(&out).unwrap();
        assert!(w.contains("## Commands run"), "{w}");
        assert!(w.contains("**hydra ssh**") && w.contains("| 0 |"), "{w}");
        assert!(!w.contains("S3cret!"), "secret leaked: {w}");
        assert!(!w.contains("curl verbose"), "copies aren't runs: {w}");
        let _ = std::fs::remove_dir_all(&dir);
    }

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
        let opts = ExportOpts { include_secrets: true, ..ExportOpts::default() };
        let out = export(Some(&room), "10.0.0.1", "codify.htb", "10.10.14.7", &creds, &opts).unwrap();
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

    #[test]
    fn redact_keeps_structure() {
        let notes = "# Box\n\n## Flags\n| flag | value |\n|---|---|\n| user | 0123456789abcdef0123456789abcdef |\n| root | |\n\n## Foothold\n| a | b |\n";
        let r = redact(notes, &[]);
        assert!(r.contains("| flag | value |\n|---|---|"), "{r}");
        assert!(r.contains("| user | ‹redacted› |"), "{r}");
        assert!(r.contains("| root |  |"), "empty cell stays empty: {r}");
        assert!(r.contains("| a | b |"), "tables outside Flags untouched: {r}");
        assert!(!r.contains("0123456789abcdef"));
        // log lines pivt writes for flags, answers and cracks
        let log = "> **12:00:01** 🚩 root flag: `s3cr3t-flag`\n> **12:00:02** 🧩 Task 3 → `admin123`\n> **12:00:03** 🔓 cracked svc: `Summer2024!`\n";
        let r = redact(log, &[]);
        for leak in ["s3cr3t-flag", "admin123", "Summer2024!"] {
            assert!(!r.contains(leak), "{leak} leaked: {r}");
        }
        assert!(r.contains("🚩 root flag: `‹redacted›`"), "{r}");
        assert!(r.ends_with('\n'));
        // known secrets in filled commands, flag-shaped tokens anywhere
        let cmd = "nxc smb 10.0.0.1 -u svc -p 'Hunter22' && echo THM{y0u_g0t_1t} HTB{x} flag{a b}\nnot a flag: deadbeef cafe";
        let r = redact(cmd, &["Hunter22".into(), "".into(), "ab".into()]);
        assert_eq!(r, "nxc smb 10.0.0.1 -u svc -p '‹redacted›' && echo ‹redacted› ‹redacted› ‹redacted›\nnot a flag: deadbeef cafe");
        // too-short secrets are ignored rather than shredding the text
        assert_eq!(redact("a b ab", &["ab".into()]), "a b ab");
        // 33 hex chars isn't a flag
        let long = "0123456789abcdef0123456789abcdef0";
        assert_eq!(redact(long, &[]), long);
    }

    #[test]
    fn export_redacts_by_default_and_marks_active_boxes() {
        let dir = std::env::temp_dir().join(format!("pivt-wr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let room = Room { name: "Lame".into(), path: dir.clone() };
        std::fs::write(room.notes_file(), "# Lame\n\n## Flags\n| flag | value |\n|---|---|\n| user | |\n| root | |\n").unwrap();
        set_flag(Some(&room), "10.0.0.9", "user", "HTB{user_flag}");
        set_flag(Some(&room), "10.0.0.9", "root", "fedcba9876543210fedcba9876543210");
        log_command(Some(&room), "10.0.0.9", "smb", "smb", "smbclient -U svc%P4ssw0rd! //10.0.0.9/tmp", None);
        log_command(Some(&room), "10.0.0.9", "pth", "ad", "nxc smb 10.0.0.9 -H aad3b435:31d6cfe0d16ae931", None);
        let creds = vec![crate::loot::Cred { user: "svc".into(), pass: "P4ssw0rd!".into(), hash: String::new(), note: "smb".into() }];
        let opts = ExportOpts {
            include_secrets: false,
            active_box: Some("Lame".into()),
            secrets: vec!["aad3b435:31d6cfe0d16ae931".into()],
            history: vec![],
        };
        let out = export(Some(&room), "10.0.0.9", "", "", &creds, &opts).unwrap();
        assert_eq!(out, dir.join("writeup-PRIVATE.md"));
        let w = std::fs::read_to_string(&out).unwrap();
        for leak in ["HTB{user_flag}", "fedcba9876543210", "P4ssw0rd!", "aad3b435"] {
            assert!(!w.contains(leak), "{leak} leaked:\n{w}");
        }
        assert!(w.contains("PRIVATE — active box") && w.contains("‹redacted›"), "{w}");
        assert!(w.contains("| svc | ‹redacted› |   | smb |"), "creds table keeps users + structure:\n{w}");
        assert!(w.contains("| user | ‹redacted› |") && w.contains("smbclient -U svc%‹redacted› //10.0.0.9/tmp"), "{w}");
        // opting in keeps everything; a retired / unknown box isn't PRIVATE
        let all = ExportOpts { include_secrets: true, ..ExportOpts::default() };
        let out = export(Some(&room), "10.0.0.9", "", "", &creds, &all).unwrap();
        assert_eq!(out, dir.join("writeup.md"));
        let w = std::fs::read_to_string(&out).unwrap();
        assert!(w.contains("HTB{user_flag}") && w.contains("| svc | P4ssw0rd! |") && !w.contains("PRIVATE"), "{w}");
        assert!(is_secret_var("PASS") && is_secret_var("WPSCAN_TOKEN") && !is_secret_var("USER"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
