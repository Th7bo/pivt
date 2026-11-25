use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

const SCROLLBACK: usize = 2000;
const MAX_RAW: usize = 256 * 1024;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// A command running inside a real PTY. Output is fed to a vt100 parser so the
/// run pane shows a proper terminal screen (colors, cursor, redraws); the
/// master writer carries keystrokes back to the program (stdin).
///
/// A job restored from a saved session has no PTY at all: it only replays the
/// saved output (`restored` is set) until it's re-run.
pub struct RunJob {
    /// Unique per job (process-wide), so session saves can tell tabs apart.
    pub id: u64,
    pub title: String,
    pub cmd: String,
    pub cwd: Option<PathBuf>,
    /// `Some(was_running)` for a dead tab brought back from a saved session.
    pub restored: Option<bool>,
    parser: Arc<Mutex<vt100::Parser>>,
    raw: Arc<Mutex<String>>,
    writer: Option<Box<dyn Write + Send>>,
    master: Option<Box<dyn MasterPty + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    eof: Arc<AtomicBool>,
    pub exit: Option<i32>,
    pub finished: bool,
    pub scroll: usize,
    rows: u16,
    cols: u16,
    /// File the output is teed to, if any.
    pub log: Option<PathBuf>,
    /// For the exit footer; dropped once it's written so the log closes.
    log_tx: Option<mpsc::Sender<LogMsg>>,
}

enum LogMsg {
    Data(Vec<u8>),
    Footer(String),
}

impl RunJob {
    pub fn spawn(
        title: impl Into<String>,
        cmd: impl Into<String>,
        rows: u16,
        cols: u16,
        cwd: Option<&Path>,
    ) -> std::io::Result<RunJob> {
        Self::spawn_logged(title, cmd, rows, cols, cwd, None)
    }

    /// [`spawn`](Self::spawn), also teeing all output to `log` (created 0600).
    /// A log that can't be opened is skipped rather than failing the job.
    pub fn spawn_logged(
        title: impl Into<String>,
        cmd: impl Into<String>,
        rows: u16,
        cols: u16,
        cwd: Option<&Path>,
        log: Option<PathBuf>,
    ) -> std::io::Result<RunJob> {
        let title = title.into();
        let cmd = cmd.into();
        let rows = rows.max(4);
        let cols = cols.max(20);

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(to_io)?;

        let mut builder = CommandBuilder::new("sh");
        builder.arg("-c");
        builder.arg(&cmd);
        builder.env("TERM", "xterm-256color");
        if let Some(d) = cwd {
            builder.cwd(d);
        }
        let child = pair.slave.spawn_command(builder).map_err(to_io)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().map_err(to_io)?;
        let writer = pair.master.take_writer().map_err(to_io)?;

        let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, SCROLLBACK)));
        let raw = Arc::new(Mutex::new(String::new()));
        let eof = Arc::new(AtomicBool::new(false));
        let (log, log_tx) = match log.and_then(|p| start_log(&p, &title, &cmd, cwd).map(|tx| (p, tx))) {
            Some((p, tx)) => (Some(p), Some(tx)),
            None => (None, None),
        };
        {
            let p = parser.clone();
            let rawc = raw.clone();
            let e = eof.clone();
            let tee = log_tx.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            // never blocks: the log writer has its own thread
                            if let Some(t) = &tee {
                                let _ = t.send(LogMsg::Data(buf[..n].to_vec()));
                            }
                            if let Ok(mut pr) = p.lock() {
                                pr.process(&buf[..n]);
                            }
                            if let Ok(mut r) = rawc.lock() {
                                r.push_str(&strip_ansi(&String::from_utf8_lossy(&buf[..n])));
                                trim_front(&mut r, MAX_RAW);
                            }
                        }
                        Err(_) => break,
                    }
                }
                e.store(true, Ordering::SeqCst);
            });
        }

        Ok(RunJob {
            id: next_id(),
            title,
            cmd,
            cwd: cwd.map(Path::to_path_buf),
            restored: None,
            parser,
            raw,
            writer: Some(writer),
            master: Some(pair.master),
            child: Some(child),
            eof,
            exit: None,
            finished: false,
            scroll: 0,
            rows,
            cols,
            log,
            log_tx,
        })
    }

    /// A finished, PTY-less tab showing `text` saved by a previous session.
    /// `exit` is the recorded exit code (None if it was still running).
    #[allow(clippy::too_many_arguments)]
    pub fn restored(
        title: impl Into<String>,
        cmd: impl Into<String>,
        cwd: Option<PathBuf>,
        text: &str,
        was_running: bool,
        exit: Option<i32>,
        rows: u16,
        cols: u16,
    ) -> RunJob {
        let rows = rows.max(4);
        let cols = cols.max(20);
        let mut parser = vt100::Parser::new(rows, cols, SCROLLBACK);
        if !text.is_empty() {
            parser.process(text.replace('\n', "\r\n").as_bytes());
            parser.process(b"\r\n");
        }
        let note = if was_running { "restored · was running" } else { "restored" };
        parser.process(format!("\x1b[2m── {note} · r re-runs ──\x1b[0m").as_bytes());
        RunJob {
            id: next_id(),
            title: title.into(),
            cmd: cmd.into(),
            cwd,
            restored: Some(was_running),
            parser: Arc::new(Mutex::new(parser)),
            raw: Arc::new(Mutex::new(text.to_string())),
            writer: None,
            master: None,
            child: None,
            eof: Arc::new(AtomicBool::new(true)),
            exit,
            finished: true,
            scroll: 0,
            rows,
            cols,
            log: None,
            log_tx: None,
        }
    }

    pub fn parser(&self) -> Option<MutexGuard<'_, vt100::Parser>> {
        self.parser.lock().ok()
    }

    /// Full captured output (ANSI stripped) for parsing.
    pub fn raw_text(&self) -> String {
        self.raw.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// Size of the captured output, a cheap "has it changed" fingerprint.
    pub fn raw_len(&self) -> usize {
        self.raw.lock().map(|r| r.len()).unwrap_or(0)
    }

    pub fn poll(&mut self) {
        // once the output has drained, close the log with the exit status
        if self.finished && self.eof.load(Ordering::SeqCst) {
            if let Some(tx) = self.log_tx.take() {
                let how = match self.exit {
                    Some(-1) => "killed".to_string(),
                    Some(c) => format!("exit {c}"),
                    None => "ended".to_string(),
                };
                let _ = tx.send(LogMsg::Footer(format!("── {how} · {} ──", crate::room::now("%Y-%m-%d %H:%M:%S"))));
            }
        }
        if self.finished {
            return;
        }
        if let Some(child) = self.child.as_mut() {
            if let Ok(Some(status)) = child.try_wait() {
                self.exit = Some(status.exit_code() as i32);
                self.finished = true;
                self.child = None;
                return;
            }
        }
        // if output ended and the child is already gone, mark finished
        if self.eof.load(Ordering::SeqCst) && self.child.is_none() {
            self.finished = true;
        }
    }

    pub fn is_running(&self) -> bool {
        !self.finished
    }

    pub fn write_input(&mut self, bytes: &[u8]) {
        if self.finished {
            return;
        }
        // typing resumes tail-follow
        self.set_scroll(0);
        if let Some(w) = self.writer.as_mut() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    pub fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
            self.exit = Some(-1);
            self.finished = true;
            self.child = None;
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(4);
        let cols = cols.max(20);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        if let Some(m) = &self.master {
            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
        if let Ok(mut p) = self.parser.lock() {
            p.set_size(rows, cols);
        }
    }

    fn set_scroll(&mut self, n: usize) {
        self.scroll = n;
        if let Ok(mut p) = self.parser.lock() {
            p.set_scrollback(n);
        }
    }
    pub fn scroll_up(&mut self, n: usize) {
        let s = (self.scroll + n).min(SCROLLBACK);
        self.set_scroll(s);
    }
    pub fn scroll_down(&mut self, n: usize) {
        let s = self.scroll.saturating_sub(n);
        self.set_scroll(s);
    }
    pub fn scroll_tail(&mut self) {
        self.set_scroll(0);
    }
}

impl Drop for RunJob {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Open `path` for appending and start the thread that writes to it.
fn start_log(path: &Path, title: &str, cmd: &str, cwd: Option<&Path>) -> Option<mpsc::Sender<LogMsg>> {
    if let Some(d) = path.parent() {
        crate::model::create_private_dir(d).ok()?;
    }
    let f = crate::model::private_options().append(true).open(path).ok()?;
    let cwd = cwd.map(|d| d.display().to_string()).or_else(|| {
        std::env::current_dir().ok().map(|d| d.display().to_string())
    });
    let header = format!(
        "── pivt job · {title} · {} ──\n$ {cmd}\n(cwd {})\n\n",
        crate::room::now("%Y-%m-%d %H:%M:%S"),
        cwd.unwrap_or_else(|| "?".into())
    );
    let (tx, rx) = mpsc::channel::<LogMsg>();
    thread::spawn(move || {
        let mut w = std::io::BufWriter::new(f);
        let _ = w.write_all(header.as_bytes());
        let mut filt = LogFilter::default();
        loop {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(LogMsg::Data(d)) => {
                    let _ = w.write_all(&filt.feed(&d));
                }
                Ok(LogMsg::Footer(s)) => {
                    let _ = w.write_all(&filt.finish());
                    let _ = w.write_all(format!("\n{s}\n").as_bytes());
                }
                // idle: push what we have to disk so `tail -f` / less see it
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = w.flush();
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        let _ = w.write_all(&filt.finish());
        let _ = w.flush();
    });
    Some(tx)
}

/// Turns a PTY byte stream into a log readable with `less -R`: colors (SGR)
/// kept, other escape sequences dropped, CRLF → LF, a bare CR overwrites the
/// line (progress bars keep only their last state), backspace erases.
/// Works across chunk boundaries; lines are emitted when complete.
#[derive(Default)]
pub struct LogFilter {
    line: Vec<u8>,
    esc: Vec<u8>,
    cr: bool,
}

impl LogFilter {
    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for &b in data {
            if !self.esc.is_empty() {
                self.escape_byte(b);
                continue;
            }
            if self.cr {
                self.cr = false;
                // bare CR: the line gets redrawn (CR CR LF is still a newline)
                if b != b'\n' && b != b'\r' {
                    self.line.clear();
                }
            }
            match b {
                0x1b => self.esc.push(b),
                b'\n' => {
                    out.append(&mut self.line);
                    out.push(b'\n');
                }
                b'\r' => self.cr = true,
                0x08 => {
                    // drop one whole UTF-8 char
                    while let Some(x) = self.line.pop() {
                        if x & 0xc0 != 0x80 {
                            break;
                        }
                    }
                }
                b'\t' => self.line.push(b),
                0..=0x1f | 0x7f => {}
                _ => self.line.push(b),
            }
        }
        out
    }

    fn escape_byte(&mut self, b: u8) {
        self.esc.push(b);
        let e = &self.esc;
        match e[1] {
            b'[' if e.len() > 2 && (0x40..=0x7e).contains(&b) => {
                if b == b'm' {
                    self.line.extend_from_slice(e);
                }
                self.esc.clear();
            }
            b'[' => {
                if e.len() > 64 {
                    self.esc.clear();
                }
            }
            // OSC: ends with BEL or ST (ESC \)
            b']' => {
                if b == 0x07 || (b == b'\\' && e[e.len() - 2] == 0x1b) || e.len() > 4096 {
                    self.esc.clear();
                }
            }
            // charset selection etc. take one more byte
            b'(' | b')' | b'#' if e.len() < 3 => {}
            _ => self.esc.clear(),
        }
    }

    /// Whatever is left (a prompt without a newline).
    pub fn finish(&mut self) -> Vec<u8> {
        self.esc.clear();
        self.cr = false;
        std::mem::take(&mut self.line)
    }
}

/// Drop bytes from the front of `s` so it's at most ~`max` bytes, cutting on
/// a char boundary (a raw byte cut would panic mid UTF-8 sequence).
fn trim_front(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut cut = s.len() - max;
    while !s.is_char_boundary(cut) {
        cut += 1;
    }
    s.drain(..cut);
}

fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e.to_string())
}

/// Strip ANSI CSI/OSC escape sequences from captured output (for the raw buffer).
fn strip_ansi(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b {
            if i + 1 < b.len() && b[i + 1] == b'[' {
                i += 2;
                while i < b.len() && !(0x40..=0x7e).contains(&b[i]) {
                    i += 1;
                }
                i += 1;
                continue;
            } else if i + 1 < b.len() && b[i + 1] == b']' {
                i += 2;
                while i < b.len() && b[i] != 0x07 {
                    i += 1;
                }
                i += 1;
                continue;
            } else {
                i += 1;
                continue;
            }
        }
        let ch_len = match b[i] {
            x if x < 0x80 => 1,
            x if x >> 5 == 0b110 => 2,
            x if x >> 4 == 0b1110 => 3,
            x if x >> 3 == 0b11110 => 4,
            _ => 1,
        };
        if i + ch_len <= b.len() {
            if let Ok(ch) = std::str::from_utf8(&b[i..i + ch_len]) {
                out.push_str(ch);
            }
            i += ch_len;
        } else {
            i += 1;
        }
    }
    out
}

/// Flags whose next argument is a file the command writes.
const OUT_FLAGS: &[&str] = &["-oN", "-oA", "-oG", "-oX", "-oS", "-o", "-O", "--output", "--outfile", "--output-file"];

/// Relative directories a command line writes into (`-oN nmap/quick.txt`,
/// `> loot/x.hash`, `--output=out/r.json`). Absolute, `~`/`$`-prefixed,
/// quoted and `..` paths are left alone.
pub fn output_dirs(cmd: &str) -> Vec<PathBuf> {
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    let mut out: Vec<PathBuf> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        let path = if OUT_FLAGS.contains(t) || *t == ">" || *t == ">>" {
            toks.get(i + 1).copied()
        } else if let Some(p) = t.strip_prefix(">>").or_else(|| t.strip_prefix('>')) {
            Some(p)
        } else {
            OUT_FLAGS.iter().filter(|f| f.starts_with("--")).find_map(|f| t.strip_prefix(&format!("{f}=")))
        };
        let Some(path) = path.map(|p| p.trim_end_matches(';')) else { continue };
        let bad = path.is_empty()
            || path.starts_with(['/', '~', '$', '\'', '"', '-', '&'])
            || path.split('/').any(|c| c == "..");
        if bad {
            continue;
        }
        if let Some(parent) = Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()) {
            if !out.iter().any(|o| o == parent) {
                out.push(parent.to_path_buf());
            }
        }
    }
    out
}

/// Create the relative output dirs `cmd` writes into under `cwd`, so
/// `nmap -oN nmap/quick.txt` works outside a room too. Returns the ones made.
pub fn prepare_output_dirs(cmd: &str, cwd: &Path) -> Vec<String> {
    let mut made = Vec::new();
    for d in output_dirs(cmd) {
        let full = cwd.join(&d);
        if !full.is_dir() && std::fs::create_dir_all(&full).is_ok() {
            made.push(format!("{}/", d.display()));
        }
    }
    made
}

/// Approximate the run pane's inner (rows, cols) from the current terminal size,
/// matching the layout in ui.rs (bottom ~42% of the body, minus borders).
pub fn pane_size(fullscreen: bool) -> (u16, u16) {
    let (cols, rows) = crossterm::terminal::size().unwrap_or((100, 32));
    // body = total - header(3) - footer(1); run pane = 42% of body (or all of
    // it when zoomed), minus panel border (2) and the tab-bar row (1).
    let body = rows.saturating_sub(4);
    let run_h = if fullscreen { body } else { (body as u32 * 42 / 100) as u16 };
    let inner_rows = run_h.saturating_sub(3).max(4);
    let inner_cols = cols.saturating_sub(2).max(20);
    (inner_rows, inner_cols)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn screen_text(j: &RunJob) -> String {
        let p = j.parser().unwrap();
        let s = p.screen();
        let (rows, cols) = s.size();
        let mut t = String::new();
        for r in 0..rows {
            for c in 0..cols {
                if let Some(cell) = s.cell(r, c) {
                    let x = cell.contents();
                    t.push_str(if x.is_empty() { " " } else { &x });
                }
            }
            t.push('\n');
        }
        t
    }

    #[test]
    fn pty_stdin_roundtrip() {
        let mut j = RunJob::spawn("t", "cat", 10, 40, None).expect("spawn");
        std::thread::sleep(Duration::from_millis(200));
        j.write_input(b"pivt-works\r");
        std::thread::sleep(Duration::from_millis(250));
        let text = screen_text(&j);
        j.kill();
        assert!(text.contains("pivt-works"), "pty echo missing; screen:\n{text}");
    }

    #[test]
    fn raw_pty_passes_ctrl_c_through() {
        // cooked PTY: ^C kills the program (what used to kill caught shells)
        let mut cooked = RunJob::spawn("t", "cat", 6, 30, None).expect("spawn");
        std::thread::sleep(Duration::from_millis(200));
        cooked.write_input(b"\x03");
        // raw PTY (listener commands): ^C is just a byte for the program
        let raw_cmd = format!("{}cat", crate::model::RAW_PTY_PREFIX);
        let mut raw = RunJob::spawn("t", raw_cmd, 6, 30, None).expect("spawn");
        std::thread::sleep(Duration::from_millis(300));
        raw.write_input(b"\x03\x1aalive\r");
        for _ in 0..20 {
            cooked.poll();
            raw.poll();
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(cooked.finished, "^C should end cat in a cooked pty");
        assert!(raw.is_running(), "^C/^Z killed or stopped the raw-pty job");
        assert!(screen_text(&raw).contains("alive"), "input not passed through:\n{}", screen_text(&raw));
        raw.kill();
    }

    #[test]
    fn finds_relative_output_dirs() {
        let p = |v: &[&str]| v.iter().map(PathBuf::from).collect::<Vec<_>>();
        assert_eq!(output_dirs("nmap -sC -sV 10.0.0.1 -oN nmap/quick.txt"), p(&["nmap"]));
        assert_eq!(output_dirs("nmap -p- -oA nmap/full 10.0.0.1"), p(&["nmap"]));
        assert_eq!(output_dirs("ssh2john id_rsa > loot/hashes/key.hash"), p(&["loot/hashes"]));
        assert_eq!(output_dirs("x >>logs/a.txt; y --output=out/r.json"), p(&["logs", "out"]));
        // inputs, absolute, home, parent and bare-name outputs are left alone
        assert!(output_dirs("ffuf -w lists/x.txt -u http://h/FUZZ").is_empty());
        assert!(output_dirs("curl http://x/f -o /tmp/f").is_empty());
        assert!(output_dirs("x -o ~/f/g; y > ../z/w; z > $HOME/q/r; w -o shell.elf 2>/dev/null").is_empty());
        let d = std::env::temp_dir().join(format!("pivt-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(prepare_output_dirs("nmap -oN nmap/q.txt h; x > loot/a/b.txt", &d), vec!["nmap/", "loot/a/"]);
        assert!(d.join("nmap").is_dir() && d.join("loot/a").is_dir());
        assert!(prepare_output_dirs("nmap -oN nmap/q.txt h", &d).is_empty(), "already there");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn log_filter_cleans_pty_stream() {
        let mut f = LogFilter::default();
        let mut out = Vec::new();
        // split mid-escape, mid-CRLF and mid-UTF-8 on purpose
        for chunk in [
            &b"\x1b[1;32mok\x1b[0m \x1b]0;title\x07line\r"[..],
            b"\nprogress 10%\rprogress 100%\r\n\xe2\x9c",
            b"\xa6 star\x1b[2K\x1b[",
            b"Ksh$ lsx\x08 \x08\r\r\n\x1b(Bprompt$ ",
        ] {
            out.extend(f.feed(chunk));
        }
        out.extend(f.finish());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\x1b[1;32mok\x1b[0m line\nprogress 100%\n✦ starsh$ ls\nprompt$ "
        );
    }

    #[test]
    fn job_output_is_teed_to_log() {
        let d = std::env::temp_dir().join(format!("pivt-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let log = d.join("logs").join("t.log");
        let mut j = RunJob::spawn_logged("t", "printf 'a\\033[31mred\\033[0m\\r\\nb\\n'; exit 2", 6, 40, None, Some(log.clone())).unwrap();
        for _ in 0..80 {
            j.poll();
            if j.finished && j.log_tx.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(j.log.as_deref(), Some(log.as_path()));
        let mut body = String::new();
        for _ in 0..80 {
            body = std::fs::read_to_string(&log).unwrap_or_default();
            if body.contains("exit 2") {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(body.contains("pivt job · t"), "{body}");
        assert!(body.contains("a\x1b[31mred\x1b[0m\nb\n"), "{body:?}");
        assert!(body.contains("── exit 2"), "{body}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&log).unwrap().permissions().mode() & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn exit_code_captured() {
        let mut j = RunJob::spawn("t", "exit 3", 6, 20, None).expect("spawn");
        for _ in 0..40 {
            j.poll();
            if j.finished {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(j.exit, Some(3));
    }

    #[test]
    fn trim_front_respects_char_boundaries() {
        // every cut point lands inside some multibyte char at least once
        for max in 0..12 {
            let mut s = "a✦bé🔓c✦".repeat(3);
            trim_front(&mut s, max);
            assert!(s.len() <= max, "max {max}: {s:?}");
            assert!("a✦bé🔓c✦".repeat(3).ends_with(&s));
        }
        let mut s = "ok".to_string();
        trim_front(&mut s, 10);
        assert_eq!(s, "ok");
    }

    #[test]
    fn multibyte_output_past_raw_cap() {
        // > MAX_RAW of 3-byte chars, odd-sized chunks: the reader must keep
        // going (no panic, no poisoned lock) and the child must exit.
        let cmd = "i=0; while [ $i -lt 120 ]; do printf '✦%.0s' $(seq 1 1001); i=$((i+1)); done; echo done-✦";
        let mut j = RunJob::spawn("t", cmd, 6, 40, None).expect("spawn");
        for _ in 0..400 {
            j.poll();
            if j.finished && j.eof.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(j.finished, "child did not exit");
        assert!(j.raw.lock().is_ok(), "raw lock poisoned");
        let raw = j.raw_text();
        assert!(raw.len() <= MAX_RAW && raw.len() > MAX_RAW / 2, "len {}", raw.len());
        assert!(raw.contains("done-✦"), "tail missing");
    }

    #[test]
    fn restored_job_replays_text() {
        let mut j = RunJob::restored("t", "id", None, "uid=0(root)\n22/tcp open ssh", false, Some(0), 8, 40);
        j.poll();
        j.write_input(b"ignored");
        j.kill();
        assert!(!j.is_running());
        assert_eq!(j.exit, Some(0));
        assert_eq!(j.raw_text(), "uid=0(root)\n22/tcp open ssh");
        let text = screen_text(&j);
        assert!(text.contains("uid=0(root)") && text.contains("restored"), "screen:\n{text}");
    }
}
