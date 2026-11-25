use crate::app::{AddField, App, Focus, Mode};
use crate::keys::Action;
use crate::model::extract_placeholders;
use crate::prefs::Border;
use crate::theme::{category_style, theme};
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(theme().bg)), area);

    if app.mode == Mode::Splash {
        draw_splash(f, app, area);
        return;
    }

    // Background behind the whole UI; widgets paint on top, leaving the effect
    // only in empty cells. Cycle with 'b'.
    if let Some(bg) = crate::anim::Bg::of(app.prefs.background) {
        draw_bg(f, app, area, bg, false);
    }

    let screen = crate::prefs::split_screen(area, &app.prefs);
    if let Some(h) = screen.header {
        draw_header(f, app, h);
    }
    draw_body(f, app, screen.body);
    draw_footer(f, app, screen.footer);

    match app.mode {
        Mode::Vars => draw_vars(f, app),
        Mode::Add => draw_add(f, app),
        Mode::Help => draw_help(f, app),
        Mode::Rooms => draw_rooms(f, app),
        Mode::NewRoom => draw_newroom(f, app),
        Mode::Note => draw_note(f, app),
        Mode::Loot => draw_loot(f, app),
        Mode::AddLoot => draw_addloot(f, app),
        Mode::Flag => draw_flag(f, app),
        Mode::Gtfo => draw_gtfo(f, app),
        Mode::Recipes => draw_recipes(f, app),
        Mode::Htb => draw_htb(f, app),
        Mode::Export => draw_export(f, app),
        Mode::Theme => draw_theme_picker(f, app),
        Mode::History => draw_history(f, app),
        Mode::Scope => draw_scope(f, app),
        Mode::Settings => draw_settings(f, app),
        _ => {}
    }

    if app.scope_prompt.is_some() {
        draw_scope_prompt(f, app);
    }
    if app.confirm_quit {
        draw_confirm_quit(f, app);
    }

    // flag-capture burst, drawn on top of everything
    if let Some(p) = app.burst_progress() {
        if app.prefs.animations {
            crate::anim::draw_burst(f.buffer_mut(), area, p, app.fx.burst_style, app.ms);
        }
        let flash = !app.prefs.animations || !app.fx.burst_flash || (app.ms / 100).is_multiple_of(2);
        let msg = format!("🚩  {}  🚩", app.burst_label);
        let mw = msg.chars().count() as u16 + 4;
        let mh = 3u16;
        let mx = area.x + area.width.saturating_sub(mw) / 2;
        let my = (area.y + area.height / 2).saturating_sub(1);
        let r = Rect::new(mx, my, mw.min(area.width), mh.min(area.height));
        f.render_widget(Clear, r);
        let color = if flash { theme().ok } else { theme().star_hi };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(if app.prefs.border == Border::Rounded { BorderType::Double } else { btype(app) })
            .border_style(Style::default().fg(color))
            .style(Style::default().bg(theme().bg));
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                msg,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )))
            .alignment(Alignment::Center)
            .block(block),
            r,
        );
    }
}

/// A background effect (+ shooting stars over the starfield); `loud` for the
/// splash.
fn draw_bg(f: &mut Frame, app: &App, area: Rect, bg: crate::anim::Bg, loud: bool) {
    let sc = crate::anim::Scene {
        ms: app.bg_ms(),
        mouse: ripple_at(app),
        tint: category_style(app.current_category().unwrap_or("")).1,
        loud,
    };
    crate::anim::draw_background(f.buffer_mut(), area, bg, &sc, &app.fx);
    if bg == crate::anim::Bg::Stars && app.prefs.animations && app.fx.shoot {
        crate::anim::draw_shooting_star(f.buffer_mut(), area, app.anim_ms(), &app.fx, loud);
    }
}

/// Mouse position for the flow ripple (None when `ripple` is off).
fn ripple_at(app: &App) -> Option<(u16, u16)> {
    app.mouse.filter(|_| app.prefs.ripple)
}

/// Border type for every box (popups too).
fn btype(app: &App) -> BorderType {
    app.prefs.border.border_type()
}

/// Border lines for the main panes and header boxes: none in compact mode
/// or with `border = "none"` (popups always keep theirs).
fn pane_borders(app: &App) -> Borders {
    if app.prefs.compact || app.prefs.border == Border::None {
        Borders::NONE
    } else {
        Borders::ALL
    }
}

fn panel<'a>(app: &App, title: &'a str, focused: bool) -> Block<'a> {
    let border = if focused { theme().border_hi } else { theme().border };
    Block::default()
        .borders(pane_borders(app))
        .border_type(btype(app))
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(if focused { theme().accent } else { theme().dim })
                .add_modifier(Modifier::BOLD),
        ))
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let h = crate::prefs::split_header(area, &app.prefs);
    let p = &app.prefs;

    // logo — twinkling accent star + active room
    if let Some(r) = h.logo {
        let star = if p.animations { ['✦', '✧', '⋆', '✦'][(app.ms / 400 % 4) as usize] } else { '✦' };
        let mut logo_spans = vec![
            Span::styled(format!(" {star} "), Style::default().fg(theme().accent)),
            Span::styled("pivt", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
        ];
        if let Some(room) = &app.active_room {
            let max = (r.width as usize).saturating_sub(12).clamp(1, 10);
            let mut name = room.name.clone();
            if name.chars().count() > max {
                name = name.chars().take(max - 1).collect();
                name.push('…');
            }
            logo_spans.push(Span::styled(" ▸ ", Style::default().fg(theme().dim)));
            logo_spans.push(Span::styled(name, Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)));
        }
        header_box(f, app, r, Line::from(logo_spans), bare_block(app, theme().border));
    }

    // search
    if let Some(r) = h.search {
        let searching = app.mode == Mode::Search;
        let content: Line = if app.search.is_empty() && !searching {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(theme().accent)),
                Span::styled("search commands…", Style::default().fg(theme().dim)),
            ])
        } else {
            Line::from(vec![
                Span::styled("  / ", Style::default().fg(theme().accent)),
                Span::styled(app.search.clone(), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)),
                Span::styled(if searching { "▏" } else { "" }, Style::default().fg(theme().accent)),
            ])
        };
        let block = bare_block(app, if searching { theme().border_hi } else { theme().border });
        header_box(f, app, r, content, block);
    }

    if let Some(r) = h.target {
        draw_target(f, app, r);
    }
}

/// Target box: ● RHOST ✓  ⇡ LHOST:LPORT, with the scope badge and box timer
/// in its border (inline when there are no borders). Each part can be hidden.
fn draw_target(f: &mut Frame, app: &App, area: Rect) {
    let p = &app.prefs;
    let rhost = app.vars.get("RHOST").cloned().unwrap_or_default();
    let lhost = app.vars.get("LHOST").cloned().unwrap_or_default();
    let lport = app.vars.get("LPORT").cloned().unwrap_or_default();
    let v = |s: &str, ph: &str| {
        if s.is_empty() {
            Span::styled(ph.to_string(), Style::default().fg(theme().warn))
        } else {
            Span::styled(s.to_string(), Style::default().fg(theme().ok).add_modifier(Modifier::BOLD))
        }
    };
    let listener = if lhost.is_empty() && lport.is_empty() {
        "·".to_string()
    } else {
        format!("{}:{}", if lhost.is_empty() { "·" } else { &lhost }, if lport.is_empty() { "·" } else { &lport })
    };
    let ping = app.ping();
    let mut spans = vec![Span::raw(" ")];
    // reachability dot (background ping)
    if p.show_ping {
        spans.push(match ping.up {
            Some(true) => Span::styled("● ", Style::default().fg(theme().ok)),
            Some(false) => Span::styled("● ", Style::default().fg(theme().warn)),
            None => Span::styled("○ ", Style::default().fg(theme().dim)),
        });
    }
    spans.push(v(&rhost, "no target"));
    // ✓ / ⚠: RHOST inside / outside the active room's scope
    if p.show_scope {
        match app.rhost_in_scope() {
            Some(true) => spans.push(Span::styled(" ✓", Style::default().fg(theme().ok))),
            Some(false) => spans.push(Span::styled(" ⚠", Style::default().fg(theme().warn).add_modifier(Modifier::BOLD))),
            None => {}
        }
    }
    if p.show_lhost {
        spans.push(Span::styled("  ⇡ ", Style::default().fg(theme().accent2)));
        spans.push(if lhost.is_empty() && lport.is_empty() {
            Span::styled("no listener", Style::default().fg(theme().warn))
        } else if app.lhost_stale {
            Span::styled(format!("{listener} ⚠ not local"), Style::default().fg(theme().warn).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(listener, Style::default().fg(theme().ok).add_modifier(Modifier::BOLD))
        });
    }
    // box timer (+ rtt) on the right of the target border
    let via = ping.tcp_port.map(|p| format!(" tcp/{p}")).unwrap_or_default();
    let timer = match (p.show_timer, app.box_elapsed(), ping.rtt_ms) {
        (false, ..) => String::new(),
        (true, Some(t), Some(r)) => format!(" ⏱ {t} · {r}ms{via} "),
        (true, Some(t), None) => format!(" ⏱ {t} "),
        _ => String::new(),
    };
    let scope = (p.show_scope && app.active_room.is_some()).then(|| {
        let n = app.scope.entries.len();
        let color = if n == 0 || app.rhost_in_scope() == Some(false) { theme().warn } else { theme().accent2 };
        Span::styled(format!("⊙ scope: {n} "), Style::default().fg(color))
    });
    if pane_borders(app) == Borders::NONE {
        // no border to hang the badges on: append them to the line
        if let Some(s) = scope {
            spans.push(Span::raw("  "));
            spans.push(s);
        }
        if !timer.is_empty() {
            spans.push(Span::styled(timer, Style::default().fg(theme().dim)));
        }
        header_box(f, app, area, Line::from(spans), Block::default());
        return;
    }
    let mut title = vec![Span::styled(" target ", Style::default().fg(theme().dim))];
    title.extend(scope);
    let mut block = bare_block(app, theme().border).title(Line::from(title));
    if !timer.is_empty() {
        block = block.title(Line::from(Span::styled(timer, Style::default().fg(theme().dim))).right_aligned());
    }
    f.render_widget(Paragraph::new(Line::from(spans)).block(block), area);
}

fn bare_block(app: &App, border: Color) -> Block<'static> {
    Block::default()
        .borders(pane_borders(app))
        .border_type(btype(app))
        .border_style(Style::default().fg(border))
}

/// A box inside a popup: always bordered.
fn box_block(app: &App, border: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(border))
}

/// Render a one-line header box: bordered as usual, or (no borders) its
/// line centred vertically in `area`.
fn header_box(f: &mut Frame, app: &App, area: Rect, line: Line, block: Block) {
    if pane_borders(app) == Borders::NONE {
        let r = Rect::new(area.x, area.y + area.height.saturating_sub(1) / 2, area.width, 1.min(area.height));
        f.render_widget(Paragraph::new(line), r);
    } else {
        f.render_widget(Paragraph::new(line).block(block), area);
    }
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    // zoomed: the run pane takes the whole body
    if app.show_run && !app.runs.is_empty() && app.run_fullscreen {
        draw_run(f, app, area);
        return;
    }
    let b = crate::prefs::split_body(area, &app.prefs, app.show_run && !app.runs.is_empty());
    if let Some(r) = b.categories {
        draw_categories(f, app, r);
    }
    draw_commands(f, app, b.commands);
    if let Some(r) = b.preview {
        draw_preview(f, app, r);
    }
    if let Some(r) = b.run {
        draw_run(f, app, r);
    }
}

fn draw_run(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Run && app.mode == Mode::Normal;
    let mut block = panel(app, "run", focused).style(Style::default().bg(theme().bg));
    if let Some(job) = app.active_run() {
        let maxw = area.width.saturating_sub(10) as usize;
        let mut c = job.cmd.clone();
        if c.chars().count() > maxw && maxw > 1 {
            c = format!("{}…", c.chars().take(maxw - 1).collect::<String>());
        }
        block = block.title(Line::from(Span::styled(format!(" {c} "), Style::default().fg(theme().syn_op))).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    let bar = Layout::default()
        .direction(Direction::Horizontal)
        // restored badges ("restored · was running") need more room
        .constraints([
            Constraint::Min(10),
            Constraint::Length(if app.active_run().is_some_and(|j| j.restored.is_some()) { 28 } else { 18 }),
        ])
        .split(rows[0]);

    // tab bar
    let mut tabs: Vec<Span> = Vec::new();
    for (i, job) in app.runs.iter().enumerate() {
        let active = i == app.run_tab;
        let (glyph, gcol) = if job.is_running() {
            ("●", app.pulse())
        } else if job.restored.is_some() {
            ("◌", theme().dim)
        } else if job.exit == Some(0) {
            ("✓", theme().ok)
        } else {
            ("✗", theme().warn)
        };
        if i > 0 {
            tabs.push(Span::styled(" │ ", Style::default().fg(theme().border)));
        }
        tabs.push(Span::styled(format!("{glyph} "), Style::default().fg(gcol)));
        tabs.push(Span::styled(
            job.title.clone(),
            if active {
                Style::default().fg(theme().fg).add_modifier(Modifier::BOLD).bg(theme().sel_bg)
            } else {
                Style::default().fg(theme().dim)
            },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(tabs)), bar[0]);

    // status badge (right of the tab bar)
    if let Some(job) = app.active_run() {
        let badge = if job.is_running() {
            Span::styled(" running ", Style::default().fg(theme().bg).bg(app.pulse()))
        } else if let Some(was_running) = job.restored {
            let what = match (was_running, job.exit) {
                (true, _) => "was running".to_string(),
                (false, Some(0)) | (false, None) => "exited".to_string(),
                (false, Some(c)) => format!("exit {c}"),
            };
            Span::styled(format!(" restored · {what} "), Style::default().fg(theme().bg).bg(theme().accent2))
        } else if job.exit == Some(0) {
            Span::styled(" exit 0 ", Style::default().fg(theme().bg).bg(theme().ok))
        } else {
            Span::styled(format!(" exit {} ", job.exit.unwrap_or(-1)), Style::default().fg(theme().bg).bg(theme().warn))
        };
        let extra = if app.interacting {
            " ⌨".to_string()
        } else if job.scroll > 0 {
            format!(" ↑{}", job.scroll)
        } else {
            String::new()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![badge, Span::styled(extra, Style::default().fg(theme().dim))]))
                .alignment(Alignment::Right),
            bar[1],
        );
    }

    // output: render the vt100 screen cell-by-cell
    let out_area = rows[1];
    if let Some(job) = app.active_run() {
        if let Some(parser) = job.parser() {
            let screen = parser.screen();
            let (srows, scols) = screen.size();
            let show_cursor = app.interacting && !screen.hide_cursor();
            let (crow, ccol) = screen.cursor_position();
            let h = out_area.height.min(srows);
            let w = out_area.width.min(scols);
            let mut lines: Vec<Line> = Vec::with_capacity(h as usize);
            for r in 0..h {
                let mut spans: Vec<Span> = Vec::new();
                let mut run = String::new();
                let mut run_style = Style::default();
                let mut started = false;
                for c in 0..w {
                    let (ch, mut style) = match screen.cell(r, c) {
                        Some(cell) => {
                            let s = cell.contents();
                            let ch = if s.is_empty() { " ".to_string() } else { s };
                            (ch, cell_style(cell))
                        }
                        None => (" ".to_string(), Style::default()),
                    };
                    if show_cursor && r == crow && c == ccol {
                        style = style.add_modifier(Modifier::REVERSED);
                    }
                    if started && style == run_style {
                        run.push_str(&ch);
                    } else {
                        if started {
                            spans.push(Span::styled(std::mem::take(&mut run), run_style));
                        }
                        run = ch;
                        run_style = style;
                        started = true;
                    }
                }
                if started {
                    spans.push(Span::styled(run, run_style));
                }
                lines.push(Line::from(spans));
            }
            f.render_widget(Paragraph::new(lines), out_area);
        }
    }
}

fn vt_color(c: vt100::Color) -> Option<Color> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(Color::Indexed(i)),
        vt100::Color::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
    }
}

fn cell_style(cell: &vt100::Cell) -> Style {
    let mut s = Style::default();
    if let Some(fg) = vt_color(cell.fgcolor()) {
        s = s.fg(fg);
    } else {
        s = s.fg(theme().fg);
    }
    if let Some(bg) = vt_color(cell.bgcolor()) {
        s = s.bg(bg);
    }
    if cell.bold() {
        s = s.add_modifier(Modifier::BOLD);
    }
    if cell.italic() {
        s = s.add_modifier(Modifier::ITALIC);
    }
    if cell.underline() {
        s = s.add_modifier(Modifier::UNDERLINED);
    }
    if cell.inverse() {
        s = s.add_modifier(Modifier::REVERSED);
    }
    s
}

fn draw_categories(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Categories && app.mode == Mode::Normal;
    let searching = !app.search.is_empty();
    let items: Vec<ListItem> = app
        .categories
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let count = app.category_count(c);
            let sel = i == app.cat_idx && !searching;
            let (glyph, color) = category_style(c);
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            ListItem::new(Line::from(vec![
                mark,
                Span::styled(format!("{glyph} "), Style::default().fg(color)),
                Span::styled(
                    format!("{c:<10}"),
                    Style::default()
                        .fg(if sel { color } else { theme().muted })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(format!("{count:>2}"), Style::default().fg(theme().dim)),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    if !searching {
        state.select(Some(app.cat_idx));
    }
    let list = List::new(items)
        .block(panel(app, "categories", focused))
        .highlight_style(Style::default().bg(theme().sel_bg));
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_commands(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Commands && app.mode == Mode::Normal;
    let visible = app.visible();
    let title = if app.search.is_empty() {
        format!("commands · {}", app.current_category().unwrap_or("-"))
    } else {
        format!("results · {}", visible.len())
    };

    let items: Vec<ListItem> = visible
        .iter()
        .enumerate()
        .map(|(row, &ci)| {
            let c = &app.commands[ci];
            let sel = row == app.cmd_idx;
            let (glyph, color) = category_style(&c.category);
            let missing = !app.missing_tools(c).is_empty();
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            let mut spans = vec![
                mark,
                Span::styled(
                    c.name.clone(),
                    Style::default()
                        .fg(match (sel, missing) {
                            (true, _) => theme().fg,
                            (false, true) => theme().dim,
                            (false, false) => theme().subtle,
                        })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ];
            if missing {
                // tool not on PATH: `x` refuses it
                spans.push(Span::styled("  ✗", Style::default().fg(theme().warn)));
            }
            if app.is_favorite_cmd(c) {
                spans.push(Span::styled("  ★", Style::default().fg(theme().gold)));
            }
            if c.user {
                spans.push(Span::styled("  ✎", Style::default().fg(theme().accent2)));
            }
            if c.is_target() {
                // target-side: copy-only, `x` refuses it
                spans.push(Span::styled("  ◎", Style::default().fg(theme().syn_num)));
            }
            if !app.search.is_empty() {
                spans.push(Span::styled(
                    format!("  {glyph} {}", c.category),
                    Style::default().fg(color),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.cmd_idx));
    }
    let list = List::new(items)
        .block(panel(app, &title, focused))
        .highlight_style(Style::default().bg(theme().sel_bg));
    f.render_stateful_widget(list, area, &mut state);
}

/// Syntax-highlight a filled command line: binary (green), flags (blue),
/// numbers/IPs/ports (amber), quoted strings (gold), shell operators (grey),
/// and any still-unset `<PLACEHOLDER>` (red).
fn command_spans(filled: &str) -> Vec<Span<'static>> {
    let mut spans: Vec<Span> = Vec::new();
    let mut expect_binary = true;
    let mut chars = filled.char_indices().peekable();
    while let Some(&(start, ch)) = chars.peek() {
        if ch.is_whitespace() {
            // consume a run of whitespace verbatim
            let mut s = String::new();
            while let Some(&(_, c)) = chars.peek() {
                if c.is_whitespace() {
                    s.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            spans.push(Span::raw(s));
            continue;
        }
        // consume a word (run of non-whitespace)
        let mut word = String::new();
        while let Some(&(_, c)) = chars.peek() {
            if c.is_whitespace() {
                break;
            }
            word.push(c);
            chars.next();
        }
        let _ = start;

        if is_operator(&word) {
            spans.push(Span::styled(word, Style::default().fg(theme().syn_op)));
            expect_binary = true;
            continue;
        }
        let base = if expect_binary {
            expect_binary = false;
            Style::default().fg(theme().ok).add_modifier(Modifier::BOLD)
        } else if word.starts_with('-') {
            Style::default().fg(theme().syn_flag)
        } else if word.starts_with('\'') || word.starts_with('"') {
            Style::default().fg(theme().syn_str)
        } else if is_numeric_like(&word) {
            Style::default().fg(theme().syn_num)
        } else {
            Style::default().fg(theme().fg)
        };
        push_word(&mut spans, &word, base);
    }
    spans
}

fn is_operator(w: &str) -> bool {
    matches!(w, "|" | "||" | "&&" | ";" | ">" | ">>" | "<" | "&" | "2>&1" | "2>" | "1>")
}

fn is_numeric_like(w: &str) -> bool {
    let core = w.trim_start_matches(':');
    !core.is_empty()
        && core.chars().any(|c| c.is_ascii_digit())
        && core.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':' || c == '/')
}

/// Push a word, coloring embedded `<PLACEHOLDER>` tokens red over the base style.
fn push_word(spans: &mut Vec<Span<'static>>, word: &str, base: Style) {
    let mut rest = word;
    while let Some(lt) = rest.find('<') {
        if lt > 0 {
            spans.push(Span::styled(rest[..lt].to_string(), base));
        }
        if let Some(gt) = rest[lt..].find('>') {
            let token = &rest[lt..lt + gt + 1];
            let inner = &token[1..token.len() - 1];
            if !inner.is_empty()
                && inner.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
            {
                spans.push(Span::styled(
                    token.to_string(),
                    Style::default().fg(theme().warn).add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled(token.to_string(), base));
            }
            rest = &rest[lt + gt + 1..];
        } else {
            spans.push(Span::styled(rest[lt..].to_string(), base));
            return;
        }
    }
    if !rest.is_empty() {
        spans.push(Span::styled(rest.to_string(), base));
    }
}

fn draw_preview(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Preview && app.mode == Mode::Normal;
    let block = panel(app, "preview", focused);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(cmd) = app.selected_cmd() else {
        let p = Paragraph::new(Span::styled("no command selected", Style::default().fg(theme().dim)));
        f.render_widget(p, inner);
        return;
    };
    let filled = crate::model::fill_template(&cmd.template, &app.vars);
    let missing = app.missing_tools(cmd);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // name + badge
            Constraint::Length(if missing.is_empty() { 0 } else { 1 }), // missing tool
            Constraint::Length(2), // description
            Constraint::Min(4),    // command box
            Constraint::Length(1), // tags
            Constraint::Length(1), // vars label
            Constraint::Min(3),    // vars
        ])
        .split(inner);

    // name + category chip + star
    let (cat_glyph, cat_color) = category_style(&cmd.category);
    let mut title_spans = vec![
        Span::styled(
            cmd.name.clone(),
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(" {cat_glyph} {} ", cmd.category),
            Style::default().fg(theme().bg).bg(cat_color).add_modifier(Modifier::BOLD),
        ),
    ];
    if cmd.user {
        title_spans.push(Span::styled("  ★ custom", Style::default().fg(theme().accent2)));
    }
    if cmd.is_target() {
        title_spans.push(Span::styled("  ◎ target", Style::default().fg(theme().syn_num).add_modifier(Modifier::BOLD)));
    }
    f.render_widget(Paragraph::new(Line::from(title_spans)), rows[0]);

    // "✗ wpscan not installed — gem install wpscan"
    if !missing.is_empty() {
        let mut spans = vec![
            Span::styled("✗ ", Style::default().fg(theme().warn).add_modifier(Modifier::BOLD)),
            Span::styled(missing.join(", "), Style::default().fg(theme().warn).add_modifier(Modifier::BOLD)),
            Span::styled(" not installed", Style::default().fg(theme().warn)),
        ];
        if let Some(h) = missing.iter().find_map(|b| crate::tools::install_hint(b)) {
            spans.push(Span::styled(format!(" — {h}"), Style::default().fg(theme().muted)));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), rows[1]);
    }

    // description
    let desc = if cmd.description.is_empty() {
        Span::styled("—", Style::default().fg(theme().dim))
    } else {
        Span::styled(cmd.description.clone(), Style::default().fg(theme().muted))
    };
    f.render_widget(Paragraph::new(desc).wrap(Wrap { trim: true }), rows[2]);

    // command box with a copy hint on the right
    let cmd_block = Block::default()
        .borders(pane_borders(app))
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::DIM))
        .title(Span::styled(" command ", Style::default().fg(theme().accent)))
        .title(
            Line::from(Span::styled(
                if cmd.is_target() {
                    format!(" {} copy · run it on the box ", app.key(Action::Copy))
                } else {
                    format!(" {} copy ", app.key(Action::Copy))
                },
                Style::default().fg(theme().dim),
            ))
            .right_aligned(),
        );
    f.render_widget(
        Paragraph::new(Line::from(command_spans(&filled)))
            .block(cmd_block)
            .wrap(Wrap { trim: false }),
        rows[3],
    );

    // tags
    let mut tag_spans: Vec<Span> = Vec::new();
    for t in &cmd.tags {
        tag_spans.push(Span::styled(format!("#{t} "), Style::default().fg(theme().star_mid)));
    }
    if tag_spans.is_empty() {
        tag_spans.push(Span::styled("", Style::default()));
    }
    f.render_widget(Paragraph::new(Line::from(tag_spans)), rows[4]);

    // vars label — hint changes when the pane is focused
    let hint = if focused {
        format!("  {} pick · {} edit here", first_pair(&app.keys, Action::Down, Action::Up), app.key(Action::Copy))
    } else {
        format!("  {} focus · {} popup", app.key(Action::FocusNext), app.key(Action::Vars))
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("─ ", Style::default().fg(theme().border)),
            Span::styled("variables", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)),
            Span::styled(hint, Style::default().fg(theme().dim)),
        ])),
        rows[5],
    );

    // vars used — selectable/editable inline when the preview is focused
    let phs = extract_placeholders(&cmd.template);
    let mut lines: Vec<Line> = Vec::new();
    if phs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no variables — static command",
            Style::default().fg(theme().dim),
        )));
    }
    for (i, ph) in phs.iter().enumerate() {
        let sel = focused && i == app.preview_var_idx;
        let editing = app
            .inline_edit
            .as_ref()
            .map(|(n, _)| n == ph)
            .unwrap_or(false);
        let val = app.vars.get(ph).cloned().unwrap_or_default();

        let gutter = if editing {
            Span::styled(" ▸ ", Style::default().fg(app.pulse()))
        } else if sel {
            Span::styled(" ▸ ", Style::default().fg(theme().accent))
        } else {
            let (b, st) = if val.is_empty() {
                ("○", Style::default().fg(theme().warn))
            } else {
                ("●", Style::default().fg(theme().ok))
            };
            Span::styled(format!(" {b} "), st)
        };

        let name_style = if sel || editing {
            Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme().muted)
        };

        let value_span = if editing {
            let buf = app.inline_edit.as_ref().map(|(_, b)| b.clone()).unwrap_or_default();
            Span::styled(format!("{buf}▏"), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD))
        } else if val.is_empty() {
            Span::styled("‹unset›".to_string(), Style::default().fg(theme().warn))
        } else {
            Span::styled(val, Style::default().fg(theme().ok))
        };

        lines.push(Line::from(vec![
            gutter,
            Span::styled(format!("{ph:<10}"), name_style),
            value_span,
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), rows[6]);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    // run-pane focus has its own hint set
    let k = &app.keys;
    // show_hints off: just the status line in normal mode (popups keep
    // their hints, their keys aren't listed anywhere else)
    if app.mode == Mode::Normal && !app.prefs.show_hints {
        render_footer::<&str>(f, app, area, &[]);
        return;
    }
    if app.mode == Mode::Normal && app.focus == Focus::Run {
        let zoom = if app.run_fullscreen { "unzoom" } else { "zoom" };
        let scroll = || first_pair(k, Action::ScrollDown, Action::ScrollUp);
        let tab = || first_pair(k, Action::TabPrev, Action::TabNext);
        let pairs: Vec<(String, &str)> = if app.interacting {
            vec![("⌨".into(), "typing → program"), (k.compact(Action::Detach), "detach")]
        } else if app.active_run().is_some_and(|j| !j.is_running()) {
            // finished / restored tab: nothing to type into, offer a re-run
            vec![
                (k.first(Action::Rerun), "re-run"), (k.first(Action::Zoom), zoom), (scroll(), "scroll"),
                (tab(), "tab"), (k.first(Action::Close), "close"), (k.first(Action::Tail), "tail"),
            ]
        } else {
            vec![
                (k.first(Action::Interact), "interact"), (k.first(Action::Zoom), zoom),
                (scroll(), "scroll"), (tab(), "tab"), (k.first(Action::Kill), "kill"),
                (k.first(Action::Close), "close"), (k.first(Action::Tail), "tail"),
            ]
        };
        render_footer(f, app, area, &pairs);
        return;
    }
    if app.mode == Mode::Normal {
        let pairs: Vec<(String, &str)> = vec![
            (k.first(Action::FocusNext), "pane"), (k.first(Action::Copy), "copy"), (k.first(Action::Run), "run"),
            (k.first(Action::Vars), "vars"), (k.first(Action::Rooms), "room"), (k.first(Action::Loot), "loot"),
            (k.first(Action::Note), "note"), (k.first(Action::Search), "find"), (k.first(Action::Help), "help"),
        ];
        render_footer(f, app, area, &pairs);
        return;
    }
    let pairs: &[(&str, &str)] = match app.mode {
        Mode::Search => &[("type", "filter"), ("⏎", "done"), ("esc", "cancel")],
        Mode::Vars => &[("j/k", "move"), ("⏎", "edit"), ("n", "new"), ("r", "LHOST"), ("esc", "close")],
        Mode::Add => &[("⇥", "field"), ("^s", "save"), ("esc", "cancel")],
        Mode::Help => &[("esc", "close")],
        Mode::Splash => &[],
        Mode::Rooms => &[("↑↓", "move"), ("⏎", "activate"), ("n", "new"), ("o", "notes"), ("p", "thm/htb"), ("s", "scope"), ("esc", "close")],
        Mode::Scope if app.scope_edit.input.is_some() => &[("type", "ip / cidr / host"), ("⏎", "add"), ("esc", "cancel")],
        Mode::Scope => &[("↑↓", "move"), ("a", "add"), ("d", "remove"), ("r", "defaults"), ("m", "mode"), ("esc", "close")],
        Mode::Note => &[("type", "note"), ("⏎", "save"), ("esc", "cancel")],
        Mode::NewRoom => &[("⇥", "field"), ("^s", "create"), ("esc", "cancel")],
        Mode::Loot => &[("↑↓", "move"), ("⏎", "use vars"), ("y", "copy pass"), ("Y/U", "hash/user"), ("c", "crack"), ("a", "add"), ("d", "del"), ("esc", "close")],
        Mode::AddLoot => &[("⇥", "field"), ("^s", "save"), ("esc", "cancel")],
        Mode::Export => &[("n/⏎", "redacted"), ("y", "include secrets"), ("esc", "cancel")],
        Mode::Flag => &[("⇥", "user/root"), ("type", "value"), ("⏎", "save+copy"), ("esc", "cancel")],
        Mode::Gtfo => &[("type", "filter binary"), ("↑↓", "move"), ("⏎", "copy"), ("esc", "close")],
        Mode::Theme => &[("↑↓", "preview"), ("⏎", "keep"), ("r", "reload files"), ("esc", "revert")],
        Mode::Recipes => &[("↑↓", "move"), ("⏎", "run recipe"), ("A", "run all matched"), ("esc", "close")],
        Mode::History if app.history_typing => &[("type", "filter"), ("↑↓", "move"), ("⏎/esc", "done")],
        Mode::History => &[("↑↓", "move"), ("⏎", "copy"), ("x", "re-run"), ("l", "log"), ("/", "filter"), ("esc", "close")],
        Mode::Htb if app.htb_input.is_some() => &[("type", "answer"), ("⏎", "submit"), ("esc", "cancel")],
        Mode::Htb => &[("↑↓", "task"), ("⏎", "answer"), ("h", "hint"), ("m", "machine"), ("s/t", "spawn/stop"), ("r", "refresh"), ("esc", "close")],
        Mode::Settings => {
            let st = &app.settings;
            let kind = app.settings_selected().map(|i| i.kind());
            use crate::settings::EditKind as K;
            let pairs: &[(&str, &str)] = if st.capture.is_some() {
                &[("press", "a key"), ("⌫", "remove last"), ("esc", "cancel")]
            } else if st.edit.is_some() {
                &[("type", "value"), ("⏎", "save"), ("esc", "cancel")]
            } else if st.filtering {
                &[("type", "filter"), ("⏎/↓", "done"), ("esc", "clear")]
            } else if !st.right {
                &[("↑↓", "section"), ("⏎/→", "options"), ("/", "filter"), ("esc", "close")]
            } else {
                match kind {
                    Some(K::Key) => &[("⏎", "add key"), ("c", "replace"), ("⌫", "remove"), ("r", "default"), ("esc", "back")],
                    Some(K::Theme) => &[("↑↓", "preview"), ("⏎", "keep"), ("r", "default"), ("esc", "back")],
                    Some(K::Bool) => &[("⏎/space", "toggle"), ("←→", "off/on"), ("r", "default"), ("/", "filter"), ("esc", "back")],
                    Some(K::Int { .. }) => &[("←→", "step"), ("⏎", "type"), ("r", "default"), ("/", "filter"), ("esc", "back")],
                    Some(K::Choice(_)) => &[("←→/⏎", "cycle"), ("r", "default"), ("/", "filter"), ("esc", "back")],
                    Some(K::Glyphs) => &[("←→", "preset"), ("⏎", "type"), ("r", "default"), ("esc", "back")],
                    _ => &[("⏎", "edit"), ("r", "default"), ("/", "filter"), ("esc", "back")],
                }
            };
            render_footer(f, app, area, pairs);
            return;
        }
        Mode::Normal => &[],
    };
    render_footer(f, app, area, pairs);
}

/// `↑↓`-style hint from the first key of two opposite actions.
fn first_pair(k: &crate::keys::Keymap, a: Action, b: Action) -> String {
    match (k.keys(a).first(), k.keys(b).first()) {
        (Some(x), Some(y)) => {
            use crossterm::event::KeyCode::{Down, Left, Right, Up};
            match (x.code, y.code) {
                _ if !x.mods.is_empty() || !y.mods.is_empty() => format!("{}/{}", x.label(), y.label()),
                (Down, Up) | (Up, Down) => "↑↓".into(),
                (Left, Right) | (Right, Left) => "←→".into(),
                _ => format!("{}{}", x.label(), y.label()),
            }
        }
        (Some(x), None) | (None, Some(x)) => x.label(),
        (None, None) => format!("[{}]", a.name()),
    }
}

fn render_footer<K: AsRef<str>>(f: &mut Frame, app: &App, area: Rect, pairs: &[(K, &str)]) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, (k, lbl)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(theme().border)));
        }
        spans.push(Span::styled(k.as_ref().to_string(), Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {lbl}"), Style::default().fg(theme().dim)));
    }
    spans.push(Span::raw(" "));

    // hints get what they need; the status keeps 10 columns, or up to 30%
    // of the row while it shows a warning (narrow terminals)
    let need = Line::from(spans.clone()).width() as u16;
    let keep = if app.status.starts_with('⚠') { area.width * 3 / 10 } else { 10 };
    let status_w = (Span::raw(app.status.as_str()).width() as u16).min(keep);
    let hints_w = if pairs.is_empty() { 0 } else { need.min(80).min(area.width.saturating_sub(status_w)) };
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(hints_w)])
        .split(area);
    f.render_widget(
        Paragraph::new(Span::styled(app.status.clone(), Style::default().fg(theme().fg)))
            .wrap(Wrap { trim: true }),
        cols[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Right),
        cols[1],
    );
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    Rect::new(x, y, w, h)
}

fn draw_vars(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 60, 20);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent))
        .title(Span::styled(
            " variables ",
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // split: vars this command uses, then everything else
    let used = app.vars_used();
    let others = app.vars_others();

    let row = |k: &str, nav_i: usize, app: &App| -> ListItem<'static> {
        let sel = nav_i == app.var_idx;
        let editing = sel && app.var_editing;
        let val = app.vars.get(k).cloned().unwrap_or_default();
        let (disp, vstyle) = if editing {
            (format!("{}▏", app.var_buf), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD))
        } else if val.is_empty() {
            ("‹unset›".into(), Style::default().fg(theme().warn))
        } else {
            (val.clone(), Style::default().fg(theme().ok))
        };
        let (bullet, bstyle) = if val.is_empty() {
            ("○", Style::default().fg(theme().warn))
        } else {
            ("●", Style::default().fg(theme().ok))
        };
        let mark = if sel {
            Span::styled(app.marker(), Style::default().fg(app.pulse()))
        } else {
            Span::styled(format!("{bullet}"), bstyle)
        };
        ListItem::new(Line::from(vec![
            Span::raw(" "),
            mark,
            Span::styled(format!(" {k:<11}"), Style::default().fg(theme().subtle)),
            Span::styled("= ", Style::default().fg(theme().dim)),
            Span::styled(disp, vstyle),
        ]))
    };
    let header = |t: &str| {
        ListItem::new(Line::from(Span::styled(
            format!(" ── {t} ──"),
            Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD),
        )))
    };

    let mut items: Vec<ListItem> = Vec::new();
    let mut nav = 0usize;
    if !used.is_empty() {
        items.push(header("this command"));
        for k in &used {
            items.push(row(k, nav, app));
            nav += 1;
        }
    }
    items.push(header("all variables"));
    for k in &others {
        items.push(row(k, nav, app));
        nav += 1;
    }

    if let Some(nk) = &app.var_new_key {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(" ▎ ", Style::default().fg(theme().accent2)),
            Span::styled("new var: ", Style::default().fg(theme().accent2)),
            Span::styled(format!("{nk}▏"), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)),
        ])));
    }

    f.render_widget(List::new(items), inner);
}

fn draw_history(f: &mut Frame, app: &App) {
    use crate::history::Action;
    let area = centered(f.area(), 104, 26);
    f.render_widget(Clear, area);
    let t = theme();
    let place = app.active_room.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| "no room".into());
    let matches = app.history_matches();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(t.accent))
        .title(Span::styled(format!(" history · {place} "), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(format!(" {}/{} ", matches.len(), app.history.len()), Style::default().fg(t.dim))).right_aligned())
        .style(Style::default().bg(t.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1), Constraint::Length(3)])
        .split(inner);

    // filter line
    let q: Line = if app.history_query.is_empty() && !app.history_typing {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(t.accent)),
            Span::styled("filter by name, command, copy/run…", Style::default().fg(t.dim)),
        ])
    } else {
        let mut v = vec![
            Span::styled(" / ", Style::default().fg(t.accent)),
            Span::styled(app.history_query.clone(), Style::default().fg(t.fg).add_modifier(Modifier::BOLD)),
        ];
        if app.history_typing {
            v.push(Span::styled("▏", Style::default().fg(t.accent)));
        }
        Line::from(v)
    };
    f.render_widget(Paragraph::new(q), rows[0]);

    if app.history.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  nothing yet — every copy (⏎) and run (x) in this room is recorded here",
                Style::default().fg(t.dim),
            )),
            rows[1],
        );
        return;
    }

    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(row, &hi)| {
            let e = &app.history[hi];
            let sel = row == app.history_idx;
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            let (act, act_color) = match e.action {
                Action::Run => ("▶ run ", t.accent2),
                _ => ("⧉ copy", t.muted),
            };
            let (exit, exit_color) = match (e.action, e.exit) {
                (Action::Copy, _) => ("—".to_string(), t.dim),
                (_, Some(0)) => ("✓ 0".to_string(), t.ok),
                (_, Some(-1)) => ("✗ killed".to_string(), t.warn),
                (_, Some(c)) => (format!("✗ {c}"), t.warn),
                (_, None) if app.history_running(&e.run) => ("… running".to_string(), t.accent),
                (_, None) => ("?".to_string(), t.dim),
            };
            let name: String = e.name.chars().take(30).collect();
            let (glyph, cat_color) = category_style(e.category());
            ListItem::new(Line::from(vec![
                mark,
                Span::styled(format!("{:<12}", e.short_time()), Style::default().fg(t.dim)),
                Span::styled(format!("{act}  "), Style::default().fg(act_color)),
                Span::styled(
                    format!("{name:<31}"),
                    Style::default()
                        .fg(if sel { t.fg } else { t.subtle })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(format!("{exit:<10}"), Style::default().fg(exit_color)),
                Span::styled(format!("{glyph} {}", e.category()), Style::default().fg(cat_color)),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.history_idx));
    }
    f.render_stateful_widget(List::new(items).highlight_style(Style::default().bg(t.sel_bg)), rows[1], &mut state);

    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(rows[2].width as usize), Style::default().fg(t.border))),
        rows[2],
    );
    // selected entry: the filled command + its log
    if let Some(e) = app.history_selected() {
        let mut lines = vec![Line::from(command_spans(&e.cmd))];
        if let Some(l) = &e.log {
            lines.push(Line::from(vec![
                Span::styled("log ", Style::default().fg(t.dim)),
                Span::styled(l.display().to_string().replace(&home_prefix(), "~"), Style::default().fg(t.muted)),
                Span::styled("  l opens it", Style::default().fg(t.dim)),
            ]));
        }
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), rows[3]);
    }
}

fn draw_gtfo(f: &mut Frame, app: &App) {
    use crate::gtfobins::GTFOBINS;
    let area = centered(f.area(), 84, 24);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().slate))
        .title(Span::styled(" GTFOBins ", Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(" privesc one-liners ", Style::default().fg(theme().dim))).right_aligned())
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // search
    let q: Line = if app.gtfo_query.is_empty() {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme().accent)),
            Span::styled("type a binary — find, vim, python…", Style::default().fg(theme().dim)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme().accent)),
            Span::styled(app.gtfo_query.clone(), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)),
            Span::styled("▏", Style::default().fg(theme().accent)),
        ])
    };
    f.render_widget(Paragraph::new(q), rows[0]);

    // kind → color
    let kind_color = |k: &str| match k {
        "sudo" => theme().warn,
        "suid" => theme().orange,
        "shell" => theme().ok,
        "pager" => theme().accent2,
        k if k.starts_with("file") => theme().accent,
        _ => theme().dim,
    };

    let matches = app.gtfo_matches();
    let items: Vec<ListItem> = matches
        .iter()
        .enumerate()
        .map(|(row, &gi)| {
            let g = &GTFOBINS[gi];
            let sel = row == app.gtfo_idx;
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            let mut spans = vec![
                mark,
                Span::styled(format!("{:<11}", g.bin), Style::default().fg(if sel { theme().fg } else { theme().subtle }).add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() })),
                Span::styled(format!("{:<11}", g.kind), Style::default().fg(kind_color(g.kind))),
            ];
            if sel {
                spans.push(Span::styled(g.code.to_string(), Style::default().fg(theme().ok)));
            } else {
                let short: String = g.code.chars().take(40).collect();
                spans.push(Span::styled(short, Style::default().fg(theme().dim)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.gtfo_idx));
    }
    f.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(theme().sel_bg)),
        rows[1],
        &mut state,
    );
}

fn draw_export(f: &mut Frame, app: &App) {
    let active = app.active_htb_machine();
    let area = centered(f.area(), 66, if active.is_some() { 10 } else { 7 });
    f.render_widget(Clear, area);
    let color = if active.is_some() { theme().warn } else { theme().accent };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(color))
        .title(Span::styled(" export writeup ", Style::default().fg(color).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let mut lines = Vec::new();
    if let Some(m) = &active {
        lines.push(Line::from(Span::styled(
            format!("⚠ active machine — {m} isn't retired."),
            Style::default().fg(theme().warn).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Sharing solutions breaks HTB rules: keep the export local.",
            Style::default().fg(theme().fg),
        )));
        lines.push(Line::from(Span::styled(
            "The file gets -PRIVATE in its name and a PRIVATE header.",
            Style::default().fg(theme().dim),
        )));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "HTB/THM rules: no flags or walkthroughs for active content.",
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(Span::styled(
        format!("By default flags + credential values become {}.", crate::writeup::REDACTED),
        Style::default().fg(theme().dim),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("include flags & credentials? ", Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)),
        Span::styled("y", Style::default().fg(theme().warn).add_modifier(Modifier::BOLD)),
        Span::styled(" / ", Style::default().fg(theme().dim)),
        Span::styled("N", Style::default().fg(theme().ok).add_modifier(Modifier::BOLD)),
    ]));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn draw_flag(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 60, 6);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().ok))
        .title(Span::styled(" capture flag ", Style::default().fg(theme().ok).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // which selector
    let pill = |label: &str, on: bool| {
        if on {
            Span::styled(format!(" {label} "), Style::default().fg(theme().bg).bg(theme().ok).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!(" {label} "), Style::default().fg(theme().dim))
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("flag: ", Style::default().fg(theme().dim)),
            pill("user", !app.flag_is_root),
            Span::raw(" "),
            pill("root", app.flag_is_root),
            Span::styled("   (⇥ toggle)", Style::default().fg(theme().dim)),
        ])),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("🚩 ", Style::default().fg(theme().ok)),
            Span::styled(app.flag_buf.clone(), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)),
            Span::styled("▏", Style::default().fg(theme().accent)),
        ])),
        rows[1],
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "⏎ writes to room notes Flags table + log, copies to clipboard",
            Style::default().fg(theme().dim),
        ))
        .wrap(Wrap { trim: true }),
        rows[2],
    );
}

fn draw_loot(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 72, 22);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(" loot ", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(
            format!(" {} creds ", app.loot.len()),
            Style::default().fg(theme().dim),
        )).right_aligned())
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.loot.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("no creds captured yet.", Style::default().fg(theme().dim))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("press ", Style::default().fg(theme().dim)),
                    Span::styled("a", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
                    Span::styled(" to add user:pass / hash as you find them.", Style::default().fg(theme().dim)),
                    Span::styled("  ⏎ loads them into USER/PASS/HASH.", Style::default().fg(theme().dim)),
                ]),
            ])
            .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let rows: Vec<ListItem> = app
        .loot
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sel = i == app.loot_idx;
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            let mut spans = vec![
                mark,
                Span::styled(" ", Style::default()),
                Span::styled(
                    c.label(),
                    Style::default()
                        .fg(if sel { theme().fg } else { theme().subtle })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ];
            if !c.hash.is_empty() && c.pass.is_empty() {
                if app.cracks.iter().any(|p| p.hash == c.hash) {
                    spans.push(Span::styled("  ⬢ cracking…", Style::default().fg(theme().syn_num)));
                } else if let Some(k) = crate::hashid::identify(&c.hash).first() {
                    let mode = k.mode.map(|m| format!(" -m {m}")).unwrap_or_else(|| " john".into());
                    spans.push(Span::styled(format!("  ⬢ {}{mode}", k.name), Style::default().fg(theme().orange)));
                }
            }
            if !c.note.is_empty() {
                spans.push(Span::styled(format!("   {}", c.note), Style::default().fg(theme().dim)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.loot_idx));
    f.render_stateful_widget(
        List::new(rows).highlight_style(Style::default().bg(theme().sel_bg)),
        inner,
        &mut state,
    );
}

/// Render `code` spans (HTB task text uses markdown backticks) in flag colour.
fn md_spans(text: &str, base: Style) -> Vec<Span<'static>> {
    text.split('`')
        .enumerate()
        .filter(|(_, part)| !part.is_empty())
        .map(|(i, part)| {
            if i % 2 == 1 {
                Span::styled(part.to_string(), Style::default().fg(theme().syn_flag))
            } else {
                Span::styled(part.to_string(), base)
            }
        })
        .collect()
}

fn draw_htb(f: &mut Frame, app: &App) {
    use crate::htb::TaskKind;
    let area = centered(f.area(), 104, 28);
    f.render_widget(Clear, area);
    let st = app.htb.snapshot();
    let status = match (&st.busy, &st.message) {
        (Some(b), _) => Span::styled(format!(" ⟳ {b} "), Style::default().fg(theme().accent)),
        (None, Some((true, m))) => Span::styled(format!(" {m} "), Style::default().fg(theme().ok)),
        (None, Some((false, m))) => Span::styled(format!(" ✗ {m} "), Style::default().fg(theme().warn)),
        _ => Span::raw(""),
    };
    let htb_green = theme().htb;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(htb_green))
        .title(Span::styled(" hack the box ", Style::default().fg(htb_green).add_modifier(Modifier::BOLD)))
        .title(Line::from(status).right_aligned())
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if !app.htb.has_token() {
        let path = crate::htb::token_path();
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("no HTB App Token yet.", Style::default().fg(theme().fg).add_modifier(Modifier::BOLD))),
                Line::from(""),
                Line::from(Span::styled("1. app.hackthebox.com → avatar → Account Settings → App Tokens → Create", Style::default().fg(theme().dim))),
                Line::from(Span::styled(format!("2. save it to {} (chmod 600), or export HTB_TOKEN", path.display()), Style::default().fg(theme().dim))),
                Line::from(Span::styled(format!("3. restart pivt and press {}", app.key(Action::Htb)), Style::default().fg(theme().dim))),
            ])
            .wrap(Wrap { trim: true }),
            inner.inner(Margin { horizontal: 1, vertical: 1 }),
        );
        return;
    }

    let rows = Layout::vertical([Constraint::Length(5), Constraint::Min(5), Constraint::Length(1)]).split(inner);

    // ── header ──
    let Some(m) = &st.machine else {
        let hint = if st.busy.is_some() { "loading…" } else { "no machine loaded — press m and type a machine name" };
        f.render_widget(Paragraph::new(Span::styled(hint, Style::default().fg(theme().dim))), rows[0].inner(Margin { horizontal: 1, vertical: 1 }));
        draw_htb_input(f, app, rows[2]);
        return;
    };
    let diff_col = match m.difficulty.as_str() {
        "Easy" => theme().ok,
        "Medium" => theme().syn_num,
        "Hard" => theme().warn,
        _ => theme().accent2,
    };
    let tick = |b: bool| if b { ("✓", theme().ok) } else { ("○", theme().dim) };
    let (u, uc) = tick(m.user_owned);
    let (r, rc) = tick(m.root_owned);
    let mut head = vec![
        Span::styled(format!(" {} ", m.name), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{} · ", m.os), Style::default().fg(theme().dim)),
        Span::styled(m.difficulty.clone(), Style::default().fg(diff_col)),
        Span::styled(format!(" · {} pts{}   ", m.points, if m.retired { " · retired" } else { "" }), Style::default().fg(theme().dim)),
    ];
    match &m.ip {
        Some(ip) => head.push(Span::styled(format!("◎ {ip}   "), Style::default().fg(theme().syn_num))),
        None if m.spawned => head.push(Span::styled("◎ spawning…   ", Style::default().fg(theme().accent))),
        None => head.push(Span::styled("◎ not spawned (s)   ", Style::default().fg(theme().dim))),
    }
    head.extend([
        Span::styled(format!("user {u}  "), Style::default().fg(uc)),
        Span::styled(format!("root {r}"), Style::default().fg(rc)),
    ]);
    if m.guided {
        head.push(Span::styled("   guided", Style::default().fg(theme().accent2)));
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::from(head),
            Line::from(Span::styled(format!(" {}", m.synopsis), Style::default().fg(theme().muted))),
        ])
        .wrap(Wrap { trim: false }),
        rows[0],
    );

    // ── tasks ──
    let cols = Layout::horizontal([Constraint::Length(30), Constraint::Min(10)]).split(rows[1]);
    let items: Vec<ListItem> = st
        .tasks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let sel = i == app.htb_idx;
            let (icon, col) = if t.completed {
                ("✓", theme().ok)
            } else if st.locked(t) {
                ("·", theme().dim)
            } else {
                match t.kind {
                    TaskKind::User => ("⚑", theme().syn_num),
                    TaskKind::Root => ("⚑", theme().warn),
                    TaskKind::Task => ("○", theme().accent),
                }
            };
            let mark = if sel { Span::styled(app.marker(), Style::default().fg(app.pulse())) } else { Span::raw(" ") };
            ListItem::new(Line::from(vec![
                mark,
                Span::styled(format!(" {icon} "), Style::default().fg(col)),
                Span::styled(
                    t.title.clone(),
                    Style::default()
                        .fg(if sel { theme().fg } else if st.locked(t) { theme().dim } else { theme().subtle })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.htb_idx.min(st.tasks.len().saturating_sub(1))));
    f.render_stateful_widget(List::new(items).highlight_style(Style::default().bg(theme().sel_bg)), cols[0], &mut state);

    if let Some(t) = st.tasks.get(app.htb_idx) {
        let mut lines = vec![Line::from(Span::styled(t.title.clone(), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD)))];
        if !t.question.is_empty() {
            lines.push(Line::from(md_spans(&t.question, Style::default().fg(theme().fg))));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("format  ", Style::default().fg(theme().dim)),
            Span::styled(t.format.clone(), Style::default().fg(theme().syn_num)),
        ]));
        if t.completed {
            lines.push(Line::from(Span::styled("✓ solved", Style::default().fg(theme().ok))));
        } else if st.locked(t) {
            lines.push(Line::from(Span::styled("locked — solve the previous task first", Style::default().fg(theme().dim))));
        }
        if !t.hint.is_empty() {
            lines.push(Line::from(""));
            if app.htb_hint {
                let mut l = vec![Span::styled("hint  ", Style::default().fg(theme().accent2))];
                l.extend(md_spans(&t.hint, Style::default().fg(theme().muted)));
                lines.push(Line::from(l));
            } else {
                lines.push(Line::from(Span::styled("h  reveal hint (free, no point cost)", Style::default().fg(theme().dim))));
            }
        }
        f.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            cols[1].inner(Margin { horizontal: 1, vertical: 0 }),
        );
    }
    draw_htb_input(f, app, rows[2]);
}

fn draw_htb_input(f: &mut Frame, app: &App, area: Rect) {
    use crate::app::HtbInput;
    let (label, buf) = match &app.htb_input {
        Some(HtbInput::Machine(b)) => ("machine › ", b),
        Some(HtbInput::Answer(b)) => ("answer › ", b),
        None => return,
    };
    let cursor = if (app.anim_ms() / 500).is_multiple_of(2) { "▌" } else { " " };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {label}"), Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
            Span::styled(buf.clone(), Style::default().fg(theme().fg)),
            Span::styled(cursor, Style::default().fg(theme().accent)),
        ]))
        .style(Style::default().bg(theme().sel_bg)),
        area,
    );
}

fn draw_recipes(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 96, 24);
    f.render_widget(Clear, area);
    let ports = app.vars.get("PORTS").cloned().unwrap_or_default();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent))
        .title(Span::styled(" recipes ", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(
            if ports.is_empty() { " no PORTS yet · scan + P ".to_string() } else { format!(" PORTS {ports} ") },
            Style::default().fg(theme().dim),
        )).right_aligned())
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let cols = Layout::horizontal([Constraint::Length(34), Constraint::Min(10)]).split(inner);

    // left: list
    let rows: Vec<ListItem> = app
        .recipes
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let sel = i == app.recipe_idx;
            let matched = app.recipe_matched(r);
            let (dot, dc) = if matched {
                ("●", theme().ok)
            } else if r.is_generic() {
                ("◆", theme().accent)
            } else {
                ("○", theme().dim)
            };
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            ListItem::new(Line::from(vec![
                mark,
                Span::styled(format!(" {dot} "), Style::default().fg(dc)),
                Span::styled(
                    r.name.clone(),
                    Style::default()
                        .fg(if sel { theme().fg } else if matched { theme().subtle } else { theme().dim })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.recipe_idx));
    f.render_stateful_widget(
        List::new(rows).highlight_style(Style::default().bg(theme().sel_bg)),
        cols[0],
        &mut state,
    );

    // right: detail
    let Some(r) = app.recipes.get(app.recipe_idx) else { return };
    let mut lines = vec![
        Line::from(Span::styled(r.name.clone(), Style::default().fg(theme().fg).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled(r.description.clone(), Style::default().fg(theme().muted))),
    ];
    let mut when: Vec<String> = r.ports.iter().map(|p| p.to_string()).collect();
    when.extend(r.services.iter().cloned());
    lines.push(Line::from(vec![
        Span::styled("when ", Style::default().fg(theme().dim)),
        Span::styled(
            if when.is_empty() { "always".to_string() } else { when.join(" · ") },
            Style::default().fg(theme().syn_num),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("steps", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)),
        Span::styled(
            if r.sequential { "  one at a time" } else { "  all at once" },
            Style::default().fg(theme().dim),
        ),
    ]));
    for step in &r.steps {
        match app.command_for_key(step) {
            Some(c) => {
                let (glyph, color) = category_style(&c.category);
                let missing = app.missing_vars(c);
                let mut spans = vec![
                    Span::styled(format!(" {glyph} "), Style::default().fg(color)),
                    Span::styled(c.name.clone(), Style::default().fg(theme().fg)),
                    Span::styled(format!("  {}", c.category), Style::default().fg(theme().dim)),
                ];
                if missing.is_empty() {
                    spans.push(Span::styled("  ✓", Style::default().fg(theme().ok)));
                } else {
                    spans.push(Span::styled(format!("  needs {}", missing.join(",")), Style::default().fg(theme().warn)));
                }
                lines.push(Line::from(spans));
            }
            None => lines.push(Line::from(vec![
                Span::styled(" ✗ ", Style::default().fg(theme().warn)),
                Span::styled(step.clone(), Style::default().fg(theme().dim)),
                Span::styled("  unknown command", Style::default().fg(theme().warn)),
            ])),
        }
    }
    f.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        cols[1].inner(Margin { horizontal: 1, vertical: 0 }),
    );
}

fn draw_addloot(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 64, 15);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(" add loot ", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3); 4])
        .split(inner);
    draw_field(f, app, rows[0], "user", &app.loot_form.user, app.loot_form.field == 0);
    draw_field(f, app, rows[1], "pass", &app.loot_form.pass, app.loot_form.field == 1);
    draw_field(f, app, rows[2], "hash", &app.loot_form.hash, app.loot_form.field == 2);
    draw_field(f, app, rows[3], "note", &app.loot_form.note, app.loot_form.field == 3);
}

fn draw_rooms(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 84, 24);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(" ctf rooms ", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(
            format!(" {} ", app.paths.rooms_dir.display()),
            Style::default().fg(theme().dim),
        )).right_aligned())
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.rooms.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("no rooms yet.", Style::default().fg(theme().dim))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("press ", Style::default().fg(theme().dim)),
                    Span::styled("n", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
                    Span::styled(" to scaffold one under ", Style::default().fg(theme().dim)),
                    Span::styled(app.paths.rooms_dir.display().to_string(), Style::default().fg(theme().fg)),
                ]),
            ])
            .wrap(Wrap { trim: true }),
            inner,
        );
        return;
    }

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(28), Constraint::Min(20)])
        .split(inner);

    let items: Vec<ListItem> = app
        .rooms
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let sel = i == app.room_idx;
            let is_active = app.active_room.as_ref().map(|a| a.path == r.path).unwrap_or(false);
            let mark = if sel {
                Span::styled(app.marker(), Style::default().fg(app.pulse()))
            } else {
                Span::raw(" ")
            };
            let dot = if is_active {
                Span::styled("◆ ", Style::default().fg(theme().ok))
            } else {
                Span::styled("· ", Style::default().fg(theme().dim))
            };
            let tag = r
                .platform()
                .map(|p| Span::styled(format!(" {}", p.label().to_lowercase()), Style::default().fg(theme().dim)))
                .unwrap_or_else(|| Span::raw(""));
            ListItem::new(Line::from(vec![
                mark,
                dot,
                Span::styled(
                    r.name.clone(),
                    Style::default()
                        .fg(if sel { theme().fg } else { theme().subtle })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
                tag,
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.room_idx));
    f.render_stateful_widget(
        List::new(items)
            .block(box_block(app, theme().border).title(Span::styled(" rooms ", Style::default().fg(theme().dim))))
            .highlight_style(Style::default().bg(theme().sel_bg)),
        cols[0],
        &mut state,
    );

    // notes preview of the highlighted room
    let mut lines: Vec<Line> = Vec::new();
    if let Some(r) = app.rooms.get(app.room_idx) {
        if let Some((ip, host)) = r.read_target() {
            lines.push(Line::from(vec![
                Span::styled("target  ", Style::default().fg(theme().dim)),
                Span::styled(ip, Style::default().fg(theme().ok).add_modifier(Modifier::BOLD)),
                Span::styled(
                    host.map(|h| format!("  {h}")).unwrap_or_default(),
                    Style::default().fg(theme().accent2),
                ),
            ]));
        }
        let sc = app.scope_for_room(r);
        lines.push(Line::from(vec![
            Span::styled("scope   ", Style::default().fg(theme().dim)),
            Span::styled(sc.summary(4), Style::default().fg(if sc.entries.is_empty() { theme().warn } else { theme().fg })),
            Span::styled(if sc.explicit { "" } else { "  (default)" }, Style::default().fg(theme().dim)),
        ]));
        lines.push(Line::from(Span::styled("── notes.md ──", Style::default().fg(theme().border))));
        for l in r.notes_preview(16) {
            lines.push(Line::from(Span::styled(l, Style::default().fg(theme().muted))));
        }
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(box_block(app, theme().border).title(Span::styled(" preview ", Style::default().fg(theme().dim))))
            .wrap(Wrap { trim: true }),
        cols[1],
    );
}

fn draw_newroom(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 64, 14);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(" new room ", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Min(1)])
        .split(inner);
    draw_field(f, app, rows[0], "name", &app.new_room.name, app.new_room.field == 0);
    draw_field(f, app, rows[1], "target ip (optional)", &app.new_room.ip, app.new_room.field == 1);
    draw_field(f, app, rows[2], "hostname (optional)", &app.new_room.host, app.new_room.field == 2);
    f.render_widget(
        Paragraph::new(Span::styled(
            format!("scaffolds {}/<name> with nmap/ loot/ exploits/ + notes.md", app.paths.rooms_dir.display()),
            Style::default().fg(theme().accent),
        ))
        .wrap(Wrap { trim: true }),
        rows[3],
    );
}

fn draw_confirm_quit(f: &mut Frame, app: &App) {
    let n = app.running_jobs();
    let msg = format!("{n} job{} running — quit and kill {}? ", if n == 1 { "" } else { "s" }, if n == 1 { "it" } else { "them" });
    let w = msg.chars().count() as u16 + 9;
    let area = centered(f.area(), w, 3);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().warn))
        .title(Span::styled(" quit ", Style::default().fg(theme().warn).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {msg}"), Style::default().fg(theme().fg)),
            Span::styled("y", Style::default().fg(theme().warn).add_modifier(Modifier::BOLD)),
            Span::styled("/", Style::default().fg(theme().dim)),
            Span::styled("N", Style::default().fg(theme().ok).add_modifier(Modifier::BOLD)),
        ])),
        inner,
    );
}

fn draw_scope_prompt(f: &mut Frame, app: &App) {
    let Some(p) = &app.scope_prompt else { return };
    let t = theme();
    let hosts = p.hosts.join(", ");
    let verb = if p.hosts.len() == 1 { "is" } else { "are" };
    let l1 = format!(" {hosts} {verb} outside this room's scope ({})", p.scope);
    let w = (l1.chars().count() as u16 + 3).clamp(52, f.area().width.saturating_sub(4));
    let area = centered(f.area(), w, 5);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(t.warn))
        .title(Span::styled(" ⊙ out of scope ", Style::default().fg(t.warn).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(t.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let k = |s: &str, c: Color| Span::styled(s.to_string(), Style::default().fg(c).add_modifier(Modifier::BOLD));
    let d = |s: &str| Span::styled(s.to_string(), Style::default().fg(t.dim));
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(format!(" {hosts}"), Style::default().fg(t.warn).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {verb} outside this room's scope "), Style::default().fg(t.fg)),
                d(&format!("({})", p.scope)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(" run anyway?  ", Style::default().fg(t.fg)),
                k("y", t.warn), d(" run once · "),
                k("a", t.accent), d(" add to scope + run · "),
                k("N", t.ok), d(" cancel"),
            ]),
        ])
        .wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_scope(f: &mut Frame, app: &App) {
    let t = theme();
    let area = centered(f.area(), 70, 20);
    f.render_widget(Clear, area);
    let name = app.scope_edit.room.as_ref().map(|r| r.name.clone()).unwrap_or_default();
    let sc = app.scope_edit_scope();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(t.accent2))
        .title(Span::styled(format!(" ⊙ scope · {name} "), Style::default().fg(t.accent2).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(format!(" mode: {} ", app.scope_mode.as_str()), Style::default().fg(t.dim))).right_aligned())
        .style(Style::default().bg(t.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(3), Constraint::Length(3), Constraint::Length(2)])
        .split(inner);
    let origin = if sc.explicit {
        "pinned in .ctfroom — only these hosts".to_string()
    } else {
        "default: the room's .target ip + host (+ HTB machine) — edit to pin".to_string()
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(format!(" {origin}"), Style::default().fg(t.dim))),
            Line::from(Span::styled(" LHOST, loopback and this machine's addresses always pass", Style::default().fg(t.dim))),
        ]),
        rows[0],
    );
    if sc.entries.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(
                "  no scope yet — every target counts as outside. a adds an IP, CIDR or hostname",
                Style::default().fg(t.warn),
            ))
            .wrap(Wrap { trim: true }),
            rows[1],
        );
    } else {
        let items: Vec<ListItem> = sc
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let sel = i == app.scope_edit.idx;
                let kind = match e {
                    crate::scope::Addr::Net(ip, p) if (ip.is_ipv4() && *p < 32) || (ip.is_ipv6() && *p < 128) => "net",
                    crate::scope::Addr::Net(..) => "ip",
                    crate::scope::Addr::Host(h) if h.starts_with("*.") => "wildcard",
                    crate::scope::Addr::Host(_) => "host",
                };
                ListItem::new(Line::from(vec![
                    if sel {
                        Span::styled(format!(" {}", app.marker()), Style::default().fg(app.pulse()))
                    } else {
                        Span::raw("  ")
                    },
                    Span::styled(
                        format!("{:<40}", e.to_string()),
                        Style::default().fg(if sel { t.fg } else { t.subtle }).add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                    ),
                    Span::styled(kind, Style::default().fg(t.dim)),
                ]))
            })
            .collect();
        let mut state = ListState::default();
        state.select(Some(app.scope_edit.idx.min(sc.entries.len() - 1)));
        f.render_stateful_widget(List::new(items).highlight_style(Style::default().bg(t.sel_bg)), rows[1], &mut state);
    }
    let input = app.scope_edit.input.as_ref();
    let field = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(if input.is_some() { t.border_hi } else { t.border }))
        .title(Span::styled(" add ", Style::default().fg(t.dim)));
    let content = match input {
        Some(b) => Line::from(vec![
            Span::styled(b.clone(), Style::default().fg(t.fg)),
            Span::styled("▏", Style::default().fg(t.accent)),
        ]),
        None => Line::from(Span::styled("a: 10.10.11.0/24 · dc-2 · *.corp.local · dead:beef::/64", Style::default().fg(t.dim))),
    };
    f.render_widget(Paragraph::new(content).block(field), rows[2]);
    f.render_widget(
        Paragraph::new(Span::styled(
            " m cycles warn → confirm → strict · copies (⏎) are only warned",
            Style::default().fg(t.dim),
        ))
        .wrap(Wrap { trim: true }),
        rows[3],
    );
}

fn draw_note(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 70, 5);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent))
        .title(Span::styled(" writeup note ", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("✎ ", Style::default().fg(theme().accent2)),
            Span::styled(app.note_buf.clone(), Style::default().fg(theme().fg)),
            Span::styled("▏", Style::default().fg(theme().accent)),
        ]))
        .wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_add(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 72, 16);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(
            " add command ",
            Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(inner);

    let field = app.add.field_kind();
    draw_field(f, app, rows[0], "category", &app.add.category, field == AddField::Category);
    draw_field(f, app, rows[1], "name", &app.add.name, field == AddField::Name);
    draw_field(f, app, rows[2], "description", &app.add.description, field == AddField::Description);
    draw_field(f, app, rows[3], "template", &app.add.template, field == AddField::Template);

    let phs = extract_placeholders(&app.add.template);
    let hint = if phs.is_empty() {
        "tip: use <UPPERCASE> tokens as placeholders, e.g. <RHOST> <LPORT>".to_string()
    } else {
        format!("placeholders detected: {}", phs.join(", "))
    };
    f.render_widget(
        Paragraph::new(Span::styled(hint, Style::default().fg(theme().accent)))
            .wrap(Wrap { trim: true }),
        rows[4],
    );
}

fn draw_field(f: &mut Frame, app: &App, area: Rect, label: &str, val: &str, active: bool) {
    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(if active { theme().accent } else { theme().border }))
        .title(Span::styled(
            format!(" {label} "),
            Style::default().fg(if active { theme().accent } else { theme().dim }),
        ));
    let text = Line::from(vec![
        Span::styled(val.to_string(), Style::default().fg(theme().fg)),
        Span::styled(if active { "▏" } else { "" }, Style::default().fg(theme().accent)),
    ]);
    f.render_widget(Paragraph::new(text).block(b).wrap(Wrap { trim: false }), area);
}

fn draw_splash(f: &mut Frame, app: &App, area: Rect) {
    // animations off: the finished banner, prompt steady
    let el = if app.prefs.animations { app.splash_ms() } else { u64::MAX / 4 };
    // bold background behind the banner
    if let Some(bg) = crate::anim::Bg::of_splash(app.prefs.splash_bg, app.prefs.background) {
        draw_bg(f, app, area, bg, true);
    }
    let Some(b) = crate::anim::draw_banner(f.buffer_mut(), area, &app.fx, el) else {
        return; // too small; just the background
    };
    let revealed = el >= app.fx.reveal_ms;
    // underline rule + subtitle once the banner has landed (when there's room)
    if revealed && b.bottom() < area.bottom() {
        let rule: String = "─".repeat(b.width as usize);
        f.render_widget(
            Paragraph::new(Span::styled(rule, Style::default().fg(theme().border))),
            Rect::new(b.x, b.bottom(), b.width, 1),
        );
        let raw = app.fx.subtitle.trim();
        let spaced = crate::anim::spaced(raw);
        let sub = if spaced.chars().count() <= area.width as usize { spaced } else { raw.to_string() };
        if !sub.is_empty() && b.bottom() + 1 < area.bottom() {
            let sw = (Span::raw(sub.as_str()).width() as u16).min(area.width);
            let sx = area.x + (area.width - sw) / 2;
            f.render_widget(
                Paragraph::new(Span::styled(sub, Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
                Rect::new(sx, b.bottom() + 1, sw, 1),
            );
        }
    }

    // blinking prompt near the bottom
    let prompt_y = area.y + area.height.saturating_sub(2);
    if el >= app.fx.reveal_ms + 150 && prompt_y > b.bottom() + 1 {
        let blink_on = !app.prefs.animations || (el / 150) % 2 == 0;
        if blink_on {
            let msg = if app.prefs.splash_hold { "✦  press any key  ✦" } else { "✦  any key skips  ✦" };
            let mw = (msg.chars().count() as u16).min(area.width);
            let mx = area.x + (area.width - mw) / 2;
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(theme().dim))),
                Rect::new(mx, prompt_y, mw, 1),
            );
        }
    }
}

/// Theme picker: every theme with a swatch strip; the UI behind it is
/// already repainted in the highlighted theme (live preview).
fn draw_theme_picker(f: &mut Frame, app: &App) {
    let list = crate::theme::all();
    let h = (list.len() as u16 + 5).min(f.area().height);
    let area = centered(f.area(), 50, h);
    f.render_widget(Clear, area);
    let t = theme();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(t.accent))
        .title(Span::styled(" theme ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(format!(" {}/{} ", app.theme_idx + 1, list.len()), Style::default().fg(t.dim))).right_aligned())
        .style(Style::default().bg(t.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();
    for (i, th) in list.iter().enumerate() {
        let sel = i == app.theme_idx;
        let custom = !crate::theme::PRESETS.iter().any(|p| std::ptr::eq(p, *th));
        let mut spans = vec![
            Span::styled(if sel { " ▸ " } else { "   " }, Style::default().fg(app.pulse())),
            Span::styled(
                format!("{:<18}", th.name),
                Style::default()
                    .fg(if sel { t.fg } else { t.subtle })
                    .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
            ),
        ];
        // swatch strip in that theme's own colours, on its own background
        for c in [th.accent, th.accent2, th.ok, th.warn, th.gold, th.syn_flag, th.orange] {
            spans.push(Span::styled("██", Style::default().fg(c).bg(th.bg)));
        }
        spans.push(Span::styled(if custom { " ✎" } else { "" }, Style::default().fg(t.dim)));
        let style = if sel { Style::default().bg(t.sel_bg) } else { Style::default() };
        lines.push(Line::from(spans).style(style));
    }
    lines.push(Line::from(""));
    let mut sample = vec![Span::raw(" ")];
    sample.extend(command_spans("nmap -sC -sV -p 22,80 10.10.11.42 | tee <OUT>"));
    lines.push(Line::from(sample));
    lines.push(Line::from(Span::styled(
        format!(" custom: {}/*.toml", crate::theme::custom_dir().display()).replace(&home_prefix(), "~"),
        Style::default().fg(t.dim),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}

fn home_prefix() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "\u{0}".into())
}

fn draw_help(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 66, 32);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(theme().accent))
        .title(Span::styled(
            " help ",
            Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let row = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("  {k:<12}"), Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
            Span::styled(d.to_string(), Style::default().fg(theme().fg)),
        ])
    };
    use Action as A;
    let k = &app.keys;
    let l = |a: Action| k.label(a);
    let j = |acts: &[Action]| acts.iter().map(|a| k.label(*a)).collect::<Vec<_>>().join(" / ");
    let lines = vec![
        Line::from(Span::styled(" navigation", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row(&k.pair(A::FocusNext, A::FocusPrev), "switch pane (categories ↔ commands)"),
        row(&k.pair(A::Down, A::Up), "move within a pane"),
        Line::from(Span::styled(" actions", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row(&l(A::Copy), "fill from vars + copy (and log it)"),
        row(&l(A::Run), "run it live (◎ target: copy only · ✗ tool missing)"),
        row(&l(A::Missing), "hide / show ✗ commands whose tool isn't installed"),
        row(&l(A::Parse), "parse scan output → PORTS + ✸ suggested"),
        row(&l(A::Recipes), "recipes: run command bundles · A = all matched"),
        row(&l(A::Htb), "Hack The Box: machine, guided tasks + hints, flags"),
        row(&l(A::Vars), "edit variables (split: this cmd / all)"),
        row(&l(A::Favorite), "pin ★ favorite (shows ★/⏱ categories)"),
        row(&[A::Settings, A::Bg, A::Theme, A::ThemePicker].map(|a| k.compact(a)).join("/"), &format!(
            "settings (all) · bg {} · theme: {}",
            app.prefs.background.as_str(),
            theme().name.chars().take(18).collect::<String>()
        )),
        row(&j(&[A::Discord, A::DiscordPrivacy]), &format!(
            "Discord presence {} · privacy: {}",
            if app.discord_rpc { "on" } else { "off" },
            app.discord_privacy.as_str()
        )),
        row(&j(&[A::Search, A::Add, A::Delete]), "fuzzy search all · add / delete a command"),
        row(&format!("{} / esc", l(A::Quit)), "quit (^c always works)"),
        Line::from(Span::styled(" run pane (⇥ into it)", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row(&l(A::Interact), &format!("interact (type into it) · {} detach", k.compact(A::Detach))),
        row(&k.pair(A::ScrollDown, A::ScrollUp), &format!(
            "scroll · {} switch tab · {} tail",
            k.pair(A::TabPrev, A::TabNext),
            l(A::Tail)
        )),
        row(&l(A::Zoom), "zoom the run tab fullscreen (bigger PTY)"),
        row(&j(&[A::Kill, A::Close, A::Rerun]), "kill job / close tab / re-run (restored too)"),
        Line::from(Span::styled(" ctf rooms + writeup", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row(&l(A::Rooms), "rooms: ⏎ use · n new · p thm/htb · s scope"),
        row(&l(A::Loot), "loot: creds · ⏎ load vars · c crack hash"),
        row(&l(A::Gtfobins), "GTFOBins privesc lookup (offline)"),
        row(&l(A::Note), "add a writeup note to the log"),
        row(&j(&[A::Flag, A::Export]), "capture flag → notes · export writeup (redacted)"),
        row(&j(&[A::Scope, A::Notes]), &format!("room scope (⊙ allowed hosts, {}) · notes", app.scope_mode.as_str())),
        row(&l(A::History), "history: ⏎ copy · x re-run · l log · / filter"),
        Line::from(Span::styled(
            "  copies auto-log · tabs restore on launch (--fresh) · pivt keys",
            Style::default().fg(theme().dim),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}

// ───────────────────────────── settings (`,`) ─────────────────────────────

/// Display width of a string.
fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

/// Truncate (with …) or pad `s` to exactly `w` columns.
fn fit(s: &str, w: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    let total = width_of(s);
    for c in s.chars() {
        let cw = width_of(&c.to_string());
        if total > w && used + cw + 1 > w {
            if w > used {
                out.push('…');
                used += 1;
            }
            break;
        }
        if used + cw > w {
            break;
        }
        out.push(c);
        used += cw;
    }
    out.push_str(&" ".repeat(w.saturating_sub(used)));
    out
}

/// The settings overlay rect: the whole screen (minus the footer) when
/// small, a centred box when large.
fn settings_area(full: Rect) -> Rect {
    let avail_h = full.height.saturating_sub(1).max(1);
    let w = if full.width <= 92 { full.width } else { (full.width as u32 * 85 / 100).clamp(92, 132) as u16 };
    let h = if avail_h <= 30 { avail_h } else { (avail_h as u32 * 85 / 100).clamp(30, 46) as u16 };
    let x = full.x + (full.width - w) / 2;
    let y = full.y + (avail_h - h) / 2;
    Rect::new(x, y, w, h)
}

fn draw_settings(f: &mut Frame, app: &App) {
    use crate::settings::{EditKind, Item, Sec};
    let t = theme();
    let area = settings_area(f.area());
    if area.width < 20 || area.height < 6 {
        return;
    }
    f.render_widget(Clear, area);
    let s = &app.settings;
    let rows = app.settings_rows();
    let changed: usize = Sec::ALL.iter().map(|x| app.settings_custom_count(*x)).sum();
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(btype(app))
        .border_style(Style::default().fg(t.accent))
        .title(Span::styled(" settings ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD)))
        .title(
            Line::from(Span::styled(
                format!(" {changed} changed in config.toml "),
                Style::default().fg(if changed > 0 { t.accent2 } else { t.dim }),
            ))
            .right_aligned(),
        )
        .style(Style::default().bg(t.panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let detail_h = if inner.height >= 14 { 4 } else { 2 };
    let v = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1), Constraint::Length(detail_h)])
        .split(inner);

    // ── filter line ──
    let filtering = s.filtering || !s.filter.is_empty();
    let mut fl = vec![Span::styled(" / ", Style::default().fg(t.accent).add_modifier(Modifier::BOLD))];
    if filtering {
        fl.push(Span::styled(
            format!("{}{}", s.filter, if s.filtering { "▏" } else { "" }),
            Style::default().fg(t.fg).add_modifier(Modifier::BOLD),
        ));
        fl.push(Span::styled(
            format!("   {} match{}", rows.len(), if rows.len() == 1 { "" } else { "es" }),
            Style::default().fg(if rows.is_empty() { t.warn } else { t.dim }),
        ));
    } else {
        fl.push(Span::styled("filter every option by name or description", Style::default().fg(t.dim)));
    }
    f.render_widget(Paragraph::new(Line::from(fl)), v[0]);

    // ── sections | options ──
    let sec_w: u16 = if v[1].width >= 70 { 22 } else if v[1].width >= 48 { 18 } else { 0 };
    let h = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sec_w), Constraint::Length(if sec_w > 0 { 1 } else { 0 }), Constraint::Min(1)])
        .split(v[1]);
    if sec_w > 0 {
        let mut lines = Vec::new();
        for (i, sec) in Sec::ALL.iter().enumerate() {
            let sel = i == s.sec && !filtering;
            let n = app.settings_custom_count(*sec);
            let mark = if sel && !s.right { app.marker() } else if sel { "›".into() } else { " ".into() };
            let count = if n > 0 { format!("{n}●") } else { String::new() };
            let name_w = (sec_w as usize).saturating_sub(3 + width_of(&count));
            let style = if sel && !s.right {
                Style::default().fg(t.fg).bg(t.sel_bg).add_modifier(Modifier::BOLD)
            } else if sel {
                Style::default().fg(t.accent2).add_modifier(Modifier::BOLD)
            } else if filtering {
                Style::default().fg(t.dim)
            } else {
                Style::default().fg(t.subtle)
            };
            lines.push(
                Line::from(vec![
                    Span::styled(mark, Style::default().fg(app.pulse())),
                    Span::raw(" "),
                    Span::styled(fit(sec.title(), name_w), style),
                    Span::styled(format!("{count} "), Style::default().fg(t.accent2)),
                ])
                .style(if sel && !s.right { Style::default().bg(t.sel_bg) } else { Style::default() }),
            );
        }
        f.render_widget(Paragraph::new(lines), h[0]);
        let sep: Vec<Line> = (0..h[1].height).map(|_| Line::from(Span::styled("│", Style::default().fg(t.border)))).collect();
        f.render_widget(Paragraph::new(sep), h[1]);
    }

    // ── options ──
    let right = h[2];
    let w = right.width as usize;
    let name_w = if w >= 90 { 22 } else if w >= 60 { 18 } else { 14 };
    let val_w = if w >= 90 { 24 } else if w >= 60 { 18 } else { 12 };
    // narrow: a one-cell ●/· badge instead of "● set" / "default"
    let badge_w = if w >= 80 { 8 } else { 1 };
    let desc_w = w.saturating_sub(2 + name_w + 1 + val_w + 1 + badge_w + 1);
    let sel_row = if s.right { Some(s.row.min(rows.len().saturating_sub(1))) } else { None };
    let mut items: Vec<ListItem> = Vec::new();
    for (i, it) in rows.iter().enumerate() {
        let sel = sel_row == Some(i);
        let set = app.setting_is_set(*it);
        let kind = it.kind();
        let mut spans = vec![
            Span::styled(if sel { app.marker() } else { " ".into() }, Style::default().fg(app.pulse())),
            Span::raw(" "),
            Span::styled(
                fit(&it.name(), name_w),
                Style::default()
                    .fg(if sel { t.fg } else { t.subtle })
                    .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
            ),
            Span::raw(" "),
        ];
        // value cell
        let editing = sel && s.edit.is_some();
        let capturing = sel && s.capture.is_some();
        if editing {
            let buf = s.edit.clone().unwrap_or_default();
            let bad = s.edit_err.is_some();
            let ew = val_w + 1 + badge_w + 1 + desc_w;
            // keep the end of a long buffer (the cursor) in view
            let shown: String = {
                let n = buf.chars().count();
                let keep = ew.saturating_sub(2);
                if n > keep { buf.chars().skip(n - keep).collect() } else { buf.clone() }
            };
            spans.push(Span::styled(
                fit(&format!("{shown}▏"), ew),
                Style::default().fg(if bad { t.warn } else { t.fg }).bg(t.sel_bg).add_modifier(Modifier::BOLD),
            ));
            items.push(ListItem::new(Line::from(spans)));
            continue;
        }
        if capturing {
            let replace = s.capture.is_some_and(|c| c.1);
            spans.push(Span::styled(
                fit(if replace { "press the new key…" } else { "press a key to add…" }, val_w),
                Style::default().fg(app.pulse()).add_modifier(Modifier::BOLD),
            ));
        } else {
            let disp = app.setting_display(*it);
            let (txt, style) = match kind {
                EditKind::Bool => {
                    let on = disp == "on";
                    (
                        if on { "● on".to_string() } else { "○ off".to_string() },
                        Style::default().fg(if on { t.ok } else { t.dim }),
                    )
                }
                EditKind::Int { .. } | EditKind::Choice(_) | EditKind::Glyphs => {
                    let unit = match it {
                        Item::Pref(k) => match crate::prefs::Opt::find(k).map(|o| &o.kind) {
                            Some(crate::prefs::Kind::Int { unit, .. }) if !unit.is_empty() => format!(" {unit}"),
                            _ => String::new(),
                        },
                        _ => String::new(),
                    };
                    (
                        if sel { format!("‹ {disp}{unit} ›") } else { format!("  {disp}{unit}") },
                        Style::default().fg(t.accent).add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                    )
                }
                EditKind::Theme => (String::new(), Style::default()),
                EditKind::Key => (
                    disp.clone(),
                    Style::default().fg(if disp.starts_with('‹') { t.warn } else { t.accent }).add_modifier(Modifier::BOLD),
                ),
                EditKind::Text => (
                    disp.clone(),
                    Style::default().fg(if disp.starts_with('‹') { t.dim } else { t.fg }),
                ),
            };
            if let (EditKind::Theme, Item::Theme(ti)) = (kind, it) {
                if let Some(th) = crate::theme::all().get(*ti) {
                    let mut used = 0;
                    for c in [th.accent, th.accent2, th.ok, th.warn, th.gold, th.syn_flag, th.orange] {
                        if used + 2 > val_w {
                            break;
                        }
                        spans.push(Span::styled("██", Style::default().fg(c).bg(th.bg)));
                        used += 2;
                    }
                    spans.push(Span::raw(" ".repeat(val_w - used)));
                }
            } else {
                spans.push(Span::styled(fit(&txt, val_w), style));
            }
        }
        spans.push(Span::raw(" "));
        // badge: set in config / default / in use
        let in_use = matches!(it, Item::Theme(_)) && app.setting_value(*it) == crate::prefs::Val::Bool(true);
        let (badge, bstyle) = if in_use {
            ("● in use", Style::default().fg(t.ok))
        } else if matches!(it, Item::Theme(0)) {
            ("default", Style::default().fg(t.dim))
        } else if matches!(it, Item::Theme(_)) {
            ("", Style::default())
        } else if set {
            ("● set", Style::default().fg(t.accent2))
        } else {
            ("default", Style::default().fg(t.dim))
        };
        let badge = if badge_w == 1 {
            match badge {
                "" => " ",
                "default" => "·",
                _ => "●",
            }
        } else {
            badge
        };
        spans.push(Span::styled(fit(badge, badge_w), bstyle));
        spans.push(Span::raw(" "));
        // description (or the first conflict, in warn)
        let conflicts = match it {
            Item::Key(a) => app.key_conflicts(*a),
            _ => Vec::new(),
        };
        if desc_w > 3 {
            let (d, ds) = if let Some(c) = conflicts.first() {
                (format!("⚠ {c}"), Style::default().fg(t.warn))
            } else {
                let mut d = it.desc();
                if filtering {
                    d = format!("{} · {d}", it.sec().title());
                }
                if let Some(n) = it.restart_note() {
                    d = format!("↻ {n} · {d}");
                }
                (d, Style::default().fg(t.muted))
            };
            spans.push(Span::styled(fit(&d, desc_w), ds));
        } else if !conflicts.is_empty() {
            spans.push(Span::styled("⚠", Style::default().fg(t.warn)));
        }
        let line = Line::from(spans);
        items.push(ListItem::new(if sel { line.style(Style::default().bg(t.sel_bg)) } else { line }));
    }
    if rows.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled("   nothing matches", Style::default().fg(t.dim)))));
    }
    let mut state = ListState::default();
    state.select(sel_row);
    f.render_stateful_widget(List::new(items), right, &mut state);

    // ── detail ──
    let d = v[2];
    let mut lines: Vec<Line> = vec![Line::from(Span::styled("─".repeat(d.width as usize), Style::default().fg(t.border)))];
    let label = |s: &str| Span::styled(s.to_string(), Style::default().fg(t.dim));
    match app.settings_selected() {
        Some(it) => {
            lines.push(Line::from(vec![
                Span::styled(format!(" {}", it.name()), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {}", it.desc()), Style::default().fg(t.fg)),
            ]));
            let mut l2 = vec![label(" "), Span::styled(it.range(), Style::default().fg(t.subtle))];
            if !matches!(it, Item::Theme(_)) {
                l2.push(label(" · default "));
                l2.push(Span::styled(app.setting_default_display(it), Style::default().fg(t.subtle)));
            }
            lines.push(Line::from(l2));
            let note = if let Some(e) = s.edit_err.as_ref().filter(|_| s.edit.is_some()) {
                Some((format!(" ✗ not saved: {e}"), t.warn))
            } else if s.edit.is_some() {
                Some((" ⏎ save · esc cancel · ^u clear".to_string(), t.dim))
            } else if s.capture.is_some() {
                Some((" press the key · esc cancel · ⌫ remove the last key · ctrl-c is reserved".into(), t.accent))
            } else if let Item::Key(a) = it {
                let c = app.key_conflicts(a);
                if c.is_empty() {
                    None
                } else {
                    Some((format!(" ⚠ {}", c.join(" · ")), t.warn))
                }
            } else {
                it.restart_note().map(|n| (format!(" ↻ {n}"), t.gold))
            };
            if let Some((n, c)) = note {
                lines.push(Line::from(Span::styled(n, Style::default().fg(c))));
            }
        }
        None => {
            let sec = s.section();
            let n = crate::settings::items(sec).len();
            lines.push(Line::from(vec![
                Span::styled(format!(" {}", sec.title()), Style::default().fg(t.accent).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!("  {n} {} · {} changed", if sec == Sec::Theme { "themes" } else { "options" }, app.settings_custom_count(sec)),
                    Style::default().fg(t.fg),
                ),
            ]));
            lines.push(Line::from(label(" changes apply live and save to config.toml · r resets (removes the key)")));
        }
    }
    // the short detail area shows the most important lines
    if d.height < lines.len() as u16 {
        lines.remove(0);
    }
    f.render_widget(Paragraph::new(lines), d);
}
