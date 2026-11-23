use crate::app::{AddField, App, Focus, Mode};
use crate::model::extract_placeholders;
use crate::theme::*;
use ratatui::prelude::*;
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(BG)), area);

    if app.mode == Mode::Splash {
        draw_splash(f, app, area);
        return;
    }

    // Calm, slowly-drifting starfield behind the whole UI; widgets paint on top,
    // leaving stars only in empty cells. A shooting star streaks by rarely.
    crate::anim::draw_field_app(f, area, app.tick);
    crate::anim::draw_shooting_star(f, area, app.tick, 260, false);

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
        Mode::Help => draw_help(f),
        Mode::Rooms => draw_rooms(f, app),
        Mode::NewRoom => draw_newroom(f, app),
        Mode::Note => draw_note(f, app),
        Mode::Loot => draw_loot(f, app),
        Mode::AddLoot => draw_addloot(f, app),
        Mode::Flag => draw_flag(f, app),
        _ => {}
    }
}

fn panel(title: &str, focused: bool) -> Block<'_> {
    let border = if focused { BORDER_HI } else { BORDER };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .title(Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(if focused { ACCENT } else { DIM })
                .add_modifier(Modifier::BOLD),
        ))
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(22), // logo
            Constraint::Min(16),    // search
            Constraint::Length(38), // target strip
        ])
        .split(area);

    // logo — twinkling accent star + active room
    let star = ['✦', '✧', '⋆', '✦'][(app.tick / 8 % 4) as usize];
    let mut logo_spans = vec![
        Span::styled(format!(" {star} "), Style::default().fg(ACCENT)),
        Span::styled("pivt", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
    ];
    if let Some(r) = &app.active_room {
        let mut name = r.name.clone();
        if name.len() > 10 {
            name.truncate(9);
            name.push('…');
        }
        logo_spans.push(Span::styled(" ▸ ", Style::default().fg(DIM)));
        logo_spans.push(Span::styled(name, Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)));
    }
    f.render_widget(Paragraph::new(Line::from(logo_spans)).block(bare_block(BORDER)), cols[0]);

    // search
    let searching = app.mode == Mode::Search;
    let content: Line = if app.search.is_empty() && !searching {
        Line::from(vec![
            Span::styled("  / ", Style::default().fg(ACCENT)),
            Span::styled("search commands…", Style::default().fg(DIM)),
        ])
    } else {
        Line::from(vec![
            Span::styled("  / ", Style::default().fg(ACCENT)),
            Span::styled(app.search.clone(), Style::default().fg(FG).add_modifier(Modifier::BOLD)),
            Span::styled(if searching { "▏" } else { "" }, Style::default().fg(ACCENT)),
        ])
    };
    f.render_widget(
        Paragraph::new(content).block(bare_block(if searching { BORDER_HI } else { BORDER })),
        cols[1],
    );

    // target strip — ◎ target   ⇡ listener
    let rhost = app.vars.get("RHOST").cloned().unwrap_or_default();
    let lhost = app.vars.get("LHOST").cloned().unwrap_or_default();
    let lport = app.vars.get("LPORT").cloned().unwrap_or_default();
    let v = |s: &str, ph: &str| {
        if s.is_empty() {
            Span::styled(ph.to_string(), Style::default().fg(WARN))
        } else {
            Span::styled(s.to_string(), Style::default().fg(OK).add_modifier(Modifier::BOLD))
        }
    };
    let listener = if lhost.is_empty() && lport.is_empty() {
        "·".to_string()
    } else {
        format!("{}:{}", if lhost.is_empty() { "·" } else { &lhost }, if lport.is_empty() { "·" } else { &lport })
    };
    let targets = Line::from(vec![
        Span::styled(" ◎ ", Style::default().fg(ACCENT2)),
        v(&rhost, "no target"),
        Span::styled("  ⇡ ", Style::default().fg(ACCENT2)),
        if lhost.is_empty() && lport.is_empty() {
            Span::styled("no listener", Style::default().fg(WARN))
        } else {
            Span::styled(listener, Style::default().fg(OK).add_modifier(Modifier::BOLD))
        },
    ]);
    f.render_widget(
        Paragraph::new(targets)
            .block(bare_block(BORDER).title(Span::styled(" target ", Style::default().fg(DIM)))),
        cols[2],
    );
}

fn bare_block(border: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
}

fn draw_body(f: &mut Frame, app: &App, area: Rect) {
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
    let mut block = panel("run", focused).style(Style::default().bg(BG));
    if let Some(job) = app.active_run() {
        let maxw = area.width.saturating_sub(10) as usize;
        let mut c = job.cmd.clone();
        if c.chars().count() > maxw && maxw > 1 {
            c = format!("{}…", c.chars().take(maxw - 1).collect::<String>());
        }
        block = block.title(Line::from(Span::styled(format!(" {c} "), Style::default().fg(SYN_OP))).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(inner);
    let bar = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(18)])
        .split(rows[0]);

    // tab bar
    let mut tabs: Vec<Span> = Vec::new();
    for (i, job) in app.runs.iter().enumerate() {
        let active = i == app.run_tab;
        let (glyph, gcol) = if job.is_running() {
            ("●", crate::anim::pulse(app.tick))
        } else if job.exit == Some(0) {
            ("✓", OK)
        } else {
            ("✗", WARN)
        };
        if i > 0 {
            tabs.push(Span::styled(" │ ", Style::default().fg(BORDER)));
        }
        tabs.push(Span::styled(format!("{glyph} "), Style::default().fg(gcol)));
        tabs.push(Span::styled(
            job.title.clone(),
            if active {
                Style::default().fg(FG).add_modifier(Modifier::BOLD).bg(SEL_BG)
            } else {
                Style::default().fg(DIM)
            },
        ));
    }
    f.render_widget(Paragraph::new(Line::from(tabs)), bar[0]);

    // status badge (right of the tab bar)
    if let Some(job) = app.active_run() {
        let badge = if job.is_running() {
            Span::styled(" running ", Style::default().fg(BG).bg(crate::anim::pulse(app.tick)))
        } else if job.exit == Some(0) {
            Span::styled(" exit 0 ", Style::default().fg(BG).bg(OK))
        } else {
            Span::styled(format!(" exit {} ", job.exit.unwrap_or(-1)), Style::default().fg(BG).bg(WARN))
        };
        let extra = if app.interacting {
            " ⌨".to_string()
        } else if job.scroll > 0 {
            format!(" ↑{}", job.scroll)
        } else {
            String::new()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![badge, Span::styled(extra, Style::default().fg(DIM))]))
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
        s = s.fg(Color::Rgb(0xc9, 0xd1, 0xd9));
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
                        .fg(if sel { color } else { Color::Rgb(0x8b, 0x98, 0xa5) })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
                Span::styled(format!("{count:>2}"), Style::default().fg(DIM)),
            ]))
        })
        .collect();

    let mut state = ListState::default();
    if !searching {
        state.select(Some(app.cat_idx));
    }
    let list = List::new(items)
        .block(panel("categories", focused))
        .highlight_style(Style::default().bg(SEL_BG));
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
                        .fg(if sel { FG } else { Color::Rgb(0xb0, 0xbc, 0xc9) })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ];
            if app.is_favorite_cmd(c) {
                spans.push(Span::styled("  ★", Style::default().fg(Color::Rgb(0xf0, 0xb4, 0x57))));
            }
            if c.user {
                spans.push(Span::styled("  ✎", Style::default().fg(ACCENT2)));
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
        .highlight_style(Style::default().bg(SEL_BG));
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
            spans.push(Span::styled(word, Style::default().fg(SYN_OP)));
            expect_binary = true;
            continue;
        }
        let base = if expect_binary {
            expect_binary = false;
            Style::default().fg(OK).add_modifier(Modifier::BOLD)
        } else if word.starts_with('-') {
            Style::default().fg(SYN_FLAG)
        } else if word.starts_with('\'') || word.starts_with('"') {
            Style::default().fg(SYN_STR)
        } else if is_numeric_like(&word) {
            Style::default().fg(SYN_NUM)
        } else {
            Style::default().fg(FG)
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
                    Style::default().fg(WARN).add_modifier(Modifier::BOLD),
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
        let p = Paragraph::new(Span::styled("no command selected", Style::default().fg(DIM)));
        f.render_widget(p, inner);
        return;
    };
    let filled = crate::model::fill_template(&cmd.template, &app.vars);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // name + badge
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
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(" {cat_glyph} {} ", cmd.category),
            Style::default().fg(BG).bg(cat_color).add_modifier(Modifier::BOLD),
        ),
    ];
    if cmd.user {
        title_spans.push(Span::styled("  ★ custom", Style::default().fg(ACCENT2)));
    }
    f.render_widget(Paragraph::new(Line::from(title_spans)), rows[0]);

    // description
    let desc = if cmd.description.is_empty() {
        Span::styled("—", Style::default().fg(DIM))
    } else {
        Span::styled(cmd.description.clone(), Style::default().fg(Color::Rgb(0x8b, 0x98, 0xa5)))
    };
    f.render_widget(Paragraph::new(desc).wrap(Wrap { trim: true }), rows[1]);

    // command box with a copy hint on the right
    let cmd_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT).add_modifier(Modifier::DIM))
        .title(Span::styled(" command ", Style::default().fg(ACCENT)))
        .title(Line::from(Span::styled(" ⏎ copy ", Style::default().fg(DIM))).right_aligned());
    f.render_widget(
        Paragraph::new(Line::from(command_spans(&filled)))
            .block(cmd_block)
            .wrap(Wrap { trim: false }),
        rows[2],
    );

    // tags
    let mut tag_spans: Vec<Span> = Vec::new();
    for t in &cmd.tags {
        tag_spans.push(Span::styled(format!("#{t} "), Style::default().fg(STAR_MID)));
    }
    if tag_spans.is_empty() {
        tag_spans.push(Span::styled("", Style::default()));
    }
    f.render_widget(Paragraph::new(Line::from(tag_spans)), rows[3]);

    // vars label — hint changes when the pane is focused
    let hint = if focused {
        "  ↑↓ pick · ⏎ edit here"
    } else {
        "  ⇥ focus · e popup"
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("─ ", Style::default().fg(BORDER)),
            Span::styled("variables", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)),
            Span::styled(hint, Style::default().fg(DIM)),
        ])),
        rows[4],
    );

    // vars used — selectable/editable inline when the preview is focused
    let phs = extract_placeholders(&cmd.template);
    let mut lines: Vec<Line> = Vec::new();
    if phs.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no variables — static command",
            Style::default().fg(DIM),
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
            Span::styled(" ▸ ", Style::default().fg(ACCENT))
        } else {
            let (b, st) = if val.is_empty() {
                ("○", Style::default().fg(WARN))
            } else {
                ("●", Style::default().fg(OK))
            };
            Span::styled(format!(" {b} "), st)
        };

        let name_style = if sel || editing {
            Style::default().fg(FG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Rgb(0x8b, 0x98, 0xa5))
        };

        let value_span = if editing {
            let buf = app.inline_edit.as_ref().map(|(_, b)| b.clone()).unwrap_or_default();
            Span::styled(format!("{buf}▏"), Style::default().fg(FG).add_modifier(Modifier::BOLD))
        } else if val.is_empty() {
            Span::styled("‹unset›".to_string(), Style::default().fg(WARN))
        } else {
            Span::styled(val, Style::default().fg(OK))
        };

        lines.push(Line::from(vec![
            gutter,
            Span::styled(format!("{ph:<10}"), name_style),
            value_span,
        ]));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), rows[5]);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    // run-pane focus has its own hint set
    if app.mode == Mode::Normal && app.focus == Focus::Run {
        let pairs: &[(&str, &str)] = if app.interacting {
            &[("⌨", "typing → program"), ("F12", "detach")]
        } else {
            &[
                ("i", "interact"), ("↑↓", "scroll"), ("←→", "tab"),
                ("K", "kill"), ("d", "close"), ("g", "tail"), ("⇥", "panes"),
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
        Mode::Rooms => &[("↑↓", "move"), ("⏎", "activate"), ("n", "new"), ("o", "notes"), ("esc", "close")],
        Mode::Note => &[("type", "note"), ("⏎", "save"), ("esc", "cancel")],
        Mode::NewRoom => &[("⇥", "field"), ("^s", "create"), ("esc", "cancel")],
        Mode::Loot => &[("↑↓", "move"), ("⏎", "use"), ("a", "add"), ("d", "del"), ("esc", "close")],
        Mode::AddLoot => &[("⇥", "field"), ("^s", "save"), ("esc", "cancel")],
        Mode::Flag => &[("⇥", "user/root"), ("type", "value"), ("⏎", "save+copy"), ("esc", "cancel")],
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
            spans.push(Span::styled(" · ", Style::default().fg(BORDER)));
        }
        spans.push(Span::styled((*k).to_string(), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!(" {lbl}"), Style::default().fg(DIM)));
    }
    spans.push(Span::raw(" "));

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Length(80)])
        .split(area);
    f.render_widget(
        Paragraph::new(Span::styled(app.status.clone(), Style::default().fg(FG)))
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
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " variables ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL));
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
            (format!("{}▏", app.var_buf), Style::default().fg(FG).add_modifier(Modifier::BOLD))
        } else if val.is_empty() {
            ("‹unset›".into(), Style::default().fg(WARN))
        } else {
            (val.clone(), Style::default().fg(OK))
        };
        let (bullet, bstyle) = if val.is_empty() {
            ("○", Style::default().fg(WARN))
        } else {
            ("●", Style::default().fg(OK))
        };
        let mark = if sel {
            Span::styled("▎", Style::default().fg(crate::anim::pulse(app.tick)))
        } else {
            Span::styled(format!("{bullet}"), bstyle)
        };
        ListItem::new(Line::from(vec![
            Span::raw(" "),
            mark,
            Span::styled(format!(" {k:<11}"), Style::default().fg(Color::Rgb(0xb0, 0xbc, 0xc9))),
            Span::styled("= ", Style::default().fg(DIM)),
            Span::styled(disp, vstyle),
        ]))
    };
    let header = |t: &str| {
        ListItem::new(Line::from(Span::styled(
            format!(" ── {t} ──"),
            Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD),
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
            Span::styled(" ▎ ", Style::default().fg(ACCENT2)),
            Span::styled("new var: ", Style::default().fg(ACCENT2)),
            Span::styled(format!("{nk}▏"), Style::default().fg(FG).add_modifier(Modifier::BOLD)),
        ])));
    }

    f.render_widget(List::new(items), inner);
}

fn draw_flag(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 60, 6);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(OK))
        .title(Span::styled(" capture flag ", Style::default().fg(OK).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)])
        .split(inner);

    // which selector
    let pill = |label: &str, on: bool| {
        if on {
            Span::styled(format!(" {label} "), Style::default().fg(BG).bg(OK).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(format!(" {label} "), Style::default().fg(DIM))
        }
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("flag: ", Style::default().fg(DIM)),
            pill("user", !app.flag_is_root),
            Span::raw(" "),
            pill("root", app.flag_is_root),
            Span::styled("   (⇥ toggle)", Style::default().fg(DIM)),
        ])),
        rows[0],
    );
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("🚩 ", Style::default().fg(OK)),
            Span::styled(app.flag_buf.clone(), Style::default().fg(FG).add_modifier(Modifier::BOLD)),
            Span::styled("▏", Style::default().fg(ACCENT)),
        ])),
        rows[1],
    );
    f.render_widget(
        Paragraph::new(Span::styled(
            "⏎ writes to room notes Flags table + log, copies to clipboard",
            Style::default().fg(DIM),
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
        .border_style(Style::default().fg(ACCENT2))
        .title(Span::styled(" loot ", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(
            format!(" {} creds ", app.loot.len()),
            Style::default().fg(DIM),
        )).right_aligned())
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.loot.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("no creds captured yet.", Style::default().fg(DIM))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("press ", Style::default().fg(DIM)),
                    Span::styled("a", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                    Span::styled(" to add user:pass / hash as you find them.", Style::default().fg(DIM)),
                    Span::styled("  ⏎ loads them into USER/PASS/HASH.", Style::default().fg(DIM)),
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
                        .fg(if sel { FG } else { Color::Rgb(0xb0, 0xbc, 0xc9) })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ];
            if !c.note.is_empty() {
                spans.push(Span::styled(format!("   {}", c.note), Style::default().fg(DIM)));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.loot_idx));
    f.render_stateful_widget(
        List::new(rows).highlight_style(Style::default().bg(SEL_BG)),
        inner,
        &mut state,
    );
}

fn draw_addloot(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 64, 15);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT2))
        .title(Span::styled(" add loot ", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(PANEL));
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
        .border_style(Style::default().fg(ACCENT2))
        .title(Span::styled(" ctf rooms ", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)))
        .title(Line::from(Span::styled(
            format!(" {} ", crate::room::ctf_root().display()),
            Style::default().fg(DIM),
        )).right_aligned())
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.rooms.is_empty() {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("no rooms yet.", Style::default().fg(DIM))),
                Line::from(""),
                Line::from(vec![
                    Span::styled("press ", Style::default().fg(DIM)),
                    Span::styled("n", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                    Span::styled(" to scaffold one under ", Style::default().fg(DIM)),
                    Span::styled(crate::room::ctf_root().display().to_string(), Style::default().fg(FG)),
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
                Span::styled("◆ ", Style::default().fg(OK))
            } else {
                Span::styled("· ", Style::default().fg(DIM))
            };
            ListItem::new(Line::from(vec![
                mark,
                dot,
                Span::styled(
                    r.name.clone(),
                    Style::default()
                        .fg(if sel { FG } else { Color::Rgb(0xb0, 0xbc, 0xc9) })
                        .add_modifier(if sel { Modifier::BOLD } else { Modifier::empty() }),
                ),
            ]))
        })
        .collect();
    let mut state = ListState::default();
    state.select(Some(app.room_idx));
    f.render_stateful_widget(
        List::new(items)
            .block(bare_block(BORDER).title(Span::styled(" rooms ", Style::default().fg(DIM))))
            .highlight_style(Style::default().bg(SEL_BG)),
        cols[0],
        &mut state,
    );

    // notes preview of the highlighted room
    let mut lines: Vec<Line> = Vec::new();
    if let Some(r) = app.rooms.get(app.room_idx) {
        if let Some((ip, host)) = r.read_target() {
            lines.push(Line::from(vec![
                Span::styled("target  ", Style::default().fg(DIM)),
                Span::styled(ip, Style::default().fg(OK).add_modifier(Modifier::BOLD)),
                Span::styled(
                    host.map(|h| format!("  {h}")).unwrap_or_default(),
                    Style::default().fg(ACCENT2),
                ),
            ]));
        }
        lines.push(Line::from(Span::styled("── notes.md ──", Style::default().fg(BORDER))));
        for l in r.notes_preview(16) {
            lines.push(Line::from(Span::styled(l, Style::default().fg(Color::Rgb(0x8b, 0x98, 0xa5)))));
        }
    }
    f.render_widget(
        Paragraph::new(lines)
            .block(bare_block(BORDER).title(Span::styled(" preview ", Style::default().fg(DIM))))
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
        .border_style(Style::default().fg(ACCENT2))
        .title(Span::styled(" new room ", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(PANEL));
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
            format!("scaffolds {}/<name> with nmap/ loot/ exploits/ + notes.md", crate::room::ctf_root().display()),
            Style::default().fg(ACCENT),
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
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(" writeup note ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("✎ ", Style::default().fg(ACCENT2)),
            Span::styled(app.note_buf.clone(), Style::default().fg(FG)),
            Span::styled("▏", Style::default().fg(ACCENT)),
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
        .border_style(Style::default().fg(ACCENT2))
        .title(Span::styled(
            " add command ",
            Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL));
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
        Paragraph::new(Span::styled(hint, Style::default().fg(ACCENT)))
            .wrap(Wrap { trim: true }),
        rows[4],
    );
}

fn draw_field(f: &mut Frame, area: Rect, label: &str, val: &str, active: bool) {
    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if active { ACCENT } else { BORDER }))
        .title(Span::styled(
            format!(" {label} "),
            Style::default().fg(if active { ACCENT } else { DIM }),
        ));
    let text = Line::from(vec![
        Span::styled(val.to_string(), Style::default().fg(FG)),
        Span::styled(if active { "▏" } else { "" }, Style::default().fg(ACCENT)),
    ]);
    f.render_widget(Paragraph::new(text).block(b).wrap(Wrap { trim: false }), area);
}

fn draw_splash(f: &mut Frame, app: &App, area: Rect) {
    let frame = app.splash_frame();
    // dense, louder starfield + a shooting star
    crate::anim::draw_field_splash(f, area, app.tick);
    crate::anim::draw_shooting_star(f, area, app.tick, 44, true);

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
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            // leading edge glows brighter while still revealing
            let edge: String = chars[shown.saturating_sub(1)..shown].iter().collect();
            let edge_color = if reveal_cols < w as usize { STAR_HI } else { ACCENT };
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
            Paragraph::new(Span::styled(rule, Style::default().fg(BORDER))),
            rule_rect,
        );
        // subtitle
        let sub = "c t f   t o o l k i t";
        let sw = sub.chars().count() as u16;
        let sx = area.x + (area.width.saturating_sub(sw)) / 2;
        f.render_widget(
            Paragraph::new(Span::styled(
                sub,
                Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD),
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
                Paragraph::new(Span::styled(msg, Style::default().fg(DIM))),
                Rect::new(mx, my, mw, 1),
            );
        }
    }
}

fn draw_help(f: &mut Frame) {
    let area = centered(f.area(), 62, 28);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(ACCENT))
        .title(Span::styled(
            " help ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let row = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("  {k:<12}"), Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::styled(d.to_string(), Style::default().fg(FG)),
        ])
    };
    let lines = vec![
        Line::from(Span::styled(" navigation", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD))),
        row("tab / ←→", "switch pane (categories ↔ commands)"),
        row("↑↓ / jk", "move within a pane"),
        Line::from(""),
        Line::from(Span::styled(" actions", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD))),
        row("enter", "fill from vars + copy (and log it)"),
        row("x", "run it live in the run pane (↓ tabs)"),
        row("e", "edit variables (split: this cmd / all)"),
        row("f", "pin ★ favorite (shows ★/⏱ categories)"),
        row("a / d", "add / delete a command"),
        row("/", "fuzzy search across all commands"),
        row("q / esc", "quit"),
        Line::from(""),
        Line::from(Span::styled(" run pane (⇥ into it)", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD))),
        row("i / ⏎", "interact (type into it) · F12 detach"),
        row("↑↓", "scroll · ←→ switch tab · g tail"),
        row("K / d", "kill job / close tab"),
        Line::from(""),
        Line::from(Span::styled(" ctf rooms + writeup", Style::default().fg(ACCENT2).add_modifier(Modifier::BOLD))),
        row("c", "rooms: pick/activate, n=new, o=notes"),
        row("L", "loot: capture creds → ⏎ load USER/PASS/HASH"),
        row("w", "add a writeup note to the log"),
        row("F / E", "capture flag → notes · export writeup.md"),
        row("o", "open active room's notes in $EDITOR"),
        Line::from(""),
        Line::from(Span::styled(
            "  copies auto-log to the room (or ~/.config/pivt/writeups).",
            Style::default().fg(DIM),
        )),
        Line::from(Span::styled(
            "  RHOST syncs with your `target` shell command.",
            Style::default().fg(DIM),
        )),
    ];
    f.render_widget(Paragraph::new(lines), inner);
}
