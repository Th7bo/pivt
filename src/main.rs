mod anim;
mod app;
mod clipboard;
mod discord;
mod gtfobins;
mod hashid;
mod htb;
mod loot;
mod model;
mod netif;
mod ping;
mod recipe;
mod room;
mod runner;
mod session;
mod theme;
mod ui;
mod writeup;

use app::{App, Focus, Mode};
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste,
        EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, Stdout};
use std::time::{Duration, Instant};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "snap" {
        snapshot(&args[2], theme_arg(&args).as_deref());
        return Ok(());
    }
    install_panic_hook();
    let mut terminal = setup()?;
    let mut app = App::new();
    if let Some(t) = theme_arg(&args) {
        app.theme_override(&t);
    }
    // `--fresh` skips restoring the last workspace (it's overwritten on the next save)
    app.restore_session(args.iter().any(|a| a == "--fresh"));
    let res = run(&mut terminal, &mut app);
    restore(&mut terminal)?;
    app.save_session(true);
    app.persist_config();
    res
}

/// `--theme <name>` / `--theme=<name>`, else `$PIVT_THEME`: a theme for this
/// run only (not saved).
fn theme_arg(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--theme" {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix("--theme=") {
            return Some(v.to_string());
        }
    }
    std::env::var("PIVT_THEME").ok().filter(|v| !v.trim().is_empty())
}

/// Headless render for development/screenshots: `pivt snap splash:8` or `pivt snap normal`.
/// Never loads or saves the persistent session, so screenshots stay clean.
/// `theme`: `--theme <name>` or `$PIVT_THEME`, e.g. `pivt snap normal --theme nord`.
fn snapshot(what: &str, theme: Option<&str>) {
    use ratatui::backend::TestBackend;
    // no Discord / ping threads: a snapshot must exit right away
    let mut app = App::headless();
    if let Some(t) = theme {
        app.theme_override(t);
    }
    app.tick = 60;
    match what {
        s if s.starts_with("splash:") => {
            app.tick = s.strip_prefix("splash:").and_then(|n| n.parse().ok()).unwrap_or(0);
            app.splash_start = 0;
        }
        "pvar" => {
            app.mode = Mode::Normal;
            app.focus = Focus::Preview;
        }
        "pedit" => {
            app.mode = Mode::Normal;
            app.focus = Focus::Preview;
            app.inline_begin();
            if let Some((_, b)) = app.inline_edit.as_mut() {
                *b = "10.10.14.7".into();
            }
        }
        "rooms" => app.open_rooms(),
        "newroom" => {
            app.mode = Mode::Normal;
            app.new_room_open();
        }
        "note" => {
            app.mode = Mode::Normal;
            app.note_open();
            app.note_buf = "found creds in /var/backups — svc:Summer2024".into();
        }
        "run" | "runzoom" => {
            app.mode = Mode::Normal;
            app.logging = false;
            let zoom = what == "runzoom";
            let (r, c) = if zoom { (26, 108) } else { (18, 100) };
            if let Ok(j) = runner::RunJob::spawn(
                "nmap quick",
                "printf 'Starting Nmap 7.95\\nNmap scan report for 10.10.11.42\\nPORT    STATE SERVICE\\n22/tcp  open  ssh\\n80/tcp  open  http\\n445/tcp open  microsoft-ds\\n'; sleep 0.05; echo 'Nmap done: 1 IP address scanned'",
                r, c, None,
            ) {
                app.runs.push(j);
            }
            if let Ok(j) = runner::RunJob::spawn("ffuf dirs", "sleep 5", r, c, None) {
                app.runs.push(j);
            }
            app.show_run = true;
            app.focus = Focus::Run;
            app.run_fullscreen = zoom;
            std::thread::sleep(std::time::Duration::from_millis(250));
            for rj in &mut app.runs {
                rj.poll();
            }
        }
        "loot" => {
            app.loot = vec![
                loot::Cred { user: "svc-web".into(), pass: "Summer2024!".into(), hash: String::new(), note: "from config.php".into() },
                loot::Cred { user: "administrator".into(), pass: String::new(), hash: "aad3b435b51404eeaad3b435b51404ee:31d6cfe0d16ae931b73c59d7e0c089c0".into(), note: "secretsdump".into() },
            ];
            app.mode = Mode::Loot;
        }
        "addloot" => {
            app.add_loot_open();
            app.loot_form.user = "svc-web".into();
            app.loot_form.pass = "Summer2024!".into();
        }
        "flow" => {
            app.mode = Mode::Normal;
            app.flow_bg = true;
            app.tick = 120;
            app.mouse = Some((70, 18));
        }
        "flowsplash" => {
            app.flow_bg = true;
            app.tick = 22;
            app.splash_start = 0;
        }
        "parse" => {
            app.mode = Mode::Normal;
            app.logging = false;
            app.vars.insert("RHOST".into(), "10.10.11.42".into());
            app.vars.insert("URL".into(), String::new());
            if let Ok(j) = runner::RunJob::spawn(
                "nmap",
                "printf 'PORT    STATE SERVICE\\n22/tcp  open  ssh\\n80/tcp  open  http\\n445/tcp open  microsoft-ds\\n389/tcp open  ldap\\n'",
                18, 100, None,
            ) {
                app.runs.push(j);
            }
            app.show_run = true;
            std::thread::sleep(std::time::Duration::from_millis(200));
            for r in &mut app.runs {
                r.poll();
            }
            app.parse_scan();
        }
        "gtfo" => {
            app.open_gtfo();
            app.gtfo_query = "find".into();
        }
        "burst" => {
            app.mode = Mode::Normal;
            app.tick = 100;
            app.burst_start = Some(88); // ~40% progress
            app.burst_label = "ROOT PWNED".into();
        }
        "flag" => {
            app.mode = Mode::Normal;
            app.flag_open();
            app.flag_is_root = true;
            app.flag_buf = "a1b2c3d4e5f6...".into();
        }
        "vars" => app.mode = Mode::Vars,
        "export" | "export-active" => {
            if what == "export-active" {
                app.htb = htb::Htb::with_state(htb::HtbState {
                    machine: Some(htb::Machine { name: "Lame".into(), ..Default::default() }),
                    ..Default::default()
                });
            }
            app.export_open();
        }
        "add" => {
            app.add_open();
            app.add.template = "ffuf -u <URL>/FUZZ -w <WORDLIST>".into();
            app.add.field = 3;
        }
        "help" => app.mode = Mode::Help,
        "themes" => app.open_theme_picker(),
        "restored" => {
            // dead tabs as they come back from a saved session
            app.mode = Mode::Normal;
            let (r, c) = (18, 100);
            app.runs.push(runner::RunJob::restored(
                "nmap quick",
                "nmap -T4 --top-ports 1000 10.10.11.42",
                None,
                "Starting Nmap 7.95\nNmap scan report for 10.10.11.42\nPORT    STATE SERVICE\n22/tcp  open  ssh\n80/tcp  open  http\nNmap done: 1 IP address scanned",
                false,
                Some(0),
                r,
                c,
            ));
            app.runs.push(runner::RunJob::restored(
                "listener", "nc -lvnp 4444", None, "listening on [any] 4444 ...", true, None, r, c,
            ));
            app.show_run = true;
            app.focus = Focus::Run;
            app.status = "↺ restored session (14h ago) · 2 tabs · r re-runs in the run pane".into();
        }
        s if s.starts_with("htb-live:") => {
            // Real API, read-only: load a machine and render the panel.
            app.htb.load(Some(s["htb-live:".len()..].to_string()), None);
            let t0 = std::time::Instant::now();
            while app.htb.is_busy() && t0.elapsed() < std::time::Duration::from_secs(30) {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            app.mode = Mode::Htb;
        }
        "htb" => {
            use htb::*;
            let t = |id, title: &str, q: &str, hint: &str, fmt: &str, done, kind, prereq| Task {
                id, title: title.into(), question: q.into(), hint: hint.into(), format: fmt.into(),
                completed: done, kind, prereq,
            };
            app.htb = Htb::with_state(HtbState {
                machine: Some(Machine {
                    id: 1, name: "Lame".into(), os: "Linux".into(), difficulty: "Easy".into(), points: 20,
                    ip: Some("10.10.10.3".into()), synopsis: "Lame is an easy Linux machine, requiring only one exploit to obtain root access.".into(),
                    user_owned: false, root_owned: false, guided: true, retired: true, spawned: true,
                }),
                tasks: vec![
                    t(1, "Task 1", "How many of the `nmap` top 1000 TCP ports are open on the remote host?", "Run a `nmap` scan on the target.", "number, such as 3, 17, or 4567", true, TaskKind::Task, None),
                    t(2, "Task 2", "What version of VSFTPd is running on Lame?", "The -sV flag tells nmap to fingerprint versions.", "*.*.*", false, TaskKind::Task, Some(1)),
                    t(3, "Task 3", "There is a famous backdoor in VSFTPd version 2.3.4, and a Metasploit module to exploit it. Does that exploit work here?", "", "yes or no", false, TaskKind::Task, Some(2)),
                    t(4, "Submit User Flag", "", "", "32 hex characters", false, TaskKind::User, None),
                    t(5, "Submit Root Flag", "", "", "32 hex characters", false, TaskKind::Root, None),
                ],
                busy: None,
                message: Some((true, "loaded Lame".into())),
                events: vec![],
            });
            app.mode = Mode::Htb;
            app.htb_idx = 1;
            app.htb_hint = true;
        }
        "recipes" => {
            app.vars.insert("PORTS".into(), "22,80,139,445".into());
            app.scan_services = vec!["ssh".into(), "http".into(), "microsoft-ds".into()];
            app.open_recipes();
        }
        _ => app.mode = Mode::Normal,
    }
    let backend = TestBackend::new(110, 32);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::draw(f, &app)).unwrap();
    let buf = term.backend().buffer();
    let area = buf.area;
    // PIVT_SNAP_ANSI=1 emits truecolor fg/bg + bold per cell (for README screenshots).
    let ansi = std::env::var_os("PIVT_SNAP_ANSI").is_some();
    let rgb = |c: ratatui::style::Color, default: (u8, u8, u8)| match c {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => default,
    };
    for y in 0..area.height {
        let mut line = String::new();
        for x in 0..area.width {
            if let Some(cell) = buf.cell((x, y)) {
                if ansi {
                    let th = theme::theme();
                    let (fr, fg, fb) = rgb(cell.fg, rgb(th.fg, (0, 0, 0)));
                    let (br, bg, bb) = rgb(cell.bg, rgb(th.bg, (0, 0, 0)));
                    let bold = cell.modifier.contains(ratatui::style::Modifier::BOLD) as u8;
                    line.push_str(&format!(
                        "\x1b[0;{}38;2;{fr};{fg};{fb};48;2;{br};{bg};{bb}m",
                        if bold == 1 { "1;" } else { "" }
                    ));
                }
                line.push_str(cell.symbol());
            }
        }
        if ansi {
            println!("{line}\x1b[0m");
        } else {
            println!("{}", line.trim_end());
        }
    }
}

/// Suspend the TUI, run $EDITOR on a file, then restore the TUI.
/// `mouse_on`: mouse capture is currently enabled (turned off for the editor,
/// or its escape sequences land in the file, then back on).
fn edit_in_external(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    path: &std::path::Path,
    mouse_on: bool,
) -> io::Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
    disable_raw_mode()?;
    if mouse_on {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    execute!(terminal.backend_mut(), DisableBracketedPaste, LeaveAlternateScreen)?;
    let status = std::process::Command::new(&editor).arg(path).status();
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableBracketedPaste)?;
    if mouse_on {
        execute!(terminal.backend_mut(), EnableMouseCapture)?;
    }
    terminal.clear()?;
    if status.is_err() {
        // best-effort; the next draw shows a message if needed
    }
    Ok(())
}

fn setup() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Bracketed paste: a paste arrives as one Event::Paste instead of a burst of
    // KeyEvents (where every newline would be Enter and letters shortcuts).
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// On a main-thread panic, put the terminal back (raw mode off, alt screen,
/// mouse, paste) before the default hook prints the message, so the shell
/// isn't left broken. Background threads (discord/htb/ping/readers) catch
/// their own panics and the UI keeps running, so for those the terminal is
/// left alone and the message is only written to a log file (stderr would
/// scribble over the TUI).
fn install_panic_hook() {
    let default = std::panic::take_hook();
    let main_id = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() != main_id {
            log_thread_panic(&info.to_string());
            return;
        }
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::Show
        );
        default(info);
    }));
}

/// Append a background thread's panic message to ~/.config/pivt/panic.log.
fn log_thread_panic(msg: &str) {
    use std::io::Write;
    let name = std::thread::current().name().unwrap_or("unnamed").to_string();
    let dir = model::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("panic.log")) {
        let _ = writeln!(f, "[{}] thread '{name}': {msg}", discord::now_unix());
    }
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>, app: &mut App) -> io::Result<()> {
    let tick_rate = Duration::from_millis(50); // ~20 fps
    let mut last_tick = Instant::now();
    let mut mouse_on = false;
    loop {
        // Capture the mouse for flow-field ripples and for scroll-wheel over the
        // run pane (so you can scroll output even while typing into it). Off
        // otherwise, so normal terminal text selection still works.
        let want_mouse = app.flow_bg || app.show_run;
        if want_mouse != mouse_on {
            if want_mouse {
                let _ = execute!(terminal.backend_mut(), EnableMouseCapture);
            } else {
                let _ = execute!(terminal.backend_mut(), DisableMouseCapture);
                app.mouse = None;
            }
            mouse_on = want_mouse;
        }
        terminal.draw(|f| ui::draw(f, app))?;
        if app.should_quit {
            return Ok(());
        }
        // Starfield animates continuously, so always pace to the tick rate.
        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => handle_key(app, key),
                Event::Paste(text) => handle_paste(app, &text),
                Event::Mouse(me) => match me.kind {
                    // wheel scrolls the run pane — works even while interacting
                    MouseEventKind::ScrollUp => app.run_scroll_up(3),
                    MouseEventKind::ScrollDown => app.run_scroll_down(3),
                    _ => app.mouse = Some((me.column, me.row)),
                },
                _ => {}
            }
        }
        if let Some(path) = app.pending_editor.take() {
            edit_in_external(terminal, &path, mouse_on)?;
            last_tick = Instant::now();
        }
        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    // the quit prompt swallows the next key: y quits, anything else cancels
    if app.confirm_quit {
        app.answer_quit(matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')));
        return;
    }
    // interacting with a job's PTY: keystrokes go to the program.
    // Detach with F12 (layout-independent; Ctrl-] also works on QWERTY).
    if app.mode == Mode::Normal && app.focus == Focus::Run && app.interacting {
        let detach = key.code == KeyCode::F(12)
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char(']'));
        if detach {
            app.end_interact();
            return;
        }
        if let Some(bytes) = encode_key(key) {
            app.run_input(&bytes);
        }
        return;
    }
    // global: ctrl-c quits (asking first while jobs are running)
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.request_quit();
        return;
    }
    match app.mode {
        Mode::Splash => app.end_splash(),
        Mode::Normal => normal_keys(app, key),
        Mode::Search => search_keys(app, key),
        Mode::Vars => vars_keys(app, key),
        Mode::Add => add_keys(app, key),
        Mode::Help => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::Enter) {
                app.mode = Mode::Normal;
            }
        }
        Mode::Rooms => rooms_keys(app, key),
        Mode::Note => note_keys(app, key),
        Mode::NewRoom => newroom_keys(app, key),
        Mode::Loot => loot_keys(app, key),
        Mode::AddLoot => addloot_keys(app, key),
        Mode::Flag => flag_keys(app, key),
        Mode::Gtfo => gtfo_keys(app, key),
        Mode::Recipes => recipe_keys(app, key),
        Mode::Htb => htb_keys(app, key),
        Mode::Theme => match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('t') => app.theme_pick_move(true),
            KeyCode::Up | KeyCode::Char('k') => app.theme_pick_move(false),
            KeyCode::Enter => app.theme_pick_confirm(),
            KeyCode::Char('r') => app.reload_themes(),
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('T') => app.theme_pick_cancel(),
            _ => {}
        },
        Mode::Export => match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => app.export_writeup(true),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Enter => app.export_writeup(false),
            KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
            _ => {}
        },
    }
}

/// Route a bracketed paste to whatever is taking text input right now. Never
/// replays it as keystrokes: with no text field active the paste is dropped.
fn handle_paste(app: &mut App, text: &str) {
    if text.is_empty() || app.confirm_quit {
        return;
    }
    // interacting with a job: hand the paste to the program in the PTY
    if app.mode == Mode::Normal && app.focus == Focus::Run && app.interacting {
        let bracketed = app
            .runs
            .get(app.run_tab)
            .and_then(|j| j.parser().map(|p| p.screen().bracketed_paste()))
            .unwrap_or(false);
        app.run_input(&pty_paste_bytes(text, bracketed));
        return;
    }
    // every in-app text field is single-line
    let line = single_line(text);
    if line.is_empty() {
        return;
    }
    match app.mode {
        Mode::Normal => {
            if app.inline_edit.is_some() {
                line.chars().for_each(|c| app.inline_push(c));
            }
            // otherwise: no text field, ignore
        }
        Mode::Search => line.chars().for_each(|c| app.search_push(c)),
        Mode::Vars => {
            if let Some(s) = app.var_new_key.as_mut() {
                s.push_str(&line);
            } else if app.var_editing {
                app.var_buf.push_str(&line);
            }
        }
        Mode::Add => app.add.buf_mut().push_str(&line),
        Mode::AddLoot => app.loot_form.buf_mut().push_str(&line),
        Mode::NewRoom => app.new_room.buf_mut().push_str(&line),
        Mode::Note => app.note_buf.push_str(&line),
        Mode::Flag => app.flag_buf.push_str(&line),
        Mode::Gtfo => line.chars().for_each(|c| app.gtfo_push(c)),
        Mode::Htb => {
            if let Some(b) = app.htb_input_buf() {
                b.push_str(&line);
            }
        }
        Mode::Splash | Mode::Help | Mode::Rooms | Mode::Loot | Mode::Recipes | Mode::Export | Mode::Theme => {}
    }
}

/// Flatten a paste for a single-line field: trailing line breaks dropped,
/// inner line breaks and tabs become spaces, other control chars removed.
fn single_line(text: &str) -> String {
    let text = text.trim_end_matches(['\r', '\n']);
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(' ');
            }
            '\n' | '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Bytes for a paste into a PTY program, as a terminal would send them: line
/// breaks become CR, and if the program enabled bracketed paste (mode 2004)
/// the text is wrapped in the 200~/201~ markers. Any end marker inside the
/// paste is stripped so pasted text can't break out of the bracket.
fn pty_paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let body = text
        .replace("\x1b[201~", "")
        .replace("\r\n", "\r")
        .replace('\n', "\r");
    let mut out = Vec::with_capacity(body.len() + 12);
    if bracketed {
        out.extend_from_slice(b"\x1b[200~");
    }
    out.extend_from_slice(body.as_bytes());
    if bracketed {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}

fn gtfo_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Enter => app.gtfo_copy(),
        KeyCode::Down => app.gtfo_next(),
        KeyCode::Up => app.gtfo_prev(),
        KeyCode::Backspace => app.gtfo_pop(),
        KeyCode::Char(c) => app.gtfo_push(c),
        _ => {}
    }
}

fn flag_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Tab | KeyCode::Up | KeyCode::Down => app.flag_toggle_which(),
        KeyCode::Enter => app.flag_save(),
        KeyCode::Backspace => {
            app.flag_buf.pop();
        }
        KeyCode::Char(c) => app.flag_buf.push(c),
        _ => {}
    }
}

fn htb_keys(app: &mut App, key: KeyEvent) {
    if app.htb_input.is_some() {
        match key.code {
            KeyCode::Esc => app.htb_input = None,
            KeyCode::Enter => app.htb_input_submit(),
            KeyCode::Backspace => {
                if let Some(b) = app.htb_input_buf() {
                    b.pop();
                }
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {}
            KeyCode::Char(c) => {
                if let Some(b) = app.htb_input_buf() {
                    b.push(c);
                }
            }
            _ => {}
        }
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Down | KeyCode::Char('j') => app.htb_next(),
        KeyCode::Up | KeyCode::Char('k') => app.htb_prev(),
        KeyCode::Enter => app.htb_answer_begin(),
        KeyCode::Char('h') => app.htb_hint = !app.htb_hint,
        KeyCode::Char('m') => app.htb_input = Some(app::HtbInput::Machine(String::new())),
        KeyCode::Char('r') => {
            app.htb.refresh();
        }
        KeyCode::Char('s') => {
            app.htb.spawn_machine();
        }
        KeyCode::Char('t') => {
            app.htb.stop_machine();
        }
        _ => {}
    }
}

fn recipe_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Down | KeyCode::Char('j') => app.recipe_next(),
        KeyCode::Up | KeyCode::Char('k') => app.recipe_prev(),
        KeyCode::Enter => app.run_selected_recipe(),
        KeyCode::Char('A') => app.run_matched_recipes(),
        _ => {}
    }
}

fn loot_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Down | KeyCode::Char('j') => app.loot_next(),
        KeyCode::Up | KeyCode::Char('k') => app.loot_prev(),
        KeyCode::Enter | KeyCode::Char('u') => app.loot_use(),
        KeyCode::Char('a') => app.add_loot_open(),
        KeyCode::Char('d') => app.loot_delete(),
        KeyCode::Char('y') => app.loot_copy_pass(),
        KeyCode::Char('Y') => app.loot_copy_field(2), // hash
        KeyCode::Char('U') => app.loot_copy_field(0), // user
        KeyCode::Char('c') => app.loot_crack(),
        _ => {}
    }
}

fn addloot_keys(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        if app.add_loot_save() {
            app.mode = Mode::Loot;
        }
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Loot,
        KeyCode::Tab | KeyCode::Down => app.add_loot_next(),
        KeyCode::BackTab | KeyCode::Up => app.add_loot_prev(),
        KeyCode::Enter => {
            if app.loot_form.field == 3 {
                if app.add_loot_save() {
                    app.mode = Mode::Loot;
                }
            } else {
                app.add_loot_next();
            }
        }
        KeyCode::Backspace => {
            app.loot_form.buf_mut().pop();
        }
        KeyCode::Char(c) => app.loot_form.buf_mut().push(c),
        _ => {}
    }
}

fn rooms_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Down | KeyCode::Char('j') => app.rooms_next(),
        KeyCode::Up | KeyCode::Char('k') => app.rooms_prev(),
        KeyCode::Enter => app.activate_selected_room(),
        KeyCode::Char('n') => app.new_room_open(),
        KeyCode::Char('p') => app.rooms_cycle_platform(),
        KeyCode::Char('o') => {
            // open the highlighted room's notes
            if let Some(r) = app.rooms.get(app.room_idx) {
                app.pending_editor = Some(r.notes_file());
                app.mode = Mode::Normal;
            }
        }
        _ => {}
    }
}

fn note_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Enter => app.note_commit(),
        KeyCode::Backspace => {
            app.note_buf.pop();
        }
        KeyCode::Char(c) => app.note_buf.push(c),
        _ => {}
    }
}

fn newroom_keys(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        if app.new_room_save() {
            app.mode = Mode::Normal;
        }
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Tab | KeyCode::Down => app.new_room_next(),
        KeyCode::BackTab | KeyCode::Up => app.new_room_prev(),
        KeyCode::Enter => {
            if app.new_room.field == 2 {
                if app.new_room_save() {
                    app.mode = Mode::Normal;
                }
            } else {
                app.new_room_next();
            }
        }
        KeyCode::Backspace => {
            app.new_room.buf_mut().pop();
        }
        KeyCode::Char(c) => app.new_room.buf_mut().push(c),
        _ => {}
    }
}

fn normal_keys(app: &mut App, key: KeyEvent) {
    // inline variable edit in the preview pane takes over input
    if app.inline_edit.is_some() {
        match key.code {
            KeyCode::Enter => app.inline_commit(),
            KeyCode::Esc => app.inline_cancel(),
            KeyCode::Backspace => app.inline_pop(),
            KeyCode::Char(c) => app.inline_push(c),
            _ => {}
        }
        return;
    }
    // run pane has focus: its keys take priority
    if app.focus == Focus::Run && run_focus_keys(app, key) {
        return;
    }
    match key.code {
        KeyCode::Char('q') => app.request_quit(),
        KeyCode::Esc => {
            if !app.search.is_empty() {
                app.search.clear();
                app.after_mutation();
            } else {
                app.request_quit();
            }
        }
        KeyCode::Char('x') => app.run_selected(),
        KeyCode::Char('`') => app.toggle_run_pane(),
        KeyCode::Tab | KeyCode::Char('l') | KeyCode::Right => cycle_focus(app, true),
        KeyCode::BackTab | KeyCode::Char('h') | KeyCode::Left => cycle_focus(app, false),
        KeyCode::Down | KeyCode::Char('j') => move_down(app),
        KeyCode::Up | KeyCode::Char('k') => move_up(app),
        KeyCode::Enter => {
            if app.focus == Focus::Preview {
                app.inline_begin();
            } else {
                app.copy_selected();
            }
        }
        KeyCode::Char('e') => {
            app.var_idx = 0;
            app.var_editing = false;
            app.var_new_key = None;
            app.mode = Mode::Vars;
        }
        KeyCode::Char('a') => app.add_open(),
        KeyCode::Char('d') => app.delete_selected(),
        KeyCode::Char('/') => app.mode = Mode::Search,
        KeyCode::Char('?') => app.mode = Mode::Help,
        KeyCode::Char('c') => app.open_rooms(),
        KeyCode::Char('w') => app.note_open(),
        KeyCode::Char('o') => app.open_active_notes(),
        KeyCode::Char('L') => app.open_loot(),
        KeyCode::Char('f') => app.toggle_favorite(),
        KeyCode::Char('b') => app.toggle_bg(),
        KeyCode::Char('t') => app.cycle_theme(true),
        KeyCode::Char('T') => app.open_theme_picker(),
        KeyCode::Char('D') => app.toggle_discord(),
        KeyCode::Char('M') => app.cycle_discord_privacy(),
        KeyCode::Char('g') => app.open_gtfo(),
        KeyCode::Char('P') => app.parse_scan(),
        KeyCode::Char('R') => app.open_recipes(),
        KeyCode::Char('H') => app.open_htb(),
        KeyCode::Char('F') => app.flag_open(),
        KeyCode::Char('E') => app.export_open(),
        _ => {}
    }
}

fn cycle_focus(app: &mut App, forward: bool) {
    let mut order = vec![Focus::Categories, Focus::Commands, Focus::Preview];
    if app.show_run {
        order.push(Focus::Run);
    }
    let cur = order.iter().position(|f| *f == app.focus).unwrap_or(0);
    let next = if forward {
        (cur + 1) % order.len()
    } else {
        (cur + order.len() - 1) % order.len()
    };
    app.focus = order[next];
    if app.focus == Focus::Preview {
        app.preview_var_idx = 0;
        app.clamp_preview_var();
    }
}

fn run_focus_keys(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => app.run_scroll_down(1),
        KeyCode::Up | KeyCode::Char('k') => app.run_scroll_up(1),
        KeyCode::PageDown => app.run_scroll_down(10),
        KeyCode::PageUp => app.run_scroll_up(10),
        KeyCode::Left | KeyCode::Char('[') => app.run_tab_prev(),
        KeyCode::Right | KeyCode::Char(']') => app.run_tab_next(),
        KeyCode::Char('K') => app.run_kill_active(),
        KeyCode::Char('d') => app.run_close_active(),
        KeyCode::Char('r') => app.run_rerun_active(),
        KeyCode::Char('g') => app.run_tail(),
        KeyCode::Char('z') => app.toggle_run_fullscreen(),
        KeyCode::Char('i') | KeyCode::Enter => app.begin_interact(),
        KeyCode::Esc => {
            if app.run_fullscreen {
                app.run_fullscreen = false; // unzoom back to the split view
            } else {
                app.focus = Focus::Commands;
            }
        }
        _ => return false,
    }
    true
}

/// Encode a key event into the bytes a PTY program expects on stdin.
fn encode_key(key: KeyEvent) -> Option<Vec<u8>> {
    use KeyCode::*;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    // xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4)
    let m = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
    // CSI <final> keys (arrows/Home/End): ESC [ 1 ; m <final> when modified
    let csi = |fin: char| {
        if m > 1 {
            format!("\x1b[1;{m}{fin}").into_bytes()
        } else {
            format!("\x1b[{fin}").into_bytes()
        }
    };
    // ESC [ n ~ keys: ESC [ n ; m ~ when modified
    let tilde = |n: u8| {
        if m > 1 {
            format!("\x1b[{n};{m}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    // Alt+key = ESC prefix (what xterm's metaSendsEscape / most terminals do)
    let meta = |mut b: Vec<u8>| {
        if alt {
            b.insert(0, 0x1b);
        }
        b
    };
    match key.code {
        // AltGr arrives as Ctrl+Alt on some terminals (AZERTY `| @ [ ] \ ^ ~ #`
        // …): a non-alphanumeric char with both is typed text, not a chord
        Char(c) if ctrl && alt && !c.is_ascii_alphanumeric() => Some(c.to_string().into_bytes()),
        Char(c) => {
            let b = if ctrl {
                let u = (c as u8).to_ascii_uppercase();
                if c.is_ascii() && (0x40..=0x5f).contains(&u) {
                    vec![u & 0x1f]
                } else if c == ' ' || c == '2' {
                    vec![0] // Ctrl-Space / Ctrl-@
                } else {
                    c.to_string().into_bytes()
                }
            } else {
                c.to_string().into_bytes()
            };
            Some(meta(b))
        }
        Enter => Some(meta(vec![b'\r'])),
        Backspace => Some(meta(vec![if ctrl { 0x08 } else { 0x7f }])),
        Tab => Some(meta(vec![b'\t'])),
        BackTab => Some(meta(b"\x1b[Z".to_vec())),
        Esc => Some(meta(vec![0x1b])),
        Left => Some(csi('D')),
        Right => Some(csi('C')),
        Up => Some(csi('A')),
        Down => Some(csi('B')),
        Home => Some(csi('H')),
        End => Some(csi('F')),
        Insert => Some(tilde(2)),
        Delete => Some(tilde(3)),
        PageUp => Some(tilde(5)),
        PageDown => Some(tilde(6)),
        _ => None,
    }
}

fn move_down(app: &mut App) {
    match app.focus {
        Focus::Categories if app.search.is_empty() => app.next_cat(),
        Focus::Preview => app.preview_var_next(),
        _ => app.next_cmd(),
    }
}

fn move_up(app: &mut App) {
    match app.focus {
        Focus::Categories if app.search.is_empty() => app.prev_cat(),
        Focus::Preview => app.preview_var_prev(),
        _ => app.prev_cmd(),
    }
}

fn search_keys(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.search.clear();
            app.after_mutation();
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            app.focus = Focus::Commands;
            app.mode = Mode::Normal;
        }
        KeyCode::Backspace => app.search_pop(),
        KeyCode::Down => app.next_cmd(),
        KeyCode::Up => app.prev_cmd(),
        KeyCode::Char(c) => app.search_push(c),
        _ => {}
    }
}

fn vars_keys(app: &mut App, key: KeyEvent) {
    // typing a new var name
    if app.var_new_key.is_some() {
        match key.code {
            KeyCode::Esc => app.var_new_key = None,
            KeyCode::Enter => app.vars_commit_new(),
            KeyCode::Backspace => {
                if let Some(s) = app.var_new_key.as_mut() {
                    s.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(s) = app.var_new_key.as_mut() {
                    s.push(c);
                }
            }
            _ => {}
        }
        return;
    }
    // editing a value
    if app.var_editing {
        match key.code {
            KeyCode::Esc => app.var_editing = false,
            KeyCode::Enter => app.vars_commit_edit(),
            KeyCode::Backspace => {
                app.var_buf.pop();
            }
            KeyCode::Char(c) => app.var_buf.push(c),
            _ => {}
        }
        return;
    }
    // list nav
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => app.mode = Mode::Normal,
        KeyCode::Down | KeyCode::Char('j') => app.vars_next(),
        KeyCode::Up | KeyCode::Char('k') => app.vars_prev(),
        KeyCode::Enter | KeyCode::Char('e') => app.vars_begin_edit(),
        KeyCode::Char('n') => app.vars_begin_new(),
        KeyCode::Char('r') => app.vars_detect_lhost(),
        _ => {}
    }
}

fn add_keys(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        if app.add_save() {
            app.mode = Mode::Normal;
            app.after_mutation();
        }
        return;
    }
    match key.code {
        KeyCode::Esc => app.mode = Mode::Normal,
        KeyCode::Tab => app.add_next_field(),
        KeyCode::BackTab => app.add_prev_field(),
        KeyCode::Enter => app.add_next_field(),
        KeyCode::Backspace => {
            app.add.buf_mut().pop();
        }
        KeyCode::Char(c) => app.add.buf_mut().push(c),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_key_modifiers() {
        let k = |code, mods| encode_key(KeyEvent::new(code, mods)).unwrap();
        let (n, c, a, s) = (KeyModifiers::NONE, KeyModifiers::CONTROL, KeyModifiers::ALT, KeyModifiers::SHIFT);
        // plain keys unchanged
        assert_eq!(k(KeyCode::Char('x'), n), b"x");
        assert_eq!(k(KeyCode::Left, n), b"\x1b[D");
        assert_eq!(k(KeyCode::Home, n), b"\x1b[H");
        assert_eq!(k(KeyCode::Delete, n), b"\x1b[3~");
        // Ctrl / Alt on characters
        assert_eq!(k(KeyCode::Char('c'), c), [0x03]);
        assert_eq!(k(KeyCode::Char('b'), a), b"\x1bb");
        assert_eq!(k(KeyCode::Char('.'), a), b"\x1b.");
        assert_eq!(k(KeyCode::Char('x'), c | a), b"\x1b\x18");
        assert_eq!(k(KeyCode::Char(' '), c), [0]);
        // AltGr reported as Ctrl+Alt: the character itself
        assert_eq!(k(KeyCode::Char('|'), c | a), b"|");
        assert_eq!(k(KeyCode::Char('@'), c | a), b"@");
        assert_eq!(k(KeyCode::Char('€'), c | a), "€".as_bytes());
        // shift alone just changes the char (crossterm reports 'X' + SHIFT)
        assert_eq!(k(KeyCode::Char('X'), s), b"X");
        assert_eq!(k(KeyCode::Backspace, a), b"\x1b\x7f");
        assert_eq!(k(KeyCode::Enter, a), b"\x1b\r");
        // xterm modified cursor keys
        assert_eq!(k(KeyCode::Left, c), b"\x1b[1;5D");
        assert_eq!(k(KeyCode::Right, c), b"\x1b[1;5C");
        assert_eq!(k(KeyCode::Up, a), b"\x1b[1;3A");
        assert_eq!(k(KeyCode::Down, s), b"\x1b[1;2B");
        assert_eq!(k(KeyCode::Left, c | s), b"\x1b[1;6D");
        assert_eq!(k(KeyCode::Home, s), b"\x1b[1;2H");
        assert_eq!(k(KeyCode::End, c), b"\x1b[1;5F");
        assert_eq!(k(KeyCode::Delete, c), b"\x1b[3;5~");
        assert_eq!(k(KeyCode::PageUp, s), b"\x1b[5;2~");
        // non-ASCII with Ctrl doesn't get mangled into a control byte
        assert_eq!(k(KeyCode::Char('é'), c), "é".as_bytes());
    }

    #[test]
    fn single_line_flattens_paste() {
        assert_eq!(single_line("10.10.10.3\n"), "10.10.10.3");
        assert_eq!(single_line("a\r\nb\nc\rd\te"), "a b c d e");
        assert_eq!(single_line("x\x1b[0my\r\n\r\n"), "x[0my");
        assert_eq!(single_line("\n\n"), "");
    }

    #[test]
    fn pty_paste_normalizes_and_brackets() {
        assert_eq!(pty_paste_bytes("ls\r\nid\n", false), b"ls\rid\r".to_vec());
        assert_eq!(pty_paste_bytes("a\nb", true), b"\x1b[200~a\rb\x1b[201~".to_vec());
        // an embedded end marker can't escape the bracket
        assert_eq!(
            pty_paste_bytes("x\x1b[201~rm -rf ~\n", true),
            b"\x1b[200~xrm -rf ~\r\x1b[201~".to_vec()
        );
    }
}
