use std::io::Write;
use std::process::{Command, Stdio};

/// Copy text to the Wayland clipboard via wl-copy. Returns Ok on success.
pub fn copy(text: &str) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("wl-copy not found: {e}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "no stdin".to_string())?;
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| e.to_string())?;
    }
    // wl-copy forks and holds the selection; don't block on wait.
    Ok(())
}
