//! Per-room command history: every copy and run, append-only JSON lines in
//! `<room>/.pivt/history.jsonl` (no room: `~/.config/pivt/session/`).
//!
//! A run is written when it starts (with its log path); its exit code is
//! appended later as a separate `exit` record that points back at it, so
//! the file is never rewritten. Filled commands can hold passwords: the
//! file is 0600 and its directory 0700.

use crate::model::{create_private_dir, private_options, restrict};
use crate::room::Room;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FILE: &str = "history.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Copy,
    Run,
    /// Exit status of an earlier run (`run` = that run's id).
    Exit,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Copy => "copy",
            Action::Run => "run",
            Action::Exit => "exit",
        }
    }
}

/// One line of history.jsonl.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Unix seconds.
    pub ts: i64,
    /// Local time as written ("2026-11-25 14:03:22").
    #[serde(default)]
    pub time: String,
    pub action: Action,
    /// Command key ("category/name").
    #[serde(default)]
    pub key: String,
    /// Display name (a run tab title, e.g. "web recon (multi) [2]").
    #[serde(default)]
    pub name: String,
    /// The filled command line.
    #[serde(default)]
    pub cmd: String,
    /// Run id (runs), or the run an `exit` record belongs to.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log: Option<PathBuf>,
}

impl Entry {
    pub fn new(action: Action, key: &str, name: &str, cmd: &str) -> Entry {
        Entry {
            ts: crate::discord::now_unix(),
            time: crate::room::now("%Y-%m-%d %H:%M:%S"),
            action,
            key: key.to_string(),
            name: name.to_string(),
            cmd: cmd.to_string(),
            run: String::new(),
            exit: None,
            log: None,
        }
    }

    /// The `exit` record for run `run`.
    pub fn exit_of(run: &str, code: i32) -> Entry {
        Entry { run: run.to_string(), exit: Some(code), ..Entry::new(Action::Exit, "", "", "") }
    }

    /// "11-25 14:03" (or the raw time / nothing if it's odd).
    pub fn short_time(&self) -> String {
        let t: Vec<char> = self.time.chars().collect();
        if t.len() >= 16 {
            t[5..16].iter().collect()
        } else {
            self.time.clone()
        }
    }

    pub fn category(&self) -> &str {
        self.key.split_once('/').map(|(c, _)| c).unwrap_or("")
    }
}

/// The history file for a room (or the global session dir).
pub fn path_for(room: Option<&Room>) -> PathBuf {
    crate::session::dir_for(room).join(FILE)
}

/// Append one entry (creating the dir 0700 and the file 0600).
pub fn append(path: &Path, e: &Entry) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let mut line = serde_json::to_string(e).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut f = private_options().append(true).open(path)?;
    restrict(path, 0o600)?;
    f.write_all(line.as_bytes())
}

/// All copies and runs, newest first, with exit codes folded into their
/// runs. Unreadable lines are skipped; a missing file is empty.
pub fn load(path: &Path) -> Vec<Entry> {
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    parse(&text)
}

pub fn parse(text: &str) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut exits: std::collections::HashMap<String, i32> = Default::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(e) = serde_json::from_str::<Entry>(line) else { continue };
        match e.action {
            Action::Exit => {
                if let (false, Some(c)) = (e.run.is_empty(), e.exit) {
                    exits.insert(e.run, c);
                }
            }
            _ => out.push(e),
        }
    }
    for e in &mut out {
        if e.action == Action::Run && e.exit.is_none() {
            e.exit = exits.get(&e.run).copied();
        }
    }
    // file order is oldest first; stable sort keeps same-second order
    out.reverse();
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pivt-hist-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn entry(ts: i64, action: Action, name: &str) -> Entry {
        Entry { ts, time: format!("2026-11-25 14:0{}:00", ts % 10), ..Entry::new(action, &format!("web/{name}"), name, &format!("{name} -x 'p@ss'")) }
    }

    #[test]
    fn append_and_read_back_newest_first() {
        let dir = tmpdir("rt");
        let path = dir.join(".pivt").join(FILE);
        let mut run = entry(2, Action::Run, "ffuf");
        run.run = "r1".into();
        run.log = Some(dir.join("logs/x.log"));
        append(&path, &entry(1, Action::Copy, "nmap")).unwrap();
        append(&path, &run).unwrap();
        append(&path, &entry(3, Action::Copy, "cewl")).unwrap();
        append(&path, &Entry::exit_of("r1", 2)).unwrap();
        let got = load(&path);
        assert_eq!(got.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["cewl", "ffuf", "nmap"]);
        assert_eq!(got[1].exit, Some(2), "exit folded into its run");
        assert_eq!(got[1].log, run.log);
        assert_eq!(got[1].cmd, "ffuf -x 'p@ss'");
        assert_eq!(got[0].exit, None);
        assert_eq!(got[0].short_time(), "11-25 14:03");
        assert_eq!(got[0].category(), "web");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&path), 0o600);
            assert_eq!(mode(path.parent().unwrap()), 0o700);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let good = serde_json::to_string(&entry(5, Action::Run, "nmap")).unwrap();
        let text = format!(
            "{{not json\n\n{good}\n{{\"ts\":1,\"action\":\"bogus\"}}\n\u{0}\u{1}garbage\n{{\"ts\":\"x\"}}\n{}\n{{\"ts\":4,\"action\":\"copy\"",
            serde_json::to_string(&entry(6, Action::Copy, "cewl")).unwrap()
        );
        let got = parse(&text);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "cewl");
        assert_eq!(got[1].name, "nmap");
        assert!(load(Path::new("/nonexistent/pivt/history.jsonl")).is_empty());
    }

    #[test]
    fn same_second_keeps_append_order_newest_first() {
        let text = [entry(7, Action::Copy, "a"), entry(7, Action::Copy, "b"), entry(9, Action::Run, "c"), entry(7, Action::Copy, "d")]
            .iter()
            .map(|e| serde_json::to_string(e).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let names: Vec<String> = parse(&text).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["c", "d", "b", "a"]);
    }
}
