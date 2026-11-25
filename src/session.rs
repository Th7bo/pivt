//! Persistent workspace sessions: what was on screen when pivt last closed
//! (room, selection, focus, run-pane layout) plus each run tab's command and
//! final output, so the next launch picks up where you left off.
//!
//! Layout (per room when one is active, else under the config dir):
//!
//! ```text
//! <room>/.pivt/session.toml           ~/.config/pivt/session/session.toml
//! <room>/.pivt/scrollback/tab-00.txt  ~/.config/pivt/session/scrollback/…
//! ~/.config/pivt/session/last-room    path of the last active room
//! ```
//!
//! Processes can't survive a restart, so tabs come back "dead": the saved
//! text is shown and `r` re-runs the command. Anything unreadable (corrupt
//! TOML, another format version, bad file names) is reported as an error
//! string and ignored; loading never panics.

use crate::model::config_dir;
use crate::room::Room;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Bump when the on-disk format changes incompatibly.
pub const VERSION: u32 = 1;
/// Most run tabs kept (the newest ones win).
pub const MAX_TABS: usize = 12;
/// Lines of scrollback kept per tab.
pub const MAX_LINES: usize = 2000;
/// Largest scrollback file read back (guards against a hand-edited monster).
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

const SESSION_FILE: &str = "session.toml";
const SCROLL_DIR: &str = "scrollback";
const LAST_ROOM_FILE: &str = "last-room";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    /// Unix seconds of the save (0 while building a comparison signature).
    pub saved_at: i64,
    /// The active room's directory, if any.
    pub room: Option<PathBuf>,
    pub category: String,
    /// Selected command as "category/name".
    pub command: String,
    /// categories | commands | preview | run
    pub focus: String,
    pub show_run: bool,
    pub run_fullscreen: bool,
    pub run_tab: usize,
    /// "✸ suggested" command keys + services from the last parsed scan.
    pub suggested: Vec<String>,
    pub scan_services: Vec<String>,
    // array of tables last: TOML needs plain values before tables
    #[serde(rename = "tab")]
    pub tabs: Vec<TabState>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(default)]
pub struct TabState {
    pub title: String,
    pub cmd: String,
    pub cwd: Option<PathBuf>,
    /// The process was still running when the session was saved.
    pub running: bool,
    pub exit: Option<i32>,
    /// Scrollback file name inside `scrollback/`.
    pub file: String,
    /// Tab output. On save, `None` leaves the existing file untouched; on
    /// load it's always `Some` (empty if the file is missing).
    #[serde(skip)]
    pub output: Option<String>,
}

/// Where the session lives for this room (or the global fallback).
pub fn dir_for(room: Option<&Room>) -> PathBuf {
    match room {
        Some(r) => r.path.join(".pivt"),
        None => config_dir().join("session"),
    }
}

pub fn tab_file(i: usize) -> String {
    format!("tab-{i:02}.txt")
}

/// The session file serialized, for writing and change detection.
pub fn to_toml(s: &Session) -> Result<String, String> {
    toml::to_string_pretty(s).map_err(|e| e.to_string())
}

/// Parse a session file. Corrupt files and other format versions are errors.
pub fn parse(text: &str) -> Result<Session, String> {
    let value: toml::Value =
        toml::from_str(text).map_err(|e| format!("corrupt session file ({})", first_line(&e.to_string())))?;
    let ver = value.get("version").and_then(|v| v.as_integer()).unwrap_or(0);
    if ver != VERSION as i64 {
        return Err(format!("session format v{ver}, this pivt reads v{VERSION}"));
    }
    let mut s: Session = value
        .try_into()
        .map_err(|e: toml::de::Error| format!("bad session file ({})", first_line(&e.to_string())))?;
    if s.tabs.len() > MAX_TABS {
        let cut = s.tabs.len() - MAX_TABS;
        s.tabs.drain(..cut);
        s.run_tab = s.run_tab.saturating_sub(cut);
    }
    Ok(s)
}

/// Load the session in `dir`. `Ok(None)` when there is none yet.
pub fn load(dir: &Path) -> Result<Option<Session>, String> {
    let text = match std::fs::read_to_string(dir.join(SESSION_FILE)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("can't read session ({e})")),
    };
    let mut s = parse(&text)?;
    let sdir = dir.join(SCROLL_DIR);
    for t in &mut s.tabs {
        let out = if safe_name(&t.file) { read_capped(&sdir.join(&t.file)) } else { String::new() };
        t.output = Some(trim_scrollback(&out, MAX_LINES));
    }
    Ok(Some(s))
}

/// Write the session to `dir`: scrollback files for tabs carrying output,
/// stale scrollback files removed, then session.toml (each atomically).
pub fn save(dir: &Path, s: &Session) -> std::io::Result<()> {
    let sdir = dir.join(SCROLL_DIR);
    std::fs::create_dir_all(&sdir)?;
    for t in &s.tabs {
        if let (Some(out), true) = (&t.output, safe_name(&t.file)) {
            write_atomic(&sdir.join(&t.file), out.as_bytes())?;
        }
    }
    if let Ok(rd) = std::fs::read_dir(&sdir) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if !s.tabs.iter().any(|t| t.file == name) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let text = to_toml(s).map_err(std::io::Error::other)?;
    write_atomic(&dir.join(SESSION_FILE), text.as_bytes())
}

/// The last active room, remembered so pivt can return to it when launched
/// from outside any room.
pub fn read_last_room() -> Option<PathBuf> {
    let s = std::fs::read_to_string(config_dir().join("session").join(LAST_ROOM_FILE)).ok()?;
    let p = PathBuf::from(s.trim());
    (!s.trim().is_empty()).then_some(p)
}

pub fn write_last_room(room: Option<&Path>) {
    let path = config_dir().join("session").join(LAST_ROOM_FILE);
    match room {
        Some(r) => {
            if let Some(d) = path.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let _ = write_atomic(&path, format!("{}\n", r.display()).as_bytes());
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Reduce captured PTY text to what a terminal would have left on screen,
/// roughly: carriage-return overwrites resolved (progress bars), control
/// characters dropped, trailing blank lines removed, last `max_lines` kept.
pub fn trim_scrollback(raw: &str, max_lines: usize) -> String {
    let mut lines: Vec<String> = raw
        .split('\n')
        .map(|l| {
            let l = l.trim_end_matches('\r');
            let l = l.rsplit('\r').next().unwrap_or("");
            l.chars().filter(|c| *c == '\t' || !c.is_control()).collect()
        })
        .collect();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].join("\n")
}

/// "3m ago" / "5h ago" / "2d ago" for a saved_at timestamp.
pub fn age(saved_at: i64, now: i64) -> String {
    let s = (now - saved_at).max(0);
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=86_399 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// A plain file name (no separators, no dot-files, no traversal).
fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn read_capped(path: &Path) -> String {
    let Ok(f) = std::fs::File::open(path) else { return String::new() };
    let mut buf = Vec::new();
    let _ = f.take(MAX_FILE_BYTES).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pivt-session-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn sample() -> Session {
        Session {
            version: VERSION,
            saved_at: 1_764_000_000,
            room: Some(PathBuf::from("/home/u/TryHackMe/lame")),
            category: "web".into(),
            command: "web/ffuf dirs".into(),
            focus: "run".into(),
            show_run: true,
            run_fullscreen: true,
            run_tab: 1,
            suggested: vec!["smb/smbclient list".into()],
            scan_services: vec!["ssh".into(), "http".into()],
            tabs: vec![
                TabState {
                    title: "nmap quick".into(),
                    cmd: "nmap -T4 10.10.10.3".into(),
                    cwd: Some(PathBuf::from("/home/u/TryHackMe/lame")),
                    running: false,
                    exit: Some(0),
                    file: tab_file(0),
                    output: Some("PORT   STATE SERVICE\n22/tcp open  ssh".into()),
                },
                TabState {
                    title: "listener".into(),
                    cmd: "nc -lvnp 4444".into(),
                    cwd: None,
                    running: true,
                    exit: None,
                    file: tab_file(1),
                    output: Some(String::new()),
                },
            ],
        }
    }

    #[test]
    fn roundtrip_through_disk() {
        let d = tmpdir("rt");
        let s = sample();
        save(&d, &s).unwrap();
        let back = load(&d).unwrap().expect("session present");
        assert_eq!(back, s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn save_without_output_keeps_file_and_prunes_stale() {
        let d = tmpdir("keep");
        let mut s = sample();
        save(&d, &s).unwrap();
        std::fs::write(d.join(SCROLL_DIR).join("tab-07.txt"), "old").unwrap();
        s.tabs[0].output = None; // unchanged: leave tab-00.txt alone
        save(&d, &s).unwrap();
        let back = load(&d).unwrap().unwrap();
        assert_eq!(back.tabs[0].output.as_deref(), Some("PORT   STATE SERVICE\n22/tcp open  ssh"));
        assert!(!d.join(SCROLL_DIR).join("tab-07.txt").exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn missing_session_is_none() {
        let d = tmpdir("none");
        assert_eq!(load(&d), Ok(None));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_and_foreign_files_are_errors_not_panics() {
        assert!(parse("this is = = not toml [[[").is_err());
        assert!(parse("").is_err()); // no version
        assert!(parse("version = 99\ncategory = \"web\"").unwrap_err().contains("v99"));
        assert!(parse("version = 1\ncategory = 5").is_err()); // wrong type
        assert!(parse("version = \"one\"").is_err());

        let d = tmpdir("corrupt");
        std::fs::write(d.join(SESSION_FILE), [0xff, 0xfe, 0x00, 0x9c]).unwrap();
        assert!(load(&d).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unknown_fields_and_missing_scrollback_tolerated() {
        let d = tmpdir("tolerant");
        std::fs::write(
            d.join(SESSION_FILE),
            "version = 1\nfuture_thing = true\n[[tab]]\ntitle = \"a\"\ncmd = \"id\"\nfile = \"tab-00.txt\"\n",
        )
        .unwrap();
        let s = load(&d).unwrap().unwrap();
        assert_eq!(s.tabs.len(), 1);
        assert_eq!(s.tabs[0].output.as_deref(), Some(""));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn traversal_file_names_are_not_read() {
        let d = tmpdir("trav");
        std::fs::write(d.join("secret"), "nope").unwrap();
        std::fs::write(
            d.join(SESSION_FILE),
            "version = 1\n[[tab]]\ntitle = \"a\"\ncmd = \"id\"\nfile = \"../secret\"\n",
        )
        .unwrap();
        let s = load(&d).unwrap().unwrap();
        assert_eq!(s.tabs[0].output.as_deref(), Some(""));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn tab_count_capped_to_newest() {
        let mut text = String::from("version = 1\nrun_tab = 14\n");
        for i in 0..15 {
            text.push_str(&format!("[[tab]]\ntitle = \"t{i}\"\ncmd = \"id\"\nfile = \"{}\"\n", tab_file(i)));
        }
        let s = parse(&text).unwrap();
        assert_eq!(s.tabs.len(), MAX_TABS);
        assert_eq!(s.tabs[0].title, "t3");
        assert_eq!(s.run_tab, 11);
    }

    #[test]
    fn scrollback_trimmed_and_cleaned() {
        assert_eq!(trim_scrollback("a\r\nb\r\n\r\n", 10), "a\nb");
        assert_eq!(trim_scrollback(" 10%\r 50%\r100%\ndone", 10), "100%\ndone");
        assert_eq!(trim_scrollback("x\x07y\tz", 10), "xy\tz");
        let many: String = (0..5000).map(|i| format!("line {i}\n")).collect();
        let t = trim_scrollback(&many, MAX_LINES);
        assert_eq!(t.lines().count(), MAX_LINES);
        assert!(t.starts_with("line 3000\n") && t.ends_with("line 4999"));
    }

    #[test]
    fn age_formats() {
        assert_eq!(age(100, 130), "just now");
        assert_eq!(age(0, 600), "10m ago");
        assert_eq!(age(0, 7200), "2h ago");
        assert_eq!(age(0, 200_000), "2d ago");
    }
}
