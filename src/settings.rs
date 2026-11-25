//! The in-app settings screen (`,`): every option in one overlay.
//!
//! Sections down the left, the selected section's options on the right. The
//! model here is pure UI: it walks [`crate::prefs::OPTIONS`] (the `[layout]`
//! / `[look]` registry), the scalar config settings ([`Scalar`]), the theme
//! list and [`crate::keys::Action::ALL`], and changes go through the App's
//! setters (`set_pref`, `setting_set`, `setting_reset`, `key_bind`…), which
//! validate, apply live and save only what the user changed.

use crate::app::{App, Mode};
use crate::keys::{Action, Key};
use crate::prefs::{Kind, Opt, Val, OPTIONS};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// A section in the left column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sec {
    General,
    Look,
    Layout,
    Theme,
    Discord,
    Scope,
    Paths,
    Keys,
}

impl Sec {
    pub const ALL: [Sec; 8] =
        [Sec::General, Sec::Look, Sec::Layout, Sec::Theme, Sec::Discord, Sec::Scope, Sec::Paths, Sec::Keys];

    pub fn title(self) -> &'static str {
        match self {
            Sec::General => "General",
            Sec::Look => "Look & motion",
            Sec::Layout => "Layout",
            Sec::Theme => "Theme",
            Sec::Discord => "Discord & privacy",
            Sec::Scope => "Scope & safety",
            Sec::Paths => "Paths & integration",
            Sec::Keys => "Keybindings",
        }
    }

    /// Short name for `pivt snap settings:<name>` and the filter.
    pub fn slug(self) -> &'static str {
        match self {
            Sec::General => "general",
            Sec::Look => "look",
            Sec::Layout => "layout",
            Sec::Theme => "theme",
            Sec::Discord => "discord",
            Sec::Scope => "scope",
            Sec::Paths => "paths",
            Sec::Keys => "keys",
        }
    }

    pub fn from_slug(s: &str) -> Option<Sec> {
        let s = s.trim().to_ascii_lowercase();
        Sec::ALL.iter().copied().find(|x| x.slug() == s || x.title().to_ascii_lowercase() == s)
    }
}

/// Top-level config.toml settings that aren't in the prefs registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scalar {
    HideMissing,
    DiscordRpc,
    DiscordPrivacy,
    DiscordAppId,
    DiscordLink,
    ScopeMode,
    RoomsDir,
    RoomTemplate,
    SyncTargetFile,
}

impl Scalar {
    pub const ALL: [Scalar; 9] = [
        Scalar::HideMissing,
        Scalar::DiscordRpc,
        Scalar::DiscordPrivacy,
        Scalar::DiscordAppId,
        Scalar::DiscordLink,
        Scalar::ScopeMode,
        Scalar::RoomsDir,
        Scalar::RoomTemplate,
        Scalar::SyncTargetFile,
    ];

    /// The config.toml key.
    pub fn key(self) -> &'static str {
        match self {
            Scalar::HideMissing => "hide_missing",
            Scalar::DiscordRpc => "discord_rpc",
            Scalar::DiscordPrivacy => "discord_privacy",
            Scalar::DiscordAppId => "discord_app_id",
            Scalar::DiscordLink => "discord_link",
            Scalar::ScopeMode => "scope_mode",
            Scalar::RoomsDir => "rooms_dir",
            Scalar::RoomTemplate => "room_template",
            Scalar::SyncTargetFile => "sync_target_file",
        }
    }

    pub fn desc(self) -> &'static str {
        match self {
            Scalar::HideMissing => "hide commands whose tools aren't installed (else marked ✗)",
            Scalar::DiscordRpc => "Discord Rich Presence (needs the Discord desktop app)",
            Scalar::DiscordPrivacy => "what the presence reveals: safe = box name, full = room/IP/ports, minimal = \"pivt\"",
            Scalar::DiscordAppId => "Discord application id; empty = the bundled pivt app",
            Scalar::DiscordLink => "URL for the \"get pivt\" presence button; empty = the pivt repo",
            Scalar::ScopeMode => "running against a host outside the room's scope: warn / confirm / strict",
            Scalar::RoomsDir => "where rooms live; empty = $CTF_ROOT, ~/TryHackMe or the XDG data dir",
            Scalar::RoomTemplate => "file copied into new rooms; empty = none, reset = legacy cheatsheet if present",
            Scalar::SyncTargetFile => "RHOST mirror for shell functions; empty = off, reset = ~/.cache/ctf-target if it exists",
        }
    }

    pub fn sec(self) -> Sec {
        match self {
            Scalar::HideMissing => Sec::General,
            Scalar::DiscordRpc | Scalar::DiscordPrivacy | Scalar::DiscordAppId | Scalar::DiscordLink => Sec::Discord,
            Scalar::ScopeMode => Sec::Scope,
            Scalar::RoomsDir | Scalar::RoomTemplate | Scalar::SyncTargetFile => Sec::Paths,
        }
    }

    /// How it's edited.
    pub fn kind(self) -> EditKind {
        match self {
            Scalar::HideMissing | Scalar::DiscordRpc => EditKind::Bool,
            Scalar::DiscordPrivacy => EditKind::Choice(&["safe", "full", "minimal"]),
            Scalar::ScopeMode => EditKind::Choice(&["warn", "confirm", "strict"]),
            _ => EditKind::Text,
        }
    }
}

/// One row on the right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// A `[layout]` / `[look]` option by key.
    Pref(&'static str),
    Scalar(Scalar),
    /// A theme, by index into [`crate::theme::all`].
    Theme(usize),
    Key(Action),
}

/// How a row is edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    Bool,
    Int { min: i64, max: i64 },
    Choice(&'static [&'static str]),
    /// Glyph set: ←/→ cycle the presets, ⏎ types your own.
    Glyphs,
    Text,
    Theme,
    Key,
}

/// The section a prefs option is shown in (the splash options are startup
/// behaviour, so they sit in General).
fn pref_sec(o: &Opt) -> Sec {
    match o.key {
        "splash" | "splash_hold" | "splash_seconds" => Sec::General,
        _ => match o.section {
            crate::prefs::Section::Layout => Sec::Layout,
            crate::prefs::Section::Look => Sec::Look,
        },
    }
}

/// The rows of a section, in display order.
pub fn items(sec: Sec) -> Vec<Item> {
    let prefs = |s: Sec| OPTIONS.iter().filter(move |o| pref_sec(o) == s).map(|o| Item::Pref(o.key));
    let scalars = |s: Sec| Scalar::ALL.into_iter().filter(move |x| x.sec() == s).map(Item::Scalar);
    match sec {
        Sec::Theme => (0..crate::theme::all().len()).map(Item::Theme).collect(),
        Sec::Keys => Action::ALL.iter().copied().map(Item::Key).collect(),
        s => scalars(s).chain(prefs(s)).collect(),
    }
}

/// Every row of every section.
pub fn all_items() -> Vec<Item> {
    Sec::ALL.iter().flat_map(|s| items(*s)).collect()
}

impl Item {
    pub fn sec(self) -> Sec {
        match self {
            Item::Pref(k) => Opt::find(k).map(pref_sec).unwrap_or(Sec::General),
            Item::Scalar(s) => s.sec(),
            Item::Theme(_) => Sec::Theme,
            Item::Key(_) => Sec::Keys,
        }
    }

    pub fn name(self) -> String {
        match self {
            Item::Pref(k) => k.to_string(),
            Item::Scalar(s) => s.key().to_string(),
            Item::Theme(i) => crate::theme::all().get(i).map(|t| t.name.to_string()).unwrap_or_default(),
            Item::Key(a) => a.name().to_string(),
        }
    }

    pub fn desc(self) -> String {
        match self {
            Item::Pref(k) => Opt::find(k).map(|o| o.desc.to_string()).unwrap_or_default(),
            Item::Scalar(s) => s.desc().to_string(),
            Item::Theme(i) => {
                let custom = crate::theme::all()
                    .get(i)
                    .is_some_and(|t| !crate::theme::PRESETS.iter().any(|p| std::ptr::eq(p, *t)));
                if custom { "colour theme (custom file)" } else { "colour theme (preset)" }.to_string()
            }
            Item::Key(a) => {
                let ctx = match a.ctx() {
                    crate::keys::Ctx::Normal => "",
                    crate::keys::Ctx::Run => " (run pane)",
                    crate::keys::Ctx::Interact => " (while interacting)",
                };
                format!("{}{ctx}", a.describe())
            }
        }
    }

    pub fn kind(self) -> EditKind {
        match self {
            Item::Pref(k) => match Opt::find(k).map(|o| &o.kind) {
                Some(Kind::Bool) => EditKind::Bool,
                Some(Kind::Int { min, max, .. }) => EditKind::Int { min: *min, max: *max },
                Some(Kind::Choice(c)) => EditKind::Choice(c),
                Some(Kind::Glyphs { .. }) => EditKind::Glyphs,
                None => EditKind::Text,
            },
            Item::Scalar(s) => s.kind(),
            Item::Theme(_) => EditKind::Theme,
            Item::Key(_) => EditKind::Key,
        }
    }

    /// Range / choices, for the detail line.
    pub fn range(self) -> String {
        match self {
            Item::Pref(k) => match Opt::find(k).map(|o| &o.kind) {
                Some(Kind::Bool) => "on / off".into(),
                Some(Kind::Int { min, max, unit }) => {
                    format!("{min}..{max}{}", if unit.is_empty() { String::new() } else { format!(" {unit}") })
                }
                Some(Kind::Choice(c)) => c.join(" / "),
                Some(Kind::Glyphs { presets, min, max, .. }) => format!(
                    "{} · or {} chars",
                    presets.iter().map(|p| p.0).collect::<Vec<_>>().join(" / "),
                    if min == max { min.to_string() } else { format!("{min}..{max}") }
                ),
                None => String::new(),
            },
            Item::Scalar(s) => match s.kind() {
                EditKind::Bool => "on / off".into(),
                EditKind::Choice(c) => c.join(" / "),
                _ => match s {
                    Scalar::DiscordAppId => "digits (Discord developer portal) or empty".into(),
                    Scalar::DiscordLink => "http(s):// URL or empty".into(),
                    Scalar::RoomTemplate => "path to an existing file, or empty".into(),
                    Scalar::SyncTargetFile => "file path (its folder must exist), or empty".into(),
                    _ => "directory path (~ ok), or empty".into(),
                },
            },
            Item::Theme(_) => "⏎ keep · moving previews it live".into(),
            Item::Key(_) => "⏎ add a key · c replace · ⌫ remove last · r default".into(),
        }
    }

    /// Takes effect only on the next start.
    pub fn restart_note(self) -> Option<&'static str> {
        match self {
            Item::Pref("splash" | "splash_hold" | "splash_seconds") => Some("applies at next start"),
            Item::Scalar(Scalar::RoomsDir) if std::env::var_os("CTF_ROOT").is_some_and(|v| !v.is_empty()) => {
                Some("$CTF_ROOT is set and wins over this")
            }
            _ => None,
        }
    }

    /// Does `q` (lowercase) match the name, description or section?
    fn matches(self, q: &str, app: &App) -> bool {
        let hay = |s: &str| s.to_lowercase().contains(q);
        hay(&self.name())
            || hay(&self.name().replace('_', " "))
            || hay(&self.desc())
            || hay(self.sec().title())
            || matches!(self, Item::Key(a) if hay(&app.keys.label(a)))
    }
}

/// The settings overlay's state.
#[derive(Clone, Debug, Default)]
pub struct SettingsUi {
    /// Selected section (index into [`Sec::ALL`]).
    pub sec: usize,
    /// Focus on the options column (else the sections).
    pub right: bool,
    /// Selected row in the options column.
    pub row: usize,
    pub filter: String,
    /// Typing into the filter.
    pub filtering: bool,
    /// Text edit of the selected row: buffer, and the error for it (red).
    pub edit: Option<String>,
    pub edit_err: Option<String>,
    /// Waiting for a key for this action; `true` = replace its keys.
    pub capture: Option<(Action, bool)>,
    /// The theme in use (saved or `--theme`), restored after a preview.
    pub theme_live: Option<&'static crate::theme::Theme>,
}

impl SettingsUi {
    pub fn section(&self) -> Sec {
        Sec::ALL[self.sec.min(Sec::ALL.len() - 1)]
    }
}

/// Lowercased, trimmed filter.
fn query(app: &App) -> String {
    app.settings.filter.trim().to_lowercase()
}

impl App {
    /// The rows on the right: the section's, or every match of the filter.
    pub fn settings_rows(&self) -> Vec<Item> {
        let q = query(self);
        if q.is_empty() {
            items(self.settings.section())
        } else {
            all_items().into_iter().filter(|i| i.matches(&q, self)).collect()
        }
    }

    pub fn settings_selected(&self) -> Option<Item> {
        if !self.settings.right {
            return None;
        }
        self.settings_rows().get(self.settings.row).copied()
    }

    /// Number of user-set rows in a section (for the left column).
    pub fn settings_custom_count(&self, sec: Sec) -> usize {
        match sec {
            Sec::Theme => usize::from(self.setting_is_set(Item::Theme(0))),
            s => items(s).into_iter().filter(|i| self.setting_is_set(*i)).count(),
        }
    }

    /// The default, as shown in the detail line.
    pub fn setting_default_display(&self, it: Item) -> String {
        match it {
            Item::Pref(k) => Opt::find(k).map(|o| o.default_val().to_string()).unwrap_or_default(),
            Item::Scalar(s) => match s {
                Scalar::HideMissing | Scalar::DiscordRpc => "off".into(),
                Scalar::DiscordPrivacy => "safe".into(),
                Scalar::ScopeMode => "confirm".into(),
                _ => "unset".into(),
            },
            Item::Theme(_) => crate::theme::PRESETS[0].name.to_string(),
            Item::Key(a) => {
                let k: Vec<String> =
                    a.defaults().iter().filter_map(|s| crate::keys::parse_key(s)).map(|k| k.label()).collect();
                k.join(" / ")
            }
        }
    }

    pub fn open_settings(&mut self) {
        let sec = self.settings.sec;
        self.settings = SettingsUi { sec, theme_live: Some(crate::theme::theme()), ..Default::default() };
        self.mode = Mode::Settings;
    }

    pub fn close_settings(&mut self) {
        if let Some(t) = self.settings.theme_live.take() {
            crate::theme::set(t);
        }
        self.settings.edit = None;
        self.settings.capture = None;
        self.mode = Mode::Normal;
    }

    /// Jump to a section with the options column focused (snapshots).
    pub fn settings_goto(&mut self, sec: Sec) {
        self.settings.sec = Sec::ALL.iter().position(|s| *s == sec).unwrap_or(0);
        self.settings_enter_right();
    }

    fn settings_enter_right(&mut self) {
        self.settings.right = true;
        self.settings.row = 0;
        if self.settings.filter.trim().is_empty() && self.settings.section() == Sec::Theme {
            let live = self.settings.theme_live.unwrap_or_else(crate::theme::theme);
            self.settings.row = crate::theme::all().iter().position(|t| std::ptr::eq(*t, live)).unwrap_or(0);
        }
        self.settings_sync_preview();
    }

    /// Preview the highlighted theme; anything else shows the live one.
    fn settings_sync_preview(&mut self) {
        let live = *self.settings.theme_live.get_or_insert_with(crate::theme::theme);
        match self.settings_selected() {
            Some(Item::Theme(i)) => {
                if let Some(t) = crate::theme::all().get(i) {
                    crate::theme::set(t);
                }
            }
            _ => crate::theme::set(live),
        }
    }

    fn settings_move(&mut self, delta: isize) {
        if self.settings.right {
            let n = self.settings_rows().len();
            if n == 0 {
                return;
            }
            self.settings.row = (self.settings.row as isize + delta).clamp(0, n as isize - 1) as usize;
            self.settings_sync_preview();
        } else {
            let n = Sec::ALL.len() as isize;
            self.settings.sec = (self.settings.sec as isize + delta).rem_euclid(n) as usize;
            self.settings.row = 0;
        }
    }

    fn settings_done(&mut self, it: Item, res: Result<(), String>) {
        match res {
            Ok(()) => {
                let v = self.setting_display(it);
                let note = it.restart_note().map(|n| format!(" ({n})")).unwrap_or_default();
                self.set_status(format!("✓ {} = {v}{note}", it.name()));
            }
            Err(e) => self.set_status(format!("✗ {e}")),
        }
    }

    /// ⏎ / space on a row.
    pub fn settings_activate(&mut self, space: bool) {
        let Some(it) = self.settings_selected() else { return };
        match it.kind() {
            EditKind::Bool | EditKind::Choice(_) => {
                self.settings_step(1);
            }
            EditKind::Int { .. } | EditKind::Glyphs | EditKind::Text if !space => {
                let cur = match self.setting_value(it) {
                    Val::Str(s) => s,
                    v => v.to_string(),
                };
                self.settings.edit_err = None;
                self.settings.edit = Some(cur);
            }
            EditKind::Theme => {
                let res = self.setting_set(it, Val::Bool(true));
                self.settings.theme_live = Some(crate::theme::theme());
                self.settings_done(it, res);
            }
            EditKind::Key if !space => self.settings.capture = Some(match it {
                Item::Key(a) => (a, false),
                _ => return,
            }),
            _ => {}
        }
    }

    /// ←/→ on a row: toggle, step, cycle. False when the row doesn't step
    /// (so ← can go back to the sections).
    pub fn settings_step(&mut self, dir: i64) -> bool {
        let Some(it) = self.settings_selected() else { return false };
        let next = match (it, it.kind()) {
            (Item::Pref(k), EditKind::Bool | EditKind::Int { .. } | EditKind::Choice(_) | EditKind::Glyphs) => {
                let Some(o) = Opt::find(k) else { return false };
                o.step(&self.setting_value(it), dir)
            }
            (_, EditKind::Bool) => match self.setting_value(it) {
                Val::Bool(b) => Val::Bool(!b),
                _ => return false,
            },
            (_, EditKind::Choice(names)) => {
                let cur = self.setting_value(it).to_string();
                let n = names.len() as i64;
                let i = names.iter().position(|x| *x == cur).unwrap_or(0) as i64;
                Val::Str(names[(i + dir.signum()).rem_euclid(n) as usize].to_string())
            }
            _ => return false,
        };
        let res = self.setting_set(it, next);
        self.settings_done(it, res);
        true
    }

    /// Validate the text being typed (red while invalid).
    fn settings_check_edit(&mut self) {
        let (Some(it), Some(buf)) = (self.settings_selected(), self.settings.edit.clone()) else { return };
        self.settings.edit_err = self.setting_parse_text(it, &buf).err();
    }

    /// ⏎ in a text edit: save when valid, else keep editing (shown red).
    pub fn settings_commit_edit(&mut self) {
        let (Some(it), Some(buf)) = (self.settings_selected(), self.settings.edit.clone()) else {
            self.settings.edit = None;
            return;
        };
        match self.setting_parse_text(it, &buf).and_then(|v| self.setting_set(it, v)) {
            Ok(()) => {
                self.settings.edit = None;
                self.settings.edit_err = None;
                self.settings_done(it, Ok(()));
            }
            Err(e) => {
                self.set_status(format!("✗ not saved: {e}"));
                self.settings.edit_err = Some(e);
            }
        }
    }

    /// `r`: back to the default (the key is removed from config.toml).
    pub fn settings_reset(&mut self) {
        let Some(it) = self.settings_selected() else { return };
        self.setting_reset(it);
        if it.kind() == EditKind::Theme {
            self.settings.theme_live = Some(crate::theme::theme());
            self.settings_sync_preview();
        }
        let v = self.setting_display(it);
        let what = if it.kind() == EditKind::Theme { "theme".to_string() } else { it.name() };
        self.set_status(format!("↺ {what} back to the default ({v}) · removed from config.toml"));
    }

    /// A key pressed while capturing for `a`.
    pub fn settings_capture_key(&mut self, ev: KeyEvent) {
        let Some((a, replace)) = self.settings.capture else { return };
        match ev.code {
            KeyCode::Esc => {
                self.settings.capture = None;
                self.set_status("key capture cancelled");
                return;
            }
            KeyCode::Backspace => {
                self.settings.capture = None;
                let msg = self.key_unbind_last(a);
                self.set_status(msg.unwrap_or_else(|e| format!("✗ {e}")));
                return;
            }
            // lone modifier presses (kitty protocol) aren't keys
            KeyCode::Modifier(_) => return,
            _ => {}
        }
        let k = Key::from_event(&ev);
        if crate::keys::is_reserved(&k) {
            // stay in capture: the user can pick another key
            self.set_status("✗ ctrl-c is reserved (quit / sent to the program) — press another key, esc cancels");
            return;
        }
        self.settings.capture = None;
        let msg = self.key_bind(a, k, replace);
        self.set_status(msg.unwrap_or_else(|e| format!("✗ {e}")));
    }
}

/// Key handling for [`Mode::Settings`].
pub fn handle_key(app: &mut App, key: KeyEvent) {
    if app.settings.capture.is_some() {
        app.settings_capture_key(key);
        return;
    }
    if app.settings.edit.is_some() {
        match key.code {
            KeyCode::Enter => app.settings_commit_edit(),
            KeyCode::Esc => {
                app.settings.edit = None;
                app.settings.edit_err = None;
            }
            KeyCode::Backspace => {
                if let Some(b) = app.settings.edit.as_mut() {
                    b.pop();
                }
                app.settings_check_edit();
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.settings.edit = Some(String::new());
                app.settings_check_edit();
            }
            KeyCode::Char(c) => {
                if let Some(b) = app.settings.edit.as_mut() {
                    b.push(c);
                }
                app.settings_check_edit();
            }
            _ => {}
        }
        return;
    }
    if app.settings.filtering {
        match key.code {
            KeyCode::Esc => {
                app.settings.filter.clear();
                app.settings.filtering = false;
                app.settings.row = 0;
                app.settings_sync_preview();
            }
            KeyCode::Enter | KeyCode::Down | KeyCode::Tab => {
                app.settings.filtering = false;
                if !app.settings.filter.trim().is_empty() {
                    app.settings.right = true;
                }
                app.settings_sync_preview();
            }
            KeyCode::Backspace => {
                app.settings.filter.pop();
                app.settings.row = 0;
            }
            KeyCode::Char(c) => {
                app.settings.filter.push(c);
                app.settings.row = 0;
                app.settings.right = true;
            }
            _ => {}
        }
        return;
    }
    let right = app.settings.right;
    let kind = app.settings_selected().map(Item::kind);
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            if !app.settings.filter.is_empty() {
                app.settings.filter.clear();
                app.settings.row = 0;
                app.settings_sync_preview();
            } else if right && key.code == KeyCode::Esc {
                app.settings.right = false;
                app.settings_sync_preview();
            } else {
                app.close_settings();
            }
        }
        KeyCode::Char('/') => {
            app.settings.filtering = true;
        }
        KeyCode::Down | KeyCode::Char('j') => app.settings_move(1),
        KeyCode::Up | KeyCode::Char('k') => app.settings_move(-1),
        KeyCode::PageDown => app.settings_move(10),
        KeyCode::PageUp => app.settings_move(-10),
        KeyCode::Home | KeyCode::Char('g') => app.settings_move(-1000),
        KeyCode::End | KeyCode::Char('G') => app.settings_move(1000),
        KeyCode::Tab | KeyCode::BackTab => {
            if right {
                app.settings.right = false;
                app.settings_sync_preview();
            } else {
                app.settings_enter_right();
            }
        }
        _ if !right => match key.code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => app.settings_enter_right(),
            // the settings key closes it again (after the overlay's own keys)
            _ if app.keys.is(Action::Settings, &key) => app.close_settings(),
            _ => {}
        },
        KeyCode::Enter => app.settings_activate(false),
        KeyCode::Char(' ') => app.settings_activate(true),
        KeyCode::Right | KeyCode::Char('l') => match kind {
            Some(EditKind::Theme) => app.settings_move(1),
            // → = on (already on: nothing)
            Some(EditKind::Bool) => {
                if app.settings_selected().map(|i| app.setting_value(i)) == Some(Val::Bool(false)) {
                    app.settings_step(1);
                }
            }
            _ => {
                app.settings_step(1);
            }
        },
        KeyCode::Left | KeyCode::Char('h') => {
            let stepped = match kind {
                Some(EditKind::Theme) => {
                    app.settings_move(-1);
                    true
                }
                // ← = off; on an off bool (or a text / key row) it goes back
                Some(EditKind::Bool) => {
                    app.settings_selected().map(|i| app.setting_value(i)) == Some(Val::Bool(true))
                        && app.settings_step(-1)
                }
                Some(EditKind::Text) | Some(EditKind::Key) | None => false,
                _ => app.settings_step(-1),
            };
            if !stepped && app.settings.filter.trim().is_empty() {
                app.settings.right = false;
                app.settings_sync_preview();
            }
        }
        KeyCode::Char('r') => app.settings_reset(),
        KeyCode::Char('c') if kind == Some(EditKind::Key) => {
            if let Some(Item::Key(a)) = app.settings_selected() {
                app.settings.capture = Some((a, true));
            }
        }
        KeyCode::Backspace | KeyCode::Delete if kind == Some(EditKind::Key) => {
            if let Some(Item::Key(a)) = app.settings_selected() {
                let msg = app.key_unbind_last(a);
                app.set_status(msg.unwrap_or_else(|e| format!("✗ {e}")));
            }
        }
        _ if app.keys.is(Action::Settings, &key) => app.close_settings(),
        _ => {}
    }
}

/// Bracketed paste into the filter or a text edit.
pub fn handle_paste(app: &mut App, line: &str) {
    if let Some(b) = app.settings.edit.as_mut() {
        b.push_str(line);
        app.settings_check_edit();
    } else if app.settings.filtering {
        app.settings.filter.push_str(line);
        app.settings.row = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crossterm::event::KeyModifiers as M;
    use ratatui::{backend::TestBackend, Terminal};

    fn press(app: &mut App, code: KeyCode) {
        crate::handle_key(app, KeyEvent::new(code, M::NONE));
    }
    fn press_mod(app: &mut App, code: KeyCode, m: M) {
        crate::handle_key(app, KeyEvent::new(code, m));
    }
    fn app() -> App {
        let mut a = App::for_tests();
        a.mode = Mode::Normal;
        a
    }
    /// Open the screen and put the cursor on `it`.
    fn select(app: &mut App, it: Item) {
        if app.mode != Mode::Settings {
            app.open_settings();
        }
        app.settings_goto(it.sec());
        app.settings.row = app.settings_rows().iter().position(|x| *x == it).expect("row in its section");
        assert_eq!(app.settings_selected(), Some(it));
    }
    fn render(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| crate::ui::draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn opens_with_comma_and_every_option_is_reachable() {
        let mut a = app();
        press(&mut a, KeyCode::Char(','));
        assert!(a.mode == Mode::Settings);
        let all = all_items();
        for o in OPTIONS {
            assert!(all.contains(&Item::Pref(o.key)), "{} missing", o.key);
        }
        for s in Scalar::ALL {
            assert!(all.contains(&Item::Scalar(s)), "{s:?} missing");
        }
        for &k in Action::ALL {
            assert!(all.contains(&Item::Key(k)), "{} missing", k.name());
        }
        for i in 0..crate::theme::all().len() {
            assert!(all.contains(&Item::Theme(i)));
        }
        // each row is in exactly one section, and the screen can select it
        let mut seen = std::collections::HashSet::new();
        for it in &all {
            assert!(seen.insert(*it), "{it:?} listed twice");
            select(&mut a, *it);
            assert!(!it.desc().is_empty() && !it.range().is_empty(), "{it:?}");
        }
        // walking the left column with ↓ visits every section
        a.settings.right = false;
        a.settings.sec = 0;
        let mut secs = vec![a.settings.section()];
        for _ in 1..Sec::ALL.len() {
            press(&mut a, KeyCode::Down);
            secs.push(a.settings.section());
        }
        assert_eq!(secs, Sec::ALL);
        press(&mut a, KeyCode::Esc);
        assert!(a.mode == Mode::Normal);
    }

    #[test]
    fn bool_int_choice_edit_updates_prefs_and_raw_table() {
        let mut a = app();
        // bool: ⏎ toggles, space toggles back
        select(&mut a, Item::Pref("compact"));
        press(&mut a, KeyCode::Enter);
        assert!(a.prefs.compact);
        assert_eq!(a.saved_for_tests().0["compact"].as_bool(), Some(true));
        press(&mut a, KeyCode::Char(' '));
        assert!(!a.prefs.compact);
        assert_eq!(a.saved_for_tests().0["compact"].as_bool(), Some(false), "a user-set value stays set");
        // int: → steps, ⏎ types an exact value
        select(&mut a, Item::Pref("categories_width"));
        press(&mut a, KeyCode::Right);
        assert_eq!(a.prefs.categories_width, 21);
        press(&mut a, KeyCode::Left);
        press(&mut a, KeyCode::Left);
        assert_eq!(a.prefs.categories_width, 19);
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.settings.edit.as_deref(), Some("19"));
        press_mod(&mut a, KeyCode::Char('u'), M::CONTROL);
        for c in "33".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        press(&mut a, KeyCode::Enter);
        assert!(a.settings.edit.is_none());
        assert_eq!(a.prefs.categories_width, 33);
        assert_eq!(a.saved_for_tests().0["categories_width"].as_integer(), Some(33));
        // choice: ←/→ cycle; live (borders feed the renderer straight away)
        select(&mut a, Item::Pref("border"));
        press(&mut a, KeyCode::Right);
        assert_eq!(a.prefs.border, crate::prefs::Border::Plain);
        assert_eq!(a.saved_for_tests().1["border"].as_str(), Some("plain"));
        select(&mut a, Item::Pref("background"));
        press(&mut a, KeyCode::Right);
        assert_eq!(a.prefs.background, crate::prefs::Background::Flow);
        // only what was touched is in the tables
        assert_eq!(a.saved_for_tests().0.len(), 2, "{:?}", a.saved_for_tests().0);
        assert_eq!(a.saved_for_tests().1.len(), 2, "{:?}", a.saved_for_tests().1);
        // scalar settings
        select(&mut a, Item::Scalar(Scalar::ScopeMode));
        assert!(!a.setting_is_set(Item::Scalar(Scalar::ScopeMode)));
        press(&mut a, KeyCode::Right);
        assert_eq!(a.scope_mode, crate::scope::Mode::Strict);
        assert_eq!(a.saved_for_tests().4, "strict");
        select(&mut a, Item::Scalar(Scalar::HideMissing));
        press(&mut a, KeyCode::Enter);
        assert!(a.hide_missing);
        select(&mut a, Item::Scalar(Scalar::DiscordPrivacy));
        press(&mut a, KeyCode::Right);
        assert_eq!(a.discord_privacy, crate::discord::Privacy::Full);
        // and the screen still renders after every change
        render(&a, 80, 24);
    }

    #[test]
    fn reset_removes_the_key() {
        let mut a = app();
        select(&mut a, Item::Pref("show_preview"));
        press(&mut a, KeyCode::Enter);
        assert!(!a.prefs.show_preview);
        assert!(a.saved_for_tests().0.contains_key("show_preview"));
        press(&mut a, KeyCode::Char('r'));
        assert!(a.prefs.show_preview, "default again");
        assert!(!a.saved_for_tests().0.contains_key("show_preview"), "key removed, not written as true");
        assert!(a.status.contains("removed from config.toml"), "{}", a.status);
        // a reset of a key that holds its default value still removes it
        a.set_pref("bg_density", Val::Int(5)).unwrap();
        select(&mut a, Item::Pref("bg_density"));
        press(&mut a, KeyCode::Char('r'));
        assert!(a.saved_for_tests().1.is_empty());
        // scalars: back to "not set"
        select(&mut a, Item::Scalar(Scalar::ScopeMode));
        press(&mut a, KeyCode::Right);
        press(&mut a, KeyCode::Char('r'));
        assert_eq!(a.saved_for_tests().4, "");
        assert_eq!(a.scope_mode, crate::scope::Mode::Confirm);
        // theme: ⏎ keeps one, r drops `theme`
        select(&mut a, Item::Theme(1));
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.saved_for_tests().3, crate::theme::all()[1].name);
        press(&mut a, KeyCode::Char('r'));
        assert_eq!(a.saved_for_tests().3, "");
        // keys: r drops the [keys] entry
        a.key_bind(Action::Note, crate::keys::parse_key("Z").unwrap(), true).unwrap();
        assert!(a.saved_for_tests().2.contains_key("note"));
        select(&mut a, Item::Key(Action::Note));
        press(&mut a, KeyCode::Char('r'));
        assert!(a.saved_for_tests().2.is_empty());
        assert_eq!(a.keys.label(Action::Note), "w");
        // nothing of this went into a serialised default config
        let cfg = crate::model::Config {
            layout: a.saved_for_tests().0.clone(),
            look: a.saved_for_tests().1.clone(),
            keys: a.saved_for_tests().2.clone(),
            ..Default::default()
        };
        let txt = toml::to_string_pretty(&cfg).unwrap();
        for k in ["[layout]", "[look]", "[keys]", "theme", "scope_mode", "discord_rpc", "discord_privacy"] {
            assert!(!txt.contains(k), "{k} in\n{txt}");
        }
    }

    #[test]
    fn invalid_text_is_rejected_and_not_saved() {
        let mut a = app();
        select(&mut a, Item::Pref("marker"));
        press(&mut a, KeyCode::Enter);
        press_mod(&mut a, KeyCode::Char('u'), M::CONTROL);
        press(&mut a, KeyCode::Char('a'));
        assert!(a.settings.edit_err.is_none(), "one char is fine");
        press(&mut a, KeyCode::Char('b'));
        assert!(a.settings.edit_err.is_some(), "shown red while typing");
        press(&mut a, KeyCode::Enter);
        assert!(a.settings.edit.is_some(), "stays in the editor");
        assert_eq!(a.prefs.marker, "bar");
        assert!(a.saved_for_tests().1.is_empty());
        assert!(render(&a, 80, 24).contains("not saved"));
        press(&mut a, KeyCode::Esc);
        assert!(a.settings.edit.is_none());
        // ints: junk and out-of-range numbers are refused (not clamped)
        for bad in ["abc", "999", "-1"] {
            assert!(a.setting_parse_text(Item::Pref("run_height"), bad).is_err(), "{bad}");
        }
        assert!(a.setting_set(Item::Scalar(Scalar::DiscordAppId), Val::Str("nope".into())).is_err());
        assert!(a.setting_set(Item::Scalar(Scalar::DiscordLink), Val::Str("ftp://x".into())).is_err());
        assert!(a.setting_set(Item::Scalar(Scalar::RoomTemplate), Val::Str("/nonexistent/pivt-x.md".into())).is_err());
        assert!(a.setting_set(Item::Scalar(Scalar::SyncTargetFile), Val::Str("/nonexistent/dir/t".into())).is_err());
        assert!(a.setting_set(Item::Scalar(Scalar::RoomsDir), Val::Str("relative/dir".into())).is_err());
        assert!(!a.setting_is_set(Item::Scalar(Scalar::DiscordAppId)));
        // a good one goes through; a glyph string too
        a.setting_set(Item::Scalar(Scalar::DiscordAppId), Val::Str("123456789012345678".into())).unwrap();
        assert_eq!(a.discord_app_id, "123456789012345678");
        a.setting_reset(Item::Scalar(Scalar::DiscordAppId));
        assert_eq!(a.discord_app_id, crate::model::DEFAULT_DISCORD_APP_ID);
        select(&mut a, Item::Pref("flow_charset"));
        press(&mut a, KeyCode::Enter);
        press_mod(&mut a, KeyCode::Char('u'), M::CONTROL);
        crate::handle_paste(&mut a, " .oO");
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.prefs.flow_charset, " .oO");
        assert_eq!(a.fx.ramp, vec![' ', '.', 'o', 'O'], "applied live");
    }

    #[test]
    fn key_capture_adds_conflicts_and_refuses_ctrl_c() {
        let mut a = app();
        select(&mut a, Item::Key(Action::Note));
        // add: ⏎ then the key
        press(&mut a, KeyCode::Enter);
        assert_eq!(a.settings.capture, Some((Action::Note, false)));
        press(&mut a, KeyCode::F(6));
        assert!(a.settings.capture.is_none());
        assert_eq!(a.keys.label(Action::Note), "w / F6");
        assert_eq!(a.saved_for_tests().2["note"], toml::Value::Array(vec!["w".into(), "f6".into()]));
        // ctrl-c is refused (and doesn't quit); capture stays open
        press(&mut a, KeyCode::Enter);
        press_mod(&mut a, KeyCode::Char('c'), M::CONTROL);
        assert!(!a.should_quit && !a.confirm_quit);
        assert!(a.status.contains("reserved"), "{}", a.status);
        assert!(a.settings.capture.is_some());
        assert_eq!(a.keys.label(Action::Note), "w / F6");
        // esc cancels
        press(&mut a, KeyCode::Esc);
        assert!(a.settings.capture.is_none() && a.mode == Mode::Settings);
        // conflict: x belongs to run by default; an explicit binding wins it
        press(&mut a, KeyCode::Enter);
        press(&mut a, KeyCode::Char('x'));
        assert!(a.status.contains("was run's"), "{}", a.status);
        assert!(a.keys.keys(Action::Run).is_empty());
        assert_eq!(a.key_conflicts(Action::Run), vec!["x is taken by note".to_string()]);
        let screen = render(&a, 120, 40);
        assert!(screen.contains("x is taken by note"), "{screen}");
        // two explicit bindings: the earlier action keeps it
        a.key_bind(Action::Quit, crate::keys::parse_key("F7").unwrap(), true).unwrap();
        let msg = a.key_bind(Action::Note, crate::keys::parse_key("F7").unwrap(), false).unwrap();
        assert!(msg.contains("stays with quit"), "{msg}");
        // c replaces; ⌫ removes the last key
        press(&mut a, KeyCode::Char('c'));
        assert_eq!(a.settings.capture, Some((Action::Note, true)));
        press(&mut a, KeyCode::Char('n'));
        assert_eq!(a.keys.label(Action::Note), "n");
        assert_eq!(a.saved_for_tests().2["note"].as_str(), Some("n"));
        assert_eq!(a.keys.label(Action::Run), "x", "run gets x back");
        // replacing with the default removes the [keys] entry again
        press(&mut a, KeyCode::Char('c'));
        press(&mut a, KeyCode::Char('w'));
        assert!(!a.saved_for_tests().2.contains_key("note"));
        // help keeps its last key; others can be emptied
        select(&mut a, Item::Key(Action::Help));
        press(&mut a, KeyCode::Backspace);
        assert_eq!(a.keys.label(Action::Help), "?");
        assert!(a.status.contains("needs a key"), "{}", a.status);
        select(&mut a, Item::Key(Action::Favorite));
        press(&mut a, KeyCode::Backspace);
        assert!(a.keys.keys(Action::Favorite).is_empty());
        assert_eq!(a.saved_for_tests().2["favorite"], toml::Value::Array(vec![]));
        // the settings key itself is remappable and closes the screen
        a.key_bind(Action::Settings, crate::keys::parse_key("F2").unwrap(), true).unwrap();
        a.settings.right = false;
        press(&mut a, KeyCode::F(2));
        assert!(a.mode == Mode::Normal);
        press(&mut a, KeyCode::F(2));
        assert!(a.mode == Mode::Settings);
    }

    #[test]
    fn filter_finds_by_name_and_description() {
        let mut a = app();
        a.open_settings();
        press(&mut a, KeyCode::Char('/'));
        for c in "ripple".chars() {
            press(&mut a, KeyCode::Char(c));
        }
        assert_eq!(a.settings_rows(), vec![Item::Pref("ripple")]);
        a.settings.filter = "reachability".into(); // show_ping's description
        assert_eq!(a.settings_rows(), vec![Item::Pref("show_ping")]);
        a.settings.filter = "Zoom".into();
        assert!(a.settings_rows().contains(&Item::Key(Action::Zoom)));
        a.settings.filter = "privacy".into();
        let rows = a.settings_rows();
        assert!(rows.contains(&Item::Scalar(Scalar::DiscordPrivacy)) && rows.contains(&Item::Key(Action::DiscordPrivacy)));
        a.settings.filter = "nord".into();
        assert!(matches!(a.settings_rows()[..], [Item::Theme(_)]));
        a.settings.filter = "zzzz-nothing".into();
        assert!(a.settings_rows().is_empty());
        assert!(render(&a, 80, 24).contains("nothing matches"));
        // ⏎ ends typing, results stay editable; esc clears
        a.settings.filter = "compact".into();
        press(&mut a, KeyCode::Enter);
        assert!(!a.settings.filtering && a.settings.right);
        press(&mut a, KeyCode::Enter);
        assert!(a.prefs.compact, "edited from the filter results");
        press(&mut a, KeyCode::Esc);
        assert!(a.settings.filter.is_empty() && a.mode == Mode::Settings);
    }

    #[test]
    fn theme_rows_preview_and_revert() {
        let mut a = app();
        a.open_settings();
        let live = a.settings.theme_live.unwrap();
        a.settings_goto(Sec::Theme);
        a.settings.row = 0;
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Down);
        press(&mut a, KeyCode::Up);
        assert_eq!(a.settings.row, 1);
        assert_eq!(a.settings_selected(), Some(Item::Theme(1)));
        assert!(std::ptr::eq(a.settings.theme_live.unwrap(), live), "moving only previews");
        assert_eq!(a.saved_for_tests().3, "", "nothing saved yet");
        press(&mut a, KeyCode::Esc); // back to sections
        press(&mut a, KeyCode::Esc); // close
        assert!(a.mode == Mode::Normal);
        assert_eq!(a.saved_for_tests().3, "");
    }

    #[test]
    fn renders_everywhere_without_panic() {
        let mut a = app();
        a.open_settings();
        for &(w, h) in &[(80, 24), (200, 60), (110, 32), (60, 16), (30, 10), (12, 5), (1, 1)] {
            for sec in Sec::ALL {
                a.settings_goto(sec);
                a.settings.row = 3.min(a.settings_rows().len() - 1);
                render(&a, w, h);
                a.settings.right = false;
                render(&a, w, h);
            }
            select(&mut a, Item::Pref("marker"));
            a.settings.edit = Some("a very long glyph string that is way too long".into());
            a.settings.edit_err = Some("too long".into());
            render(&a, w, h);
            a.settings.edit = None;
            select(&mut a, Item::Key(Action::Run));
            a.settings.capture = Some((Action::Run, false));
            render(&a, w, h);
            a.settings.capture = None;
            a.settings.filter = "show".into();
            render(&a, w, h);
            a.settings.filter.clear();
        }
        // the 80x24 frame fits the sections and the hints
        a.settings_goto(Sec::Layout);
        let s = render(&a, 80, 24);
        for needle in ["settings", "Keybindings", "Paths & integration", "categories_width", "r default"] {
            assert!(s.contains(needle), "{needle}:\n{s}");
        }
        let big = render(&a, 200, 60);
        assert!(big.contains("categories pane width") && big.contains("default"), "{big}");
    }

    #[test]
    fn tests_never_write_the_real_config() {
        let path = crate::model::config_file();
        let before = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        let mut a = app();
        assert!(!a.saved_for_tests().5, "persist guard is on");
        for it in all_items() {
            if let Item::Pref(_) | Item::Scalar(Scalar::HideMissing | Scalar::ScopeMode | Scalar::DiscordPrivacy) = it {
                select(&mut a, it);
                press(&mut a, KeyCode::Right);
                press(&mut a, KeyCode::Char('r'));
            }
        }
        a.key_bind(Action::Note, crate::keys::parse_key("F9").unwrap(), false).unwrap();
        a.setting_set(Item::Theme(2), Val::Bool(true)).unwrap();
        a.persist_config();
        let after = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        assert_eq!(before, after, "{} was touched", path.display());
    }
}
