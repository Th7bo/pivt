//! Remappable keybindings: the `[keys]` table in config.toml.
//!
//! Every Normal-mode action, the run-pane actions and the interact-mode
//! detach key are an [`Action`] with default keys (the historical, AZERTY-safe
//! layout). The config table maps action names to one key or a list of keys:
//!
//! ```toml
//! [keys]
//! run = "x"
//! history = ["y", "ctrl-r"]
//! detach = ["f12", "ctrl-]"]
//! ```
//!
//! Parsing is lenient like `theme` / `scope_mode`: an unknown action, a key
//! that doesn't parse or a value of the wrong type is skipped with a warning,
//! never a config parse failure. Overlay-internal keys (j/k in lists, y/N
//! prompts, text entry) and Esc stay fixed.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;

/// Where a binding is live. Run-pane keys shadow Normal ones while the run
/// pane is focused (same key in both is by design, not a conflict); unbound
/// keys there fall through to Normal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ctx {
    Normal,
    Run,
    Interact,
}

macro_rules! actions {
    ($( $v:ident => $name:literal, $ctx:ident, [$($k:literal),*], $desc:literal; )*) => {
        /// A remappable action.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Action { $($v),* }

        impl Action {
            /// Every action, in priority order (earlier wins a tie).
            pub const ALL: &'static [Action] = &[$(Action::$v),*];

            /// The name used in the `[keys]` table.
            pub fn name(self) -> &'static str {
                match self { $(Action::$v => $name),* }
            }
            pub fn ctx(self) -> Ctx {
                match self { $(Action::$v => Ctx::$ctx),* }
            }
            /// Default keys (the current, AZERTY-friendly layout).
            pub fn defaults(self) -> &'static [&'static str] {
                match self { $(Action::$v => &[$($k),*]),* }
            }
            pub fn describe(self) -> &'static str {
                match self { $(Action::$v => $desc),* }
            }
        }
    };
}

actions! {
    // ── normal mode ──
    Up => "up", Normal, ["up", "k"], "move up in a pane";
    Down => "down", Normal, ["down", "j"], "move down in a pane";
    FocusNext => "focus_next", Normal, ["tab", "l", "right"], "next pane";
    FocusPrev => "focus_prev", Normal, ["shift-tab", "h", "left"], "previous pane";
    Copy => "copy", Normal, ["enter"], "fill + copy (preview pane: edit the variable)";
    Run => "run", Normal, ["x"], "run the command live";
    RunPane => "run_pane", Normal, ["`"], "show / hide the run pane";
    Quit => "quit", Normal, ["q"], "quit (ctrl-c always works too)";
    Vars => "vars", Normal, ["e"], "variables editor";
    Add => "add", Normal, ["a"], "add a command";
    Delete => "delete", Normal, ["d"], "delete a user command";
    Search => "search", Normal, ["/"], "fuzzy search";
    Help => "help", Normal, ["?"], "this help";
    Rooms => "rooms", Normal, ["c"], "rooms";
    Note => "note", Normal, ["w"], "writeup note";
    Notes => "notes", Normal, ["o"], "open room notes in $EDITOR";
    Loot => "loot", Normal, ["L"], "loot / creds";
    Favorite => "favorite", Normal, ["f"], "pin / unpin favorite";
    Bg => "bg", Normal, ["b"], "cycle background: stars / flow / matrix / snow / plasma / off";
    Theme => "theme", Normal, ["t"], "next theme";
    ThemePicker => "theme_picker", Normal, ["T"], "theme picker";
    Settings => "settings", Normal, [","], "settings: every option, live";
    Discord => "discord", Normal, ["D"], "Discord presence on / off";
    DiscordPrivacy => "discord_privacy", Normal, ["M"], "cycle Discord privacy";
    Gtfobins => "gtfobins", Normal, ["g"], "GTFOBins lookup";
    Parse => "parse", Normal, ["P"], "parse scan output";
    Recipes => "recipes", Normal, ["R"], "recipes";
    Htb => "htb", Normal, ["H"], "Hack The Box panel";
    Flag => "flag", Normal, ["F"], "capture a flag";
    Export => "export", Normal, ["E"], "export writeup";
    Missing => "missing", Normal, ["m"], "rescan + hide/show missing tools";
    History => "history", Normal, ["y", "ctrl-r"], "command history";
    Scope => "scope", Normal, ["S"], "room scope editor";
    // ── run pane focused ──
    ScrollDown => "scroll_down", Run, ["down", "j"], "scroll output down";
    ScrollUp => "scroll_up", Run, ["up", "k"], "scroll output up";
    PageDown => "page_down", Run, ["pgdn"], "scroll a page down";
    PageUp => "page_up", Run, ["pgup"], "scroll a page up";
    TabPrev => "tab_prev", Run, ["left", "["], "previous run tab";
    TabNext => "tab_next", Run, ["right", "]"], "next run tab";
    Kill => "kill", Run, ["K"], "kill the job";
    Close => "close", Run, ["d"], "close the tab";
    Rerun => "rerun", Run, ["r"], "re-run the tab";
    Tail => "tail", Run, ["g"], "jump to the end";
    Zoom => "zoom", Run, ["z"], "zoom the run tab fullscreen";
    Interact => "interact", Run, ["i", "enter"], "type into the program";
    // ── interacting with a job ──
    Detach => "detach", Interact, ["f12", "ctrl-]"], "stop typing into the program";
}

impl Action {
    /// Look up a `[keys]` name: case-insensitive, `-` accepted for `_`.
    pub fn from_name(s: &str) -> Option<Action> {
        let n = s.trim().to_ascii_lowercase().replace('-', "_");
        Action::ALL.iter().copied().find(|a| a.name() == n)
    }
}

/// A normalised key: only ctrl/alt/shift modifiers; a character carries its
/// own case (so shift is dropped), ctrl-letters are lowercase, and shift-tab
/// is `BackTab`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Key {
    pub fn new(code: KeyCode, mods: KeyModifiers) -> Key {
        let mut mods = mods & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
        let mut code = code;
        match code {
            KeyCode::Char(c) => {
                mods.remove(KeyModifiers::SHIFT);
                if mods.contains(KeyModifiers::CONTROL) {
                    code = KeyCode::Char(c.to_ascii_lowercase());
                }
            }
            KeyCode::BackTab => mods.remove(KeyModifiers::SHIFT),
            KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
                code = KeyCode::BackTab;
                mods.remove(KeyModifiers::SHIFT);
            }
            _ => {}
        }
        Key { code, mods }
    }

    pub fn from_event(ev: &KeyEvent) -> Key {
        Key::new(ev.code, ev.modifiers)
    }

    fn bare(self) -> Key {
        Key { code: self.code, mods: KeyModifiers::NONE }
    }

    /// Display label: `x`, `^r`, `alt-x`, `F12`, `⏎`, `⇧⇥`, `↑`…
    pub fn label(&self) -> String {
        let base = match self.code {
            KeyCode::Char(' ') => "space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::F(n) => format!("F{n}"),
            KeyCode::Enter => "⏎".into(),
            KeyCode::Esc => "esc".into(),
            KeyCode::Tab => "⇥".into(),
            KeyCode::BackTab => "⇧⇥".into(),
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::PageUp => "PgUp".into(),
            KeyCode::PageDown => "PgDn".into(),
            KeyCode::Home => "Home".into(),
            KeyCode::End => "End".into(),
            KeyCode::Delete => "Del".into(),
            KeyCode::Insert => "Ins".into(),
            KeyCode::Backspace => "⌫".into(),
            other => format!("{other:?}"),
        };
        let mut out = String::new();
        if self.mods.contains(KeyModifiers::CONTROL) {
            out.push('^');
        }
        if self.mods.contains(KeyModifiers::ALT) {
            out.push_str("alt-");
        }
        if self.mods.contains(KeyModifiers::SHIFT) {
            out.push('⇧');
        }
        out.push_str(&base);
        out
    }
}

/// Parse a key string: a single character (case-sensitive), or
/// `ctrl-` / `alt-` / `shift-` prefixes (also `+`) on a character or a named
/// key: `f1`…`f24`, `enter`, `esc`, `tab`, `space`, `up`/`down`/`left`/
/// `right`, `pgup`/`pgdn`, `home`/`end`, `delete`, `backspace`, `insert`.
pub fn parse_key(s: &str) -> Option<Key> {
    if s.chars().count() == 1 {
        return Some(Key::new(KeyCode::Char(s.chars().next()?), KeyModifiers::NONE));
    }
    let mut rest = s.trim();
    if rest.is_empty() {
        return None;
    }
    let mut mods = KeyModifiers::NONE;
    loop {
        if rest.chars().count() <= 1 {
            break;
        }
        let lower = rest.to_ascii_lowercase();
        let pre = ["ctrl-", "ctrl+", "control-", "control+", "c-"]
            .iter()
            .map(|p| (p, KeyModifiers::CONTROL))
            .chain(["alt-", "alt+", "meta-", "meta+", "m-", "a-"].iter().map(|p| (p, KeyModifiers::ALT)))
            .chain(["shift-", "shift+", "s-"].iter().map(|p| (p, KeyModifiers::SHIFT)))
            .find(|(p, _)| lower.starts_with(**p) && lower.len() > p.len());
        match pre {
            Some((p, m)) => {
                mods |= m;
                rest = &rest[p.len()..];
            }
            None => break,
        }
    }
    let code = if rest.chars().count() == 1 {
        let c = rest.chars().next()?;
        if mods.contains(KeyModifiers::SHIFT) && c.is_ascii_alphabetic() {
            mods.remove(KeyModifiers::SHIFT);
            KeyCode::Char(c.to_ascii_uppercase())
        } else {
            KeyCode::Char(c)
        }
    } else {
        let n = rest.to_ascii_lowercase();
        match n.as_str() {
            "enter" | "return" | "ret" | "cr" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "space" | "spc" => KeyCode::Char(' '),
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "pgup" | "pageup" => KeyCode::PageUp,
            "pgdn" | "pgdown" | "pagedown" => KeyCode::PageDown,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "delete" | "del" => KeyCode::Delete,
            "backspace" | "bs" => KeyCode::Backspace,
            "insert" | "ins" => KeyCode::Insert,
            f if f.starts_with('f') => match f[1..].parse::<u8>() {
                Ok(n @ 1..=24) => KeyCode::F(n),
                _ => return None,
            },
            _ => return None,
        }
    };
    Some(Key::new(code, mods))
}

/// The config spelling of a key ([`parse_key`] reads it back), or `None`
/// for keys `[keys]` can't express (media keys, keypad begin…).
pub fn key_string(k: &Key) -> Option<String> {
    let base = match k.code {
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) if !c.is_control() => c.to_string(),
        KeyCode::F(n) => format!("f{n}"),
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => "backtab".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdn".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Insert => "insert".into(),
        _ => return None,
    };
    let mut out = String::new();
    if k.mods.contains(KeyModifiers::CONTROL) {
        out.push_str("ctrl-");
    }
    if k.mods.contains(KeyModifiers::ALT) {
        out.push_str("alt-");
    }
    if k.mods.contains(KeyModifiers::SHIFT) {
        out.push_str("shift-");
    }
    if k.code == KeyCode::BackTab && out.is_empty() {
        return Some("shift-tab".into());
    }
    out.push_str(&base);
    Some(out)
}

/// ctrl-c: always quits (or reaches the program), never remappable.
pub fn is_reserved(k: &Key) -> bool {
    *k == ctrl_c()
}

fn ctrl_c() -> Key {
    Key::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
}

/// The resolved bindings.
#[derive(Clone, Debug)]
pub struct Keymap {
    binds: HashMap<Action, Vec<Key>>,
    lookup: HashMap<(Ctx, Key), Action>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap::from_config(&toml::Table::new()).0
    }
}

impl Keymap {
    /// Build from the `[keys]` table; returns the warnings to show at startup.
    ///
    /// Rules: an explicitly configured binding beats a default one for the
    /// same key in the same context; between two of the same kind the action
    /// listed first in [`Action::ALL`] wins. A conflict between two explicit
    /// bindings always warns; a default losing a key warns only when it
    /// leaves that action with no key at all. `help` falls back to `?` and
    /// `detach` to its defaults if they end up unbound; ctrl-c is reserved
    /// (always quits, with the confirm while jobs run).
    pub fn from_config(table: &toml::Table) -> (Keymap, Vec<String>) {
        let mut warnings = Vec::new();
        let mut explicit: HashMap<Action, Vec<Key>> = HashMap::new();
        for (name, val) in table {
            let Some(action) = Action::from_name(name) else {
                warnings.push(format!("unknown action '{name}'"));
                continue;
            };
            let strs: Vec<String> = match val {
                toml::Value::String(s) => vec![s.clone()],
                toml::Value::Array(a) => {
                    let mut v = Vec::new();
                    for item in a {
                        match item.as_str() {
                            Some(s) => v.push(s.to_string()),
                            None => warnings.push(format!("{name}: {item} isn't a key string")),
                        }
                    }
                    v
                }
                other => {
                    warnings.push(format!("{name}: expected a key or a list of keys, got {other}"));
                    continue;
                }
            };
            let mut keys = Vec::new();
            for s in strs {
                match parse_key(&s) {
                    Some(k) if k == ctrl_c() => {
                        warnings.push(format!("{name}: ctrl-c is reserved (quit / sent to the program)"))
                    }
                    Some(k) => {
                        if !keys.contains(&k) {
                            keys.push(k);
                        }
                    }
                    None => warnings.push(format!("{name}: can't parse key '{s}'")),
                }
            }
            explicit.entry(action).or_default().extend(keys);
        }

        let mut binds: HashMap<Action, Vec<Key>> = HashMap::new();
        let mut lookup: HashMap<(Ctx, Key), Action> = HashMap::new();
        // explicit bindings first: they claim their keys
        for &a in Action::ALL {
            let Some(keys) = explicit.get(&a) else { continue };
            let mut kept = Vec::new();
            for &k in keys {
                match lookup.get(&(a.ctx(), k)) {
                    Some(&other) => warnings.push(format!(
                        "{} is bound to both {} and {} — {} wins",
                        k.label(),
                        other.name(),
                        a.name(),
                        other.name()
                    )),
                    None => {
                        lookup.insert((a.ctx(), k), a);
                        kept.push(k);
                    }
                }
            }
            binds.insert(a, kept);
        }
        // then defaults for everything not configured, skipping stolen keys
        for &a in Action::ALL {
            if explicit.contains_key(&a) {
                continue;
            }
            let mut kept = Vec::new();
            let mut lost = Vec::new();
            for k in a.defaults().iter().filter_map(|s| parse_key(s)) {
                match lookup.get(&(a.ctx(), k)) {
                    Some(&other) => lost.push((k, other)),
                    None => {
                        lookup.insert((a.ctx(), k), a);
                        kept.push(k);
                    }
                }
            }
            if kept.is_empty() {
                if let Some((k, other)) = lost.first() {
                    warnings.push(format!("{} now does {} — {} has no key left", k.label(), other.name(), a.name()));
                }
            }
            binds.insert(a, kept);
        }
        // fallbacks: never lose help, never get stuck interacting
        for (a, fallback) in [(Action::Help, &["?"][..]), (Action::Detach, Action::Detach.defaults())] {
            if binds.get(&a).is_some_and(|k| !k.is_empty()) {
                continue;
            }
            let keys: Vec<Key> = fallback.iter().filter_map(|s| parse_key(s)).collect();
            for &k in &keys {
                if let Some(other) = lookup.insert((a.ctx(), k), a) {
                    if let Some(v) = binds.get_mut(&other) {
                        v.retain(|x| *x != k);
                    }
                }
            }
            warnings.push(format!(
                "{} can't be unbound — using {}",
                a.name(),
                keys.iter().map(|k| k.label()).collect::<Vec<_>>().join(" / ")
            ));
            binds.insert(a, keys);
        }
        if binds.get(&Action::Quit).is_some_and(|k| k.is_empty()) {
            warnings.push("quit has no key — ctrl-c still quits".into());
        }
        (Keymap { binds, lookup }, warnings)
    }

    /// The action for a key in the first context that binds it exactly, else
    /// (Normal / Run only) the first that binds it with the modifiers ignored
    /// — so `X` with alt still runs, like the hardcoded keys always did.
    /// Interact only falls back for non-character keys (shift-F12 detaches,
    /// a plain `]` binding doesn't swallow ctrl-]).
    pub fn resolve(&self, ctxs: &[Ctx], ev: &KeyEvent) -> Option<Action> {
        let k = Key::from_event(ev);
        for c in ctxs {
            if let Some(a) = self.lookup.get(&(*c, k)) {
                return Some(*a);
            }
        }
        if k.mods.is_empty() {
            return None;
        }
        let bare = k.bare();
        for c in ctxs {
            if *c == Ctx::Interact && matches!(k.code, KeyCode::Char(_)) {
                continue;
            }
            if let Some(a) = self.lookup.get(&(*c, bare)) {
                return Some(*a);
            }
        }
        None
    }

    /// The action holding exactly this key in `ctx`, if any.
    pub fn owner(&self, ctx: Ctx, k: Key) -> Option<Action> {
        self.lookup.get(&(ctx, k)).copied()
    }

    /// Does this key trigger `a` (in `a`'s own context)?
    pub fn is(&self, a: Action, ev: &KeyEvent) -> bool {
        self.resolve(&[a.ctx()], ev) == Some(a)
    }

    pub fn keys(&self, a: Action) -> &[Key] {
        self.binds.get(&a).map(Vec::as_slice).unwrap_or(&[])
    }

    /// First bound key's label, for one-key hints (`x run`).
    pub fn first(&self, a: Action) -> String {
        self.keys(a).first().map(Key::label).unwrap_or_else(|| format!("[{}]", a.name()))
    }

    /// Every bound key, joined: `y / ^r`.
    pub fn label(&self, a: Action) -> String {
        self.join(a, " / ")
    }

    /// Every bound key, compact: `F12/^]`.
    pub fn compact(&self, a: Action) -> String {
        self.join(a, "/")
    }

    fn join(&self, a: Action, sep: &str) -> String {
        let k = self.keys(a);
        if k.is_empty() {
            return format!("[{}]", a.name());
        }
        k.iter().map(Key::label).collect::<Vec<_>>().join(sep)
    }

    /// Two opposite actions as one label: `↑↓ / jk`, `←→ / []`, `⇥ / hl`.
    pub fn pair(&self, a: Action, b: Action) -> String {
        let (ka, kb) = (self.keys(a), self.keys(b));
        if ka.is_empty() && kb.is_empty() {
            return format!("[{}]", a.name());
        }
        let mut parts = Vec::new();
        for i in 0..ka.len().max(kb.len()) {
            let part = match (ka.get(i), kb.get(i)) {
                (Some(x), Some(y)) => {
                    let set = |p: KeyCode, q: KeyCode| {
                        x.mods.is_empty()
                            && y.mods.is_empty()
                            && ((x.code == p && y.code == q) || (x.code == q && y.code == p))
                    };
                    if set(KeyCode::Up, KeyCode::Down) {
                        "↑↓".to_string()
                    } else if set(KeyCode::Left, KeyCode::Right) {
                        "←→".to_string()
                    } else if set(KeyCode::Tab, KeyCode::BackTab) {
                        "⇥".to_string()
                    } else {
                        // up/down-ish pairs read best in vi order (jk, hl, [])
                        let (lx, ly) = (x.label(), y.label());
                        let (l, r) = if lx > ly { (ly, lx) } else { (lx, ly) };
                        if l.chars().count() == 1 && r.chars().count() == 1 {
                            format!("{l}{r}")
                        } else {
                            format!("{l}/{r}")
                        }
                    }
                }
                (Some(x), None) | (None, Some(x)) => x.label(),
                (None, None) => continue,
            };
            parts.push(part);
        }
        parts.join(" / ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }
    fn ch(c: char) -> KeyEvent {
        ev(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn table(src: &str) -> toml::Table {
        toml::from_str(src).unwrap()
    }
    const N: KeyModifiers = KeyModifiers::NONE;
    const C: KeyModifiers = KeyModifiers::CONTROL;
    const A: KeyModifiers = KeyModifiers::ALT;
    const S: KeyModifiers = KeyModifiers::SHIFT;

    #[test]
    fn parse_key_all_forms() {
        let k = |s: &str| parse_key(s).unwrap_or_else(|| panic!("{s}"));
        assert_eq!(k("x"), Key::new(KeyCode::Char('x'), N));
        assert_eq!(k("X"), Key::new(KeyCode::Char('X'), N), "case-sensitive");
        assert_ne!(k("x"), k("X"));
        assert_eq!(k("?"), Key::new(KeyCode::Char('?'), N));
        assert_eq!(k("-"), Key::new(KeyCode::Char('-'), N));
        assert_eq!(k("+"), Key::new(KeyCode::Char('+'), N));
        assert_eq!(k("é"), Key::new(KeyCode::Char('é'), N));
        assert_eq!(k("ctrl-r"), Key::new(KeyCode::Char('r'), C));
        assert_eq!(k("Ctrl-R"), k("ctrl-r"), "ctrl letters fold case");
        assert_eq!(k("ctrl+r"), k("ctrl-r"));
        assert_eq!(k("ctrl-]"), Key::new(KeyCode::Char(']'), C));
        assert_eq!(k("ctrl--"), Key::new(KeyCode::Char('-'), C));
        assert_eq!(k("alt-x"), Key::new(KeyCode::Char('x'), A));
        assert_eq!(k("alt-X"), Key::new(KeyCode::Char('X'), A));
        assert_eq!(k("ctrl-alt-x"), Key::new(KeyCode::Char('x'), C | A));
        assert_eq!(k("shift-x"), k("X"));
        assert_eq!(k("shift-tab"), Key::new(KeyCode::BackTab, N));
        assert_eq!(k("shift-up"), Key::new(KeyCode::Up, S));
        for n in 1..=12u8 {
            assert_eq!(k(&format!("f{n}")), Key::new(KeyCode::F(n), N));
            assert_eq!(k(&format!("F{n}")), Key::new(KeyCode::F(n), N));
        }
        assert_eq!(k("enter"), Key::new(KeyCode::Enter, N));
        assert_eq!(k("esc"), Key::new(KeyCode::Esc, N));
        assert_eq!(k("tab"), Key::new(KeyCode::Tab, N));
        assert_eq!(k("space"), Key::new(KeyCode::Char(' '), N));
        assert_eq!(k("up"), Key::new(KeyCode::Up, N));
        assert_eq!(k("down"), Key::new(KeyCode::Down, N));
        assert_eq!(k("left"), Key::new(KeyCode::Left, N));
        assert_eq!(k("right"), Key::new(KeyCode::Right, N));
        assert_eq!(k("pgup"), Key::new(KeyCode::PageUp, N));
        assert_eq!(k("pgdn"), Key::new(KeyCode::PageDown, N));
        assert_eq!(k("home"), Key::new(KeyCode::Home, N));
        assert_eq!(k("end"), Key::new(KeyCode::End, N));
        assert_eq!(k("delete"), Key::new(KeyCode::Delete, N));
        assert_eq!(k("ctrl-f5"), Key::new(KeyCode::F(5), C));
        for bad in ["", "ctrl-", "f0", "f99", "hyper-x", "nope", "ctrl-nope"] {
            assert!(parse_key(bad).is_none(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn key_string_round_trips() {
        for s in [
            "x", "X", "?", "-", ",", "é", "space", "ctrl-r", "ctrl-]", "ctrl--", "alt-x", "alt-X", "ctrl-alt-x",
            "shift-tab", "shift-up", "f1", "f12", "ctrl-f5", "enter", "esc", "tab", "up", "down", "left", "right",
            "pgup", "pgdn", "home", "end", "delete", "backspace", "insert",
        ] {
            let k = parse_key(s).unwrap();
            let back = key_string(&k).unwrap_or_else(|| panic!("{s}"));
            assert_eq!(parse_key(&back), Some(k), "{s} → {back}");
        }
        assert_eq!(key_string(&parse_key("shift-tab").unwrap()).unwrap(), "shift-tab");
        assert_eq!(key_string(&Key::new(KeyCode::Char('R'), S)).unwrap(), "R");
        assert!(key_string(&Key::new(KeyCode::Null, N)).is_none());
        assert!(is_reserved(&Key::new(KeyCode::Char('C'), C)));
        let m = Keymap::default();
        assert_eq!(m.owner(Ctx::Normal, parse_key("x").unwrap()), Some(Action::Run));
        assert_eq!(m.owner(Ctx::Run, parse_key("x").unwrap()), None);
    }

    #[test]
    fn labels_render_nicely() {
        let l = |s: &str| parse_key(s).unwrap().label();
        assert_eq!(l("ctrl-r"), "^r");
        assert_eq!(l("f12"), "F12");
        assert_eq!(l("enter"), "⏎");
        assert_eq!(l("ctrl-]"), "^]");
        assert_eq!(l("alt-x"), "alt-x");
        assert_eq!(l("shift-tab"), "⇧⇥");
        assert_eq!(l("pgdn"), "PgDn");
        let m = Keymap::default();
        assert_eq!(m.label(Action::History), "y / ^r");
        assert_eq!(m.compact(Action::Detach), "F12/^]");
        assert_eq!(m.pair(Action::Down, Action::Up), "↑↓ / jk");
        assert_eq!(m.pair(Action::TabPrev, Action::TabNext), "←→ / []");
        assert_eq!(m.pair(Action::FocusNext, Action::FocusPrev), "⇥ / hl / ←→");
    }

    #[test]
    fn defaults_match_the_hardcoded_layout() {
        let (m, w) = Keymap::from_config(&toml::Table::new());
        assert!(w.is_empty(), "default map has no conflicts: {w:?}");
        let nm = |e: KeyEvent| m.resolve(&[Ctx::Normal], &e);
        let run = |e: KeyEvent| m.resolve(&[Ctx::Run, Ctx::Normal], &e);
        use Action::*;
        let normal = [
            ('q', Quit), ('x', Run), ('`', RunPane), ('l', FocusNext), ('h', FocusPrev),
            ('j', Down), ('k', Up), ('e', Vars), ('a', Add), ('d', Delete), ('/', Search),
            ('?', Help), ('c', Rooms), ('w', Note), ('o', Notes), ('L', Loot), ('f', Favorite),
            ('b', Bg), ('t', Theme), ('T', ThemePicker), ('D', Discord), ('M', DiscordPrivacy),
            ('g', Gtfobins), ('P', Parse), ('R', Recipes), ('H', Htb), ('F', Flag), ('E', Export),
            ('m', Missing), ('y', History), ('S', Scope), (',', Settings),
        ];
        for (c, a) in normal {
            assert_eq!(nm(ch(c)), Some(a), "{c}");
        }
        assert_eq!(nm(ev(KeyCode::Tab, N)), Some(FocusNext));
        assert_eq!(nm(ev(KeyCode::BackTab, S)), Some(FocusPrev));
        assert_eq!(nm(ev(KeyCode::Right, N)), Some(FocusNext));
        assert_eq!(nm(ev(KeyCode::Left, N)), Some(FocusPrev));
        assert_eq!(nm(ev(KeyCode::Down, N)), Some(Down));
        assert_eq!(nm(ev(KeyCode::Up, N)), Some(Up));
        assert_eq!(nm(ev(KeyCode::Enter, N)), Some(Copy));
        assert_eq!(nm(ev(KeyCode::Char('r'), C)), Some(History));
        // shift arrives with uppercase letters; modifiers were always ignored
        assert_eq!(nm(ev(KeyCode::Char('L'), S)), Some(Loot));
        assert_eq!(nm(ev(KeyCode::Char('x'), A)), Some(Run));
        assert_eq!(nm(ev(KeyCode::Esc, N)), None, "esc stays fixed");
        // run pane: its keys shadow normal ones, the rest fall through
        let runs = [
            ('j', ScrollDown), ('k', ScrollUp), ('[', TabPrev), (']', TabNext), ('K', Kill),
            ('d', Close), ('r', Rerun), ('g', Tail), ('z', Zoom), ('i', Interact), ('x', Run), ('L', Loot),
        ];
        for (c, a) in runs {
            assert_eq!(run(ch(c)), Some(a), "run {c}");
        }
        assert_eq!(run(ev(KeyCode::PageDown, N)), Some(PageDown));
        assert_eq!(run(ev(KeyCode::PageUp, N)), Some(PageUp));
        assert_eq!(run(ev(KeyCode::Left, N)), Some(TabPrev));
        assert_eq!(run(ev(KeyCode::Right, N)), Some(TabNext));
        assert_eq!(run(ev(KeyCode::Down, N)), Some(ScrollDown));
        assert_eq!(run(ev(KeyCode::Enter, N)), Some(Interact));
        // ^r opens history even though plain r re-runs in the run pane
        assert_eq!(run(ev(KeyCode::Char('r'), C)), Some(History));
        // interact: F12 (any modifiers) or ^] detach; plain ] is typed
        let it = |e: KeyEvent| m.resolve(&[Ctx::Interact], &e);
        assert_eq!(it(ev(KeyCode::F(12), N)), Some(Detach));
        assert_eq!(it(ev(KeyCode::F(12), S)), Some(Detach));
        assert_eq!(it(ev(KeyCode::Char(']'), C)), Some(Detach));
        assert_eq!(it(ch(']')), None);
        assert_eq!(it(ev(KeyCode::Char('c'), C)), None);
        // every action has a key by default
        for &a in Action::ALL {
            assert!(!m.keys(a).is_empty(), "{}", a.name());
        }
    }

    #[test]
    fn override_and_multi_key() {
        let (m, w) = Keymap::from_config(&table(
            "run = \"X\"\nzoom = \"Z\"\nhistory = [\"F5\", \"ctrl-h\"]\ndetach = [\"f10\", \"ctrl-g\"]\nfocus-next = \"tab\"",
        ));
        assert!(w.is_empty(), "{w:?}");
        let nm = |e: KeyEvent| m.resolve(&[Ctx::Normal], &e);
        assert_eq!(nm(ch('X')), Some(Action::Run));
        assert_eq!(nm(ch('x')), None, "old default is gone");
        assert_eq!(nm(ev(KeyCode::F(5), N)), Some(Action::History));
        assert_eq!(nm(ev(KeyCode::Char('h'), C)), Some(Action::History), "exact ^h beats lenient h");
        assert_eq!(nm(ch('h')), Some(Action::FocusPrev));
        assert_eq!(nm(ch('y')), None);
        assert_eq!(nm(ch('l')), None, "focus_next replaced wholesale");
        assert_eq!(m.resolve(&[Ctx::Run], &ch('Z')), Some(Action::Zoom));
        let it = |e: KeyEvent| m.resolve(&[Ctx::Interact], &e);
        assert_eq!(it(ev(KeyCode::F(10), N)), Some(Action::Detach));
        assert_eq!(it(ev(KeyCode::Char('g'), C)), Some(Action::Detach));
        assert_eq!(it(ev(KeyCode::F(12), N)), None);
        assert_eq!(m.label(Action::History), "F5 / ^h");
    }

    #[test]
    fn unknown_names_and_bad_keys_are_tolerated() {
        let (m, w) = Keymap::from_config(&table(
            "nope = \"x\"\nrun = [\"ctrl-\", \"Q\", 7]\nzoom = 3\nkill = \"hyper-k\"\nquit = \"ctrl-c\"",
        ));
        assert_eq!(w.len(), 7, "{w:?}");
        assert!(w.iter().any(|s| s.contains("unknown action 'nope'")));
        assert!(w.iter().any(|s| s.contains("ctrl-c is reserved")));
        assert_eq!(m.resolve(&[Ctx::Normal], &ch('Q')), Some(Action::Run), "the good key survives");
        assert_eq!(m.resolve(&[Ctx::Run], &ch('z')), Some(Action::Zoom), "wrong type → default");
        assert!(m.keys(Action::Kill).is_empty(), "explicit but unparsable → unbound");
        assert!(w.iter().any(|s| s.contains("quit has no key")));
        // the config file itself still parses with junk in [keys]
        let cfg: crate::model::Config =
            toml::from_str("theme = \"nord\"\n[keys]\nrun = 5\nbogus = [1, \"x\"]\n[vars]\nRHOST = \"h\"").unwrap();
        assert_eq!(cfg.theme, "nord");
        assert_eq!(cfg.vars["RHOST"], "h");
        let (_, w) = Keymap::from_config(&cfg.keys);
        assert_eq!(w.len(), 2, "{w:?}");
    }

    #[test]
    fn conflicts_warn_and_resolve_deterministically() {
        // two explicit bindings on one key: the earlier action wins
        let (m, w) = Keymap::from_config(&table("zoom = \"v\"\nkill = \"v\""));
        assert_eq!(m.resolve(&[Ctx::Run], &ch('v')), Some(Action::Kill));
        assert!(w.iter().any(|s| s.contains("both kill and zoom")), "{w:?}");
        // explicit beats default; the loser warns when it has nothing left
        let (m, w) = Keymap::from_config(&table("history = \"c\""));
        assert_eq!(m.resolve(&[Ctx::Normal], &ch('c')), Some(Action::History));
        assert!(m.keys(Action::Rooms).is_empty());
        assert!(w.iter().any(|s| s.contains("rooms has no key left")), "{w:?}");
        // a default keeping other keys is silent; other contexts don't clash
        let (m, w) = Keymap::from_config(&table("run = \"j\"\nnote = \"z\""));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(m.keys(Action::Down).len(), 1);
        assert_eq!(m.resolve(&[Ctx::Run, Ctx::Normal], &ch('z')), Some(Action::Zoom));
        assert_eq!(m.resolve(&[Ctx::Normal], &ch('z')), Some(Action::Note));
        // help falls back to ? (and takes it back), detach to its defaults
        let (m, w) = Keymap::from_config(&table("help = []\nsearch = \"?\"\ndetach = []"));
        assert_eq!(m.resolve(&[Ctx::Normal], &ch('?')), Some(Action::Help));
        assert!(m.keys(Action::Search).is_empty());
        assert_eq!(m.resolve(&[Ctx::Interact], &ev(KeyCode::F(12), N)), Some(Action::Detach));
        assert_eq!(w.iter().filter(|s| s.contains("can't be unbound")).count(), 2, "{w:?}");
        // explicitly rebinding help frees ?
        let (m, _) = Keymap::from_config(&table("help = \"F1\""));
        assert_eq!(m.resolve(&[Ctx::Normal], &ch('?')), None);
        assert_eq!(m.resolve(&[Ctx::Normal], &ev(KeyCode::F(1), N)), Some(Action::Help));
        // same input → same result, every time
        let src = table("zoom = \"v\"\nkill = \"v\"\nclose = \"v\"\ntail = \"K\"");
        let first = Keymap::from_config(&src);
        for _ in 0..20 {
            let again = Keymap::from_config(&src);
            assert_eq!(again.1, first.1);
            assert_eq!(again.0.keys(Action::Kill), first.0.keys(Action::Kill));
        }
    }
}
