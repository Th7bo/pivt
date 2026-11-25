//! Colour themes.
//!
//! Every colour the UI paints comes from the active [`Theme`], read through
//! [`theme()`]. Six presets are compiled in (the original pivt palette is the
//! default); user themes are loaded from `~/.config/pivt/themes/*.toml` and
//! inherit any role they leave out. Switching is a pointer swap behind an
//! `RwLock`, so a frame only pays for an uncontended read lock per lookup.
use ratatui::style::Color;
use std::path::Path;
use std::sync::RwLock;

const fn hex(v: u32) -> Color {
    Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

const fn mix8(x: u8, y: u8, t: u16) -> u8 {
    ((x as u16 * (100 - t) + y as u16 * t) / 100) as u8
}

/// Linear mix of two colours, `t` in 0..=100 (percent of `b`).
const fn mix(a: Color, b: Color, t: u16) -> Color {
    match (a, b) {
        (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
            Color::Rgb(mix8(ar, br, t), mix8(ag, bg, t), mix8(ab, bb, t))
        }
        _ => a,
    }
}

// ───────────────────────────── categories ─────────────────────────────

/// Named hue slots each preset fills in; categories pick a slot so a palette
/// only has to define 15 hues instead of one colour per category.
#[derive(Clone, Copy)]
enum Hue {
    Teal,
    Blue,
    Violet,
    Green,
    Amber,
    Cyan,
    Red,
    Orange,
    Pink,
    Slate,
    Emerald,
    Indigo,
    Purple,
    Crimson,
    Steel,
}
const NHUE: usize = 15;
use Hue::*;

/// Every category with its own glyph, and the hue slot it is drawn in.
/// (`scripts/presence_icons.py` reads this table and the pivt hues.)
const CATEGORIES: &[(&str, char, Hue)] = &[
    ("nmap", '⌖', Teal),
    ("web", '◉', Blue),
    ("smb", '▣', Violet),
    ("shells", '⌁', Green),
    ("upgrade", '▲', Amber),
    ("transfer", '⇅', Cyan),
    ("ad", '✦', Red),
    ("crack", '⬢', Orange),
    ("pivot", '⇄', Pink),
    ("misc", '●', Slate),
    ("encode", '⧉', Emerald),
    ("dns", '◈', Indigo),
    ("snmp", '▦', Teal),
    ("rpc", '⊞', Purple),
    ("db", '◰', Pink),
    ("tunnel", '≈', Cyan),
    ("forensics", '⌕', Amber),
    ("pwn", '⚑', Red),
    ("win", '❖', Blue),
    ("msf", '⊕', Crimson),
    ("favorites", '★', Amber),
    ("recent", '⏱', Steel),
    ("suggested", '✸', Teal),
];
const NCAT: usize = 23;

const fn cats_from(h: [Color; NHUE]) -> [Color; NCAT] {
    let mut out = [Color::Reset; NCAT];
    let mut i = 0;
    while i < NCAT {
        out[i] = h[CATEGORIES[i].2 as usize];
        i += 1;
    }
    out
}

fn cat_index(name: &str) -> Option<usize> {
    CATEGORIES.iter().position(|c| c.0 == name)
}

/// True when `name` has its own entry in [`category_style`] (not the fallback).
pub fn has_category_style(name: &str) -> bool {
    cat_index(name).is_some()
}

/// Per-category glyph + colour in the active theme. Unknown categories fall
/// back to a diamond in the accent colour.
pub fn category_style(name: &str) -> (char, Color) {
    let t = theme();
    let (mut glyph, mut color) = match cat_index(name) {
        Some(i) => (CATEGORIES[i].1, t.cats[i]),
        None => ('◆', t.accent),
    };
    // user overrides from [look.categories]
    if let Ok(o) = CAT_OVERRIDES.read() {
        if let Some(st) = o.iter().find(|s| s.name == name) {
            glyph = st.glyph.unwrap_or(glyph);
            color = match &st.color {
                Some(CatColor::Hex(c)) => *c,
                Some(CatColor::Role(r)) => t.role(r).unwrap_or(color),
                None => color,
            };
        }
    }
    (glyph, color)
}

use crate::prefs::{CatColor, CatStyle};

/// `[look.categories]` overrides, consulted by [`category_style`].
static CAT_OVERRIDES: RwLock<Vec<CatStyle>> = RwLock::new(Vec::new());

/// Replace the category glyph/colour overrides.
pub fn set_category_overrides(o: Vec<CatStyle>) {
    if let Ok(mut w) = CAT_OVERRIDES.write() {
        *w = o;
    }
}

/// `#rrggbb` / `#rgb`, or the name of a theme role (`accent`, `ok`, …).
pub fn parse_color_spec(s: &str) -> Option<CatColor> {
    let s = s.trim();
    if s.starts_with('#') {
        return parse_hex(s).map(CatColor::Hex);
    }
    theme().role(s).map(|_| CatColor::Role(s.to_string()))
}

// ─────────────────────────────── roles ───────────────────────────────

macro_rules! theme_roles {
    ($($role:ident : $doc:literal),* $(,)?) => {
        /// A full palette. Every field is a colour role the UI paints with.
        #[derive(Clone, Debug, PartialEq)]
        pub struct Theme {
            pub name: &'static str,
            $(#[doc = $doc] pub $role: Color,)*
            /// One colour per entry in `CATEGORIES`.
            cats: [Color; NCAT],
        }

        /// Role names as written in a custom theme file.
        #[cfg(test)]
        pub const ROLES: &[&str] = &[$(stringify!($role)),*];

        impl Theme {
            fn role_mut(&mut self, role: &str) -> Option<&mut Color> {
                match role {
                    $(stringify!($role) => Some(&mut self.$role),)*
                    _ => None,
                }
            }
            pub fn role(&self, role: &str) -> Option<Color> {
                match role {
                    $(stringify!($role) => Some(self.$role),)*
                    _ => None,
                }
            }
        }
    };
}

theme_roles! {
    bg: "Screen background.",
    panel: "Popup / overlay background.",
    fg: "Main text.",
    subtle: "Unselected list rows.",
    muted: "Descriptions and secondary text.",
    dim: "Hints, labels, disabled rows.",
    accent: "Primary accent (focus, keys, banner).",
    accent2: "Secondary accent (headings, subtitles).",
    accent_hi: "Bright end of the pulsing accent.",
    warn: "Errors, unset placeholders, sudo.",
    ok: "Success, set values, flags.",
    border: "Unfocused borders.",
    border_hi: "Focused borders.",
    sel_bg: "Selected row background.",
    star_faint: "Background effect, faintest.",
    star_dim: "Background effect, dim.",
    star_mid: "Background effect, mid.",
    star_hi: "Background effect, brightest (shooting star).",
    syn_flag: "Syntax: -flags.",
    syn_str: "Syntax: 'quoted strings'.",
    syn_num: "Syntax: numbers, IPs, ports.",
    syn_op: "Syntax: shell operators.",
    gold: "Favourite stars.",
    orange: "Hashes / crack mode, SUID.",
    slate: "Neutral frame (GTFOBins).",
    htb: "Hack The Box panel.",
}

impl Theme {
    /// The category hues, in `CATEGORIES` order (a colour ring for
    /// rainbow / confetti effects).
    pub fn cat_colors(&self) -> &[Color] {
        &self.cats
    }
}

/// Base roles for a preset (everything except the derived defaults).
struct Base {
    bg: u32,
    panel: u32,
    fg: u32,
    subtle: u32,
    muted: u32,
    dim: u32,
    accent: u32,
    accent2: u32,
    accent_hi: u32,
    warn: u32,
    ok: u32,
    border: u32,
    sel_bg: u32,
    stars: [u32; 4],
    syn: [u32; 4], // flag, str, num, op
    gold: u32,
    orange: u32,
    slate: u32,
    htb: u32,
}

const fn hues(v: [u32; NHUE]) -> [Color; NHUE] {
    let mut out = [Color::Reset; NHUE];
    let mut i = 0;
    while i < NHUE {
        out[i] = hex(v[i]);
        i += 1;
    }
    out
}

const fn preset(name: &'static str, b: Base, h: [u32; NHUE]) -> Theme {
    Theme {
        name,
        bg: hex(b.bg),
        panel: hex(b.panel),
        fg: hex(b.fg),
        subtle: hex(b.subtle),
        muted: hex(b.muted),
        dim: hex(b.dim),
        accent: hex(b.accent),
        accent2: hex(b.accent2),
        accent_hi: hex(b.accent_hi),
        warn: hex(b.warn),
        ok: hex(b.ok),
        border: hex(b.border),
        border_hi: hex(b.accent),
        sel_bg: hex(b.sel_bg),
        star_faint: hex(b.stars[0]),
        star_dim: hex(b.stars[1]),
        star_mid: hex(b.stars[2]),
        star_hi: hex(b.stars[3]),
        syn_flag: hex(b.syn[0]),
        syn_str: hex(b.syn[1]),
        syn_num: hex(b.syn[2]),
        syn_op: hex(b.syn[3]),
        gold: hex(b.gold),
        orange: hex(b.orange),
        slate: hex(b.slate),
        htb: hex(b.htb),
        cats: cats_from(hues(h)),
    }
}

/// Packs a colour back into 0xRRGGBB (for in-between roles built with `mix`).
const fn packed(c: Color) -> u32 {
    match c {
        Color::Rgb(r, g, b) => ((r as u32) << 16) | ((g as u32) << 8) | b as u32,
        _ => 0,
    }
}

// ─────────────────────────────── presets ───────────────────────────────
// Hue order: teal blue violet green amber cyan red orange pink slate
//            emerald indigo purple crimson steel

/// The original pivt palette (the default).
pub const PIVT: Theme = preset(
    "pivt",
    Base {
        bg: 0x0d1117,
        panel: 0x121821,
        fg: 0xc9d1d9,
        subtle: 0xb0bcc9,
        muted: 0x8b98a5,
        dim: 0x566270,
        accent: 0x2dd4bf,
        accent2: 0xa78bfa,
        accent_hi: 0xd4f5ff,
        warn: 0xf87272,
        ok: 0x7ee787,
        border: 0x232b36,
        sel_bg: 0x1b2a33,
        stars: [0x1e2630, 0x303b49, 0x4b5a6b, 0x8fa6bd],
        syn: [0x8fb8ff, 0xe5c07b, 0xf0b457, 0x6d7a8a],
        gold: 0xf0b457,
        orange: 0xfb923c,
        slate: 0x94a3b8,
        htb: 0x9fef00,
    },
    [
        0x2dd4bf, 0x60a5fa, 0xa78bfa, 0x7ee787, 0xf0b457, 0x22d3ee, 0xf87272, 0xfb923c, 0xf472b6,
        0x94a3b8, 0x34d399, 0x818cf8, 0xc08bfa, 0xdc4b4b, 0x8fa6bd,
    ],
);

/// Catppuccin Mocha (catppuccin.com/palette).
pub const CATPPUCCIN_MOCHA: Theme = preset(
    "catppuccin mocha",
    Base {
        bg: 0x1e1e2e,       // base
        panel: 0x181825,    // mantle
        fg: 0xcdd6f4,       // text
        subtle: 0xbac2de,   // subtext1
        muted: 0xa6adc8,    // subtext0
        dim: 0x7f849c,      // overlay1
        accent: 0xcba6f7,   // mauve
        accent2: 0x89b4fa,  // blue
        accent_hi: 0xf5c2e7, // pink
        warn: 0xf38ba8,     // red
        ok: 0xa6e3a1,       // green
        border: 0x45475a,   // surface1
        sel_bg: 0x313244,   // surface0
        stars: [0x2a2b3c, 0x45475a, 0x6c7086, 0xa6adc8],
        syn: [0x89b4fa, 0xa6e3a1, 0xfab387, 0x9399b2],
        gold: 0xf9e2af,
        orange: 0xfab387,
        slate: 0x9399b2,
        htb: 0xa6e3a1,
    },
    [
        0x94e2d5, 0x89b4fa, 0xcba6f7, 0xa6e3a1, 0xf9e2af, 0x89dceb, 0xf38ba8, 0xfab387, 0xf5c2e7,
        0x9399b2, 0x74c7ec, 0xb4befe, 0xf2cdcd, 0xeba0ac, 0xa6adc8,
    ],
);

/// Gruvbox dark, medium contrast (github.com/morhetz/gruvbox).
pub const GRUVBOX_DARK: Theme = preset(
    "gruvbox dark",
    Base {
        bg: 0x282828,       // bg0
        panel: 0x1d2021,    // bg0_h
        fg: 0xebdbb2,       // fg1
        subtle: 0xd5c4a1,   // fg2
        muted: 0xbdae93,    // fg3
        dim: 0x928374,      // gray
        accent: 0xfabd2f,   // yellow
        accent2: 0x83a598,  // blue
        accent_hi: 0xfbf1c7, // fg0
        warn: 0xfb4934,     // red
        ok: 0xb8bb26,       // green
        border: 0x504945,   // bg2
        sel_bg: 0x3c3836,   // bg1
        stars: [0x32302f, 0x3c3836, 0x665c54, 0xa89984],
        syn: [0x83a598, 0xb8bb26, 0xd3869b, 0xa89984],
        gold: 0xfabd2f,
        orange: 0xfe8019,
        slate: 0xa89984,
        htb: 0xb8bb26,
    },
    [
        0x8ec07c, 0x83a598, 0xd3869b, 0xb8bb26, 0xfabd2f, 0x689d6a, 0xfb4934, 0xfe8019, 0xb16286,
        0xa89984, 0x98971a, 0x458588, 0xd3869b, 0xfb4934, 0xbdae93,
    ],
);

/// Tokyo Night, "night" style (github.com/folke/tokyonight.nvim).
pub const TOKYO_NIGHT: Theme = preset(
    "tokyo night",
    Base {
        bg: 0x1a1b26,
        panel: 0x16161e,    // bg_dark
        fg: 0xc0caf5,
        subtle: 0xa9b1d6,   // fg_dark
        muted: 0x9aa5ce,
        dim: 0x737aa2,      // dark5
        accent: 0x7aa2f7,   // blue
        accent2: 0xbb9af7,  // magenta
        accent_hi: 0xb4f9f8, // blue6
        warn: 0xf7768e,     // red
        ok: 0x9ece6a,       // green
        border: 0x3b4261,   // fg_gutter
        sel_bg: 0x283457,   // bg_visual
        stars: [0x232433, 0x292e42, 0x414868, 0x737aa2],
        syn: [0x7dcfff, 0x9ece6a, 0xff9e64, 0x89ddff],
        gold: 0xe0af68,
        orange: 0xff9e64,
        slate: 0x565f89,
        htb: 0x9ece6a,
    },
    [
        0x73daca, 0x7aa2f7, 0xbb9af7, 0x9ece6a, 0xe0af68, 0x7dcfff, 0xf7768e, 0xff9e64, 0xff007c,
        0xa9b1d6, 0x1abc9c, 0x2ac3de, 0x9d7cd8, 0xdb4b4b, 0x737aa2,
    ],
);

/// Nord (nordtheme.com). Nord has 16 colours, so a few categories share a hue.
pub const NORD: Theme = preset(
    "nord",
    Base {
        bg: 0x2e3440,       // nord0
        panel: 0x2e3440,    // nord0 (nord keeps one surface; borders frame popups)
        fg: 0xeceff4,       // nord6
        subtle: 0xd8dee9,   // nord4
        muted: packed(mix(hex(0xd8dee9), hex(0x4c566a), 30)),
        dim: packed(mix(hex(0xd8dee9), hex(0x4c566a), 55)),
        accent: 0x88c0d0,   // nord8
        accent2: 0xb48ead,  // nord15
        accent_hi: 0xeceff4,
        warn: 0xbf616a,     // nord11
        ok: 0xa3be8c,       // nord14
        border: 0x4c566a,   // nord3
        sel_bg: 0x3b4252,   // nord1
        stars: [0x363d4b, 0x434c5e, 0x4c566a, 0x81a1c1],
        syn: [0x88c0d0, 0xa3be8c, 0xb48ead, 0x81a1c1],
        gold: 0xebcb8b,
        orange: 0xd08770,
        slate: 0x81a1c1,
        htb: 0xa3be8c,
    },
    [
        0x8fbcbb, 0x81a1c1, 0xb48ead, 0xa3be8c, 0xebcb8b, 0x88c0d0, 0xbf616a, 0xd08770, 0xb48ead,
        0xd8dee9, 0x8fbcbb, 0x5e81ac, 0x5e81ac, 0xbf616a, 0xe5e9f0,
    ],
);

/// Rosé Pine, main variant (rosepinetheme.com/palette).
pub const ROSE_PINE: Theme = preset(
    "rosé pine",
    Base {
        bg: 0x191724,       // base
        panel: 0x1f1d2e,    // surface
        fg: 0xe0def4,       // text
        subtle: packed(mix(hex(0xe0def4), hex(0x908caa), 45)),
        muted: 0x908caa,    // subtle
        dim: 0x6e6a86,      // muted
        accent: 0xebbcba,   // rose
        accent2: 0xc4a7e7,  // iris
        accent_hi: 0xe0def4,
        warn: 0xeb6f92,     // love
        ok: 0x9ccfd8,       // foam
        border: 0x403d52,   // highlight med
        sel_bg: 0x2a283e,   // highlight low (dark: 21202e) lifted for contrast
        stars: [0x26233a, 0x403d52, 0x524f67, 0x908caa],
        syn: [0x9ccfd8, 0xf6c177, 0xebbcba, 0x908caa],
        gold: 0xf6c177,
        orange: 0xebbcba,
        slate: 0x908caa,
        htb: 0x9ccfd8,
    },
    [
        0x9ccfd8, 0x31748f, 0xc4a7e7, 0x9ccfd8, 0xf6c177, 0x9ccfd8, 0xeb6f92, 0xebbcba, 0xeb6f92,
        0x908caa, 0x31748f, 0xc4a7e7, 0xc4a7e7, 0xeb6f92, 0x6e6a86,
    ],
);

pub static PRESETS: [Theme; 6] = [PIVT, CATPPUCCIN_MOCHA, GRUVBOX_DARK, TOKYO_NIGHT, NORD, ROSE_PINE];

// ───────────────────────────── registry ─────────────────────────────

static ACTIVE: RwLock<&'static Theme> = RwLock::new(&PRESETS[0]);
static CUSTOM: RwLock<Vec<&'static Theme>> = RwLock::new(Vec::new());

/// The active theme.
pub fn theme() -> &'static Theme {
    *ACTIVE.read().unwrap_or_else(|e| e.into_inner())
}

/// Presets first, then user themes (a user theme may not shadow a preset name).
pub fn all() -> Vec<&'static Theme> {
    let custom = CUSTOM.read().unwrap_or_else(|e| e.into_inner());
    PRESETS.iter().chain(custom.iter().copied()).collect()
}

/// Lowercase, ASCII letters/digits only, é → e: "Rosé Pine" == "rose-pine".
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| if matches!(c, 'é' | 'É') { 'e' } else { c })
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Look a theme up by name (loose: case, spaces, dashes and accents ignored;
/// "catppuccin", "mocha", "gruvbox" and "tokyo" also work).
pub fn find(name: &str) -> Option<&'static Theme> {
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    let alias = match n.as_str() {
        "catppuccin" | "mocha" => "catppuccinmocha",
        "gruvbox" => "gruvboxdark",
        "tokyo" | "tokyonightnight" => "tokyonight",
        "rosepinemain" | "rosepine" => "rosepine",
        "default" => "pivt",
        other => other,
    };
    all().into_iter().find(|t| norm(t.name) == alias)
}

/// Theme for a config value: unknown or empty names fall back to the default.
pub fn resolve(name: &str) -> &'static Theme {
    find(name).unwrap_or(&PRESETS[0])
}

/// Make `t` the active theme.
pub fn set(t: &'static Theme) {
    *ACTIVE.write().unwrap_or_else(|e| e.into_inner()) = t;
}

/// Theme `offset` steps from the active one in [`all`] order (wraps).
pub fn step(offset: isize) -> &'static Theme {
    let list = all();
    let cur = list.iter().position(|t| std::ptr::eq(*t, theme())).unwrap_or(0) as isize;
    let n = list.len() as isize;
    list[(cur + offset).rem_euclid(n) as usize]
}

// ─────────────────────────── custom themes ───────────────────────────

/// "#rrggbb", "rrggbb" or "#rgb".
fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    let full = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok().map(hex)
}

/// Parse one theme file. Missing roles inherit from `base = "<theme>"` (the
/// default theme when absent); `border_hi` follows `accent` unless given.
/// Bad colours and unknown keys are skipped, never fatal. `None` only when the
/// file isn't TOML at all.
pub fn parse_custom(src: &str, fallback_name: &str) -> Option<Theme> {
    let v: toml::Table = toml::from_str(src).ok()?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .unwrap_or(fallback_name);
    let base = v.get("base").and_then(|b| b.as_str()).map(resolve).unwrap_or(&PRESETS[0]);
    let mut t = base.clone();
    let mut accent_set = false;
    let mut border_hi_set = false;
    for (k, val) in &v {
        let (Some(c), Some(slot)) = (val.as_str().and_then(parse_hex), t.role_mut(k)) else {
            continue;
        };
        *slot = c;
        accent_set |= k == "accent";
        border_hi_set |= k == "border_hi";
    }
    if accent_set && !border_hi_set {
        t.border_hi = t.accent;
    }
    if let Some(cats) = v.get("categories").and_then(|c| c.as_table()) {
        for (k, val) in cats {
            if let (Some(i), Some(c)) = (cat_index(k), val.as_str().and_then(parse_hex)) {
                t.cats[i] = c;
            }
        }
    }
    t.name = Box::leak(name.to_string().into_boxed_str());
    Some(t)
}

/// Load `dir/*.toml` as user themes (replacing any loaded before). Files that
/// reuse a preset's name are skipped. Returns how many were loaded.
pub fn load_custom_dir(dir: &Path) -> usize {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut loaded: Vec<&'static Theme> = Vec::new();
    for p in files {
        let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("custom").to_string();
        let Some(t) = std::fs::read_to_string(&p).ok().and_then(|s| parse_custom(&s, &stem)) else {
            continue;
        };
        let clash = PRESETS.iter().chain(loaded.iter().copied()).any(|o| norm(o.name) == norm(t.name));
        if !clash {
            loaded.push(Box::leak(Box::new(t)));
        }
    }
    let n = loaded.len();
    *CUSTOM.write().unwrap_or_else(|e| e.into_inner()) = loaded;
    n
}

/// Where user themes live.
pub fn custom_dir() -> std::path::PathBuf {
    crate::model::config_dir().join("themes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else { panic!("non-rgb colour {c:?}") };
        let ch = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
    }
    fn contrast(a: Color, b: Color) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_preset_defines_every_role() {
        assert_eq!(CATEGORIES.len(), NCAT);
        for t in &PRESETS {
            for r in ROLES {
                let c = t.role(r).unwrap_or_else(|| panic!("{} lacks {r}", t.name));
                assert!(matches!(c, Color::Rgb(..)), "{}.{r} is not truecolour", t.name);
            }
            assert!(t.cats.iter().all(|c| matches!(c, Color::Rgb(..))), "{} category gap", t.name);
            assert_eq!(find(t.name).map(|f| f.name), Some(t.name));
        }
        // pivt stays the original palette
        assert_eq!(PRESETS[0].bg, Color::Rgb(0x0d, 0x11, 0x17));
        assert_eq!(PRESETS[0].cats[cat_index("msf").unwrap()], Color::Rgb(0xdc, 0x4b, 0x4b));
    }

    #[test]
    fn presets_keep_readable_contrast() {
        for t in &PRESETS {
            let n = t.name;
            let need = |what: &str, a: Color, b: Color, min: f64| {
                let c = contrast(a, b);
                assert!(c >= min, "{n}: {what} contrast {c:.2} < {min}");
            };
            need("fg/bg", t.fg, t.bg, 7.0);
            need("fg/panel", t.fg, t.panel, 7.0);
            need("fg/sel_bg", t.fg, t.sel_bg, 6.0);
            need("subtle/bg", t.subtle, t.bg, 5.0);
            need("muted/bg", t.muted, t.bg, 4.0);
            need("dim/bg", t.dim, t.bg, 2.5);
            need("dim/panel", t.dim, t.panel, 2.3);
            need("sel_bg/bg", t.sel_bg, t.bg, 1.15);
            for (what, c) in [("accent", t.accent), ("accent2", t.accent2), ("warn", t.warn),
                ("ok", t.ok), ("gold", t.gold), ("orange", t.orange), ("htb", t.htb),
                ("syn_flag", t.syn_flag), ("syn_str", t.syn_str), ("syn_num", t.syn_num),
                ("syn_op", t.syn_op)] {
                need(what, c, t.bg, 3.0);
                need(&format!("{what}/sel"), c, t.sel_bg, 2.3);
            }
            // background effect: visible but quiet, and brightening in order
            let s = [t.star_faint, t.star_dim, t.star_mid, t.star_hi];
            let fb = contrast(t.star_faint, t.bg);
            assert!((1.05..2.0).contains(&fb), "{n}: star_faint/bg {fb:.2}");
            assert!(s.windows(2).all(|w| lum(w[0]) < lum(w[1])), "{n}: star ramp not increasing");
            assert!(contrast(t.star_mid, t.bg) < contrast(t.dim, t.bg) + 0.6, "{n}: stars too loud");
            // categories: readable, and enough distinct hues
            for (i, c) in t.cats.iter().enumerate() {
                need(CATEGORIES[i].0, *c, t.bg, 2.9);
            }
            let mut distinct = t.cats.to_vec();
            distinct.sort_by_key(|c| format!("{c:?}"));
            distinct.dedup();
            assert!(distinct.len() >= 7, "{n}: only {} category colours", distinct.len());
            // a category never matches the one listed right before it
            for w in t.cats.windows(2) {
                assert_ne!(w[0], w[1], "{n}: neighbouring categories share a colour");
            }
        }
    }

    #[test]
    fn custom_theme_parses_and_inherits() {
        let src = r##"
name = "dracula"
bg = "#282a36"
accent = "#bd93f9"
fg = "not a colour"
nonsense = "#ffffff"
[categories]
nmap = "#50fa7b"
bogus = "#000000"
"##;
        let t = parse_custom(src, "file-stem").unwrap();
        assert_eq!(t.name, "dracula");
        assert_eq!(t.bg, Color::Rgb(0x28, 0x2a, 0x36));
        assert_eq!(t.accent, Color::Rgb(0xbd, 0x93, 0xf9));
        assert_eq!(t.border_hi, t.accent, "border_hi follows a custom accent");
        assert_eq!(t.fg, PIVT.fg, "bad hex inherits");
        assert_eq!(t.warn, PIVT.warn, "missing role inherits");
        assert_eq!(t.cats[cat_index("nmap").unwrap()], Color::Rgb(0x50, 0xfa, 0x7b));
        assert_eq!(t.cats[cat_index("web").unwrap()], PIVT.cats[cat_index("web").unwrap()]);

        // base = another preset; name falls back to the file stem; #rgb works
        let t = parse_custom("base = \"nord\"\nwarn = \"#f00\"", "mine").unwrap();
        assert_eq!(t.name, "mine");
        assert_eq!(t.bg, NORD.bg);
        assert_eq!(t.warn, Color::Rgb(0xff, 0, 0));
        assert!(parse_custom("this is = = not toml", "x").is_none());
    }

    #[test]
    fn readme_sample_documents_every_role() {
        let readme = include_str!("../README.md");
        let start = readme.find("# ~/.config/pivt/themes/dracula.toml").expect("sample in README");
        let sample = &readme[start..start + readme[start..].find("```").unwrap()];
        let t = parse_custom(sample, "x").expect("README sample parses");
        assert_eq!(t.name, "dracula");
        for r in ROLES {
            assert!(sample.contains(&format!("\n{r} = \"#")), "README sample lacks role {r}");
        }
        for (name, _, _) in CATEGORIES {
            assert!(readme.contains(&format!(" {name}")), "README lacks category {name}");
        }
    }

    #[test]
    fn custom_dir_loads_and_skips_clashes() {
        let dir = std::env::temp_dir().join(format!("pivt-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), "name = \"Test Theme A\"\nbg = \"#101010\"").unwrap();
        std::fs::write(dir.join("b.toml"), "name = \"nord\"\nbg = \"#ffffff\"").unwrap();
        std::fs::write(dir.join("c.toml"), "{{ broken").unwrap();
        std::fs::write(dir.join("d.txt"), "name = \"ignored\"").unwrap();
        assert_eq!(load_custom_dir(&dir), 1);
        assert_eq!(find("test-theme-a").map(|t| t.bg), Some(Color::Rgb(0x10, 0x10, 0x10)));
        assert_eq!(resolve("nord").bg, NORD.bg, "a file can't shadow a preset");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn unknown_theme_falls_back() {
        assert_eq!(resolve("no-such-theme").name, "pivt");
        assert_eq!(resolve("").name, "pivt");
        assert_eq!(resolve("Rose-Pine").name, "rosé pine");
        assert_eq!(resolve("CATPPUCCIN").name, "catppuccin mocha");
        assert_eq!(resolve("tokyo_night").name, "tokyo night");
        assert!(find("no-such-theme").is_none());
    }
}
