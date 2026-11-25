use crate::discord::{DiscordRpc, PresenceInput, Privacy};

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
use crate::{clipboard, netif, session, writeup};
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
    Htb,
}

/// Text being typed inside the HTB panel.
pub enum HtbInput {
    /// Machine name or id to load.
    Machine(String),
    /// Answer / flag for the selected task.
    Answer(String),
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
/// Ticks between session change checks (~2s) and forced output saves (~30s).
const SESSION_CHECK_TICKS: u64 = 40;
const SESSION_FULL_TICKS: u64 = 600;
/// Ticks between checks that LHOST is still a local address (~10s).
const LHOST_CHECK_TICKS: u64 = 200;

/// A hashcat run started from the loot view, waiting for its tab to exit.
pub struct PendingCrack {
    /// The crack tab's [`RunJob::id`] (titles repeat across retries).
    pub job_id: u64,
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

/// Steps of sequential recipes, run one job at a time.
#[derive(Default)]
pub struct Sequence {
    /// Jobs of the step that is running now (a multi-step command has several).
    pub waiting: Vec<u64>,
    /// Key of the command those jobs belong to.
    pub current: Option<String>,
    pub queue: std::collections::VecDeque<Command>,
}

impl Sequence {
    fn has(&self, key: &str) -> bool {
        self.current.as_deref() == Some(key) || self.queue.iter().any(|c| cmd_key(c) == key)
    }
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
    /// "N jobs running — quit? y/N" is showing.
    pub confirm_quit: bool,
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
    /// Output dirs the last spawn created (for the status line).
    made_dirs: Vec<String>,
    pub interacting: bool, // keystrokes go to the focused job's PTY

    // loot / creds
    pub loot: Vec<Cred>,
    pub loot_idx: usize,
    /// hashcat runs started from the loot view, resolved when their tab exits.
    pub cracks: Vec<PendingCrack>,
    /// Sequential recipe steps still to run.
    pub sequence: Sequence,
    // recipes (R)
    pub recipes: Vec<crate::recipe::Recipe>,
    pub recipe_idx: usize,
    /// nmap service names from the last parsed scan (P), for recipe matching.
    pub scan_services: Vec<String>,
    // hack the box (H)
    pub htb: crate::htb::Htb,
    pub htb_idx: usize,
    pub htb_hint: bool,
    pub htb_input: Option<HtbInput>,
    /// Last IP HTB reported, so RHOST only follows HTB when HTB changes.
    htb_last_ip: Option<String>,
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
    /// LHOST isn't assigned to any local interface (header shows a warning).
    pub lhost_stale: bool,
    /// Re-check `lhost_stale` periodically (off for snapshots/tests).
    lhost_watch: bool,
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
    /// What the presence may reveal (config `discord_privacy`, `M` cycles).
    pub discord_privacy: Privacy,
    last_rpc_tick: u64,

    /// Rooms dir / template / target mirror, resolved from config + env.
    pub paths: Paths,
    /// The path settings as written in config.toml, saved back unchanged.
    path_cfg: (String, Option<String>, Option<String>),

    // persistent workspace session (src/session.rs); off for `pivt snap`
    pub session_on: bool,
    /// Dir the session was last written to (changes with the active room).
    session_dir: Option<PathBuf>,
    /// session.toml as last written (saved_at zeroed), to skip no-op saves.
    session_sig: String,
    /// Per-tab (job id, output length, finished) as last written.
    session_out: Vec<(u64, usize, bool)>,
    session_full_tick: u64,

    matcher: SkimMatcherV2,
}

impl App {
    pub fn new() -> Self {
        Self::build(load_commands(), load_config(), true)
    }

    /// For `pivt snap`: same data, but no Discord / pinger background threads.
    pub fn headless() -> Self {
        Self::build(load_commands(), load_config(), false)
    }

    /// Bundled commands, default config, no background threads (no user files read).
    #[cfg(test)]
    pub fn for_tests() -> Self {
        let mut a = Self::build(crate::model::load_bundled_for_tests(), Config::default(), false);
        a.logging = false;
        a.paths.target_file = None; // never touch the real shell target file
        a
    }

    /// `background` = start the Discord RPC and ping threads.
    fn build(commands: Vec<Command>, cfg: Config, background: bool) -> Self {
        let favorites = cfg.favorites.clone();
        let recent = cfg.recent.clone();
        let flow_bg = cfg.flow_bg;
        let paths = Paths::from_config(&cfg);
        let path_cfg = (cfg.rooms_dir.clone(), cfg.room_template.clone(), cfg.sync_target_file.clone());
        let discord_rpc = cfg.discord_rpc;
        let discord_app_id = if cfg.discord_app_id.trim().is_empty() {
            crate::model::DEFAULT_DISCORD_APP_ID.to_string()
        } else {
            cfg.discord_app_id.clone()
        };
        // Connect to Discord only when opted in and an app id is configured.
        let discord = if background && discord_rpc && !discord_app_id.is_empty() {
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
        // LHOST: fill when empty; follow a new VPN lease when the saved IP is
        // gone from every interface; never override a choice that's still live
        let ifaces = netif::list_ifaces();
        let saved = vars.get("LHOST").cloned().unwrap_or_default();
        let mut lhost_note = None;
        let mut lhost_changed = false;
        match netif::check_lhost(&saved, &ifaces) {
            netif::LhostCheck::Fill(_, ip) => {
                vars.insert("LHOST".into(), ip);
            }
            netif::LhostCheck::Replace(iface, ip) => {
                lhost_note = Some(format!("⇡ LHOST {saved} is gone — now {ip} ({iface})"));
                vars.insert("LHOST".into(), ip);
                lhost_changed = true;
            }
            netif::LhostCheck::Stale => {
                lhost_note = Some(format!("⚠ LHOST {saved} isn't on any interface — VPN down? (e → r re-detects)"));
            }
            netif::LhostCheck::Keep => {}
        }
        let lhost_stale = netif::is_local(vars.get("LHOST").map(String::as_str).unwrap_or(""), &ifaces) == Some(false);
        // IFACE (masscan -e): the interface carrying LHOST
        if vars.get("IFACE").map(|s| s.is_empty()).unwrap_or(true) {
            let lhost = vars.get("LHOST").cloned().unwrap_or_default();
            if let Some((n, _)) = ifaces.iter().find(|(_, ip)| *ip == lhost) {
                vars.insert("IFACE".into(), n.clone());
            }
        }
        if vars.get("LPORT").map(|s| s.is_empty()).unwrap_or(true) {
            vars.insert("LPORT".into(), "4444".into());
        }

        // ctf room + shared target: room .target > sync_target_file > config
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
            if let Some(ip) = paths.target_file.as_deref().and_then(read_ctf_target) {
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
            status: lhost_note.unwrap_or_else(|| "ready — type / to search, ? for help".into()),
            should_quit: false,
            confirm_quit: false,
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
            made_dirs: Vec::new(),
            interacting: false,
            loot,
            loot_idx: 0,
            cracks: Vec::new(),
            sequence: Sequence::default(),
            recipes: crate::recipe::load_all(),
            recipe_idx: 0,
            scan_services: Vec::new(),
            htb: crate::htb::Htb::new(),
            htb_idx: 0,
            htb_hint: false,
            htb_input: None,
            htb_last_ip: None,
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
            pinger: if background { Pinger::start() } else { Pinger::idle() },
            lhost_stale,
            lhost_watch: background,
            box_started: None,
            box_epoch: None,
            last_rhost: String::new(),
            flags_captured: 0,
            discord,
            discord_rpc,
            discord_app_id,
            discord_link: cfg.discord_link.clone(),
            discord_privacy: Privacy::parse(&cfg.discord_privacy),
            last_rpc_tick: 0,
            paths,
            path_cfg,
            session_on: false,
            session_dir: None,
            session_sig: String::new(),
            session_out: Vec::new(),
            session_full_tick: 0,
            matcher: SkimMatcherV2::default(),
        };
        app.refresh_categories();
        app.clamp();
        app.pinger.set_ports(&app.port_list());
        if lhost_changed && background {
            app.persist_config();
        }
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
        self.poll_sequence();
        self.poll_htb();
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
        if self.lhost_watch && self.tick.is_multiple_of(LHOST_CHECK_TICKS) {
            self.refresh_lhost_stale();
        }
        if self.session_on && self.tick.is_multiple_of(SESSION_CHECK_TICKS) {
            let full = self.tick.saturating_sub(self.session_full_tick) >= SESSION_FULL_TICKS;
            self.save_session(full);
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
    /// [`RPC_ROTATE_TICKS`]). What gets in at all depends on
    /// [`Self::discord_privacy`]; see [`crate::discord::build_presence`].
    fn push_discord(&self) {
        let Some(d) = &self.discord else { return };
        d.update(crate::discord::build_presence(&self.presence_input(), self.discord_privacy));
    }

    /// Everything the presence could show, before the privacy filter.
    fn presence_input(&self) -> PresenceInput {
        let htb = self.htb.snapshot();
        let htb_line = htb.machine.as_ref().map(|m| {
            let mut line = format!("HTB {} · {}", m.name, m.difficulty);
            let guided: Vec<_> = htb.tasks.iter().filter(|t| t.kind == crate::htb::TaskKind::Task).collect();
            if !guided.is_empty() {
                let done = guided.iter().filter(|t| t.completed).count();
                line.push_str(&format!(" · {done}/{} tasks", guided.len()));
            }
            line
        });
        let public_box = crate::discord::public_box(
            self.active_room.as_ref().map(|r| (r.name.as_str(), r.platform())),
            htb.machine.as_ref().map(|m| m.name.as_str()),
        );
        // small badge = current category's glyph, served from the repo
        let badge = self.current_category().map(|cat| {
            let file = if crate::theme::has_category_style(cat) { cat } else { "default" };
            (format!("{PRESENCE_IMG_BASE}/{file}.png"), cat.to_string())
        });
        let link = self.discord_link.trim();
        PresenceInput {
            room: self.active_room.as_ref().map(|r| r.name.clone()),
            public_box,
            flags: self.flags_captured,
            running_jobs: self.runs.iter().filter(|j| j.is_running()).count(),
            loot: self.loot.len(),
            category: self.current_category().map(str::to_string),
            badge,
            htb_line,
            rhost: self.vars.get("RHOST").cloned().unwrap_or_default(),
            rhost_up: self.ping().up,
            elapsed: self.box_elapsed(),
            ports: self.vars.get("PORTS").cloned().unwrap_or_default(),
            start_epoch: self.box_epoch,
            step: (self.tick / RPC_ROTATE_TICKS) as usize,
            link: if link.is_empty() { crate::model::REPO_URL } else { link }.to_string(),
        }
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
                let m = self.discord_privacy;
                self.set_status(format!("discord rich presence: on · {} ({}) · M cycles", m.as_str(), m.describe()));
            }
        } else {
            self.discord = None; // Drop clears the activity + closes the pipe
            self.set_status("discord rich presence: off");
        }
    }

    /// Cycle what the presence reveals: safe → full → minimal (persisted).
    pub fn cycle_discord_privacy(&mut self) {
        self.discord_privacy = self.discord_privacy.next();
        self.persist_config();
        self.last_rpc_tick = 0; // re-push now, so a downgrade takes effect at once
        let m = self.discord_privacy;
        let off = if self.discord_rpc { "" } else { " · presence is off (D)" };
        self.set_status(format!("discord privacy: {} — {}{off}", m.as_str(), m.describe()));
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
            Ok(how) => {
                let mut msg = format!("✓ copied ({})", how.label());
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
                        None,
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
            discord_privacy: self.discord_privacy.as_str().to_string(),
            rooms_dir: self.path_cfg.0.clone(),
            room_template: self.path_cfg.1.clone(),
            sync_target_file: self.path_cfg.2.clone(),
        };
        let _ = save_config(&cfg);
        // keep the shell target file (if enabled) + room .target in sync with RHOST
        if let Some(rhost) = self.vars.get("RHOST") {
            if !rhost.is_empty() {
                if let Some(t) = &self.paths.target_file {
                    write_ctf_target(t, rhost);
                }
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
            let lhost = k == "LHOST";
            self.vars.insert(k, self.var_buf.clone());
            self.persist_config();
            if lhost {
                self.refresh_lhost_stale();
            }
        }
        self.var_editing = false;
    }
    /// Recompute the header's "LHOST is not a local address" warning.
    pub fn refresh_lhost_stale(&mut self) {
        let lhost = self.vars.get("LHOST").cloned().unwrap_or_default();
        self.lhost_stale = !lhost.is_empty() && netif::is_local(&lhost, &netif::list_ifaces()) == Some(false);
    }
    pub fn vars_detect_lhost(&mut self) {
        if let Some(ip) = netif::detect_lhost() {
            self.vars.insert("LHOST".into(), ip.clone());
            self.persist_config();
            self.lhost_stale = false;
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
        let mut cmd = Command {
            category: cat.to_string(),
            name: name.to_string(),
            description: self.add.description.trim().to_string(),
            template: tmpl.to_string(),
            tags: vec![],
            steps: vec![],
            side: Default::default(),
            raw_pty: false,
            user: true,
        };
        // overriding an existing command keeps where it runs
        if let Some(old) = self.commands.iter().find(|c| c.category == cmd.category && c.name == cmd.name) {
            cmd.side = old.side;
            cmd.raw_pty = old.raw_pty;
        }
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
        self.rooms = room::list_rooms(&self.paths.rooms_dir);
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
            self.leave_room_session();
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
    /// Mark the highlighted room as a THM room, an HTB machine, or neither.
    /// Only marked rooms are named in the safe Discord presence.
    pub fn rooms_cycle_platform(&mut self) {
        use crate::room::Platform;
        let Some(r) = self.rooms.get(self.room_idx).cloned() else { return };
        let next = match r.platform() {
            None => Some(Platform::Thm),
            Some(Platform::Thm) => Some(Platform::Htb),
            Some(Platform::Htb) => None,
        };
        match r.set_platform(next) {
            Ok(()) => {
                self.last_rpc_tick = 0;
                self.set_status(match next {
                    Some(p) => format!("room '{}' marked {} (public box: named in Discord safe mode)", r.name, p.label()),
                    None => format!("room '{}' unmarked (private: never named in Discord safe mode)", r.name),
                });
            }
            Err(e) => self.set_status(format!("✗ couldn't mark room: {e}")),
        }
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
        match room::scaffold(&self.paths.rooms_dir, self.paths.room_template.as_deref(), name, &ip, &host) {
            Ok(r) => {
                // named after the machine loaded in the HTB panel: it's that box
                if self.htb.snapshot().machine.is_some_and(|m| m.name.eq_ignore_ascii_case(&r.name)) {
                    let _ = r.set_platform(Some(crate::room::Platform::Htb));
                }
                self.leave_room_session();
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

    // ── quitting ──
    /// Run tabs whose process is still alive (quitting would kill them).
    pub fn running_jobs(&self) -> usize {
        self.runs.iter().filter(|j| j.is_running()).count()
    }

    /// Quit now, or ask first while jobs (shells, scans, cracks) are running.
    pub fn request_quit(&mut self) {
        if self.running_jobs() > 0 {
            self.confirm_quit = true;
        } else {
            self.should_quit = true;
        }
    }

    /// Answer the quit prompt: `yes` quits, anything else goes back.
    pub fn answer_quit(&mut self, yes: bool) {
        self.confirm_quit = false;
        if yes {
            self.should_quit = true;
        }
    }

    // ── run pane ──
    /// Execute the selected command's step(s), each in its own tab.
    pub fn run_selected(&mut self) {
        let Some(cmd) = self.selected_cmd().cloned() else { return };
        if cmd.is_target() {
            self.set_status(format!("⛔ {TARGET_SIDE_MSG}"));
            return;
        }
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
        let made = if self.made_dirs.is_empty() {
            String::new()
        } else {
            format!(" · created {}", self.made_dirs.join(" "))
        };
        self.set_status(format!(
            "▶ running {} ({spawned} tab{}){made} · i to interact",
            cmd.name,
            if spawned == 1 { "" } else { "s" }
        ));
    }

    /// Spawn every step of `cmd` in its own run tab (logging each), and push it
    /// to recent. Returns how many tabs started. Callers check missing vars.
    fn spawn_cmd(&mut self, cmd: &Command) -> usize {
        // never execute victim-side payloads on this machine
        if cmd.is_target() {
            self.set_status(format!("⛔ {TARGET_SIDE_MSG}"));
            return 0;
        }
        let steps = cmd.run_steps();
        let multi = steps.len() > 1;
        let (rows, cols) = crate::runner::pane_size(self.run_fullscreen);
        let cwd = self.active_room.as_ref().map(|r| r.path.clone());
        let mut spawned = 0;
        let mut made: Vec<String> = Vec::new();
        for (i, step) in steps.iter().enumerate() {
            let filled = fill_template(step, &self.vars);
            // `-oN nmap/…` etc. need their dir, in the room or wherever pivt runs
            if let Some(dir) = cwd.clone().or_else(|| std::env::current_dir().ok()) {
                made.extend(crate::runner::prepare_output_dirs(&filled, &dir));
            }
            let title = if multi {
                format!("{} [{}]", cmd.name, i + 1)
            } else {
                cmd.name.clone()
            };
            let log = self.job_log(&title);
            match RunJob::spawn_logged(title.clone(), cmd.wrap_for_run(&filled), rows, cols, cwd.as_deref(), log) {
                Ok(job) => {
                    if self.logging {
                        let target = self.vars.get("RHOST").cloned().unwrap_or_default();
                        writeup::log_command(self.active_room.as_ref(), &target, &title, &cmd.category, &filled, job.log.as_deref());
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
        self.made_dirs = made;
        spawned
    }

    /// Show the run pane focused on the first tab spawned at `first_new`.
    fn show_runs_from(&mut self, first_new: usize) {
        self.show_run = true;
        self.run_tab = first_new;
        self.focus = Focus::Run;
        self.interacting = false;
    }

    // ── hack the box ──
    pub fn open_htb(&mut self) {
        self.mode = Mode::Htb;
        self.htb_input = None;
        if !self.htb.has_token() {
            return; // panel explains how to add one
        }
        if self.htb.snapshot().machine.is_none() && !self.htb.is_busy() {
            let room = self.active_room.as_ref().map(|r| r.name.clone());
            self.htb.load(None, room);
        }
    }
    pub fn htb_next(&mut self) {
        let n = self.htb.snapshot().tasks.len();
        if n > 0 {
            self.htb_idx = (self.htb_idx + 1) % n;
            self.htb_hint = false;
        }
    }
    pub fn htb_prev(&mut self) {
        let n = self.htb.snapshot().tasks.len();
        if n > 0 {
            self.htb_idx = (self.htb_idx + n - 1) % n;
            self.htb_hint = false;
        }
    }
    /// Start typing an answer for the selected task.
    pub fn htb_answer_begin(&mut self) {
        let st = self.htb.snapshot();
        let Some(t) = st.tasks.get(self.htb_idx) else { return };
        if t.completed {
            self.set_status(format!("✓ {} already done", t.title));
        } else if st.locked(t) {
            self.set_status("🔒 finish the previous task first");
        } else {
            self.htb_input = Some(HtbInput::Answer(String::new()));
        }
    }
    pub fn htb_input_submit(&mut self) {
        match self.htb_input.take() {
            Some(HtbInput::Machine(name)) if !name.trim().is_empty() => {
                self.htb_idx = 0;
                self.htb.load(Some(name.trim().to_string()), None);
            }
            Some(HtbInput::Answer(ans)) if !ans.trim().is_empty() => {
                if let Some(t) = self.htb.snapshot().tasks.get(self.htb_idx).cloned() {
                    self.htb.submit(t, ans.trim().to_string());
                }
            }
            _ => {}
        }
    }
    pub fn htb_input_buf(&mut self) -> Option<&mut String> {
        match self.htb_input.as_mut()? {
            HtbInput::Machine(b) | HtbInput::Answer(b) => Some(b),
        }
    }

    /// React to HTB results: flag bursts + notes, task log lines, RHOST sync.
    fn poll_htb(&mut self) {
        let target = self.vars.get("RHOST").cloned().unwrap_or_default();
        for ev in self.htb.take_events() {
            match ev {
                crate::htb::HtbEvent::Owned { which, value } => {
                    writeup::set_flag(self.active_room.as_ref(), &target, which, &value);
                    self.flags_captured += 1;
                    self.burst_start = Some(self.tick);
                    self.burst_label = format!("{} PWNED", which.to_uppercase());
                    self.set_status(format!("🚩 HTB accepted the {which} flag"));
                }
                crate::htb::HtbEvent::TaskSolved { title, question, answer } => {
                    if self.logging {
                        let q = if question.is_empty() { String::new() } else { format!(" — {question}") };
                        writeup::log_note(self.active_room.as_ref(), &target, &format!("🧩 {title}{q} → `{answer}`"));
                    }
                    self.set_status(format!("✓ {title} correct"));
                }
            }
        }
        // Follow the machine's IP into RHOST when HTB reports a new one.
        let ip = self.htb.snapshot().machine.and_then(|m| m.ip);
        if ip.is_some() && ip != self.htb_last_ip {
            self.htb_last_ip = ip.clone();
            let ip = ip.unwrap_or_default();
            if self.vars.get("RHOST") != Some(&ip) {
                self.vars.insert("RHOST".into(), ip.clone());
                self.persist_config();
                self.set_status(format!("◎ RHOST = {ip} (from HTB)"));
            }
        }
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
        let mut queued = 0;
        let mut skipped: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        for r in recipes {
            for step in &r.steps {
                if seen.contains(step) || self.sequence.has(step) {
                    continue; // same command in two recipes / already queued: run once
                }
                seen.push(step.clone());
                let Some(cmd) = self.command_for_key(step).cloned() else {
                    skipped.push(format!("{step} (unknown)"));
                    continue;
                };
                if cmd.is_target() {
                    skipped.push(format!("{} (target-side, copy it)", cmd.name));
                    continue;
                }
                let missing = self.missing_vars(&cmd);
                if !missing.is_empty() {
                    skipped.push(format!("{} (needs {})", cmd.name, missing.join(",")));
                    continue;
                }
                if r.sequential {
                    // all sequential steps share one queue, so stacking
                    // recipes (A) never runs two of them at once
                    if self.sequence.current.is_some() {
                        self.sequence.queue.push_back(cmd);
                        queued += 1;
                    } else {
                        let n = self.spawn_sequence_step(&cmd);
                        started += n;
                    }
                } else {
                    started += self.spawn_cmd(&cmd);
                }
            }
        }
        let names = recipes.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(", ");
        if started == 0 && queued > 0 {
            self.mode = Mode::Normal;
            self.set_status(format!("⏳ {names}: {queued} step{} queued after the running scan", if queued == 1 { "" } else { "s" }));
            return;
        }
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
        let waiting = self.sequence.queue.len();
        if waiting > 0 {
            msg.push_str(&format!(" · {waiting} queued, one at a time (K stops the sequence)"));
        }
        if !skipped.is_empty() {
            msg.push_str(&format!(" · skipped {}", skipped.join("; ")));
        }
        self.set_status(msg);
    }

    /// Spawn `cmd` as the running step of the sequence. Returns tabs started.
    fn spawn_sequence_step(&mut self, cmd: &Command) -> usize {
        let before = self.runs.len();
        let n = self.spawn_cmd(cmd);
        if n > 0 {
            self.sequence.waiting = self.runs[before..].iter().map(|j| j.id).collect();
            self.sequence.current = Some(cmd_key(cmd));
        }
        n
    }

    /// Start the next sequential step once every job of the current one has
    /// exited. A step that was killed (K) or closed while running stops the
    /// whole sequence.
    fn poll_sequence(&mut self) {
        if self.sequence.current.is_none() {
            return;
        }
        let mut stopped = false;
        for id in &self.sequence.waiting {
            match self.runs.iter().find(|j| j.id == *id) {
                Some(j) if j.is_running() => return,
                Some(j) if j.exit == Some(-1) => stopped = true,
                Some(_) => {}
                None => stopped = true,
            }
        }
        self.sequence.waiting.clear();
        self.sequence.current = None;
        if stopped {
            let n = self.sequence.queue.len();
            self.sequence.queue.clear();
            if n > 0 {
                self.set_status(format!("⏹ sequence stopped — {n} queued step{} dropped", if n == 1 { "" } else { "s" }));
            }
            return;
        }
        while let Some(cmd) = self.sequence.queue.pop_front() {
            // vars may have changed since it was queued
            let missing = self.missing_vars(&cmd);
            if !missing.is_empty() {
                self.set_status(format!("⚠ skipped {} (needs {})", cmd.name, missing.join(",")));
                continue;
            }
            if self.spawn_sequence_step(&cmd) > 0 {
                let left = self.sequence.queue.len();
                self.set_status(format!("▶ next in sequence: {} · {left} left", cmd.name));
                return;
            }
        }
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

    /// Re-run the active tab's command in place (restored or finished tabs).
    pub fn run_rerun_active(&mut self) {
        let Some(job) = self.runs.get(self.run_tab) else {
            self.set_status("no run tab to re-run");
            return;
        };
        if job.is_running() {
            self.set_status("still running — K to kill it first");
            return;
        }
        let (title, cmd) = (job.title.clone(), job.cmd.clone());
        // a tab from an older session could hold a target-side command
        if self.is_target_tab(&title) {
            self.set_status(format!("⛔ {TARGET_SIDE_MSG}"));
            return;
        }
        // the room dir may be gone since the session was saved
        let cwd = job.cwd.clone().filter(|d| d.is_dir());
        if let Some(dir) = cwd.clone().or_else(|| std::env::current_dir().ok()) {
            crate::runner::prepare_output_dirs(&cmd, &dir);
        }
        let (rows, cols) = crate::runner::pane_size(self.run_fullscreen);
        let log = self.job_log(&title);
        match RunJob::spawn_logged(title.clone(), cmd.clone(), rows, cols, cwd.as_deref(), log) {
            Ok(new) => {
                if self.logging {
                    let target = self.vars.get("RHOST").cloned().unwrap_or_default();
                    writeup::log_command(self.active_room.as_ref(), &target, &title, "rerun", &cmd, new.log.as_deref());
                }
                self.runs[self.run_tab] = new;
                self.interacting = false;
                self.set_status(format!("↻ re-running {title} · i to interact"));
            }
            Err(e) => self.set_status(format!("✗ spawn failed: {e}")),
        }
    }

    /// Where a new job's output is teed (`<room>/logs/…` or the data dir),
    /// unless logging is off (PIVT_NO_LOG).
    fn job_log(&self, title: &str) -> Option<PathBuf> {
        self.logging.then(|| writeup::job_log_path(self.active_room.as_ref(), title))
    }

    /// Whether a run tab titled `title` ("name" or "name [n]") belongs to a
    /// target-side command.
    fn is_target_tab(&self, title: &str) -> bool {
        let base = match title.rsplit_once(" [") {
            Some((b, rest)) if rest.ends_with(']') => b,
            _ => title,
        };
        self.commands.iter().any(|c| c.is_target() && (c.name == title || c.name == base))
    }

    // ── persistent session ──
    /// Turn sessions on and bring back the last workspace (unless `fresh`).
    /// The room comes from the cwd if pivt was started inside one, else from
    /// the last active room.
    pub fn restore_session(&mut self, fresh: bool) {
        self.session_on = true;
        if fresh {
            self.set_status("fresh start — previous session not restored");
            return;
        }
        if self.active_room.is_none() {
            if let Some(path) = session::read_last_room().filter(|p| p.join(".ctfroom").exists()) {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                self.enter_room(Room { name, path });
            }
        }
        let dir = session::dir_for(self.active_room.as_ref());
        self.session_dir = Some(dir.clone());
        match session::load(&dir) {
            Ok(Some(s)) => self.apply_session(s),
            Ok(None) => {}
            Err(e) => self.set_status(format!("⚠ session ignored: {e}")),
        }
    }

    /// Make `r` the active room the way startup does: target + loot follow it.
    fn enter_room(&mut self, r: Room) {
        if let Some((ip, host)) = r.read_target() {
            self.vars.insert("RHOST".into(), ip);
            if let Some(h) = host {
                self.vars.insert("DOMAIN".into(), h);
            }
        }
        self.active_room = Some(r);
        self.loot = loot::load(&self.loot_file());
        self.loot_idx = 0;
    }

    fn apply_session(&mut self, s: session::Session) {
        self.suggested = s.suggested;
        self.scan_services = s.scan_services;
        self.refresh_categories();
        self.search.clear();
        self.cat_idx = self.categories.iter().position(|c| *c == s.category).unwrap_or(0);
        self.cmd_idx = 0;
        if let Some(ci) = self.command_by_key(&s.command) {
            let pos = self.visible().iter().position(|&i| i == ci);
            match pos {
                Some(p) => self.cmd_idx = p,
                None => {
                    // not in that category (e.g. picked from search): go to its own
                    let cat = self.commands[ci].category.clone();
                    if let Some(c) = self.categories.iter().position(|x| *x == cat) {
                        self.cat_idx = c;
                        self.cmd_idx = self.visible().iter().position(|&i| i == ci).unwrap_or(0);
                    }
                }
            }
        }
        self.clamp();

        let (rows, cols) = crate::runner::pane_size(s.run_fullscreen);
        let n = s.tabs.len();
        for t in s.tabs {
            let text = t.output.unwrap_or_default();
            let exit = if t.running { None } else { t.exit };
            self.runs.push(RunJob::restored(t.title, t.cmd, t.cwd, &text, t.running, exit, rows, cols));
        }
        self.run_tab = s.run_tab.min(self.runs.len().saturating_sub(1));
        self.show_run = s.show_run && !self.runs.is_empty();
        self.run_fullscreen = s.run_fullscreen && self.show_run;
        self.focus = match s.focus.as_str() {
            "commands" => Focus::Commands,
            "preview" => Focus::Preview,
            "run" if self.show_run => Focus::Run,
            "run" => Focus::Commands,
            _ => Focus::Categories,
        };
        // what we just loaded is what's on disk
        self.session_sig = self.session_toml(&self.build_session(false));
        self.session_out = self.runs.iter().map(|j| (j.id, j.raw_len(), j.finished)).collect();
        self.session_full_tick = self.tick;

        let when = session::age(s.saved_at, crate::discord::now_unix());
        let mut msg = format!("↺ restored session ({when})");
        if n > 0 {
            msg.push_str(&format!(" · {n} tab{} · r re-runs in the run pane", if n == 1 { "" } else { "s" }));
        }
        self.set_status(msg);
    }

    /// Snapshot of the workspace. Tab output is attached only when `outputs`.
    fn build_session(&self, outputs: bool) -> session::Session {
        let skip = self.runs.len().saturating_sub(session::MAX_TABS);
        let tabs = self
            .runs
            .iter()
            .skip(skip)
            .enumerate()
            .map(|(i, j)| session::TabState {
                title: j.title.clone(),
                cmd: j.cmd.clone(),
                cwd: j.cwd.clone(),
                running: j.is_running() || j.restored == Some(true),
                exit: if j.is_running() { None } else { j.exit },
                file: session::tab_file(i),
                output: outputs.then(|| session::trim_scrollback(&j.raw_text(), session::MAX_LINES)),
            })
            .collect();
        let focus = match self.focus {
            Focus::Categories => "categories",
            Focus::Commands => "commands",
            Focus::Preview => "preview",
            Focus::Run => "run",
        };
        session::Session {
            version: session::VERSION,
            saved_at: 0,
            room: self.active_room.as_ref().map(|r| r.path.clone()),
            category: self.current_category().unwrap_or_default().to_string(),
            command: self.selected_cmd().map(cmd_key).unwrap_or_default(),
            focus: focus.into(),
            show_run: self.show_run,
            run_fullscreen: self.run_fullscreen,
            run_tab: self.run_tab.saturating_sub(skip),
            suggested: self.suggested.clone(),
            scan_services: self.scan_services.clone(),
            tabs,
        }
    }

    fn session_toml(&self, s: &session::Session) -> String {
        session::to_toml(s).unwrap_or_default()
    }

    /// Write the session if anything changed. Layout/selection changes are
    /// written right away; a running tab's growing output only when `full`
    /// (periodic + on quit). Finished tabs are written once they finish.
    pub fn save_session(&mut self, full: bool) {
        if !self.session_on {
            return;
        }
        let dir = session::dir_for(self.active_room.as_ref());
        if self.session_dir.as_ref() != Some(&dir) {
            // new location: write everything there
            self.session_dir = Some(dir.clone());
            self.session_sig.clear();
            self.session_out.clear();
        }
        let mut s = self.build_session(false);
        let sig = self.session_toml(&s);
        let skip = self.runs.len().saturating_sub(session::MAX_TABS);
        let fps: Vec<(u64, usize, bool)> =
            self.runs.iter().skip(skip).map(|j| (j.id, j.raw_len(), j.finished)).collect();
        let mut dirty_out = false;
        for (i, (t, fp)) in s.tabs.iter_mut().zip(&fps).enumerate() {
            let old = self.session_out.get(i);
            if old == Some(fp) {
                continue;
            }
            // a different job in this slot, or it just finished: write now;
            // a running job's growing output waits for the next full save
            let moved = old.map(|o| o.0) != Some(fp.0);
            let finished_now = old.map(|o| o.2) != Some(fp.2);
            if full || moved || finished_now {
                let raw: String = self.runs[skip + i].raw_text();
                t.output = Some(session::trim_scrollback(&raw, session::MAX_LINES));
                dirty_out = true;
            }
        }
        if full {
            self.session_full_tick = self.tick;
        }
        if sig == self.session_sig && !dirty_out {
            return;
        }
        s.saved_at = crate::discord::now_unix();
        match session::save(&dir, &s) {
            Ok(()) => {
                self.session_sig = sig;
                // only tabs whose output went out are up to date on disk
                let mut out = Vec::with_capacity(fps.len());
                for (i, fp) in fps.iter().enumerate() {
                    if s.tabs[i].output.is_some() {
                        out.push(*fp);
                    } else {
                        out.push(self.session_out.get(i).copied().unwrap_or((0, usize::MAX, false)));
                    }
                }
                self.session_out = out;
                session::write_last_room(self.active_room.as_ref().map(|r| r.path.as_path()));
            }
            Err(e) => self.set_status(format!("⚠ session save failed: {e}")),
        }
    }

    /// Before switching rooms: flush the workspace to the old room's session.
    fn leave_room_session(&mut self) {
        self.save_session(true);
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
        self.pinger.set_ports(&self.port_list());
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
                Ok(how) => self.set_status(format!("✓ copied {label} ({}): {val}", how.label())),
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
        if let Err(e) = create_private_dir(&dir) {
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
        // the cracker's `> out` keeps this 0600 file's mode
        if let Err(e) = write_private(&hash_file, format!("{body}\n")).and_then(|_| write_private(&out_file, "")) {
            self.set_status(format!("✗ can't write hash file: {e}"));
            return;
        }
        let q = |p: &std::path::Path| shell_quote(&p.display().to_string());
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
        let log = self.job_log(&title);
        match RunJob::spawn_logged(title.clone(), cmd.clone(), rows, cols, cwd.as_deref(), log) {
            Ok(job) => {
                if self.logging {
                    let target = self.vars.get("RHOST").cloned().unwrap_or_default();
                    writeup::log_command(self.active_room.as_ref(), &target, &title, "crack", &cmd, job.log.as_deref());
                }
                let job_id = job.id;
                self.runs.push(job);
                self.cracks.push(PendingCrack { job_id, title, hash: c.hash.clone(), out: out_file, john: matches!(tool, CrackTool::John(_)) });
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
            match self.runs.iter().find(|j| j.id == p.job_id) {
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
        // Start empty: USER/PASS vars are global and carry stale creds across
        // boxes, so pre-filling from them saved the wrong account.
        self.loot_form = LootForm::default();
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
        let copied = match clipboard::copy(&value) {
            Ok(how) => format!("copied ({})", how.label()),
            Err(_) => "copy failed".to_string(),
        };
        self.flags_captured += 1;
        self.burst_start = Some(self.tick);
        self.burst_label = format!("{} PWNED", which.to_uppercase());
        self.set_status(format!("🚩 {which} flag saved + {copied}"));
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
                Ok(how) => self.set_status(format!("✓ copied ({}): {code}", how.label())),
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
            if name == "LHOST" {
                self.refresh_lhost_stale();
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Point the selection at `category/name`.
    fn select(app: &mut App, key: &str) {
        let ci = app.command_by_key(key).unwrap_or_else(|| panic!("no command {key}"));
        let cat = app.commands[ci].category.clone();
        app.cat_idx = app.categories.iter().position(|c| *c == cat).unwrap();
        app.cmd_idx = app.visible().iter().position(|&i| i == ci).unwrap();
        assert_eq!(app.selected_cmd().map(cmd_key).as_deref(), Some(key));
    }

    #[test]
    fn x_refuses_target_side_commands() {
        let mut app = App::for_tests();
        for (k, v) in [("LHOST", "10.10.14.2"), ("LPORT", "4444"), ("FILE", "x"), ("RHOST", "10.10.10.10")] {
            app.vars.insert(k.into(), v.into());
        }
        for key in ["shells/bash tcp", "misc/sudo -l", "misc/linpeas pull+run", "win/winpeas pull+run", "upgrade/python pty spawn"] {
            select(&mut app, key);
            app.run_selected();
            assert!(app.runs.is_empty(), "{key} was spawned");
            assert!(app.status.contains(TARGET_SIDE_MSG), "{key}: status {:?}", app.status);
        }
        // and spawn_cmd (used by recipes) refuses on its own
        let c = app.commands[app.command_by_key("misc/find SUID").unwrap()].clone();
        assert_eq!(app.spawn_cmd(&c), 0);
        assert!(app.runs.is_empty());
    }

    #[test]
    fn recipes_skip_target_side_steps() {
        let mut app = App::for_tests();
        let r = crate::recipe::Recipe {
            name: "evil".into(),
            description: String::new(),
            ports: vec![],
            services: vec![],
            steps: vec!["misc/sudo -l".into(), "shells/busybox nc".into()],
            sequential: false,
        };
        app.run_recipes(&[r]);
        assert!(app.runs.is_empty());
        assert!(app.status.contains("target-side"), "status {:?}", app.status);
    }

    /// An app with local test commands `t/a`, `t/b`, `t/c` (`sleep`s).
    fn seq_app() -> App {
        let mut app = App::for_tests();
        for (n, t) in [("a", "sleep 0.3"), ("b", "sleep 0.3"), ("c", "true")] {
            app.commands.push(Command {
                category: "t".into(),
                name: n.into(),
                description: String::new(),
                template: t.into(),
                tags: vec![],
                steps: vec![],
                side: Side::Local,
                raw_pty: false,
                user: false,
            });
        }
        app
    }

    fn recipe(name: &str, steps: &[&str], sequential: bool) -> crate::recipe::Recipe {
        crate::recipe::Recipe {
            name: name.into(),
            description: String::new(),
            ports: vec![],
            services: vec![],
            steps: steps.iter().map(|s| s.to_string()).collect(),
            sequential,
        }
    }

    /// Poll jobs + the sequence until `n` tabs exist and all have finished.
    fn settle(app: &mut App, n: usize) {
        for _ in 0..200 {
            for r in &mut app.runs {
                r.poll();
            }
            app.poll_sequence();
            if app.runs.len() == n && app.runs.iter().all(|j| !j.is_running()) && app.sequence.current.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        panic!("didn't settle at {n} tabs: {} tabs, status {:?}", app.runs.len(), app.status);
    }

    #[test]
    fn sequential_recipe_runs_one_step_at_a_time() {
        let mut app = seq_app();
        app.run_recipes(&[recipe("seq", &["t/a", "t/b", "t/c"], true)]);
        assert_eq!(app.runs.len(), 1, "only the first step starts");
        assert_eq!(app.sequence.queue.len(), 2);
        assert!(app.status.contains("2 queued"), "{}", app.status);
        // the second step waits for the first to exit
        app.poll_sequence();
        assert_eq!(app.runs.len(), 1);
        // stacking (A) the same + another sequential recipe joins the queue,
        // without duplicates and without starting anything in parallel
        app.run_recipes(&[recipe("more", &["t/b", "t/c", "t/a"], true)]);
        assert_eq!(app.runs.len(), 1);
        assert_eq!(app.sequence.queue.len(), 2, "already queued/running steps aren't added twice");
        settle(&mut app, 3);
        let titles: Vec<_> = app.runs.iter().map(|j| j.title.as_str()).collect();
        assert_eq!(titles, ["a", "b", "c"]);
        // each started after the previous one ended: exits are all clean
        assert!(app.runs.iter().all(|j| j.exit == Some(0)));
    }

    #[test]
    fn parallel_recipe_starts_everything() {
        let mut app = seq_app();
        app.run_recipes(&[recipe("par", &["t/a", "t/b", "t/c"], false)]);
        assert_eq!(app.runs.len(), 3);
        assert!(app.sequence.current.is_none() && app.sequence.queue.is_empty());
        for j in &mut app.runs {
            j.kill();
        }
    }

    #[test]
    fn killing_a_sequential_step_stops_the_rest() {
        let mut app = seq_app();
        app.run_recipes(&[recipe("seq", &["t/a", "t/b", "t/c"], true)]);
        app.run_tab = 0;
        app.run_kill_active();
        app.poll_sequence();
        assert_eq!(app.runs.len(), 1, "nothing after a killed step");
        assert!(app.sequence.queue.is_empty() && app.sequence.current.is_none());
        assert!(app.status.contains("sequence stopped"), "{}", app.status);
    }

    #[test]
    fn rerun_refuses_restored_target_tab() {
        let mut app = App::for_tests();
        app.runs.push(RunJob::restored("sudo -l", "sudo -l", None, "", false, Some(0), 6, 30));
        app.run_tab = 0;
        app.run_rerun_active();
        assert!(app.status.contains(TARGET_SIDE_MSG));
        assert!(app.runs[0].restored.is_some(), "tab was replaced by a new run");
    }

    #[test]
    fn quit_asks_while_jobs_run() {
        let mut app = App::for_tests();
        app.request_quit();
        assert!(app.should_quit && !app.confirm_quit, "idle: quits at once");

        let mut app = App::for_tests();
        app.runs.push(RunJob::spawn("sleep", "sleep 5", 6, 30, None).unwrap());
        app.request_quit();
        assert!(app.confirm_quit && !app.should_quit);
        app.answer_quit(false);
        assert!(!app.confirm_quit && !app.should_quit);
        app.request_quit();
        app.answer_quit(true);
        assert!(app.should_quit);
        app.runs[0].kill();
    }

    #[test]
    fn crack_poll_follows_job_id_not_title() {
        let mut app = App::for_tests();
        let title = "crack admin (NTLM)";
        // an old, finished tab with the same title is still open
        let mut old = RunJob::restored(title, "hashcat …", None, "", false, Some(1), 6, 30);
        old.finished = true;
        app.runs.push(old);
        let retry = RunJob::spawn(title, "sleep 5", 6, 30, None).unwrap();
        let id = retry.id;
        app.runs.push(retry);
        app.cracks.push(PendingCrack {
            job_id: id,
            title: title.into(),
            hash: "31d6cfe0d16ae931b73c59d7e0c089c0".into(),
            out: std::env::temp_dir().join("pivt-test-nonexistent.out"),
            john: false,
        });
        app.poll_cracks();
        assert_eq!(app.cracks.len(), 1, "retry resolved against the old tab");
        assert_eq!(app.cracks[0].job_id, id);
        // the retry tab closing drops it
        app.runs.pop();
        app.poll_cracks();
        assert!(app.cracks.is_empty());
    }
}
