use crate::app::{AddField, App, Focus, Mode};
use crate::model::extract_placeholders;
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
    // only in empty cells. Toggle with 'b'.
    if app.flow_bg {
        let tint = category_style(app.current_category().unwrap_or("")).1;
        crate::anim::draw_flow_app(f, area, app.tick, app.mouse, tint);
    } else {
        crate::anim::draw_field_app(f, area, app.tick);
        crate::anim::draw_shooting_star(f, area, app.tick, 260, false);
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header
            Constraint::Min(5),    // body
            Constraint::Length(1), // footer
        ])
        .split(area);

    draw_header(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);

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
        crate::anim::draw_burst_ring(f, area, p);
        let flash = (app.tick / 2) % 2 == 0;
        let msg = format!("🚩  {}  🚩", app.burst_label);
        let mw = msg.chars().count() as u16 + 4;
        let mh = 3u16;
        let mx = area.x + area.width.saturating_sub(mw) / 2;
        let my = area.y + area.height / 2 - 1;
        let r = Rect::new(mx, my, mw.min(area.width), mh.min(area.height));
        f.render_widget(Clear, r);
        let color = if flash { theme().ok } else { theme().star_hi };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
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

fn panel(title: &str, focused: bool) -> Block<'_> {
    let border = if focused { theme().border_hi } else { theme().border };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(if focused { theme().accent } else { theme().dim })
                .add_modifier(Modifier::BOLD),
        ))
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(22), // logo
            Constraint::Min(16),    // search
            Constraint::Length(40), // target strip
        ])
        .split(area);

    // logo — twinkling accent star + active room
    let star = ['✦', '✧', '⋆', '✦'][(app.tick / 8 % 4) as usize];
    let mut logo_spans = vec![
        Span::styled(format!(" {star} "), Style::default().fg(theme().accent)),
        Span::styled("pivt", Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)),
    ];
    if let Some(r) = &app.active_room {
        let mut name = r.name.clone();
        if name.chars().count() > 10 {
            name = name.chars().take(9).collect();
            name.push('…');
        }
        logo_spans.push(Span::styled(" ▸ ", Style::default().fg(theme().dim)));
        logo_spans.push(Span::styled(name, Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)));
    }
    f.render_widget(Paragraph::new(Line::from(logo_spans)).block(bare_block(theme().border)), cols[0]);

    // search
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
    f.render_widget(
        Paragraph::new(content).block(bare_block(if searching { theme().border_hi } else { theme().border })),
        cols[1],
    );

    // target strip — ◎ target   ⇡ listener
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
    // reachability dot (background ping)
    let ping = app.ping();
    let dot = match ping.up {
        Some(true) => Span::styled("● ", Style::default().fg(theme().ok)),
        Some(false) => Span::styled("● ", Style::default().fg(theme().warn)),
        None => Span::styled("○ ", Style::default().fg(theme().dim)),
    };
    // ✓ / ⚠: RHOST inside / outside the active room's scope
    let mark = match app.rhost_in_scope() {
        Some(true) => Span::styled(" ✓", Style::default().fg(theme().ok)),
        Some(false) => Span::styled(" ⚠", Style::default().fg(theme().warn).add_modifier(Modifier::BOLD)),
        None => Span::raw(""),
    };
    let targets = Line::from(vec![
        Span::raw(" "),
        dot,
        v(&rhost, "no target"),
        mark,
        Span::styled("  ⇡ ", Style::default().fg(theme().accent2)),
        if lhost.is_empty() && lport.is_empty() {
            Span::styled("no listener", Style::default().fg(theme().warn))
        } else if app.lhost_stale {
            Span::styled(format!("{listener} ⚠ not local"), Style::default().fg(theme().warn).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(listener, Style::default().fg(theme().ok).add_modifier(Modifier::BOLD))
        },
    ]);
    // box timer (+ rtt) on the right of the target border
    let via = ping.tcp_port.map(|p| format!(" tcp/{p}")).unwrap_or_default();
    let timer = match (app.box_elapsed(), ping.rtt_ms) {
        (Some(t), Some(r)) => format!(" ⏱ {t} · {r}ms{via} "),
        (Some(t), None) => format!(" ⏱ {t} "),
        _ => String::new(),
    };
    let mut title = vec![Span::styled(" target ", Style::default().fg(theme().dim))];
    if app.active_room.is_some() {
        let n = app.scope.entries.len();
        let color = if n == 0 || app.rhost_in_scope() == Some(false) { theme().warn } else { theme().accent2 };
        title.push(Span::styled(format!("⊙ scope: {n} "), Style::default().fg(color)));
    }
    let mut block = bare_block(theme().border).title(Line::from(title));
    if !timer.is_empty() {
        block = block.title(Line::from(Span::styled(timer, Style::default().fg(theme().dim))).right_aligned());
    }
    f.render_widget(Paragraph::new(targets).block(block), cols[2]);
}

fn bare_block(border: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
    // zoomed: the run pane takes the whole body
    if app.show_run && !app.runs.is_empty() && app.run_fullscreen {
        draw_run(f, app, area);
        return;
    }
    // reserve the lower part for the run pane when it's visible
    let (top, run) = if app.show_run && !app.runs.is_empty() {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Percentage(42)])
            .split(area);
        (rows[0], Some(rows[1]))
    } else {
        (area, None)
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(20),
            Constraint::Percentage(42),
            Constraint::Min(30),
        ])
        .split(top);

    draw_categories(f, app, cols[0]);
    draw_commands(f, app, cols[1]);
    draw_preview(f, app, cols[2]);

    if let Some(run_area) = run {
        draw_run(f, app, run_area);
    }
}

fn draw_run(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Run && app.mode == Mode::Normal;
    let mut block = panel("run", focused).style(Style::default().bg(theme().bg));
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
            ("●", crate::anim::pulse(app.tick))
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
            Span::styled(" running ", Style::default().fg(theme().bg).bg(crate::anim::pulse(app.tick)))
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .block(panel("categories", focused))
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .block(panel(&title, focused))
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
    let block = panel("preview", focused);
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
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme().accent).add_modifier(Modifier::DIM))
        .title(Span::styled(" command ", Style::default().fg(theme().accent)))
        .title(
            Line::from(Span::styled(
                if cmd.is_target() { " ⏎ copy · run it on the box " } else { " ⏎ copy " },
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
        "  ↑↓ pick · ⏎ edit here"
    } else {
        "  ⇥ focus · e popup"
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
            Span::styled(" ▸ ", Style::default().fg(crate::anim::pulse(app.tick)))
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
    if app.mode == Mode::Normal && app.focus == Focus::Run {
        let zoom = if app.run_fullscreen { "unzoom" } else { "zoom" };
        let pairs: &[(&str, &str)] = if app.interacting {
            &[("⌨", "typing → program"), ("F12", "detach")]
        } else if app.active_run().is_some_and(|j| !j.is_running()) {
            // finished / restored tab: nothing to type into, offer a re-run
            &[("r", "re-run"), ("z", zoom), ("↑↓", "scroll"), ("←→", "tab"), ("d", "close"), ("g", "tail")]
        } else {
            &[
                ("i", "interact"), ("z", zoom),
                ("↑↓", "scroll"), ("←→", "tab"), ("K", "kill"), ("d", "close"), ("g", "tail"),
            ]
        };
        render_footer(f, app, area, pairs);
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
        Mode::Normal => &[
            ("↹", "pane"), ("⏎", "copy"), ("x", "run"), ("e", "vars"),
            ("c", "room"), ("L", "loot"), ("w", "note"), ("/", "find"), ("?", "help"),
        ],
    };
    render_footer(f, app, area, pairs);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect, pairs: &[(&str, &str)]) {
    let mut spans: Vec<Span> = Vec::new();
    for (i, (k, lbl)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", Style::default().fg(theme().border)));
        }
        spans.push(Span::styled((*k).to_string(), Style::default().fg(theme().accent).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {lbl}"), Style::default().fg(theme().dim)));
    }
    spans.push(Span::raw(" "));

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(80)])
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
        .border_type(BorderType::Rounded)
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
            Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .border_type(BorderType::Rounded)
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .border_type(BorderType::Rounded)
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .border_type(BorderType::Rounded)
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
                Line::from(Span::styled("3. restart pivt and press H", Style::default().fg(theme().dim))),
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
            let mark = if sel { Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick))) } else { Span::raw(" ") };
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
    let cursor = if app.tick / 10 % 2 == 0 { "▌" } else { " " };
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
        .border_type(BorderType::Rounded)
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(" add loot ", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3); 4])
        .split(inner);
    draw_field(f, rows[0], "user", &app.loot_form.user, app.loot_form.field == 0);
    draw_field(f, rows[1], "pass", &app.loot_form.pass, app.loot_form.field == 1);
    draw_field(f, rows[2], "hash", &app.loot_form.hash, app.loot_form.field == 2);
    draw_field(f, rows[3], "note", &app.loot_form.note, app.loot_form.field == 3);
}

fn draw_rooms(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 84, 24);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
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
                Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
            .block(bare_block(theme().border).title(Span::styled(" rooms ", Style::default().fg(theme().dim))))
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
            .block(bare_block(theme().border).title(Span::styled(" preview ", Style::default().fg(theme().dim))))
            .wrap(Wrap { trim: true }),
        cols[1],
    );
}

fn draw_newroom(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 64, 14);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme().accent2))
        .title(Span::styled(" new room ", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(theme().panel));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Length(3), Constraint::Min(1)])
        .split(inner);
    draw_field(f, rows[0], "name", &app.new_room.name, app.new_room.field == 0);
    draw_field(f, rows[1], "target ip (optional)", &app.new_room.ip, app.new_room.field == 1);
    draw_field(f, rows[2], "hostname (optional)", &app.new_room.host, app.new_room.field == 2);
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
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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
                        Span::styled(" ▎", Style::default().fg(crate::anim::pulse(app.tick)))
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
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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
        .border_type(BorderType::Rounded)
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
    draw_field(f, rows[0], "category", &app.add.category, field == AddField::Category);
    draw_field(f, rows[1], "name", &app.add.name, field == AddField::Name);
    draw_field(f, rows[2], "description", &app.add.description, field == AddField::Description);
    draw_field(f, rows[3], "template", &app.add.template, field == AddField::Template);

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

fn draw_field(f: &mut Frame, area: Rect, label: &str, val: &str, active: bool) {
    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
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
    let frame = app.splash_frame();
    // bold background behind the banner
    if app.flow_bg {
        crate::anim::draw_flow_splash(f, area, app.tick, app.mouse);
    } else {
        crate::anim::draw_field_splash(f, area, app.tick);
        crate::anim::draw_shooting_star(f, area, app.tick, 44, true);
    }

    let banner = crate::anim::BANNER;
    let w = banner.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
    let h = banner.len() as u16;
    if area.width < w + 2 || area.height < h + 6 {
        return; // too small; just show starfield
    }
    let bx = area.x + (area.width - w) / 2;
    let by = area.y + area.height / 2 - (h / 2) - 1;

    // typewriter reveal across columns
    const REVEAL: u64 = 15;
    let reveal_cols = if frame >= REVEAL {
        w as usize
    } else {
        ((frame as usize * w as usize) / REVEAL as usize).max(1)
    };

    let mut lines: Vec<Line> = Vec::new();
    for row in banner {
        let chars: Vec<char> = row.chars().collect();
        let mut spans: Vec<Span> = Vec::new();
        let shown = reveal_cols.min(chars.len());
        if shown > 0 {
            let body: String = chars[..shown.saturating_sub(1)].iter().collect();
            spans.push(Span::styled(
                body,
                Style::default().fg(theme().accent).add_modifier(Modifier::BOLD),
            ));
            // leading edge glows brighter while still revealing
            let edge: String = chars[shown.saturating_sub(1)..shown].iter().collect();
            let edge_color = if reveal_cols < w as usize { theme().star_hi } else { theme().accent };
            spans.push(Span::styled(
                edge,
                Style::default().fg(edge_color).add_modifier(Modifier::BOLD),
            ));
        }
        lines.push(Line::from(spans));
    }
    let banner_rect = Rect::new(bx, by, w, h);
    f.render_widget(Paragraph::new(lines), banner_rect);

    // underline rule that draws across after the banner lands
    if frame >= REVEAL {
        let rule_w = w;
        let rule: String = "─".repeat(rule_w as usize);
        let rule_rect = Rect::new(bx, by + h, w, 1);
        f.render_widget(
            Paragraph::new(Span::styled(rule, Style::default().fg(theme().border))),
            rule_rect,
        );
        // subtitle
        let sub = "c t f   t o o l k i t";
        let sw = sub.chars().count() as u16;
        let sx = area.x + (area.width.saturating_sub(sw)) / 2;
        f.render_widget(
            Paragraph::new(Span::styled(
                sub,
                Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD),
            )),
            Rect::new(sx, by + h + 1, sw, 1),
        );
    }

    // blinking prompt near the bottom
    if frame >= REVEAL + 3 {
        let blink_on = (frame / 3) % 2 == 0;
        if blink_on {
            let msg = "✦  press any key  ✦";
            let mw = msg.chars().count() as u16;
            let mx = area.x + (area.width.saturating_sub(mw)) / 2;
            let my = area.y + area.height - 2;
            f.render_widget(
                Paragraph::new(Span::styled(msg, Style::default().fg(theme().dim))),
                Rect::new(mx, my, mw, 1),
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
        .border_type(BorderType::Rounded)
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
            Span::styled(if sel { " ▸ " } else { "   " }, Style::default().fg(crate::anim::pulse(app.tick))),
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
        .border_type(BorderType::Rounded)
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
    let lines = vec![
        Line::from(Span::styled(" navigation", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row("tab / ←→", "switch pane (categories ↔ commands)"),
        row("↑↓ / jk", "move within a pane"),
        Line::from(Span::styled(" actions", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row("enter", "fill from vars + copy (and log it)"),
        row("x", "run it live (◎ target: copy only · ✗ tool missing)"),
        row("m", "hide / show ✗ commands whose tool isn't installed"),
        row("P", "parse scan output → PORTS + ✸ suggested"),
        row("R", "recipes: run command bundles · A = all matched"),
        row("H", "Hack The Box: machine, guided tasks + hints, flags"),
        row("e", "edit variables (split: this cmd / all)"),
        row("f", "pin ★ favorite (shows ★/⏱ categories)"),
        row("b / t / T", &format!(
            "background fx · t / T theme: {}",
            theme().name.chars().take(20).collect::<String>()
        )),
        row("D / M", &format!(
            "Discord presence {} · M cycles privacy: {}",
            if app.discord_rpc { "on" } else { "off" },
            app.discord_privacy.as_str()
        )),
        row("/ · a / d", "fuzzy search all · add / delete a command"),
        row("q / esc", "quit"),
        Line::from(Span::styled(" run pane (⇥ into it)", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row("i / ⏎", "interact (type into it) · F12 detach"),
        row("↑↓", "scroll · ←→ switch tab · g tail"),
        row("z", "zoom the run tab fullscreen (bigger PTY)"),
        row("K / d / r", "kill job / close tab / re-run (restored too)"),
        Line::from(Span::styled(" ctf rooms + writeup", Style::default().fg(theme().accent2).add_modifier(Modifier::BOLD))),
        row("c", "rooms: ⏎ use · n new · p thm/htb · s scope"),
        row("L", "loot: creds · ⏎ load vars · c crack hash"),
        row("g", "GTFOBins privesc lookup (offline)"),
        row("w", "add a writeup note to the log"),
        row("F / E", "capture flag → notes · export writeup (redacted)"),
        row("S / o", &format!("room scope (⊙ allowed hosts, {}) · o notes", app.scope_mode.as_str())),
        row("y / ^r", "history: ⏎ copy · x re-run · l log · / filter"),
        Line::from(Span::styled(
            "  copies auto-log · run tabs come back on launch (--fresh)",
            Style::default().fg(theme().dim),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
