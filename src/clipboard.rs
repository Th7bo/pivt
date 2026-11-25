//! Clipboard copy with fallbacks: wl-copy (Wayland), xclip / xsel (X11), and
//! finally OSC 52, which asks the terminal itself to set the clipboard and so
//! also works over SSH.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A way of getting text onto the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    WlCopy,
    Xclip,
    Xsel,
    Osc52,
}

impl Method {
    pub fn label(self) -> &'static str {
        match self {
            Method::WlCopy => "wl-copy",
            Method::Xclip => "xclip",
            Method::Xsel => "xsel",
            Method::Osc52 => "OSC 52",
        }
    }

    fn argv(self) -> &'static [&'static str] {
        match self {
            Method::WlCopy => &["wl-copy"],
            Method::Xclip => &["xclip", "-selection", "clipboard"],
            Method::Xsel => &["xsel", "--clipboard", "--input"],
            Method::Osc52 => &[],
        }
    }
}

/// What the environment offers, for [`plan`].
#[derive(Clone, Copy, Default)]
pub struct Env {
    pub wayland: bool,
    pub x11: bool,
    pub wl_copy: bool,
    pub xclip: bool,
    pub xsel: bool,
}

impl Env {
    fn detect() -> Env {
        let set = |k: &str| std::env::var_os(k).is_some_and(|v| !v.is_empty());
        Env {
            wayland: set("WAYLAND_DISPLAY"),
            x11: set("DISPLAY"),
            wl_copy: on_path("wl-copy"),
            xclip: on_path("xclip"),
            xsel: on_path("xsel"),
        }
    }
}

/// Methods to try, in order. OSC 52 always comes last: it can't report
/// failure, so it's only used when nothing local worked.
pub fn plan(env: Env) -> Vec<Method> {
    let mut v = Vec::new();
    if env.wayland && env.wl_copy {
        v.push(Method::WlCopy);
    }
    if env.x11 {
        if env.xclip {
            v.push(Method::Xclip);
        }
        if env.xsel {
            v.push(Method::Xsel);
        }
    }
    v.push(Method::Osc52);
    v
}

/// Copy `text`; returns the method that took it.
pub fn copy(text: &str) -> Result<Method, String> {
    let mut errs = Vec::new();
    for m in plan(Env::detect()) {
        let r = match m {
            Method::Osc52 => osc52(text),
            _ => run_tool(m, text),
        };
        match r {
            Ok(()) => return Ok(m),
            Err(e) => errs.push(format!("{}: {e}", m.label())),
        }
    }
    Err(errs.join("; "))
}

/// Pipe `text` into a clipboard tool. They all fork to keep serving the
/// selection, so the foreground process exits quickly; a non-zero exit (no
/// compositor / X server) counts as failure.
fn run_tool(m: Method, text: &str) -> Result<(), String> {
    let argv = m.argv();
    let mut child = Command::new(argv[0])
        .args(&argv[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    {
        let stdin = child.stdin.as_mut().ok_or("no stdin")?;
        stdin.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    }
    drop(child.stdin.take());
    let deadline = Instant::now() + Duration::from_millis(800);
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return Ok(()),
            Ok(Some(st)) => return Err(format!("exited {}", st.code().unwrap_or(-1))),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                // still holding the selection in the foreground: fine, reap later
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
                return Ok(());
            }
            Err(e) => return Err(e.to_string()),
        }
    }
}

fn osc52(text: &str) -> Result<(), String> {
    let seq = osc52_sequence(text, std::env::var_os("TMUX").is_some());
    let mut out = std::io::stdout();
    out.write_all(seq.as_bytes()).and_then(|_| out.flush()).map_err(|e| e.to_string())
}

/// `ESC ] 52 ; c ; <base64> BEL`, wrapped in a DCS passthrough inside tmux.
pub fn osc52_sequence(text: &str, tmux: bool) -> String {
    let seq = format!("\x1b]52;c;{}\x07", base64(text.as_bytes()));
    if tmux {
        format!("\x1bPtmux;{}\x1b\\", seq.replace('\x1b', "\x1b\x1b"))
    } else {
        seq
    }
}

/// Standard base64 with padding.
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                s.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o, "{i:?}");
        }
        assert_eq!(base64("✦'$".as_bytes()), "4pymJyQ=");
    }

    #[test]
    fn osc52_plain_and_tmux() {
        assert_eq!(osc52_sequence("hi", false), "\x1b]52;c;aGk=\x07");
        assert_eq!(osc52_sequence("hi", true), "\x1bPtmux;\x1b\x1b]52;c;aGk=\x07\x1b\\");
    }

    #[test]
    fn plan_prefers_native_tools_then_osc52() {
        use Method::*;
        let all = Env { wayland: true, x11: true, wl_copy: true, xclip: true, xsel: true };
        assert_eq!(plan(all), vec![WlCopy, Xclip, Xsel, Osc52]);
        // XWayland session without wl-copy installed
        assert_eq!(plan(Env { wl_copy: false, ..all }), vec![Xclip, Xsel, Osc52]);
        // plain X11, only xsel
        assert_eq!(plan(Env { x11: true, xsel: true, ..Env::default() }), vec![Xsel, Osc52]);
        // wl-copy installed but not on Wayland (e.g. over SSH): skip it
        assert_eq!(plan(Env { wl_copy: true, xclip: true, ..Env::default() }), vec![Osc52]);
        assert_eq!(plan(Env::default()), vec![Osc52]);
    }
}
