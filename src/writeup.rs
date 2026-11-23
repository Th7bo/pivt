use crate::model::config_dir;
use crate::room::{now, Room};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

/// Where the running log goes: the active room's dir if set, else
/// ~/.config/pivt/writeups/<target|date>.md.
pub fn log_path(active_room: Option<&Room>, target: &str) -> PathBuf {
    if let Some(r) = active_room {
        return r.log_file();
    }
    let base = config_dir().join("writeups");
    let _ = std::fs::create_dir_all(&base);
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

fn ensure_header(path: &PathBuf, target: &str) {
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
    let _ = std::fs::write(path, header);
}

fn append(path: &PathBuf, text: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
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
}
