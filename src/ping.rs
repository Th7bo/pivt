use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy, Default)]
pub struct Ping {
    pub up: Option<bool>, // None = unknown / no target
    pub rtt_ms: Option<u32>,
}

/// Background reachability checker for the current target (RHOST).
pub struct Pinger {
    state: Arc<Mutex<Ping>>,
    target: Arc<Mutex<String>>,
}

impl Pinger {
    pub fn start() -> Pinger {
        let state = Arc::new(Mutex::new(Ping::default()));
        let target = Arc::new(Mutex::new(String::new()));
        let s = state.clone();
        let t = target.clone();
        thread::spawn(move || loop {
            let host = t.lock().map(|g| g.clone()).unwrap_or_default();
            if host.is_empty() {
                if let Ok(mut g) = s.lock() {
                    *g = Ping::default();
                }
                thread::sleep(Duration::from_millis(800));
                continue;
            }
            let (up, rtt) = ping_once(&host);
            if let Ok(mut g) = s.lock() {
                g.up = Some(up);
                g.rtt_ms = rtt;
            }
            thread::sleep(Duration::from_secs(3));
        });
        Pinger { state, target }
    }

    /// A pinger with no background thread (headless snapshots, tests).
    pub fn idle() -> Pinger {
        Pinger { state: Arc::new(Mutex::new(Ping::default())), target: Arc::new(Mutex::new(String::new())) }
    }

    pub fn set_target(&self, host: &str) {
        if let Ok(mut g) = self.target.lock() {
            *g = host.to_string();
        }
    }

    pub fn get(&self) -> Ping {
        self.state.lock().map(|g| *g).unwrap_or_default()
    }
}

/// One ping; returns (reachable, rtt_ms). Uses the system `ping`.
fn ping_once(host: &str) -> (bool, Option<u32>) {
    let out = Command::new("ping")
        .args(["-c", "1", "-W", "1", host])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let rtt = text
                .split("time=")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<f32>().ok())
                .map(|f| f.round() as u32);
            (true, rtt)
        }
        _ => (false, None),
    }
}
