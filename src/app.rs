use crate::discord::{DiscordRpc, Presence};

/// Ticks (~50ms each) between rotating the presence text lines. ~15s keeps us
/// well inside Discord's 5-updates-per-20s rate limit.
const RPC_ROTATE_TICKS: u64 = 300;
/// Where the per-category presence badges live (`assets/presence/<cat>.png`).
const PRESENCE_IMG_BASE: &str = "https://raw.githubusercontent.com/Th7bo/pivt/main/assets/presence";
use crate::loot::{self, Cred};
use crate::model::*;
use crate::ping::{Ping, Pinger};
use crate::room::{self, Room};
use crate::runner::RunJob;
use crate::{clipboard, netif, writeup};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

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
    Gtfo,
    Recipes,
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
pub const SUGGEST_CAT: &str = "suggested";
const RECENT_MAX: usize = 25;

/// A hashcat run started from the loot view, waiting for its tab to exit.
pub struct PendingCrack {
    pub title: String,
    pub hash: String,
    pub out: std::path::PathBuf,
    /// Output is `john --show` format rather than hashcat's plain list.
    pub john: bool,
}

#[derive(Clone, Copy)]
enum CrackTool {
    Hashcat(u32),
    John(&'static str),
}

/// Whether `bin` is an executable on $PATH.
fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

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
    pub run_fullscreen: bool,
    pub interacting: bool, // keystrokes go to the focused job's PTY

    // loot / creds
    pub loot: Vec<Cred>,
    pub loot_idx: usize,
    /// hashcat runs started from the loot view, resolved when their tab exits.
    pub cracks: Vec<PendingCrack>,
    // recipes (R)
    pub recipes: Vec<crate::recipe::Recipe>,
    pub recipe_idx: usize,
    /// nmap service names from the last parsed scan (P), for recipe matching.
    pub scan_services: Vec<String>,
    pub loot_form: LootForm,

    // favorites + recent (command keys "category/name")
    pub favorites: Vec<String>,
    pub recent: Vec<String>,
    // commands suggested from a parsed scan
    pub suggested: Vec<String>,

    // flag capture
    pub flag_is_root: bool,
    pub flag_buf: String,
    pub burst_start: Option<u64>, // tick the flag-capture burst began
    pub burst_label: String,

    // gtfobins lookup
    pub gtfo_query: String,
    pub gtfo_idx: usize,

    // background effect
    pub flow_bg: bool,
    pub mouse: Option<(u16, u16)>,

    // live target status
    pinger: Pinger,
    pub box_started: Option<Instant>,
    /// Unix epoch of the box start (mirrors box_started for Discord timestamps).
    box_epoch: Option<i64>,
    last_rhost: String,

    // flag counter (for Discord presence)
    pub flags_captured: usize,

    // discord rich presence
    discord: Option<DiscordRpc>,
    pub discord_rpc: bool,
    pub discord_app_id: String,
    /// Optional project URL for the "get pivt" presence button.
    pub discord_link: String,
    last_rpc_tick: u64,

    matcher: SkimMatcherV2,
}

impl App {
    pub fn new() -> Self {
        let commands = load_commands();
        let cfg = load_config();
        let favorites = cfg.favorites.clone();
        let recent = cfg.recent.clone();
        let flow_bg = cfg.flow_bg;
        let discord_rpc = cfg.discord_rpc;
        let discord_app_id = if cfg.discord_app_id.trim().is_empty() {
            crate::model::DEFAULT_DISCORD_APP_ID.to_string()
        } else {
            cfg.discord_app_id.clone()
        };
        // Connect to Discord only when opted in and an app id is configured.
        let discord = if discord_rpc && !discord_app_id.is_empty() {
            Some(DiscordRpc::start(discord_app_id.clone()))
        } else {
            None
        };
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
            run_fullscreen: false,
            interacting: false,
            loot,
            loot_idx: 0,
            cracks: Vec::new(),
            recipes: crate::recipe::load_all(),
            recipe_idx: 0,
            scan_services: Vec::new(),
            loot_form: LootForm::default(),
            favorites,
            recent,
            suggested: Vec::new(),
            flag_is_root: false,
            flag_buf: String::new(),
            burst_start: None,
            burst_label: String::new(),
            gtfo_query: String::new(),
            gtfo_idx: 0,
            flow_bg,
            mouse: None,
            pinger: Pinger::start(),
            box_started: None,
            box_epoch: None,
            last_rhost: String::new(),
            flags_captured: 0,
            discord,
            discord_rpc,
            discord_app_id,
            discord_link: cfg.discord_link.clone(),
            last_rpc_tick: 0,
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
        let (rows, cols) = crate::runner::pane_size(self.run_fullscreen);
        for r in &mut self.runs {
            r.poll();
            r.resize(rows, cols);
        }
        self.poll_cracks();
        // track RHOST changes: re-point the pinger and (re)start the box timer
        let rhost = self.vars.get("RHOST").cloned().unwrap_or_default();
        if rhost != self.last_rhost {
            self.last_rhost = rhost.clone();
            self.pinger.set_target(&rhost);
            if rhost.is_empty() {
                self.box_started = None;
                self.box_epoch = None;
            } else {
                self.box_started = Some(Instant::now());
                // compute the epoch once, here, so Discord's elapsed clock is stable
                self.box_epoch = Some(crate::discord::now_unix());
            }
        }
        // Rich Presence: refresh about every 5s (not every ~50ms tick).
        if self.discord.is_some()
            && (self.last_rpc_tick == 0 || self.tick.saturating_sub(self.last_rpc_tick) >= 100)
        {
            self.last_rpc_tick = self.tick;
            self.push_discord();
        }
    }

    /// Build the current presence and hand it to the background RPC thread.
    ///
    /// Discord only shows two text lines, so each line rotates through every
    /// candidate that currently has something to say (one step per
    /// [`RPC_ROTATE_TICKS`]). A line with a single candidate stays put.
    fn push_discord(&self) {
        let Some(d) = &self.discord else { return };
        let plural = |n: usize| if n == 1 { "" } else { "s" };

        // top line candidates
        let mut details = vec![match &self.active_room {
            Some(r) => format!("{} · {} flag{}", r.name, self.flags_captured, plural(self.flags_captured)),
            None => "browsing commands".to_string(),
        }];
        let running = self.runs.iter().filter(|j| j.is_running()).count();
        if running > 0 {
            details.push(format!("⚡ {running} job{} running", plural(running)));
        }
        if !self.loot.is_empty() {
            let n = self.loot.len();
            details.push(format!("🔑 {n} cred{} looted", plural(n)));
        }
        if let Some(cat) = self.current_category() {
            let (glyph, _) = crate::theme::category_style(cat);
            details.push(format!("{glyph} in {cat}"));
        }

        // bottom line candidates
        let rhost = self.vars.get("RHOST").cloned().unwrap_or_default();
        let mut states = vec![if rhost.is_empty() {
            "no target set".to_string()
        } else {
            let status = match self.ping().up {
                Some(true) => "up",
                Some(false) => "down",
                None => "…",
            };
            let mut s = format!("◎ {rhost} {status}");
            if let Some(el) = self.box_elapsed() {
                s.push_str(&format!(" · ⏱ {el}"));
            }
            s
        }];
        if let Some(ports) = self.vars.get("PORTS").filter(|p| !p.trim().is_empty()) {
            let mut line = format!("open ports {ports}");
            if line.chars().count() > 120 {
                line = line.chars().take(119).collect::<String>() + "…";
            }
            states.push(line);
        }

        let step = (self.tick / RPC_ROTATE_TICKS) as usize;
        let details = details[step % details.len()].clone();
        let state = states[step % states.len()].clone();

        // buttons (max 2)
        let mut buttons = Vec::new();
        if let Some(r) = &self.active_room {
            buttons.push(("view room".to_string(), format!("https://tryhackme.com/room/{}", r.name)));
        }
        let link = self.discord_link.trim();
        let link = if link.is_empty() { crate::model::REPO_URL } else { link };
        buttons.push(("get pivt".to_string(), link.to_string()));

        // small badge = current category's glyph, served from the repo
        let (small_image, small_text) = match self.current_category() {
            Some(cat) => {
                let file = if crate::theme::has_category_style(cat) { cat } else { "default" };
                (format!("{PRESENCE_IMG_BASE}/{file}.png"), cat.to_string())
            }
            None => (String::new(), String::new()),
        };

        d.update(Presence {
            details,
            state,
            start_epoch: self.box_epoch,
            large_image: "pivt".to_string(),
            large_text: "pivt — ctf toolkit".to_string(),
            small_image,
            small_text,
            buttons,
        });
    }

    pub fn ping(&self) -> Ping {
        self.pinger.get()
    }

    /// Elapsed time on the current box, as mm:ss or h:mm:ss.
    pub fn box_elapsed(&self) -> Option<String> {
        let start = self.box_started?;
        let secs = start.elapsed().as_secs();
        let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
        Some(if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m:02}:{s:02}")
        })
    }

    pub fn current_category(&self) -> Option<&str> {
        self.categories.get(self.cat_idx).map(|s| s.as_str())
    }

    /// Rebuild the category list: virtual favorites/recent (when non-empty)
    /// followed by the real categories.
    pub fn refresh_categories(&mut self) {
        let mut v = Vec::new();
        if !self.suggested.is_empty() {
            v.push(SUGGEST_CAT.to_string());
        }
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
            SUGGEST_CAT => self.suggested.len(),
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
                Some(SUGGEST_CAT) => self
                    .suggested
                    .iter()
                    .filter_map(|k| self.command_by_key(k))
                    .collect(),
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

    /// Toggle Discord Rich Presence on/off (persisted).
    pub fn toggle_discord(&mut self) {
        self.discord_rpc = !self.discord_rpc;
        self.persist_config();
        if self.discord_rpc {
            if self.discord_app_id.is_empty() {
                self.set_status("discord rpc on — set discord_app_id in ~/.config/pivt/config.toml");
            } else {
                if self.discord.is_none() {
                    self.discord = Some(DiscordRpc::start(self.discord_app_id.clone()));
                }
                self.last_rpc_tick = 0; // push presence promptly
                self.set_status("discord rich presence: on");
            }
        } else {
            self.discord = None; // Drop clears the activity + closes the pipe
            self.set_status("discord rich presence: off");
        }
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

    /// Placeholders across all of a command's steps that have no value yet.
    pub fn missing_vars(&self, c: &Command) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for step in c.run_steps() {
            for p in extract_placeholders(&step) {
                if self.vars.get(&p).map(|v| v.is_empty()).unwrap_or(true) && !out.contains(&p) {
                    out.push(p);
                }
            }
        }
        out
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
            discord_rpc: self.discord_rpc,
            discord_app_id: self.discord_app_id.clone(),
            discord_link: self.discord_link.clone(),
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
        let missing = self.missing_vars(&cmd);
        if !missing.is_empty() {
            self.set_status(format!("⚠ set {} before running (e)", missing.join(", ")));
            return;
        }
        let first_new = self.runs.len();
        let spawned = self.spawn_cmd(&cmd);
        if spawned == 0 {
            return;
        }
        self.show_runs_from(first_new);
        self.set_status(format!(
            "▶ running {} ({spawned} tab{}) · i to interact",
            cmd.name,
            if spawned == 1 { "" } else { "s" }
        ));
    }

    /// Spawn every step of `cmd` in its own run tab (logging each), and push it
    /// to recent. Returns how many tabs started. Callers check missing vars.
    fn spawn_cmd(&mut self, cmd: &Command) -> usize {
        let steps = cmd.run_steps();
        let multi = steps.len() > 1;
        let (rows, cols) = crate::runner::pane_size(self.run_fullscreen);
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
        if spawned > 0 {
            self.push_recent(cmd_key(cmd));
            self.persist_config();
        }
        spawned
    }

    /// Show the run pane focused on the first tab spawned at `first_new`.
    fn show_runs_from(&mut self, first_new: usize) {
        self.show_run = true;
        self.run_tab = first_new;
        self.focus = Focus::Run;
        self.interacting = false;
    }

    // ── recipes ──
    /// Open the picker with recipes that match the box first, then generic
    /// ones, then the rest.
    pub fn open_recipes(&mut self) {
        self.recipes = crate::recipe::load_all();
        let ports = self.port_list();
        let services = self.scan_services.clone();
        self.recipes.sort_by_key(|r| {
            if r.matches(&ports, &services) {
                0
            } else if r.is_generic() {
                1
            } else {
                2
            }
        });
        self.recipe_idx = 0;
        self.mode = Mode::Recipes;
    }
    pub fn port_list(&self) -> Vec<u16> {
        crate::recipe::parse_ports(self.vars.get("PORTS").map(String::as_str).unwrap_or(""))
    }
    pub fn recipe_matched(&self, r: &crate::recipe::Recipe) -> bool {
        r.matches(&self.port_list(), &self.scan_services)
    }
    pub fn recipe_next(&mut self) {
        if !self.recipes.is_empty() {
            self.recipe_idx = (self.recipe_idx + 1) % self.recipes.len();
        }
    }
    pub fn recipe_prev(&mut self) {
        if !self.recipes.is_empty() {
            self.recipe_idx = (self.recipe_idx + self.recipes.len() - 1) % self.recipes.len();
        }
    }
    pub fn command_for_key(&self, key: &str) -> Option<&Command> {
        self.commands.iter().find(|c| cmd_key(c) == key)
    }

    /// Run the selected recipe.
    pub fn run_selected_recipe(&mut self) {
        let Some(r) = self.recipes.get(self.recipe_idx).cloned() else { return };
        self.run_recipes(&[r]);
    }
    /// Run every recipe that matches the box's ports/services (plus generic ones).
    pub fn run_matched_recipes(&mut self) {
        let picked: Vec<_> = self
            .recipes
            .iter()
            .filter(|r| self.recipe_matched(r))
            .cloned()
            .collect();
        if picked.is_empty() {
            self.set_status("no recipe matches PORTS yet — run a scan and press P first");
            return;
        }
        self.run_recipes(&picked);
    }

    fn run_recipes(&mut self, recipes: &[crate::recipe::Recipe]) {
        let first_new = self.runs.len();
        let mut started = 0;
        let mut skipped: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for r in recipes {
            for step in &r.steps {
                if seen.contains(step) {
                    continue; // same command in two recipes: run once
                }
                seen.push(step.clone());
                let Some(cmd) = self.command_for_key(step).cloned() else {
                    skipped.push(format!("{step} (unknown)"));
                    continue;
                };
                let missing = self.missing_vars(&cmd);
                if !missing.is_empty() {
                    skipped.push(format!("{} (needs {})", cmd.name, missing.join(",")));
                    continue;
                }
                started += self.spawn_cmd(&cmd);
            }
        }
        let names = recipes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
        if started == 0 {
            self.set_status(format!("✗ nothing ran for {names}: {}", skipped.join("; ")));
            return;
        }
        if self.logging {
            let target = self.vars.get("RHOST").cloned().unwrap_or_default();
            writeup::log_note(self.active_room.as_ref(), &target, &format!("▶ recipe: {names}"));
        }
        self.mode = Mode::Normal;
        self.show_runs_from(first_new);
        let mut msg = format!("▶ {names}: {started} tab{}", if started == 1 { "" } else { "s" });
        if !skipped.is_empty() {
            msg.push_str(&format!(" · skipped {}", skipped.join("; ")));
        }
        self.set_status(msg);
    }

    pub fn active_run(&self) -> Option<&RunJob> {
        self.runs.get(self.run_tab)
    }
    pub fn toggle_run_fullscreen(&mut self) {
        self.run_fullscreen = !self.run_fullscreen;
        if self.run_fullscreen {
            self.focus = Focus::Run;
        }
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

    /// Parse the active run tab's output for open ports, set PORTS (+URL), and
    /// build a "suggested" category of commands matching the services found.
    pub fn parse_scan(&mut self) {
        let text = match self.active_run() {
            Some(j) => j.raw_text(),
            None => {
                self.set_status("no run tab — run a scan with x first");
                return;
            }
        };
        let mut ports: Vec<String> = Vec::new();
        let mut services: Vec<String> = Vec::new();
        for line in text.lines() {
            let mut it = line.split_whitespace();
            if let Some(p0) = it.next() {
                let port = p0.strip_suffix("/tcp").or_else(|| p0.strip_suffix("/udp"));
                if let Some(port) = port {
                    if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) {
                        if it.next() == Some("open") {
                            if !ports.contains(&port.to_string()) {
                                ports.push(port.to_string());
                            }
                            if let Some(svc) = it.next() {
                                let svc = svc.to_string();
                                if !services.contains(&svc) {
                                    services.push(svc);
                                }
                            }
                        }
                    }
                }
            }
        }
        if ports.is_empty() {
            self.set_status("no open ports found in that output");
            return;
        }
        self.vars.insert("PORTS".into(), ports.join(","));
        self.scan_services = services.clone();
        // seed URL if a web port is open and URL unset
        let rhost = self.vars.get("RHOST").cloned().unwrap_or_default();
        if self.vars.get("URL").map(|u| u.is_empty()).unwrap_or(true) && !rhost.is_empty() {
            if services.iter().any(|s| s.contains("https")) || ports.iter().any(|p| p == "443") {
                self.vars.insert("URL".into(), format!("https://{rhost}"));
            } else if services.iter().any(|s| s.contains("http")) || ports.iter().any(|p| p == "80" || p == "8080") {
                self.vars.insert("URL".into(), format!("http://{rhost}"));
            }
        }
        // build suggestions
        let mut keywords: Vec<&str> = Vec::new();
        for s in &services {
            keywords.extend(service_keywords(s));
        }
        for p in &ports {
            keywords.extend(port_keywords(p));
        }
        let mut suggested: Vec<String> = Vec::new();
        for c in &self.commands {
            let hay = format!("{} {} {}", c.category, c.name, c.tags.join(" ")).to_lowercase();
            if keywords.iter().any(|k| hay.contains(k)) {
                let key = cmd_key(c);
                if !suggested.contains(&key) {
                    suggested.push(key);
                }
            }
        }
        self.suggested = suggested;
        self.refresh_categories();
        self.persist_config();
        // jump to the suggested category
        if let Some(pos) = self.categories.iter().position(|c| c == SUGGEST_CAT) {
            self.cat_idx = pos;
            self.cmd_idx = 0;
        }
        let recipes = self.recipes.iter().filter(|r| self.recipe_matched(r) && !r.is_generic()).count();
        self.set_status(format!(
            "✓ {} ports ({}) → PORTS set, {} suggested, {recipes} recipe{} (R)",
            ports.len(),
            services.join(","),
            self.suggested.len(),
            if recipes == 1 { "" } else { "s" }
        ));
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
    /// Copy a loot field to the clipboard. field: 0=user, 1=pass, 2=hash.
    pub fn loot_copy_field(&mut self, field: usize) {
        let picked = self.loot.get(self.loot_idx).map(|c| match field {
            0 => ("user", c.user.clone()),
            2 => ("hash", c.hash.clone()),
            _ => ("pass", c.pass.clone()),
        });
        if let Some((label, val)) = picked {
            if val.is_empty() {
                self.set_status(format!("no {label} on this entry"));
                return;
            }
            match clipboard::copy(&val) {
                Ok(_) => self.set_status(format!("✓ copied {label}: {val}")),
                Err(e) => self.set_status(format!("✗ copy failed: {e}")),
            }
        }
    }
    /// Yank the password (or hash if there's no password).
    pub fn loot_copy_pass(&mut self) {
        let use_hash = self
            .loot
            .get(self.loot_idx)
            .map(|c| c.pass.is_empty() && !c.hash.is_empty())
            .unwrap_or(false);
        self.loot_copy_field(if use_hash { 2 } else { 1 });
    }

    /// Crack the selected entry's hash: identify it, write it to a file, and
    /// run hashcat in a run tab. When the tab exits, `poll_cracks` reads the
    /// cracked plaintext back into the entry (and PASS).
    pub fn loot_crack(&mut self) {
        let Some(c) = self.loot.get(self.loot_idx).cloned() else { return };
        if c.hash.is_empty() {
            self.set_status("no hash on this entry");
            return;
        }
        if !c.pass.is_empty() {
            self.set_status(format!("already cracked: {}", c.pass));
            return;
        }
        if self.cracks.iter().any(|p| p.hash == c.hash) {
            self.set_status("already cracking that one — see the run pane");
            return;
        }
        let kinds = crate::hashid::identify(&c.hash);
        let Some(kind) = kinds.first().copied() else {
            self.set_status("✗ unknown hash format — try `hashid` / `john` manually");
            return;
        };
        let wordlist = self
            .vars
            .get("WORDLIST")
            .filter(|w| !w.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| "/usr/share/wordlists/rockyou.txt".into());

        let dir = match &self.active_room {
            Some(r) => r.path.join("loot").join("hashes"),
            None => crate::model::config_dir().join("loot").join("hashes"),
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.set_status(format!("✗ can't create {}: {e}", dir.display()));
            return;
        }
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let hash_file = dir.join(format!("{n}.hash"));
        let out_file = dir.join(format!("{n}.out"));
        let hashcat = on_path("hashcat");
        let tool = match (hashcat, kind.mode, kind.john()) {
            (true, Some(m), _) => CrackTool::Hashcat(m),
            (_, _, Some(f)) if on_path("john") => CrackTool::John(f),
            _ => {
                self.set_status(format!("✗ no cracker for {} — install hashcat or john", kind.name));
                return;
            }
        };
        // john wants login:hash; NetNTLM lines already carry their user.
        let body = crate::hashid::crackable(&c.hash);
        let body = match tool {
            CrackTool::John(_) if !kind.name.starts_with("NetNTLM") => format!("pivt:{body}"),
            _ => body,
        };
        if let Err(e) = std::fs::write(&hash_file, format!("{body}\n")) {
            self.set_status(format!("✗ can't write hash file: {e}"));
            return;
        }
        let q = |p: &std::path::Path| format!("'{}'", p.display().to_string().replace('\'', "'\\''"));
        let (hf, of, wl) = (q(&hash_file), q(&out_file), q(std::path::Path::new(&wordlist)));
        // --show after the attack also covers hashes already in the potfile.
        let (cmd, how) = match tool {
            CrackTool::Hashcat(m) => (
                format!("hashcat -m {m} -a 0 {hf} {wl}; hashcat -m {m} {hf} --show --outfile-format 2 > {of}"),
                format!("hashcat -m {m}"),
            ),
            CrackTool::John(f) => (
                format!("john --format={f} --wordlist={wl} {hf}; john --show --format={f} {hf} > {of}"),
                format!("john --format={f}"),
            ),
        };
        let who = if c.user.is_empty() { "hash".to_string() } else { c.user.clone() };
        let title = format!("crack {who} ({})", kind.name);
        let (rows, cols) = crate::runner::pane_size(self.run_fullscreen);
        let cwd = self.active_room.as_ref().map(|r| r.path.clone());
        match RunJob::spawn(title.clone(), cmd.clone(), rows, cols, cwd.as_deref()) {
            Ok(job) => {
                if self.logging {
                    let target = self.vars.get("RHOST").cloned().unwrap_or_default();
                    writeup::log_command(self.active_room.as_ref(), &target, &title, "crack", &cmd);
                }
                self.runs.push(job);
                self.cracks.push(PendingCrack { title, hash: c.hash.clone(), out: out_file, john: matches!(tool, CrackTool::John(_)) });
                self.run_tab = self.runs.len() - 1;
                self.show_run = true;
                self.mode = Mode::Normal;
                self.focus = Focus::Run;
                self.set_status(format!("⬢ cracking {} with {how} · {wordlist}", kind.name));
            }
            Err(e) => self.set_status(format!("✗ spawn failed: {e}")),
        }
    }

    /// Resolve finished crack tabs: read the plaintext hashcat wrote and store
    /// it on the matching loot entry. Tabs the user closed are dropped.
    fn poll_cracks(&mut self) {
        if self.cracks.is_empty() {
            return;
        }
        let mut still = Vec::new();
        for p in std::mem::take(&mut self.cracks) {
            match self.runs.iter().find(|j| j.title == p.title) {
                Some(j) if j.is_running() => still.push(p),
                Some(_) => self.finish_crack(&p),
                None => {} // tab closed before it finished
            }
        }
        self.cracks = still;
    }

    fn finish_crack(&mut self, p: &PendingCrack) {
        let text = std::fs::read_to_string(&p.out).unwrap_or_default();
        let plain = if p.john {
            // `john --show`: "pivt:<plain>" or "user:<plain>:…" (NetNTLM), then a summary line.
            text.lines()
                .find(|l| l.contains(':') && !l.contains("password hash"))
                .and_then(|l| match l.strip_prefix("pivt:") {
                    Some(rest) => Some(rest.to_string()),
                    None => l.split(':').nth(1).map(String::from),
                })
        } else {
            text.lines().map(str::trim_end).find(|l| !l.is_empty()).map(String::from)
        };
        let Some(plain) = plain else {
            self.set_status(format!("✗ {} — no hit (not in wordlist, or see the tab for errors)", p.title));
            return;
        };
        let Some(idx) = self.loot.iter().position(|c| c.hash == p.hash) else { return };
        self.loot[idx].pass = plain.clone();
        let user = self.loot[idx].user.clone();
        self.loot_persist();
        self.vars.insert("PASS".into(), plain.clone());
        if !user.is_empty() {
            self.vars.insert("USER".into(), user.clone());
        }
        self.persist_config();
        let who = if user.is_empty() { "hash".to_string() } else { user };
        if self.logging {
            let target = self.vars.get("RHOST").cloned().unwrap_or_default();
            writeup::log_note(self.active_room.as_ref(), &target, &format!("🔓 cracked {who}: `{plain}`"));
        }
        self.set_status(format!("🔓 cracked {who}: {plain} → PASS set"));
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
        self.flags_captured += 1;
        self.burst_start = Some(self.tick);
        self.burst_label = format!("{} PWNED", which.to_uppercase());
        self.set_status(format!("🚩 {which} flag saved + copied"));
        self.mode = Mode::Normal;
    }

    pub const BURST_LEN: u64 = 30;
    /// Burst progress 0..1, or None when inactive.
    pub fn burst_progress(&self) -> Option<f32> {
        let start = self.burst_start?;
        let d = self.tick.saturating_sub(start);
        if d >= Self::BURST_LEN {
            None
        } else {
            Some(d as f32 / Self::BURST_LEN as f32)
        }
    }

    // ── gtfobins ──
    pub fn open_gtfo(&mut self) {
        self.gtfo_query.clear();
        self.gtfo_idx = 0;
        self.mode = Mode::Gtfo;
    }
    pub fn gtfo_matches(&self) -> Vec<usize> {
        use crate::gtfobins::GTFOBINS;
        if self.gtfo_query.is_empty() {
            return (0..GTFOBINS.len()).collect();
        }
        let mut scored: Vec<(i64, usize)> = GTFOBINS
            .iter()
            .enumerate()
            .filter_map(|(i, g)| {
                let hay = format!("{} {} {}", g.bin, g.kind, g.code);
                self.matcher.fuzzy_match(&hay, &self.gtfo_query).map(|s| (s, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, i)| i).collect()
    }
    pub fn gtfo_push(&mut self, c: char) {
        self.gtfo_query.push(c);
        self.gtfo_idx = 0;
    }
    pub fn gtfo_pop(&mut self) {
        self.gtfo_query.pop();
        self.gtfo_idx = 0;
    }
    pub fn gtfo_next(&mut self) {
        let n = self.gtfo_matches().len();
        if n > 0 {
            self.gtfo_idx = (self.gtfo_idx + 1) % n;
        }
    }
    pub fn gtfo_prev(&mut self) {
        let n = self.gtfo_matches().len();
        if n > 0 {
            self.gtfo_idx = (self.gtfo_idx + n - 1) % n;
        }
    }
    pub fn gtfo_copy(&mut self) {
        use crate::gtfobins::GTFOBINS;
        let m = self.gtfo_matches();
        if let Some(&i) = m.get(self.gtfo_idx) {
            let code = GTFOBINS[i].code;
            match clipboard::copy(code) {
                Ok(_) => self.set_status(format!("✓ copied: {code}")),
                Err(e) => self.set_status(format!("✗ copy failed: {e}")),
            }
        }
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

/// Map an nmap service name to lowercase keywords matched against command
/// categories/tags/names.
fn service_keywords(svc: &str) -> Vec<&'static str> {
    let s = svc.to_lowercase();
    let mut k = Vec::new();
    if s.contains("http") {
        k.push("web");
    }
    if s.contains("microsoft-ds") || s.contains("netbios") || s.contains("smb") {
        k.push("smb");
    }
    if s.contains("ssh") {
        k.push("ssh");
    }
    if s.contains("ldap") {
        k.push("ldap");
        k.push("ad");
    }
    if s.contains("kerberos") {
        k.push("ad");
    }
    if s.contains("msrpc") || s.contains("rpc") {
        k.push("rpc");
    }
    if s.contains("domain") {
        k.push("dns");
    }
    if s.contains("mysql") {
        k.push("mysql");
    }
    if s.contains("ms-sql") || s.contains("mssql") {
        k.push("mssql");
    }
    if s.contains("postgres") {
        k.push("postgres");
    }
    if s.contains("snmp") {
        k.push("snmp");
    }
    if s.contains("ftp") {
        k.push("ftp");
    }
    if s.contains("winrm") {
        k.push("winrm");
    }
    k
}

fn port_keywords(port: &str) -> Vec<&'static str> {
    match port {
        "80" | "8080" | "443" | "8443" => vec!["web"],
        "445" | "139" => vec!["smb"],
        "22" => vec!["ssh"],
        "389" | "636" => vec!["ldap", "ad"],
        "88" => vec!["ad"],
        "53" => vec!["dns"],
        "3306" => vec!["mysql"],
        "1433" => vec!["mssql"],
        "161" => vec!["snmp"],
        "21" => vec!["ftp"],
        "5985" | "5986" => vec!["winrm"],
        _ => vec![],
    }
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
