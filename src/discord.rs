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

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Seconds between reconnect attempts while Discord is unreachable.
const RETRY_SECS: u64 = 15;
/// Background loop granularity (keeps shutdown responsive).
const POLL: Duration = Duration::from_millis(500);

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
    /// Up to two (label, url) buttons. Discord hides your own buttons from
    /// you; other people see them on your profile.
    pub buttons: Vec<(String, String)>,
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
                    thread::sleep(Duration::from_secs(RETRY_SECS));
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
        // The loop checks `stop` every POLL, so this joins quickly; best-effort.
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
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

        thread::sleep(POLL);
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
