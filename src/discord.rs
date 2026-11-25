//! Discord Rich Presence ("Discord RPC").
//!
//! A [`DiscordRpc`] handle owns a background thread that connects (and
//! reconnects) to Discord's local IPC pipe and applies whatever presence the
//! UI last pushed. Mirrors the style of `ping.rs`: the UI thread only ever
//! touches an `Arc<Mutex<_>>`, so it never blocks on Discord.
//!
//! Robustness contract: the thread never panics out, no-ops gracefully when
//! Discord isn't running, and retries connecting periodically. When Discord is
//! absent the whole feature is a silent no-op.

use crate::room::Platform;
use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Seconds between reconnect attempts while Discord is unreachable.
const RETRY_SECS: u64 = 15;
/// Background loop granularity (keeps shutdown responsive).
const POLL: Duration = Duration::from_millis(500);
/// How long dropping the handle waits for the thread before detaching it.
const SHUTDOWN_WAIT: Duration = Duration::from_millis(300);

/// The presence the UI wants shown. Pushed to the background thread, which
/// applies it whenever it differs from what was last sent.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Presence {
    pub details: String,
    pub state: String,
    /// Unix epoch seconds the box timer started (drives Discord's elapsed clock).
    pub start_epoch: Option<i64>,
    pub large_image: String,
    pub large_text: String,
    /// Small corner badge: an asset key or an https URL (Discord proxies it).
    pub small_image: String,
    pub small_text: String,
    /// Up to two (label, url) buttons. Discord hides your own buttons from
    /// you; other people see them on your profile.
    pub buttons: Vec<(String, String)>,
}

/// How much the presence tells other people. Anyone who can see your
/// Discord profile sees it, so the default never names a target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Privacy {
    /// Platform + box name only for known public CTF boxes, else "on a box".
    /// Never an IP, ports or services.
    #[default]
    Safe,
    /// Everything: room, target IP + status, open ports, category, loot count.
    Full,
    /// Just "pivt".
    Minimal,
}

impl Privacy {
    /// Config value → mode; anything unknown falls back to `Safe`.
    pub fn parse(s: &str) -> Privacy {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" => Privacy::Full,
            "minimal" => Privacy::Minimal,
            _ => Privacy::Safe,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Privacy::Safe => "safe",
            Privacy::Full => "full",
            Privacy::Minimal => "minimal",
        }
    }
    pub fn next(self) -> Privacy {
        match self {
            Privacy::Safe => Privacy::Full,
            Privacy::Full => Privacy::Minimal,
            Privacy::Minimal => Privacy::Safe,
        }
    }
    pub fn describe(self) -> &'static str {
        match self {
            Privacy::Safe => "public box name only, no IP/ports",
            Privacy::Full => "room, target IP, ports, loot count",
            Privacy::Minimal => "just \"pivt\"",
        }
    }
}

/// A box known to be a public CTF machine, safe to name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicBox {
    pub platform: Platform,
    pub name: String,
}

impl PublicBox {
    pub fn url(&self) -> String {
        self.platform.url(&self.name)
    }
}

/// Decide whether the current box is a public CTF machine.
///
/// `room` = active room's (name, recorded platform); `htb` = machine loaded
/// in the HTB panel. The HTB machine counts when there's no room, the room is
/// marked HTB, or the room is named after it; otherwise a room counts only
/// when its `.ctfroom` records a platform. Anything else is unknown, which
/// may be a real engagement.
pub fn public_box(room: Option<(&str, Option<Platform>)>, htb: Option<&str>) -> Option<PublicBox> {
    if let Some(m) = htb {
        let fits = match room {
            None => true,
            Some((name, p)) => p == Some(Platform::Htb) || name.eq_ignore_ascii_case(m),
        };
        if fits {
            return Some(PublicBox { platform: Platform::Htb, name: m.to_string() });
        }
    }
    let (name, platform) = room?;
    Some(PublicBox { platform: platform?, name: name.to_string() })
}

/// Everything the presence may draw from. Built by the app each refresh; the
/// privacy mode decides what of it reaches Discord.
#[derive(Clone, Debug, Default)]
pub struct PresenceInput {
    pub room: Option<String>,
    pub public_box: Option<PublicBox>,
    pub flags: usize,
    pub running_jobs: usize,
    pub loot: usize,
    /// Current category (shown in full mode only: it hints at services).
    pub category: Option<String>,
    /// (image url, text) corner badge for the current category.
    pub badge: Option<(String, String)>,
    /// "HTB Lame · Easy · 2/5 tasks" when the HTB panel has a machine.
    pub htb_line: Option<String>,
    pub rhost: String,
    pub rhost_up: Option<bool>,
    pub elapsed: Option<String>,
    pub ports: String,
    pub start_epoch: Option<i64>,
    /// Rotation step: which candidate each text line shows.
    pub step: usize,
    /// "get pivt" button target.
    pub link: String,
}

/// Build the presence for `mode`. Pure, so the privacy rules are testable.
pub fn build_presence(i: &PresenceInput, mode: Privacy) -> Presence {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    let base = Presence {
        large_image: "pivt".to_string(),
        large_text: "pivt — ctf toolkit".to_string(),
        ..Presence::default()
    };
    if mode == Privacy::Minimal {
        return Presence { details: "pivt".to_string(), ..base };
    }
    let pick = |v: &[String]| if v.is_empty() { String::new() } else { v[i.step % v.len()].clone() };

    let mut buttons = Vec::new();
    if let Some(b) = &i.public_box {
        let label = if b.platform == Platform::Htb { "view machine" } else { "view room" };
        buttons.push((label.to_string(), b.url()));
    }
    buttons.push(("get pivt".to_string(), i.link.clone()));

    if mode == Privacy::Safe {
        let details = match (&i.public_box, &i.room) {
            (Some(b), _) => format!("{} · {}", b.platform.label(), b.name),
            (None, Some(_)) => "on a box".to_string(),
            (None, None) if !i.rhost.is_empty() => "on a box".to_string(),
            (None, None) => "browsing commands".to_string(),
        };
        let mut states = Vec::new();
        if i.public_box.is_some() && i.flags > 0 {
            states.push(format!("🚩 {} flag{}", i.flags, plural(i.flags)));
        }
        if i.running_jobs > 0 {
            states.push(format!("⚡ {} job{} running", i.running_jobs, plural(i.running_jobs)));
        }
        return Presence {
            details,
            state: pick(&states),
            start_epoch: i.start_epoch,
            buttons,
            ..base
        };
    }

    // full: everything (the pre-privacy behaviour)
    let mut details = vec![match &i.room {
        Some(r) => format!("{r} · {} flag{}", i.flags, plural(i.flags)),
        None => "browsing commands".to_string(),
    }];
    if i.running_jobs > 0 {
        details.push(format!("⚡ {} job{} running", i.running_jobs, plural(i.running_jobs)));
    }
    if i.loot > 0 {
        details.push(format!("🔑 {} cred{} looted", i.loot, plural(i.loot)));
    }
    if let Some(cat) = &i.category {
        let (glyph, _) = crate::theme::category_style(cat);
        details.push(format!("{glyph} in {cat}"));
    }
    if let Some(l) = &i.htb_line {
        details.push(l.clone());
    }
    let mut states = vec![if i.rhost.is_empty() {
        "no target set".to_string()
    } else {
        let status = match i.rhost_up {
            Some(true) => "up",
            Some(false) => "down",
            None => "…",
        };
        let mut s = format!("◎ {} {status}", i.rhost);
        if let Some(el) = &i.elapsed {
            s.push_str(&format!(" · ⏱ {el}"));
        }
        s
    }];
    if !i.ports.trim().is_empty() {
        let mut line = format!("open ports {}", i.ports);
        if line.chars().count() > 120 {
            line = line.chars().take(119).collect::<String>() + "…";
        }
        states.push(line);
    }
    let (small_image, small_text) = i.badge.clone().unwrap_or_default();
    Presence {
        details: pick(&details),
        state: pick(&states),
        start_epoch: i.start_epoch,
        small_image,
        small_text,
        buttons,
        ..base
    }
}

/// Current unix time in seconds.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Handle to the background RPC thread.
pub struct DiscordRpc {
    shared: Arc<Mutex<Option<Presence>>>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl DiscordRpc {
    /// Start the background thread for a Discord application (client) id. The
    /// thread connects lazily and retries forever, so calling this when Discord
    /// is closed is harmless.
    pub fn start(app_id: String) -> DiscordRpc {
        let shared: Arc<Mutex<Option<Presence>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let s = shared.clone();
        let st = stop.clone();
        let handle = thread::Builder::new()
            .name("pivt-discord".into())
            .spawn(move || {
                // Guard against any unexpected panic inside the IPC code: catch
                // it and keep the thread alive so the UI never notices.
                while !st.load(Ordering::Relaxed) {
                    let s2 = s.clone();
                    let st2 = st.clone();
                    let id = app_id.clone();
                    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        run_loop(id, s2, st2);
                    }));
                    if res.is_ok() {
                        break; // loop exited cleanly (stop requested)
                    }
                    // back off, but stay responsive to shutdown
                    nap(Duration::from_secs(RETRY_SECS), &st);
                }
            })
            .ok();
        DiscordRpc { shared, stop, handle }
    }

    /// Push the latest presence for the background thread to apply.
    pub fn update(&self, p: Presence) {
        if let Ok(mut g) = self.shared.lock() {
            *g = Some(p);
        }
    }
}

impl Drop for DiscordRpc {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Never block shutdown on Discord: an IPC call (connect, set/clear
        // activity) can hang waiting on the pipe. Give the thread a moment to
        // clear the activity, then detach it; process exit reaps it.
        if let Some(h) = self.handle.take() {
            let deadline = Instant::now() + SHUTDOWN_WAIT;
            while !h.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(10));
            }
            if h.is_finished() {
                let _ = h.join();
            }
        }
    }
}

/// Sleep up to `d`, waking early once `stop` is set.
fn nap(d: Duration, stop: &AtomicBool) {
    let until = Instant::now() + d;
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        if now >= until {
            break;
        }
        thread::sleep((until - now).min(Duration::from_millis(25)));
    }
}

/// Connect, then apply pushed presence until stop is requested. Never panics on
/// IO: every Discord call's error just drops the connection and schedules a
/// reconnect.
fn run_loop(app_id: String, shared: Arc<Mutex<Option<Presence>>>, stop: Arc<AtomicBool>) {
    let mut client: Option<DiscordIpcClient> = None;
    let mut last_sent: Option<Presence> = None;
    let mut retry_at = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        // (Re)connect if needed, backing off when Discord is unreachable.
        if client.is_none() && Instant::now() >= retry_at {
            let mut c = DiscordIpcClient::new(&app_id);
            if c.connect().is_ok() {
                client = Some(c);
                last_sent = None; // force a fresh push after (re)connect
            } else {
                retry_at = Instant::now() + Duration::from_secs(RETRY_SECS);
            }
        }

        // Apply the latest desired presence if it changed.
        if let Some(c) = client.as_mut() {
            let want = shared.lock().ok().and_then(|g| g.clone());
            if let Some(p) = want {
                if last_sent.as_ref() != Some(&p) {
                    if apply(c, &p) {
                        last_sent = Some(p);
                    } else {
                        // Pipe broke (Discord closed): drop and reconnect later.
                        let _ = c.close();
                        client = None;
                        last_sent = None;
                        retry_at = Instant::now() + Duration::from_secs(RETRY_SECS);
                    }
                }
            }
        }

        nap(POLL, &stop);
    }

    // Best-effort clean shutdown.
    if let Some(mut c) = client.take() {
        let _ = c.clear_activity();
        let _ = c.close();
    }
}

/// Build and send one activity; returns false if the IPC write failed.
fn apply(client: &mut DiscordIpcClient, p: &Presence) -> bool {
    // "Competing in pivt" reads better than "Playing" for a CTF session.
    let mut act = activity::Activity::new().activity_type(activity::ActivityType::Competing);
    if !p.details.is_empty() {
        act = act.details(p.details.as_str());
    }
    if !p.state.is_empty() {
        act = act.state(p.state.as_str());
    }
    if let Some(ep) = p.start_epoch {
        act = act.timestamps(activity::Timestamps::new().start(ep));
    }
    if !p.large_image.is_empty() {
        let mut assets = activity::Assets::new().large_image(p.large_image.as_str());
        if !p.large_text.is_empty() {
            assets = assets.large_text(p.large_text.as_str());
        }
        if !p.small_image.is_empty() {
            assets = assets.small_image(p.small_image.as_str());
            if !p.small_text.is_empty() {
                assets = assets.small_text(p.small_text.as_str());
            }
        }
        act = act.assets(assets);
    }
    let buttons: Vec<activity::Button> = p
        .buttons
        .iter()
        .take(2)
        .map(|(label, url)| activity::Button::new(label.as_str(), url.as_str()))
        .collect();
    if !buttons.is_empty() {
        act = act.buttons(buttons);
    }
    client.set_activity(act).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nap_wakes_on_stop() {
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let t0 = Instant::now();
        let h = thread::spawn(move || nap(Duration::from_secs(RETRY_SECS), &s));
        thread::sleep(Duration::from_millis(50));
        stop.store(true, Ordering::Relaxed);
        h.join().unwrap();
        assert!(t0.elapsed() < Duration::from_secs(1), "nap ignored stop");
    }

    /// A busy engagement: room, target, ports, loot, a category.
    fn engagement(public: Option<PublicBox>) -> PresenceInput {
        PresenceInput {
            room: Some("acme-dc01".into()),
            public_box: public,
            flags: 1,
            running_jobs: 2,
            loot: 3,
            category: Some("smb".into()),
            badge: Some(("https://x/smb.png".into(), "smb".into())),
            htb_line: None,
            rhost: "10.20.30.40".into(),
            rhost_up: Some(true),
            elapsed: Some("12:34".into()),
            ports: "22,80,445,3389".into(),
            start_epoch: Some(1_700_000_000),
            step: 0,
            link: "https://github.com/Th7bo/pivt".into(),
        }
    }

    /// Every string that reaches Discord, across all rotation steps.
    fn all_text(i: &PresenceInput, mode: Privacy) -> String {
        let mut out = String::new();
        for step in 0..12 {
            let p = build_presence(&PresenceInput { step, ..i.clone() }, mode);
            out.push_str(&format!("{}|{}|{}|{}|{}|{}|", p.details, p.state, p.large_text, p.small_image, p.small_text, p.large_image));
            for (l, u) in &p.buttons {
                out.push_str(&format!("{l}|{u}|"));
            }
        }
        out
    }

    fn assert_no_leak(text: &str) {
        for needle in ["10.20.30.40", "445", "3389", "22,80", "open ports", "acme-dc01", "smb", "cred", "◎"] {
            assert!(!text.contains(needle), "{needle:?} leaked: {text}");
        }
    }

    #[test]
    fn safe_and_minimal_never_show_target_or_ports() {
        let i = engagement(None);
        let safe = all_text(&i, Privacy::Safe);
        assert_no_leak(&safe);
        assert!(safe.contains("on a box"), "{safe}");
        assert!(!safe.contains("view room") && !safe.contains("view machine"), "no room link for unknown platform");
        let min = all_text(&i, Privacy::Minimal);
        assert_no_leak(&min);
        let p = build_presence(&i, Privacy::Minimal);
        assert_eq!(p.details, "pivt");
        assert!(p.state.is_empty() && p.buttons.is_empty() && p.start_epoch.is_none() && p.small_image.is_empty());
        // nothing going on at all
        let idle = PresenceInput { room: None, rhost: String::new(), ..i.clone() };
        assert_eq!(build_presence(&idle, Privacy::Safe).details, "browsing commands");
        // a target but no room is still just "on a box"
        let bare = PresenceInput { room: None, ..i };
        assert_eq!(build_presence(&bare, Privacy::Safe).details, "on a box");
    }

    #[test]
    fn safe_names_public_boxes_with_the_right_link() {
        let htb = engagement(Some(PublicBox { platform: Platform::Htb, name: "Lame".into() }));
        let p = build_presence(&htb, Privacy::Safe);
        assert_eq!(p.details, "HTB · Lame");
        assert_eq!(p.buttons[0], ("view machine".into(), "https://app.hackthebox.com/machines/Lame".into()));
        assert_no_leak(&all_text(&htb, Privacy::Safe));
        let thm = engagement(Some(PublicBox { platform: Platform::Thm, name: "Blue".into() }));
        let p = build_presence(&thm, Privacy::Safe);
        assert_eq!(p.details, "THM · Blue");
        assert_eq!(p.buttons[0].1, "https://tryhackme.com/room/blue");
        assert!(all_text(&thm, Privacy::Safe).contains("🚩 1 flag"));
    }

    #[test]
    fn full_keeps_everything_but_links_only_known_platforms() {
        let i = engagement(None);
        let text = all_text(&i, Privacy::Full);
        for needle in ["10.20.30.40 up", "open ports 22,80,445,3389", "acme-dc01 · 1 flag", "3 creds looted", "in smb"] {
            assert!(text.contains(needle), "{needle:?} missing: {text}");
        }
        let p = build_presence(&i, Privacy::Full);
        assert_eq!(p.buttons.len(), 1, "unknown platform: only the get-pivt button");
        assert!(!text.contains("tryhackme.com"), "no hardcoded THM link: {text}");
        let htb = engagement(Some(PublicBox { platform: Platform::Htb, name: "Lame".into() }));
        assert_eq!(build_presence(&htb, Privacy::Full).buttons[0].1, "https://app.hackthebox.com/machines/Lame");
    }

    #[test]
    fn public_box_decision() {
        use Platform::*;
        let pb = |p, n: &str| Some(PublicBox { platform: p, name: n.into() });
        // HTB panel machine, no room / matching room / HTB-marked room
        assert_eq!(public_box(None, Some("Lame")), pb(Htb, "Lame"));
        assert_eq!(public_box(Some(("lame", None)), Some("Lame")), pb(Htb, "Lame"));
        assert_eq!(public_box(Some(("box1", Some(Htb))), Some("Lame")), pb(Htb, "Lame"));
        // a stale HTB machine doesn't name an unrelated, unmarked room
        assert_eq!(public_box(Some(("acme", None)), Some("Lame")), None);
        // marked rooms
        assert_eq!(public_box(Some(("blue", Some(Thm))), Some("Lame")), pb(Thm, "blue"));
        assert_eq!(public_box(Some(("blue", Some(Thm))), None), pb(Thm, "blue"));
        assert_eq!(public_box(Some(("Lame", Some(Htb))), None), pb(Htb, "Lame"));
        // unknown
        assert_eq!(public_box(Some(("acme", None)), None), None);
        assert_eq!(public_box(None, None), None);
    }

    #[test]
    fn privacy_parse_and_cycle() {
        assert_eq!(Privacy::parse(""), Privacy::Safe);
        assert_eq!(Privacy::parse("FULL"), Privacy::Full);
        assert_eq!(Privacy::parse("minimal"), Privacy::Minimal);
        assert_eq!(Privacy::parse("typo"), Privacy::Safe);
        let mut m = Privacy::Safe;
        for _ in 0..3 {
            m = m.next();
            assert_eq!(Privacy::parse(m.as_str()), m);
        }
        assert_eq!(m, Privacy::Safe);
    }
}
