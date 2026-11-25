mod anim;
mod app;
mod clipboard;
mod discord;
mod gtfobins;
mod hashid;
mod loot;
mod model;
mod netif;
mod ping;
mod recipe;
mod room;
mod runner;
mod theme;
mod ui;
mod writeup;

use app::{App, Focus, Mode};
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
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
        snapshot(&args[2]);
        return Ok(());
    }
    let mut terminal = setup()?;
    let mut app = App::new();
    let res = run(&mut terminal, &mut app);
    restore(&mut terminal)?;
    app.persist_config();
    res
}

/// Headless render for development/screenshots: `pivt snap splash:8` or `pivt snap normal`.
fn snapshot(what: &str) {
    use ratatui::backend::TestBackend;
    let mut app = App::new();
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
        "add" => {
            app.add_open();
            app.add.template = "ffuf -u <URL>/FUZZ -w <WORDLIST>".into();
            app.add.field = 3;
        }
        "help" => app.mode = Mode::Help,
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
                    let (fr, fg, fb) = rgb(cell.fg, (0xc9, 0xd1, 0xd9));
                    let (br, bg, bb) = rgb(cell.bg, (0x0d, 0x11, 0x17));
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
fn edit_in_external(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    path: &std::path::Path,
) -> io::Result<()> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    let status = std::process::Command::new(&editor).arg(path).status();
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;
    if status.is_err() {
        // best-effort; the next draw shows a message if needed
    }
    Ok(())
}

fn setup() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    Terminal::new(backend)
}

fn restore(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
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
            edit_in_external(terminal, &path)?;
            last_tick = Instant::now();
        }
        if last_tick.elapsed() >= tick_rate {
            app.on_tick();
            last_tick = Instant::now();
        }
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
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
    // global: ctrl-c always quits
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
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
    }
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
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Esc => {
            if !app.search.is_empty() {
                app.search.clear();
                app.after_mutation();
            } else {
                app.should_quit = true;
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
        KeyCode::Char('D') => app.toggle_discord(),
        KeyCode::Char('g') => app.open_gtfo(),
        KeyCode::Char('P') => app.parse_scan(),
        KeyCode::Char('R') => app.open_recipes(),
        KeyCode::Char('F') => app.flag_open(),
        KeyCode::Char('E') => app.export_writeup(),
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
    match key.code {
        Char(c) => {
            if ctrl {
                let b = (c as u8).to_ascii_uppercase();
                if (0x40..=0x5f).contains(&b) {
                    Some(vec![b & 0x1f])
                } else {
                    Some(c.to_string().into_bytes())
                }
            } else {
                Some(c.to_string().into_bytes())
            }
        }
        Enter => Some(vec![b'\r']),
        Backspace => Some(vec![0x7f]),
        Tab => Some(vec![b'\t']),
        BackTab => Some(b"\x1b[Z".to_vec()),
        Esc => Some(vec![0x1b]),
        Left => Some(b"\x1b[D".to_vec()),
        Right => Some(b"\x1b[C".to_vec()),
        Up => Some(b"\x1b[A".to_vec()),
        Down => Some(b"\x1b[B".to_vec()),
        Home => Some(b"\x1b[H".to_vec()),
        End => Some(b"\x1b[F".to_vec()),
        PageUp => Some(b"\x1b[5~".to_vec()),
        PageDown => Some(b"\x1b[6~".to_vec()),
        Delete => Some(b"\x1b[3~".to_vec()),
        Insert => Some(b"\x1b[2~".to_vec()),
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
