use crate::loot::{self, Cred};
use crate::model::*;
use crate::room::{self, Room};
use crate::runner::RunJob;
use crate::{clipboard, netif, writeup};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Focus {
    Categories,
    Commands,
    Preview,
    Run,
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub enum Mode {
    Splash,
    Normal,
    Search,
    Vars,
    Add,
    Help,
    Rooms,
    Note,
    NewRoom,
    Loot,
    AddLoot,
    Flag,
}

#[derive(Default)]
pub struct LootForm {
    pub user: String,
    pub pass: String,
    pub hash: String,
    pub note: String,
    pub field: usize, // 0..=3
}

impl LootForm {
    pub fn buf_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.user,
            1 => &mut self.pass,
            2 => &mut self.hash,
            _ => &mut self.note,
        }
    }
}

#[derive(Default)]
pub struct NewRoomForm {
    pub name: String,
    pub ip: String,
    pub host: String,
    pub field: usize, // 0 name, 1 ip, 2 host
}

impl NewRoomForm {
    pub fn buf_mut(&mut self) -> &mut String {
        match self.field {
            0 => &mut self.name,
            1 => &mut self.ip,
            _ => &mut self.host,
        }
    }
}


#[derive(PartialEq, Eq, Clone, Copy)]
pub enum AddField {
    Category,
    Name,
    Description,
    Template,
}

#[derive(Default)]
pub struct AddForm {
    pub category: String,
    pub name: String,
    pub description: String,
    pub template: String,
    pub field: usize, // 0..=3
}

impl AddForm {
    pub fn field_kind(&self) -> AddField {
        match self.field {
            0 => AddField::Category,
            1 => AddField::Name,
            2 => AddField::Description,
            _ => AddField::Template,
        }
    }
    pub fn buf_mut(&mut self) -> &mut String {
        match self.field_kind() {
            AddField::Category => &mut self.category,
            AddField::Name => &mut self.name,
            AddField::Description => &mut self.description,
            AddField::Template => &mut self.template,
        }
    }
}

pub const DEFAULT_VARS: &[&str] = &[
    "RHOST", "RPORT", "LHOST", "LPORT", "URL", "DOMAIN", "USER", "PASS", "WORDLIST", "FILE",
];

/// Virtual category names (not real command categories).
pub const FAV_CAT: &str = "favorites";
pub const RECENT_CAT: &str = "recent";
const RECENT_MAX: usize = 25;

pub fn cmd_key(c: &Command) -> String {
    format!("{}/{}", c.category, c.name)
}

pub struct App {
    pub commands: Vec<Command>,
    pub vars: BTreeMap<String, String>,
    pub categories: Vec<String>,
    pub cat_idx: usize,
    pub cmd_idx: usize,
    pub focus: Focus,
    pub mode: Mode,
    pub search: String,
    pub status: String,
    pub should_quit: bool,
    pub tick: u64,
    pub splash_start: u64,

    // vars overlay
    pub var_idx: usize,
    pub var_editing: bool,
    pub var_buf: String,
    pub var_new_key: Option<String>, // Some(buf) while typing a new var name

    pub add: AddForm,

    // inline variable editing from the preview pane
    pub preview_var_idx: usize,
    pub inline_edit: Option<(String, String)>, // (var name, buffer)

    // ctf rooms + writeup logging
    pub active_room: Option<Room>,
    pub rooms: Vec<Room>,
    pub room_idx: usize,
    pub new_room: NewRoomForm,
    pub note_buf: String,
    pub logging: bool,
    /// Set to a notes path when the user asks to open it in $EDITOR; the event
    /// loop suspends the TUI, runs the editor, then clears this.
    pub pending_editor: Option<PathBuf>,

    // run pane (live command execution, one tab per job)
    pub runs: Vec<RunJob>,
    pub run_tab: usize,
    pub show_run: bool,
    pub interacting: bool, // keystrokes go to the focused job's PTY

    // loot / creds
    pub loot: Vec<Cred>,
    pub loot_idx: usize,
    pub loot_form: LootForm,

    // favorites + recent (command keys "category/name")
    pub favorites: Vec<String>,
    pub recent: Vec<String>,

    // flag capture
    pub flag_is_root: bool,
    pub flag_buf: String,

    // background effect
    pub flow_bg: bool,
    pub mouse: Option<(u16, u16)>,

    matcher: SkimMatcherV2,
}

impl App {
    pub fn new() -> Self {
        let commands = load_commands();
        let cfg = load_config();
        let favorites = cfg.favorites.clone();
        let recent = cfg.recent.clone();
        let flow_bg = cfg.flow_bg;
        let mut vars: BTreeMap<String, String> = cfg.vars;
        for k in DEFAULT_VARS {
            vars.entry((*k).to_string()).or_default();
        }
        // seed any placeholders used by commands
        for c in &commands {
            for ph in extract_placeholders(&c.template) {
                vars.entry(ph).or_default();
            }
        }
        if vars.get("LHOST").map(|s| s.is_empty()).unwrap_or(true) {
            if let Some(ip) = netif::detect_lhost() {
                vars.insert("LHOST".into(), ip);
            }
        }
        if vars.get("LPORT").map(|s| s.is_empty()).unwrap_or(true) {
            vars.insert("LPORT".into(), "4444".into());
        }

        // ctf room + shared target: room .target > ~/.cache/ctf-target > config
        let active_room = room::find_room_cwd();
        if let Some(r) = &active_room {
            if let Some((ip, host)) = r.read_target() {
                vars.insert("RHOST".into(), ip);
                if let Some(h) = host {
                    vars.insert("DOMAIN".into(), h);
                }
            }
        }
        if vars.get("RHOST").map(|s| s.is_empty()).unwrap_or(true) {
            if let Some(ip) = read_ctf_target() {
                vars.insert("RHOST".into(), ip);
            }
        }

        let loot = {
            let target = vars.get("RHOST").cloned().unwrap_or_default();
            loot::load(&loot::loot_path(active_room.as_ref(), &target))
        };

        let skip_splash = std::env::var("PIVT_NO_SPLASH").is_ok()
            || std::env::args().any(|a| a == "--no-splash");
        let categories = derive_categories(&commands);
        let mut app = App {
            commands,
            vars,
            categories,
            cat_idx: 0,
            cmd_idx: 0,
            focus: Focus::Categories,
            mode: if skip_splash { Mode::Normal } else { Mode::Splash },
            search: String::new(),
            status: "ready — type / to search, ? for help".into(),
            should_quit: false,
            tick: 0,
            splash_start: 0,
            var_idx: 0,
            var_editing: false,
            var_buf: String::new(),
            var_new_key: None,
            add: AddForm::default(),
            preview_var_idx: 0,
            inline_edit: None,
            active_room,
            rooms: Vec::new(),
            room_idx: 0,
            new_room: NewRoomForm::default(),
            note_buf: String::new(),
            logging: std::env::var("PIVT_NO_LOG").is_err(),
            pending_editor: None,
            runs: Vec::new(),
            run_tab: 0,
            show_run: false,
            interacting: false,
            loot,
            loot_idx: 0,
            loot_form: LootForm::default(),
            favorites,
            recent,
            flag_is_root: false,
            flag_buf: String::new(),
            flow_bg,
            mouse: None,
            matcher: SkimMatcherV2::default(),
        };
        app.refresh_categories();
        app.clamp();
        app
    }

    /// Frame index within the splash (0-based).
    pub fn splash_frame(&self) -> u64 {
        self.tick.saturating_sub(self.splash_start)
    }

    pub fn end_splash(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Advance animation; called once per tick from the event loop.
    /// The splash animates to completion then holds (starfield keeps twinkling)
    /// until the user presses a key.
    pub fn on_tick(&mut self) {
        self.tick += 1;
        let (rows, cols) = crate::runner::pane_size();
        for r in &mut self.runs {
            r.poll();
            r.resize(rows, cols);
        }
    }

    pub fn current_category(&self) -> Option<&str> {
        self.categories.get(self.cat_idx).map(|s| s.as_str())
    }

    /// Rebuild the category list: virtual favorites/recent (when non-empty)
    /// followed by the real categories.
    pub fn refresh_categories(&mut self) {
        let mut v = Vec::new();
        if !self.favorites.is_empty() {
            v.push(FAV_CAT.to_string());
        }
        if !self.recent.is_empty() {
            v.push(RECENT_CAT.to_string());
        }
        v.extend(derive_categories(&self.commands));
        self.categories = v;
    }

    fn command_by_key(&self, key: &str) -> Option<usize> {
        self.commands.iter().position(|c| cmd_key(c) == key)
    }

    pub fn category_count(&self, name: &str) -> usize {
        match name {
            FAV_CAT => self.favorites.len(),
            RECENT_CAT => self.recent.len(),
            _ => self.commands.iter().filter(|c| c.category == name).count(),
        }
    }

    pub fn is_favorite_cmd(&self, c: &Command) -> bool {
        self.favorites.contains(&cmd_key(c))
    }

    pub fn toggle_favorite(&mut self) {
        if let Some(c) = self.selected_cmd() {
            let key = cmd_key(c);
            if let Some(pos) = self.favorites.iter().position(|k| k == &key) {
                self.favorites.remove(pos);
                self.set_status("☆ unpinned");
            } else {
                self.favorites.push(key);
                self.set_status("★ pinned to favorites");
            }
            let in_virtual =
                matches!(self.current_category(), Some(FAV_CAT));
            self.refresh_categories();
            self.persist_config();
            if in_virtual {
                self.clamp();
            }
        }
    }

    fn push_recent(&mut self, key: String) {
        self.recent.retain(|k| k != &key);
        self.recent.insert(0, key);
        self.recent.truncate(RECENT_MAX);
        self.refresh_categories();
    }

    /// Indices into self.commands currently visible (after search / category filter).
    pub fn visible(&self) -> Vec<usize> {
        if !self.search.is_empty() {
            let mut scored: Vec<(i64, usize)> = self
                .commands
                .iter()
                .enumerate()
                .filter_map(|(i, c)| {
                    let hay = format!(
                        "{} {} {} {} {}",
                        c.category,
                        c.name,
                        c.description,
                        c.template,
                        c.tags.join(" ")
                    );
                    self.matcher.fuzzy_match(&hay, &self.search).map(|s| (s, i))
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0));
            scored.into_iter().map(|(_, i)| i).collect()
        } else {
            match self.current_category() {
                Some(FAV_CAT) => self
                    .favorites
                    .iter()
                    .filter_map(|k| self.command_by_key(k))
                    .collect(),
                Some(RECENT_CAT) => self
                    .recent
                    .iter()
                    .filter_map(|k| self.command_by_key(k))
                    .collect(),
                Some(cat) => self
                    .commands
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.category == cat)
                    .map(|(i, _)| i)
                    .collect(),
                None => Vec::new(),
            }
        }
    }

    pub fn selected_cmd(&self) -> Option<&Command> {
        let v = self.visible();
        v.get(self.cmd_idx).map(|&i| &self.commands[i])
    }

    pub fn filled_selected(&self) -> Option<String> {
        self.selected_cmd().map(|c| fill_template(&c.template, &self.vars))
    }

    fn clamp(&mut self) {
        if self.categories.is_empty() {
            self.cat_idx = 0;
        } else if self.cat_idx >= self.categories.len() {
            self.cat_idx = self.categories.len() - 1;
        }
        let n = self.visible().len();
        if n == 0 {
            self.cmd_idx = 0;
        } else if self.cmd_idx >= n {
            self.cmd_idx = n - 1;
        }
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    pub fn toggle_bg(&mut self) {
        self.flow_bg = !self.flow_bg;
        self.persist_config();
        self.set_status(if self.flow_bg { "background: flow field" } else { "background: starfield" });
    }

    // ── navigation ──
    pub fn next_cat(&mut self) {
        if !self.categories.is_empty() {
            self.cat_idx = (self.cat_idx + 1) % self.categories.len();
            self.cmd_idx = 0;
        }
    }
    pub fn prev_cat(&mut self) {
        if !self.categories.is_empty() {
            self.cat_idx = (self.cat_idx + self.categories.len() - 1) % self.categories.len();
            self.cmd_idx = 0;
        }
    }
    pub fn next_cmd(&mut self) {
        let n = self.visible().len();
        if n > 0 {
            self.cmd_idx = (self.cmd_idx + 1) % n;
        }
    }
    pub fn prev_cmd(&mut self) {
        let n = self.visible().len();
        if n > 0 {
            self.cmd_idx = (self.cmd_idx + n - 1) % n;
        }
    }

    // ── actions ──
    pub fn copy_selected(&mut self) {
        let Some(filled) = self.filled_selected() else { return };
        let (name, category, key) = match self.selected_cmd() {
            Some(c) => (c.name.clone(), c.category.clone(), cmd_key(c)),
            None => return,
        };
        match clipboard::copy(&filled) {
            Ok(_) => {
                let mut msg = String::from("✓ copied");
                let missing = self.missing_vars_for_selected();
                if !missing.is_empty() {
                    msg.push_str(&format!(" (⚠ unset: {})", missing.join(", ")));
                }
                if self.logging {
                    let target = self.vars.get("RHOST").cloned().unwrap_or_default();
                    let path = writeup::log_command(
                        self.active_room.as_ref(),
                        &target,
                        &name,
                        &category,
                        &filled,
                    );
                    msg.push_str(&format!(" · logged → {}", short_path(&path)));
                }
                self.push_recent(key);
                self.persist_config();
                self.set_status(msg);
            }
            Err(e) => self.set_status(format!("✗ copy failed: {e}")),
        }
    }

    pub fn missing_vars_for_selected(&self) -> Vec<String> {
        match self.selected_cmd() {
            Some(c) => extract_placeholders(&c.template)
                .into_iter()
                .filter(|p| self.vars.get(p).map(|v| v.is_empty()).unwrap_or(true))
                .collect(),
            None => Vec::new(),
        }
    }

    pub fn persist_config(&self) {
        let cfg = Config {
            vars: self.vars.clone(),
            favorites: self.favorites.clone(),
            recent: self.recent.clone(),
            flow_bg: self.flow_bg,
        };
        let _ = save_config(&cfg);
        // keep the shell kit's target file + room .target in sync with RHOST
        if let Some(rhost) = self.vars.get("RHOST") {
            if !rhost.is_empty() {
                write_ctf_target(rhost);
                if let Some(r) = &self.active_room {
                    let host = self.vars.get("DOMAIN").cloned().unwrap_or_default();
                    r.write_target(rhost, &host);
                }
            }
        }
    }

    // ── search mode ──
    pub fn search_push(&mut self, c: char) {
        self.search.push(c);
        self.cmd_idx = 0;
    }
    pub fn search_pop(&mut self) {
        self.search.pop();
        self.cmd_idx = 0;
    }

    // ── vars overlay ──
    pub fn vars_next(&mut self) {
        let n = self.vars.len();
        if n > 0 {
            self.var_idx = (self.var_idx + 1) % n;
        }
    }
    pub fn vars_prev(&mut self) {
        let n = self.vars.len();
        if n > 0 {
            self.var_idx = (self.var_idx + n - 1) % n;
        }
    }
    pub fn vars_begin_edit(&mut self) {
        let keys = self.vars_nav_keys();
        if let Some(k) = keys.get(self.var_idx) {
            self.var_buf = self.vars.get(k).cloned().unwrap_or_default();
            self.var_editing = true;
        }
    }
    pub fn vars_commit_edit(&mut self) {
        let keys = self.vars_nav_keys();
        if let Some(k) = keys.get(self.var_idx).cloned() {
            self.vars.insert(k, self.var_buf.clone());
            self.persist_config();
        }
        self.var_editing = false;
    }
    pub fn vars_detect_lhost(&mut self) {
        if let Some(ip) = netif::detect_lhost() {
            self.vars.insert("LHOST".into(), ip.clone());
            self.persist_config();
            self.set_status(format!("LHOST = {ip}"));
        } else {
            self.set_status("could not detect an interface IP");
        }
    }
    pub fn vars_begin_new(&mut self) {
        self.var_new_key = Some(String::new());
    }
    pub fn vars_commit_new(&mut self) {
        if let Some(key) = self.var_new_key.take() {
            let key = key.trim().to_uppercase();
            if !key.is_empty() {
                self.vars.entry(key.clone()).or_default();
                // move cursor to it
                if let Some(pos) = self.vars_nav_keys().iter().position(|k| k == &key) {
                    self.var_idx = pos;
                }
                self.persist_config();
            }
        }
    }

    // ── add-command overlay ──
    pub fn add_open(&mut self) {
        self.add = AddForm::default();
        if let Some(cat) = self.current_category() {
            self.add.category = cat.to_string();
        }
        self.mode = Mode::Add;
    }
    pub fn add_next_field(&mut self) {
        self.add.field = (self.add.field + 1) % 4;
    }
    pub fn add_prev_field(&mut self) {
        self.add.field = (self.add.field + 3) % 4;
    }
    pub fn add_save(&mut self) -> bool {
        let cat = self.add.category.trim().to_string();
        let name = self.add.name.trim().to_string();
        let tmpl = self.add.template.trim().to_string();
        if cat.is_empty() || name.is_empty() || tmpl.is_empty() {
            self.set_status("category, name and template are required");
            return false;
        }
        let new_phs = extract_placeholders(&tmpl);
        let cmd = Command {
            category: cat.to_string(),
            name: name.to_string(),
            description: self.add.description.trim().to_string(),
            template: tmpl.to_string(),
            tags: vec![],
            steps: vec![],
            user: true,
        };
        // override existing same (cat,name) or push
        if let Some(slot) = self
            .commands
            .iter_mut()
            .find(|c| c.category == cmd.category && c.name == cmd.name)
        {
            *slot = cmd;
        } else {
            self.commands.push(cmd);
        }
        // seed placeholders into vars
        for ph in new_phs {
            self.vars.entry(ph).or_default();
        }
        let cat_owned = cat.to_string();
        self.refresh_categories();
        if let Some(pos) = self.categories.iter().position(|c| c == &cat_owned) {
            self.cat_idx = pos;
        }
        let _ = save_user_commands(&self.commands);
        self.persist_config();
        self.set_status(format!("✓ saved '{name}' to {}", user_commands_file().display()));
        true
    }

    /// Delete the selected command if it is a user command.
    pub fn delete_selected(&mut self) {
        let v = self.visible();
        if let Some(&idx) = v.get(self.cmd_idx) {
            if self.commands[idx].user {
                let name = self.commands[idx].name.clone();
                self.commands.remove(idx);
                self.refresh_categories();
                let _ = save_user_commands(&self.commands);
                self.clamp();
                self.set_status(format!("✗ deleted '{name}'"));
            } else {
                self.set_status("only user-added commands can be deleted");
            }
        }
    }

    pub fn after_mutation(&mut self) {
        self.clamp();
    }

    // ── variables popup: split into "this command" vs "all others" ──
    /// Placeholders of the selected command that exist as vars.
    pub fn vars_used(&self) -> Vec<String> {
        let used = self.preview_vars();
        used.into_iter().filter(|k| self.vars.contains_key(k)).collect()
    }
    pub fn vars_others(&self) -> Vec<String> {
        let used = self.vars_used();
        self.vars.keys().filter(|k| !used.contains(k)).cloned().collect()
    }
    /// Navigation order for the popup: used vars first, then the rest.
    pub fn vars_nav_keys(&self) -> Vec<String> {
        let mut v = self.vars_used();
        v.extend(self.vars_others());
        v
    }

    // ── rooms ──
    pub fn open_rooms(&mut self) {
        self.rooms = room::list_rooms();
        self.room_idx = self
            .active_room
            .as_ref()
            .and_then(|a| self.rooms.iter().position(|r| r.path == a.path))
            .unwrap_or(0);
        self.mode = Mode::Rooms;
    }
    pub fn rooms_next(&mut self) {
        if !self.rooms.is_empty() {
            self.room_idx = (self.room_idx + 1) % self.rooms.len();
        }
    }
    pub fn rooms_prev(&mut self) {
        if !self.rooms.is_empty() {
            self.room_idx = (self.room_idx + self.rooms.len() - 1) % self.rooms.len();
        }
    }
    pub fn activate_selected_room(&mut self) {
        if let Some(r) = self.rooms.get(self.room_idx).cloned() {
            let tgt = r.read_target();
            self.active_room = Some(r.clone());
            if let Some((ip, host)) = tgt {
                self.vars.insert("RHOST".into(), ip);
                if let Some(h) = host {
                    self.vars.insert("DOMAIN".into(), h);
                }
            }
            self.persist_config();
            self.set_status(format!("◆ room '{}' active", r.name));
        }
        self.mode = Mode::Normal;
    }
    pub fn open_active_notes(&mut self) {
        if let Some(r) = &self.active_room {
            self.pending_editor = Some(r.notes_file());
        } else {
            self.set_status("no active room — press c to pick/create one");
        }
    }

    // ── new room form ──
    pub fn new_room_open(&mut self) {
        self.new_room = NewRoomForm::default();
        if let Some(rhost) = self.vars.get("RHOST") {
            self.new_room.ip = rhost.clone();
        }
        if let Some(d) = self.vars.get("DOMAIN") {
            self.new_room.host = d.clone();
        }
        self.mode = Mode::NewRoom;
    }
    pub fn new_room_next(&mut self) {
        self.new_room.field = (self.new_room.field + 1) % 3;
    }
    pub fn new_room_prev(&mut self) {
        self.new_room.field = (self.new_room.field + 2) % 3;
    }
    pub fn new_room_save(&mut self) -> bool {
        let name = self.new_room.name.trim();
        if name.is_empty() {
            self.set_status("room name required");
            return false;
        }
        let ip = self.new_room.ip.trim().to_string();
        let host = self.new_room.host.trim().to_string();
        match room::scaffold(name, &ip, &host) {
            Ok(r) => {
                self.active_room = Some(r.clone());
                if !ip.is_empty() {
                    self.vars.insert("RHOST".into(), ip);
                }
                if !host.is_empty() {
                    self.vars.insert("DOMAIN".into(), host);
                }
                self.persist_config();
                self.set_status(format!("✓ room '{}' created → {}", r.name, short_path(&r.path)));
                true
            }
            Err(e) => {
                self.set_status(format!("✗ create failed: {e}"));
                false
            }
        }
    }

    // ── writeup note ──
    pub fn note_open(&mut self) {
        self.note_buf.clear();
        self.mode = Mode::Note;
    }
    pub fn note_commit(&mut self) {
        let text = self.note_buf.trim().to_string();
        if !text.is_empty() {
            let target = self.vars.get("RHOST").cloned().unwrap_or_default();
            let path = writeup::log_note(self.active_room.as_ref(), &target, &text);
            self.set_status(format!("✎ noted → {}", short_path(&path)));
        }
        self.mode = Mode::Normal;
    }

    // ── run pane ──
    /// Execute the selected command's step(s), each in its own tab.
    pub fn run_selected(&mut self) {
        let Some(cmd) = self.selected_cmd().cloned() else { return };
        let missing = self.missing_vars_for_selected();
        if !missing.is_empty() {
            self.set_status(format!("⚠ set {} before running (e)", missing.join(", ")));
            return;
        }
        let steps = cmd.run_steps();
        let multi = steps.len() > 1;
        let first_new = self.runs.len();
        let (rows, cols) = crate::runner::pane_size();
        let cwd = self.active_room.as_ref().map(|r| r.path.clone());
        let mut spawned = 0;
        for (i, step) in steps.iter().enumerate() {
            let filled = fill_template(step, &self.vars);
            let title = if multi {
                format!("{} [{}]", cmd.name, i + 1)
            } else {
                cmd.name.clone()
            };
            match RunJob::spawn(title.clone(), filled.clone(), rows, cols, cwd.as_deref()) {
                Ok(job) => {
                    if self.logging {
                        let target = self.vars.get("RHOST").cloned().unwrap_or_default();
                        writeup::log_command(self.active_room.as_ref(), &target, &title, &cmd.category, &filled);
                    }
                    self.runs.push(job);
                    spawned += 1;
                }
                Err(e) => self.set_status(format!("✗ spawn failed: {e}")),
            }
        }
        if spawned == 0 {
            return;
        }
        self.push_recent(cmd_key(&cmd));
        self.persist_config();
        self.show_run = true;
        self.run_tab = first_new;
        self.focus = Focus::Run;
        self.interacting = false;
        self.set_status(format!(
            "▶ running {} ({spawned} tab{}) · i to interact",
            cmd.name,
            if spawned == 1 { "" } else { "s" }
        ));
    }

    pub fn active_run(&self) -> Option<&RunJob> {
        self.runs.get(self.run_tab)
    }
    pub fn run_tab_next(&mut self) {
        if !self.runs.is_empty() {
            self.run_tab = (self.run_tab + 1) % self.runs.len();
        }
    }
    pub fn run_tab_prev(&mut self) {
        if !self.runs.is_empty() {
            self.run_tab = (self.run_tab + self.runs.len() - 1) % self.runs.len();
        }
    }
    pub fn run_scroll_up(&mut self, n: usize) {
        if let Some(r) = self.runs.get_mut(self.run_tab) {
            r.scroll_up(n);
        }
    }
    pub fn run_scroll_down(&mut self, n: usize) {
        if let Some(r) = self.runs.get_mut(self.run_tab) {
            r.scroll_down(n);
        }
    }
    pub fn run_tail(&mut self) {
        if let Some(r) = self.runs.get_mut(self.run_tab) {
            r.scroll_tail();
        }
    }
    pub fn run_kill_active(&mut self) {
        let mut title = None;
        if let Some(r) = self.runs.get_mut(self.run_tab) {
            r.kill();
            title = Some(r.title.clone());
        }
        if let Some(t) = title {
            self.set_status(format!("✗ killed {t}"));
        }
    }
    pub fn run_close_active(&mut self) {
        if self.run_tab < self.runs.len() {
            self.runs.remove(self.run_tab); // Drop kills the child
            if self.run_tab >= self.runs.len() && self.run_tab > 0 {
                self.run_tab -= 1;
            }
        }
        self.interacting = false;
        if self.runs.is_empty() {
            self.show_run = false;
            self.focus = Focus::Commands;
        }
    }

    // ── interactive stdin ──
    pub fn begin_interact(&mut self) {
        if self.active_run().map(|j| j.is_running()).unwrap_or(false) {
            self.interacting = true;
            self.run_tail();
            self.set_status("-- interacting -- Ctrl-] to detach");
        } else {
            self.set_status("job finished — nothing to type into");
        }
    }
    pub fn end_interact(&mut self) {
        self.interacting = false;
        self.set_status("detached (scroll/tabs active)");
    }
    pub fn run_input(&mut self, bytes: &[u8]) {
        if let Some(job) = self.runs.get_mut(self.run_tab) {
            job.write_input(bytes);
        }
    }
    pub fn toggle_run_pane(&mut self) {
        if self.runs.is_empty() {
            self.set_status("nothing running — press x on a command to run it");
            return;
        }
        self.show_run = !self.show_run;
        if !self.show_run && self.focus == Focus::Run {
            self.focus = Focus::Commands;
        }
    }

    // ── loot / creds ──
    fn loot_file(&self) -> std::path::PathBuf {
        let target = self.vars.get("RHOST").cloned().unwrap_or_default();
        loot::loot_path(self.active_room.as_ref(), &target)
    }
    fn loot_persist(&self) {
        loot::save(&self.loot_file(), &self.loot);
    }
    pub fn open_loot(&mut self) {
        // reload in case the room/target changed
        self.loot = loot::load(&self.loot_file());
        if self.loot_idx >= self.loot.len() {
            self.loot_idx = self.loot.len().saturating_sub(1);
        }
        self.mode = Mode::Loot;
    }
    pub fn loot_next(&mut self) {
        if !self.loot.is_empty() {
            self.loot_idx = (self.loot_idx + 1) % self.loot.len();
        }
    }
    pub fn loot_prev(&mut self) {
        if !self.loot.is_empty() {
            self.loot_idx = (self.loot_idx + self.loot.len() - 1) % self.loot.len();
        }
    }
    pub fn loot_use(&mut self) {
        if let Some(c) = self.loot.get(self.loot_idx).cloned() {
            let mut set = Vec::new();
            if !c.user.is_empty() {
                self.vars.insert("USER".into(), c.user.clone());
                set.push("USER");
            }
            if !c.pass.is_empty() {
                self.vars.insert("PASS".into(), c.pass.clone());
                set.push("PASS");
            }
            if !c.hash.is_empty() {
                self.vars.insert("HASH".into(), c.hash.clone());
                set.push("HASH");
            }
            self.persist_config();
            if set.is_empty() {
                self.set_status("that entry has nothing to load");
            } else {
                self.set_status(format!("✓ loaded {} from loot", set.join("+")));
            }
        }
    }
    pub fn loot_delete(&mut self) {
        if self.loot_idx < self.loot.len() {
            self.loot.remove(self.loot_idx);
            if self.loot_idx >= self.loot.len() && self.loot_idx > 0 {
                self.loot_idx -= 1;
            }
            self.loot_persist();
            self.set_status("✗ loot entry removed");
        }
    }
    pub fn add_loot_open(&mut self) {
        self.loot_form = LootForm::default();
        // pre-fill from current vars for convenience
        self.loot_form.user = self.vars.get("USER").cloned().unwrap_or_default();
        self.loot_form.pass = self.vars.get("PASS").cloned().unwrap_or_default();
        self.mode = Mode::AddLoot;
    }
    pub fn add_loot_next(&mut self) {
        self.loot_form.field = (self.loot_form.field + 1) % 4;
    }
    pub fn add_loot_prev(&mut self) {
        self.loot_form.field = (self.loot_form.field + 3) % 4;
    }
    pub fn add_loot_save(&mut self) -> bool {
        let c = Cred {
            user: self.loot_form.user.trim().to_string(),
            pass: self.loot_form.pass.trim().to_string(),
            hash: self.loot_form.hash.trim().to_string(),
            note: self.loot_form.note.trim().to_string(),
        };
        if c.is_empty() {
            self.set_status("nothing to save");
            return false;
        }
        self.loot.push(c);
        self.loot_idx = self.loot.len() - 1;
        self.loot_persist();
        self.set_status(format!("✓ loot saved → {}", short_path(&self.loot_file())));
        true
    }

    // ── flags ──
    pub fn flag_open(&mut self) {
        self.flag_is_root = false;
        self.flag_buf.clear();
        self.mode = Mode::Flag;
    }
    pub fn flag_toggle_which(&mut self) {
        self.flag_is_root = !self.flag_is_root;
    }
    pub fn flag_save(&mut self) {
        let value = self.flag_buf.trim().to_string();
        if value.is_empty() {
            self.mode = Mode::Normal;
            return;
        }
        let which = if self.flag_is_root { "root" } else { "user" };
        let target = self.vars.get("RHOST").cloned().unwrap_or_default();
        writeup::set_flag(self.active_room.as_ref(), &target, which, &value);
        let _ = clipboard::copy(&value);
        self.set_status(format!("🚩 {which} flag saved + copied"));
        self.mode = Mode::Normal;
    }

    // ── writeup export ──
    pub fn export_writeup(&mut self) {
        let target = self.vars.get("RHOST").cloned().unwrap_or_default();
        let domain = self.vars.get("DOMAIN").cloned().unwrap_or_default();
        let lhost = self.vars.get("LHOST").cloned().unwrap_or_default();
        match writeup::export(self.active_room.as_ref(), &target, &domain, &lhost, &self.loot) {
            Ok(p) => self.set_status(format!("✓ writeup exported → {}", short_path(&p))),
            Err(e) => self.set_status(format!("✗ export failed: {e}")),
        }
    }

    // ── inline variable editing (preview pane) ──
    /// Placeholders of the currently selected command, in order.
    pub fn preview_vars(&self) -> Vec<String> {
        match self.selected_cmd() {
            Some(c) => extract_placeholders(&c.template),
            None => Vec::new(),
        }
    }
    pub fn preview_var_next(&mut self) {
        let n = self.preview_vars().len();
        if n > 0 {
            self.preview_var_idx = (self.preview_var_idx + 1) % n;
        }
    }
    pub fn preview_var_prev(&mut self) {
        let n = self.preview_vars().len();
        if n > 0 {
            self.preview_var_idx = (self.preview_var_idx + n - 1) % n;
        }
    }
    pub fn clamp_preview_var(&mut self) {
        let n = self.preview_vars().len();
        if n == 0 {
            self.preview_var_idx = 0;
        } else if self.preview_var_idx >= n {
            self.preview_var_idx = n - 1;
        }
    }
    pub fn inline_begin(&mut self) {
        let vars = self.preview_vars();
        if let Some(name) = vars.get(self.preview_var_idx).cloned() {
            let cur = self.vars.get(&name).cloned().unwrap_or_default();
            self.inline_edit = Some((name, cur));
        }
    }
    pub fn inline_push(&mut self, c: char) {
        if let Some((_, buf)) = self.inline_edit.as_mut() {
            buf.push(c);
        }
    }
    pub fn inline_pop(&mut self) {
        if let Some((_, buf)) = self.inline_edit.as_mut() {
            buf.pop();
        }
    }
    pub fn inline_commit(&mut self) {
        if let Some((name, buf)) = self.inline_edit.take() {
            self.vars.insert(name.clone(), buf);
            self.persist_config();
            self.set_status(format!("✓ {name} updated"));
        }
    }
    pub fn inline_cancel(&mut self) {
        self.inline_edit = None;
    }
}

/// Abbreviate a path with ~ for display in the status line.
fn short_path(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().to_string();
    if let Some(home) = dirs::home_dir() {
        let h = home.to_string_lossy().to_string();
        if let Some(rest) = s.strip_prefix(&h) {
            return format!("~{rest}");
        }
    }
    s
}

fn derive_categories(commands: &[Command]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in commands {
        if !out.contains(&c.category) {
            out.push(c.category.clone());
        }
    }
    out
}
