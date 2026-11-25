//! Layout and look/motion preferences: the `[layout]` and `[look]` tables in
//! config.toml.
//!
//! Every option lives in one registry, [`OPTIONS`]: a stable key, the table it
//! belongs to, a one-line description and its [`Kind`] (bool, int range,
//! choice list or glyph set with presets). [`Prefs`] holds the parsed values
//! and can be read and written by key ([`Prefs::get`] / [`Prefs::set`]), so a
//! settings screen only has to walk [`OPTIONS`].
//!
//! ```toml
//! [layout]
//! categories_width = 16
//! show_preview = false
//! compact = true
//!
//! [look]
//! background = "flow"
//! flow_charset = "blocks"
//! animations = false
//! border = "plain"
//!
//! [look.categories]
//! web = { glyph = "W", color = "#7aa2f7" }
//! mine = { color = "accent2" }
//! ```
//!
//! Parsing is lenient like `[keys]`: an unknown key, a value of the wrong type
//! or an unknown choice is skipped with a startup warning; out-of-range numbers
//! are clamped (with a warning). The raw tables are written back untouched, so
//! only what the user set ends up in their config.

use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::text::Span;
use ratatui::widgets::BorderType;

/// The config table an option lives in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Layout,
    Look,
}

impl Section {
    pub fn name(self) -> &'static str {
        match self {
            Section::Layout => "layout",
            Section::Look => "look",
        }
    }
}

/// A preference value, as the settings screen sees it.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Bool(bool),
    Int(i64),
    Str(String),
}

impl Val {
    pub fn to_toml(&self) -> toml::Value {
        match self {
            Val::Bool(b) => toml::Value::Boolean(*b),
            Val::Int(i) => toml::Value::Integer(*i),
            Val::Str(s) => toml::Value::String(s.clone()),
        }
    }
}

impl std::fmt::Display for Val {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Val::Bool(b) => write!(f, "{}", if *b { "on" } else { "off" }),
            Val::Int(i) => write!(f, "{i}"),
            Val::Str(s) => write!(f, "{s}"),
        }
    }
}

/// What values an option takes.
#[derive(Debug)]
pub enum Kind {
    Bool,
    /// Whole number, clamped into `min..=max`. `unit` is for display.
    Int { min: i64, max: i64, unit: &'static str },
    /// One of a fixed list of names.
    Choice(&'static [&'static str]),
    /// A glyph set: a preset name, or a literal string of `min..=max`
    /// single-width characters (`space` allows a blank).
    Glyphs { presets: &'static [(&'static str, &'static str)], min: usize, max: usize, space: bool },
}

/// One entry in the option registry.
#[derive(Debug)]
pub struct Opt {
    pub section: Section,
    pub key: &'static str,
    pub desc: &'static str,
    pub kind: Kind,
}

/// Values a [`Prefs`] field can hold, converted to and from [`Val`].
trait PrefValue: Sized {
    fn to_val(&self) -> Val;
    fn from_val(v: Val) -> Option<Self>;
}
impl PrefValue for bool {
    fn to_val(&self) -> Val {
        Val::Bool(*self)
    }
    fn from_val(v: Val) -> Option<Self> {
        match v {
            Val::Bool(b) => Some(b),
            _ => None,
        }
    }
}
impl PrefValue for u16 {
    fn to_val(&self) -> Val {
        Val::Int(*self as i64)
    }
    fn from_val(v: Val) -> Option<Self> {
        match v {
            Val::Int(i) => u16::try_from(i).ok(),
            _ => None,
        }
    }
}
impl PrefValue for String {
    fn to_val(&self) -> Val {
        Val::Str(self.clone())
    }
    fn from_val(v: Val) -> Option<Self> {
        match v {
            Val::Str(s) => Some(s),
            _ => None,
        }
    }
}

macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($v:ident => $s:literal),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($v),* }
        impl $name {
            pub const NAMES: &'static [&'static str] = &[$($s),*];
            pub fn as_str(self) -> &'static str {
                match self { $($name::$v => $s),* }
            }
            pub fn parse(s: &str) -> Option<Self> {
                match s.trim().to_ascii_lowercase().as_str() { $($s => Some($name::$v),)* _ => None }
            }
        }
        impl PrefValue for $name {
            fn to_val(&self) -> Val { Val::Str(self.as_str().to_string()) }
            fn from_val(v: Val) -> Option<Self> {
                match v { Val::Str(s) => Self::parse(&s), _ => None }
            }
        }
    };
}

choice!(
    /// What's painted behind the UI.
    Background { Stars => "stars", Flow => "flow", Off => "off" }
);
choice!(
    /// Pane / popup border style.
    Border { Rounded => "rounded", Plain => "plain", Double => "double", Thick => "thick", None => "none" }
);

impl Border {
    /// The ratatui border type ("none" still draws plain lines where a box
    /// is required, e.g. popups).
    pub fn border_type(self) -> BorderType {
        match self {
            Border::Rounded => BorderType::Rounded,
            Border::Plain | Border::None => BorderType::Plain,
            Border::Double => BorderType::Double,
            Border::Thick => BorderType::Thick,
        }
    }
}

/// Flow-field character ramps, faintest → densest (index 0 is never drawn).
pub const FLOW_PRESETS: &[(&str, &str)] = &[
    ("classic", " .,:;-=+*x#%@"),
    ("blocks", " ░▒▓█"),
    ("dots", " ·•●"),
    ("ascii", " .'`^\",:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$"),
    ("braille", " ⠁⠃⠇⡇⣇⣧⣷⣿"),
];
/// Starfield glyphs, dimmest → brightest. `classic` is the original
/// hand-tuned set (dots, `˙`, `+`, `*`, `✦`/`✧`).
pub const STAR_PRESETS: &[(&str, &str)] = &[
    ("classic", "·.+*✦"),
    ("dots", "·∙•●"),
    ("ascii", ".+*"),
    ("sparkle", "·✧✦"),
];
/// Selected-row marker presets.
pub const MARKER_PRESETS: &[(&str, &str)] = &[("bar", "▎"), ("arrow", "▸"), ("dot", "•"), ("chevron", "›")];

macro_rules! options {
    ($( $sec:ident . $field:ident : $ty:ty = $def:expr, $kind:expr, $desc:literal; )*) => {
        /// Parsed `[layout]` + `[look]` values. Field name = config key.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Prefs { $(pub $field: $ty,)* }

        impl Default for Prefs {
            fn default() -> Self { Prefs { $($field: $def,)* } }
        }

        /// The option registry, in display order.
        pub static OPTIONS: &[Opt] = &[
            $(Opt { section: Section::$sec, key: stringify!($field), desc: $desc, kind: $kind },)*
        ];

        impl Prefs {
            /// Current value of the option `key`.
            pub fn get(&self, key: &str) -> Option<Val> {
                match key { $(stringify!($field) => Some(self.$field.to_val()),)* _ => None }
            }
            /// Set `key` (already validated by [`Opt::parse`]). False when the
            /// key is unknown or the value has the wrong shape.
            pub fn set(&mut self, key: &str, v: Val) -> bool {
                match key {
                    $(stringify!($field) => match <$ty as PrefValue>::from_val(v) {
                        Some(x) => { self.$field = x; true }
                        None => false,
                    },)*
                    _ => false,
                }
            }
        }
    };
}

const fn int(min: i64, max: i64, unit: &'static str) -> Kind {
    Kind::Int { min, max, unit }
}

options! {
    // ── [layout] ──
    Layout.categories_width: u16 = 20, int(10, 40, "cols"), "categories pane width";
    Layout.commands_width: u16 = 42, int(20, 80, "%"), "commands pane width (preview takes the rest)";
    Layout.run_height: u16 = 42, int(15, 85, "%"), "run pane height when split";
    Layout.show_categories: bool = true, Kind::Bool, "show the categories pane";
    Layout.show_preview: bool = true, Kind::Bool, "show the preview pane";
    Layout.show_logo: bool = true, Kind::Bool, "header: pivt logo + active room";
    Layout.show_search: bool = true, Kind::Bool, "header: search box";
    Layout.show_target: bool = true, Kind::Bool, "header: target box";
    Layout.show_ping: bool = true, Kind::Bool, "target box: reachability dot";
    Layout.show_timer: bool = true, Kind::Bool, "target box: box timer + rtt";
    Layout.show_scope: bool = true, Kind::Bool, "target box: scope badge + ✓/⚠ mark";
    Layout.show_lhost: bool = true, Kind::Bool, "target box: LHOST:LPORT listener";
    Layout.show_hints: bool = true, Kind::Bool, "footer key hints (the status line stays)";
    Layout.compact: bool = false, Kind::Bool, "no pane borders, one-line header (small terminals)";
    // ── [look] ──
    Look.background: Background = Background::Stars, Kind::Choice(Background::NAMES), "background effect (b cycles)";
    Look.bg_density: u16 = 5, int(1, 10, ""), "background density";
    Look.bg_slowdown: u16 = 1, int(1, 10, "×"), "background tick divisor (higher = slower)";
    Look.flow_charset: String = "classic".into(),
        Kind::Glyphs { presets: FLOW_PRESETS, min: 2, max: 96, space: true },
        "flow field ramp: preset or your own chars, faint → dense";
    Look.star_glyphs: String = "classic".into(),
        Kind::Glyphs { presets: STAR_PRESETS, min: 1, max: 12, space: false },
        "starfield glyphs: preset or your own chars, dim → bright";
    Look.ripple: bool = true, Kind::Bool, "mouse ripple in the flow field";
    Look.splash: bool = true, Kind::Bool, "startup splash";
    Look.splash_hold: bool = true, Kind::Bool, "splash waits for a key (off = auto-continue)";
    Look.splash_seconds: u16 = 3, int(1, 30, "s"), "auto-continue after this long (splash_hold off)";
    Look.animations: bool = true, Kind::Bool, "pulse, twinkle, shooting stars, burst (off = static, fewer redraws)";
    Look.border: Border = Border::Rounded, Kind::Choice(Border::NAMES), "border style";
    Look.marker: String = "bar".into(),
        Kind::Glyphs { presets: MARKER_PRESETS, min: 1, max: 1, space: false },
        "selected-row marker: preset or one character";
}

/// Display width of one char (0 for control chars).
fn char_width(c: char) -> usize {
    if c.is_control() {
        return 0;
    }
    Span::raw(c.to_string()).width()
}

impl Opt {
    pub fn find(key: &str) -> Option<&'static Opt> {
        OPTIONS.iter().find(|o| o.key == key)
    }

    /// Default value.
    pub fn default_val(&self) -> Val {
        Prefs::default().get(self.key).expect("every option is a Prefs field")
    }

    /// Validate a config value. `Ok((value, note))`: `note` is set when the
    /// value was adjusted (clamped). `Err` = skipped, keep the default.
    pub fn parse(&self, v: &toml::Value) -> Result<(Val, Option<String>), String> {
        let key = self.key;
        match (&self.kind, v) {
            (Kind::Bool, toml::Value::Boolean(b)) => Ok((Val::Bool(*b), None)),
            (Kind::Bool, other) => Err(format!("{key}: expected true/false, got {other}")),
            (Kind::Int { min, max, .. }, toml::Value::Integer(i)) => {
                let c = (*i).clamp(*min, *max);
                let note = (c != *i).then(|| format!("{key}: {i} clamped to {c} ({min}..{max})"));
                Ok((Val::Int(c), note))
            }
            (Kind::Int { min, max, .. }, other) => Err(format!("{key}: expected a number {min}..{max}, got {other}")),
            (Kind::Choice(names), toml::Value::String(s)) => {
                let l = s.trim().to_ascii_lowercase();
                if names.contains(&l.as_str()) {
                    Ok((Val::Str(l), None))
                } else {
                    Err(format!("{key}: unknown '{s}' (one of {})", names.join("/")))
                }
            }
            (Kind::Choice(names), other) => Err(format!("{key}: expected one of {}, got {other}", names.join("/"))),
            (Kind::Glyphs { presets, min, max, space }, toml::Value::String(s)) => {
                if presets.iter().any(|(n, _)| n == s) {
                    return Ok((Val::Str(s.clone()), None));
                }
                let n = s.chars().count();
                if n < *min || n > *max {
                    return Err(format!("{key}: '{s}' needs {min}..{max} characters (or a preset: {})", preset_names(presets)));
                }
                if let Some(c) = s.chars().find(|&c| !(char_width(c) == 1 || (*space && c == ' '))) {
                    return Err(format!("{key}: '{c}' isn't a single-width character"));
                }
                if !*space && s.contains(' ') {
                    return Err(format!("{key}: no spaces allowed"));
                }
                Ok((Val::Str(s.clone()), None))
            }
            (Kind::Glyphs { presets, .. }, other) => {
                Err(format!("{key}: expected a preset ({}) or a string, got {other}", preset_names(presets)))
            }
        }
    }

    /// Next value for a settings screen: toggle a bool, step an int by
    /// `dir`, cycle a choice / glyph preset (a custom glyph string steps to
    /// the first preset).
    pub fn step(&self, cur: &Val, dir: i64) -> Val {
        let cycle = |names: Vec<&str>, cur: &str| {
            let n = names.len() as i64;
            let i = names.iter().position(|x| *x == cur).map(|i| i as i64).unwrap_or(-dir.signum());
            Val::Str(names[(i + dir.signum()).rem_euclid(n) as usize].to_string())
        };
        match (&self.kind, cur) {
            (Kind::Bool, Val::Bool(b)) => Val::Bool(!b),
            (Kind::Int { min, max, .. }, Val::Int(i)) => Val::Int((i + dir).clamp(*min, *max)),
            (Kind::Choice(names), Val::Str(s)) => cycle(names.to_vec(), s),
            (Kind::Glyphs { presets, .. }, Val::Str(s)) => cycle(presets.iter().map(|p| p.0).collect(), s),
            _ => self.default_val(),
        }
    }
}

fn preset_names(p: &[(&str, &str)]) -> String {
    p.iter().map(|x| x.0).collect::<Vec<_>>().join("/")
}

/// A glyph option's chars: the preset's, else the literal string.
fn glyphs(presets: &[(&str, &str)], spec: &str) -> Vec<char> {
    presets.iter().find(|(n, _)| *n == spec).map(|(_, g)| *g).unwrap_or(spec).chars().collect()
}

impl Prefs {
    /// Parse `[layout]` / `[look]`. `flow_bg` is the legacy background switch,
    /// used when `[look] background` isn't set. Returns warnings for the
    /// startup status line.
    pub fn load(layout: &toml::Table, look: &toml::Table, flow_bg: bool) -> (Prefs, Vec<String>) {
        let mut p = Prefs::default();
        if flow_bg {
            p.background = Background::Flow;
        }
        let mut warnings = Vec::new();
        for (section, table) in [(Section::Layout, layout), (Section::Look, look)] {
            for (k, v) in table {
                if section == Section::Look && k == "categories" {
                    continue; // see parse_categories
                }
                let Some(opt) = OPTIONS.iter().find(|o| o.section == section && o.key == k) else {
                    let hint = OPTIONS
                        .iter()
                        .find(|o| o.key == k)
                        .map(|o| format!(" (it belongs in [{}])", o.section.name()))
                        .unwrap_or_default();
                    warnings.push(format!("[{}] unknown key '{k}'{hint}", section.name()));
                    continue;
                };
                match opt.parse(v) {
                    Ok((val, note)) => {
                        p.set(k, val);
                        warnings.extend(note);
                    }
                    Err(e) => warnings.push(e),
                }
            }
        }
        (p, warnings)
    }

    /// Flow-field ramp; index 0 is the blank level.
    pub fn flow_ramp(&self) -> Vec<char> {
        let mut g = glyphs(FLOW_PRESETS, &self.flow_charset);
        if g.first() != Some(&' ') {
            g.insert(0, ' ');
        }
        if g.len() < 2 {
            g = FLOW_PRESETS[0].1.chars().collect();
        }
        g
    }

    /// Starfield glyphs, or `None` for the classic hand-tuned set.
    pub fn star_set(&self) -> Option<Vec<char>> {
        if self.star_glyphs == "classic" {
            return None;
        }
        let g = glyphs(STAR_PRESETS, &self.star_glyphs);
        (!g.is_empty()).then_some(g)
    }

    /// The selected-row marker (always one cell wide).
    pub fn marker(&self) -> String {
        glyphs(MARKER_PRESETS, &self.marker)
            .first()
            .filter(|c| char_width(**c) == 1)
            .unwrap_or(&'▎')
            .to_string()
    }

    /// Write `key = v` into the matching raw config table (for a settings
    /// screen); `None` removes it so the default applies again.
    pub fn store(layout: &mut toml::Table, look: &mut toml::Table, key: &str, v: Option<&Val>) -> bool {
        let Some(opt) = Opt::find(key) else { return false };
        let t = match opt.section {
            Section::Layout => layout,
            Section::Look => look,
        };
        match v {
            Some(v) => t.insert(key.to_string(), v.to_toml()),
            None => t.remove(key),
        };
        true
    }

    /// Header height: 0 (nothing shown), 1 (compact) or 3.
    fn header_h(&self) -> u16 {
        if !(self.show_logo || self.show_search || self.show_target) {
            0
        } else if self.compact {
            1
        } else {
            3
        }
    }
}

// ───────────────────────────── layout math ─────────────────────────────

/// The screen split into header / body / footer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub header: Option<Rect>,
    pub body: Rect,
    pub footer: Rect,
}

/// Split the screen. The header is dropped when the terminal is too short to
/// keep a usable body (3 rows).
pub fn split_screen(area: Rect, p: &Prefs) -> Screen {
    let footer_h = 1.min(area.height);
    let mut header_h = p.header_h();
    if area.height < header_h + footer_h + 3 {
        header_h = 0;
    }
    let body_h = area.height - header_h - footer_h;
    Screen {
        header: (header_h > 0).then(|| Rect::new(area.x, area.y, area.width, header_h)),
        body: Rect::new(area.x, area.y + header_h, area.width, body_h),
        footer: Rect::new(area.x, area.y + header_h + body_h, area.width, footer_h),
    }
}

/// Header boxes: logo | search | target. Hidden ones give their space to the
/// search box (or the target box / logo when search is hidden too).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Header {
    pub logo: Option<Rect>,
    pub search: Option<Rect>,
    pub target: Option<Rect>,
}

pub fn split_header(area: Rect, p: &Prefs) -> Header {
    let w = area.width;
    let mut logo = if p.show_logo { 22.min(w / 3) } else { 0 };
    let reserve = if p.show_search { 10 } else { 0 };
    let mut target = if p.show_target { 40.min(w.saturating_sub(logo + reserve)) } else { 0 };
    let mut search = if p.show_search { w - logo - target } else { 0 };
    let spare = w - logo - target - search;
    if target > 0 {
        target += spare;
    } else if search == 0 {
        logo += spare;
    } else {
        search += spare;
    }
    let mut x = area.x;
    let mut take = |width: u16| {
        let r = (width > 0).then(|| Rect::new(x, area.y, width, area.height));
        x += width;
        r
    };
    Header { logo: take(logo), search: take(search), target: take(target) }
}

/// The body's panes. `commands` is always present.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub categories: Option<Rect>,
    pub commands: Rect,
    pub preview: Option<Rect>,
    pub run: Option<Rect>,
}

/// Below this many columns the categories pane gives way to the commands.
const MIN_CMDS: u16 = 12;
/// Preview narrower than this isn't worth showing.
const MIN_PREVIEW: u16 = 14;

/// Split the body into panes. `run`: the run pane is visible (split, not
/// zoomed). Widths are clamped so tiny terminals still get a usable
/// commands pane; a pane that can't fit is dropped rather than squashed.
pub fn split_body(area: Rect, p: &Prefs, run: bool) -> Body {
    let (top, run_rect) = if run && area.height >= 4 {
        let h = area.height;
        let mut run_h = (h as u32 * p.run_height as u32 / 100) as u16;
        // keep 6 rows of panes above when there's room, and 3 rows of output
        run_h = run_h.clamp(3.min(h / 2), h.saturating_sub(6.min(h / 2)));
        let top_h = h - run_h;
        (
            Rect::new(area.x, area.y, area.width, top_h),
            Some(Rect::new(area.x, area.y + top_h, area.width, run_h)),
        )
    } else {
        (area, None)
    };

    let w = top.width;
    let cat = if p.show_categories && w >= MIN_CMDS + 10 {
        p.categories_width.clamp(8, (w / 3).max(8))
    } else {
        0
    };
    let rest = w - cat;
    let mut cmd = rest;
    if p.show_preview {
        let wanted = (w as u32 * p.commands_width as u32 / 100) as u16;
        // like the original Min(30) preview, but never more than half
        let pmin = 30.min(rest / 2).max(MIN_PREVIEW);
        cmd = wanted.min(rest.saturating_sub(pmin)).max(MIN_CMDS.min(rest));
        if rest - cmd < MIN_PREVIEW {
            cmd = rest;
        }
    }
    let prev = rest - cmd;
    Body {
        categories: (cat > 0).then(|| Rect::new(top.x, top.y, cat, top.height)),
        commands: Rect::new(top.x + cat, top.y, cmd, top.height),
        preview: (prev > 0).then(|| Rect::new(top.x + cat + cmd, top.y, prev, top.height)),
        run: run_rect,
    }
}

/// The run pane's inner (rows, cols) on a `term`-sized screen, for sizing
/// the PTY: the split (or zoomed) run rect minus borders / title and the
/// tab-bar row.
pub fn run_inner(term: Rect, p: &Prefs, fullscreen: bool) -> (u16, u16) {
    let body = split_screen(term, p).body;
    let run = if fullscreen { body } else { split_body(body, p, true).run.unwrap_or(body) };
    let bordered = !(p.compact || p.border == Border::None);
    let (bh, bw) = if bordered { (2, 2) } else { (1, 0) }; // no borders: the title row
    let rows = run.height.saturating_sub(bh + 1).max(4);
    let cols = run.width.saturating_sub(bw).max(20);
    (rows, cols)
}

// ───────────────────────────── category styles ─────────────────────────────

/// Colour for a category override: a hex value or a theme role, resolved
/// against the active theme at draw time.
#[derive(Clone, Debug, PartialEq)]
pub enum CatColor {
    Hex(Color),
    Role(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatStyle {
    pub name: String,
    pub glyph: Option<char>,
    pub color: Option<CatColor>,
}

/// Parse `[look.categories]`: `name = { glyph = "W", color = "#hex" | "role" }`
/// or the shorthand `name = "W"` (glyph only).
pub fn parse_categories(look: &toml::Table) -> (Vec<CatStyle>, Vec<String>) {
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    let Some(v) = look.get("categories") else { return (out, warnings) };
    let Some(t) = v.as_table() else {
        warnings.push("[look.categories] should be a table of category = { glyph, color }".into());
        return (out, warnings);
    };
    let glyph_of = |name: &str, s: &str, w: &mut Vec<String>| {
        let mut cs = s.chars();
        match (cs.next(), cs.next()) {
            (Some(c), None) if char_width(c) == 1 => Some(c),
            _ => {
                w.push(format!("categories.{name}: glyph '{s}' must be one single-width character"));
                None
            }
        }
    };
    for (name, val) in t {
        let mut st = CatStyle { name: name.clone(), glyph: None, color: None };
        match val {
            toml::Value::String(s) => st.glyph = glyph_of(name, s, &mut warnings),
            toml::Value::Table(tt) => {
                for (k, v) in tt {
                    match (k.as_str(), v.as_str()) {
                        ("glyph", Some(s)) => st.glyph = glyph_of(name, s, &mut warnings),
                        ("color" | "colour", Some(s)) => match crate::theme::parse_color_spec(s) {
                            Some(c) => st.color = Some(c),
                            None => warnings.push(format!("categories.{name}: unknown colour '{s}' (#rrggbb or a theme role)")),
                        },
                        _ => warnings.push(format!("categories.{name}: unknown or non-string key '{k}'")),
                    }
                }
            }
            other => warnings.push(format!("categories.{name}: expected a table or a glyph string, got {other}")),
        }
        if st.glyph.is_some() || st.color.is_some() {
            out.push(st);
        }
    }
    (out, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tbl(s: &str) -> toml::Table {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn registry_is_consistent() {
        let d = Prefs::default();
        let mut seen = std::collections::HashSet::new();
        for o in OPTIONS {
            assert!(seen.insert(o.key), "duplicate key {}", o.key);
            assert!(!o.desc.is_empty());
            let v = d.get(o.key).unwrap();
            // every default passes its own validation
            let (back, note) = o.parse(&v.to_toml()).unwrap_or_else(|e| panic!("{}: {e}", o.key));
            assert_eq!(back, v, "{}", o.key);
            assert!(note.is_none());
            // and round-trips through set
            let mut p = Prefs::default();
            assert!(p.set(o.key, v.clone()));
            // stepping stays valid
            let s = o.step(&v, 1);
            o.parse(&s.to_toml()).unwrap_or_else(|e| panic!("{} step: {e}", o.key));
        }
        assert!(d.get("nope").is_none());
    }

    #[test]
    fn lenient_parsing_and_clamping() {
        let (p, w) = Prefs::load(
            &tbl("categories_width = 500\ncommands_width = -3\nshow_preview = \"no\"\nbogus = 1\nbackground = \"flow\""),
            &tbl("background = \"plasma\"\nborder = \"DOUBLE\"\nbg_density = 0\nflow_charset = \"blocks\"\nmarker = \"ab\"\nsplash_seconds = 9\nanimations = false\ncompact = true"),
            false,
        );
        assert_eq!(p.categories_width, 40);
        assert_eq!(p.commands_width, 20);
        assert!(p.show_preview, "bad type keeps the default");
        assert_eq!(p.background, Background::Stars, "unknown choice keeps the default");
        assert_eq!(p.border, Border::Double, "choices are case-insensitive");
        assert_eq!(p.bg_density, 1);
        assert_eq!(p.flow_charset, "blocks");
        assert_eq!(p.marker, "bar", "two-char marker rejected");
        assert_eq!(p.splash_seconds, 9);
        assert!(!p.animations);
        assert!(!p.compact, "compact belongs in [layout]");
        let all = w.join("\n");
        for needle in [
            "categories_width: 500 clamped to 40",
            "commands_width: -3 clamped to 20",
            "show_preview: expected true/false",
            "[layout] unknown key 'bogus'",
            "[layout] unknown key 'background' (it belongs in [look])",
            "background: unknown 'plasma'",
            "bg_density: 0 clamped to 1",
            "marker: 'ab' needs 1..1",
            "[look] unknown key 'compact' (it belongs in [layout])",
        ] {
            assert!(all.contains(needle), "missing '{needle}' in:\n{all}");
        }
        // legacy flow_bg feeds the default; an explicit background wins
        assert_eq!(Prefs::load(&tbl(""), &tbl(""), true).0.background, Background::Flow);
        assert_eq!(Prefs::load(&tbl(""), &tbl("background = \"off\""), true).0.background, Background::Off);
    }

    #[test]
    fn glyph_sets() {
        let mut p = Prefs::default();
        assert_eq!(p.flow_ramp().iter().collect::<String>(), " .,:;-=+*x#%@");
        assert!(p.star_set().is_none());
        assert_eq!(p.marker(), "▎");
        p.flow_charset = "abc".into();
        assert_eq!(p.flow_ramp(), vec![' ', 'a', 'b', 'c'], "blank level prepended");
        p.star_glyphs = "dots".into();
        assert_eq!(p.star_set().unwrap().len(), 4);
        p.marker = "»".into();
        assert_eq!(p.marker(), "»");
        let o = Opt::find("flow_charset").unwrap();
        assert!(o.parse(&toml::Value::String("ab\tc".into())).is_err(), "control chars rejected");
        assert!(o.parse(&toml::Value::String("中文".into())).is_err(), "wide chars rejected");
        assert!(Opt::find("star_glyphs").unwrap().parse(&toml::Value::String(". *".into())).is_err());
        for (name, _) in FLOW_PRESETS {
            p.flow_charset = name.to_string();
            assert!(p.flow_ramp().len() >= 2);
        }
    }

    #[test]
    fn step_cycles() {
        let o = Opt::find("border").unwrap();
        assert_eq!(o.step(&Val::Str("none".into()), 1), Val::Str("rounded".into()));
        assert_eq!(o.step(&Val::Str("rounded".into()), -1), Val::Str("none".into()));
        let o = Opt::find("categories_width").unwrap();
        assert_eq!(o.step(&Val::Int(40), 1), Val::Int(40));
        let o = Opt::find("flow_charset").unwrap();
        assert_eq!(o.step(&Val::Str("xyz".into()), 1), Val::Str("classic".into()));
    }

    #[test]
    fn store_writes_only_the_key() {
        let (mut l, mut k) = (toml::Table::new(), toml::Table::new());
        assert!(Prefs::store(&mut l, &mut k, "compact", Some(&Val::Bool(true))));
        assert!(Prefs::store(&mut l, &mut k, "border", Some(&Val::Str("thick".into()))));
        assert!(!Prefs::store(&mut l, &mut k, "nope", Some(&Val::Bool(true))));
        assert_eq!(l.len(), 1);
        assert_eq!(k.len(), 1);
        let (p, w) = Prefs::load(&l, &k, false);
        assert!(w.is_empty());
        assert!(p.compact);
        assert_eq!(p.border, Border::Thick);
        Prefs::store(&mut l, &mut k, "compact", None);
        assert!(l.is_empty());
    }

    fn check_body(b: &Body, area: Rect) {
        let mut x = area.x;
        for r in [b.categories, Some(b.commands), b.preview].into_iter().flatten() {
            assert_eq!(r.x, x, "{b:?}");
            x += r.width;
        }
        assert_eq!(x, area.x + area.width, "panes fill the width: {b:?}");
        assert!(b.commands.width >= MIN_CMDS.min(area.width), "{b:?}");
        if let Some(p) = b.preview {
            assert!(p.width >= MIN_PREVIEW, "{b:?}");
        }
        let top_h = b.commands.height;
        match b.run {
            Some(r) => assert_eq!(top_h + r.height, area.height),
            None => assert_eq!(top_h, area.height),
        }
    }

    #[test]
    fn body_split_defaults_hidden_and_tiny() {
        let p = Prefs::default();
        let a = Rect::new(0, 3, 120, 40);
        let b = split_body(a, &p, false);
        assert_eq!(b.categories.unwrap().width, 20);
        assert_eq!(b.commands.width, 50);
        assert_eq!(b.preview.unwrap().width, 50);
        // 80 cols: preview keeps its 30 (as the old Min(30) did)
        let b = split_body(Rect::new(0, 0, 80, 20), &p, false);
        assert_eq!((b.categories.unwrap().width, b.commands.width, b.preview.unwrap().width), (20, 30, 30));

        let mut hide = p.clone();
        hide.show_categories = false;
        let b = split_body(a, &hide, false);
        assert!(b.categories.is_none());
        assert_eq!(b.commands.x, 0);
        assert_eq!(b.commands.width + b.preview.unwrap().width, 120);
        hide.show_preview = false;
        let b = split_body(a, &hide, false);
        assert_eq!(b.commands, Rect::new(0, 3, 120, 40), "commands take everything");
        let mut np = p.clone();
        np.show_preview = false;
        let b = split_body(a, &np, false);
        assert_eq!(b.commands.width, 100);

        // silly values at every size: never overflow, commands always usable
        let mut silly = p.clone();
        for (cw, pct, rh) in [(40, 80, 85), (10, 20, 15), (40, 20, 85)] {
            silly.categories_width = cw;
            silly.commands_width = pct;
            silly.run_height = rh;
            for (w, h) in [(200, 60), (120, 40), (80, 24), (60, 20), (40, 12), (20, 8), (5, 3), (1, 1), (0, 0)] {
                let a = Rect::new(0, 0, w, h);
                for run in [false, true] {
                    check_body(&split_body(a, &silly, run), a);
                }
            }
        }
        // a very narrow terminal drops categories, then preview
        let b = split_body(Rect::new(0, 0, 20, 10), &p, false);
        assert!(b.categories.is_none() && b.preview.is_none());
    }

    #[test]
    fn run_split_clamps() {
        let p = Prefs::default();
        let b = split_body(Rect::new(0, 0, 100, 40), &p, true);
        assert_eq!(b.run.unwrap().height, 16); // 42% of 40
        let mut tall = p.clone();
        tall.run_height = 85;
        let b = split_body(Rect::new(0, 0, 100, 20), &tall, true);
        assert_eq!(b.commands.height, 6, "panes keep 6 rows");
        let b = split_body(Rect::new(0, 0, 100, 5), &tall, true);
        assert!(b.run.unwrap().height >= 2 && b.commands.height >= 2);
    }

    #[test]
    fn screen_and_header_split() {
        let mut p = Prefs::default();
        let a = Rect::new(0, 0, 80, 24);
        let s = split_screen(a, &p);
        assert_eq!(s.header.unwrap().height, 3);
        assert_eq!(s.body, Rect::new(0, 3, 80, 20));
        assert_eq!(s.footer, Rect::new(0, 23, 80, 1));
        p.compact = true;
        let s = split_screen(a, &p);
        assert_eq!(s.header.unwrap().height, 1);
        assert_eq!(s.body.height, 22);
        // no header items → no header
        let none = Prefs { show_logo: false, show_search: false, show_target: false, ..Prefs::default() };
        assert!(split_screen(a, &none).header.is_none());
        // too short: header dropped
        assert!(split_screen(Rect::new(0, 0, 80, 5), &Prefs::default()).header.is_none());
        assert_eq!(split_screen(Rect::new(0, 0, 80, 0), &p).body.height, 0);

        let h = split_header(Rect::new(0, 0, 120, 3), &Prefs::default());
        assert_eq!(h.logo.unwrap().width, 22);
        assert_eq!(h.target.unwrap().width, 40);
        assert_eq!(h.search.unwrap().width, 58);
        let mut q = Prefs { show_search: false, ..Prefs::default() };
        let h = split_header(Rect::new(0, 0, 120, 3), &q);
        assert!(h.search.is_none());
        assert_eq!(h.target.unwrap().width, 98, "target takes the space");
        q.show_target = false;
        assert_eq!(split_header(Rect::new(0, 0, 120, 3), &q).logo.unwrap().width, 120);
        for w in [0, 5, 30, 80, 200] {
            let h = split_header(Rect::new(0, 0, w, 3), &Prefs::default());
            let total: u16 = [h.logo, h.search, h.target].iter().flatten().map(|r| r.width).sum();
            assert_eq!(total, w);
        }
    }

    #[test]
    fn run_inner_matches_layout() {
        let p = Prefs::default();
        // 100x32: body 28, run 42% = 11 → 11 - 2 borders - 1 tab bar
        assert_eq!(run_inner(Rect::new(0, 0, 100, 32), &p, false), (8, 98));
        assert_eq!(run_inner(Rect::new(0, 0, 100, 32), &p, true), (25, 98));
        let mut c = p.clone();
        c.compact = true;
        // body 30, run 12, minus title + tab bar
        assert_eq!(run_inner(Rect::new(0, 0, 100, 32), &c, false), (10, 100));
        assert_eq!(run_inner(Rect::new(0, 0, 3, 3), &p, false), (4, 20), "floors");
    }

    #[test]
    fn categories_table() {
        let (s, w) = parse_categories(&tbl(
            "[categories]\nweb = { glyph = \"W\", color = \"#ff0000\" }\nmine = { color = \"accent2\" }\nbad = { glyph = \"toolong\", color = \"nocolor\" }\nshort = \"S\"\nnum = 3",
        ));
        let names: Vec<_> = s.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["mine", "short", "web"]);
        assert_eq!(s[2].glyph, Some('W'));
        assert_eq!(s[2].color, Some(CatColor::Hex(Color::Rgb(255, 0, 0))));
        assert_eq!(s[0].color, Some(CatColor::Role("accent2".into())));
        assert_eq!(w.len(), 3, "{w:?}");
    }
}
