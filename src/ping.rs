use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Ping {
    pub up: Option<bool>, // None = unknown / no target
    pub rtt_ms: Option<u32>,
    /// Set when ICMP got no answer but this TCP port did.
    pub tcp_port: Option<u16>,
}

/// Ports tried over TCP when ICMP gets no reply (Windows drops ping by default).
pub const COMMON_PORTS: &[u16] = &[445, 80, 443, 22, 3389, 5985];
const MAX_TCP_PROBES: usize = 6;
const TCP_TIMEOUT: Duration = Duration::from_millis(400);
const INTERVAL: Duration = Duration::from_secs(3);

#[derive(Default)]
struct Target {
    host: String,
    ports: Vec<u16>,
}

/// Background reachability checker for the current target (RHOST).
pub struct Pinger {
    state: Arc<Mutex<Ping>>,
    target: Arc<Mutex<Target>>,
}

impl Pinger {
    pub fn start() -> Pinger {
        let state = Arc::new(Mutex::new(Ping::default()));
        let target = Arc::new(Mutex::new(Target::default()));
        let s = state.clone();
        let t = target.clone();
        thread::spawn(move || loop {
            let (host, ports) = t.lock().map(|g| (g.host.clone(), g.ports.clone())).unwrap_or_default();
            if host.is_empty() {
                thread::sleep(Duration::from_millis(300));
                continue;
            }
            let result = probe(&host, &ports);
            // drop a result for a target that changed while we were probing
            let current = t.lock().map(|g| g.host == host).unwrap_or(false);
            if current {
                if let Ok(mut g) = s.lock() {
                    *g = result;
                }
            }
            // sleep, but wake early when the target changes
            let until = Instant::now() + INTERVAL;
            while Instant::now() < until {
                thread::sleep(Duration::from_millis(100));
                if t.lock().map(|g| g.host != host).unwrap_or(true) {
                    break;
                }
            }
        });
        Pinger { state, target }
    }

    /// A pinger with no background thread (headless snapshots, tests).
    pub fn idle() -> Pinger {
        Pinger { state: Arc::new(Mutex::new(Ping::default())), target: Arc::new(Mutex::new(Target::default())) }
    }

    /// Point at a new host. The old host's status is cleared right away
    /// instead of lingering until the next probe finishes.
    pub fn set_target(&self, host: &str) {
        let changed = self.target.lock().map(|mut g| {
            let changed = g.host != host;
            g.host = host.to_string();
            changed
        });
        if changed.unwrap_or(false) {
            if let Ok(mut g) = self.state.lock() {
                *g = Ping::default();
            }
        }
    }

    /// Open ports from the last parsed scan, tried first for the TCP fallback.
    pub fn set_ports(&self, ports: &[u16]) {
        if let Ok(mut g) = self.target.lock() {
            g.ports = ports.to_vec();
        }
    }

    pub fn get(&self) -> Ping {
        self.state.lock().map(|g| *g).unwrap_or_default()
    }
}

/// ICMP first; if that gets no reply, a TCP connect to likely ports.
fn probe(host: &str, known: &[u16]) -> Ping {
    if let Some(rtt) = icmp(host) {
        return Ping { up: Some(true), rtt_ms: rtt, tcp_port: None };
    }
    for port in tcp_ports(known) {
        let Some(addr) = resolve(host, port) else { break };
        let start = Instant::now();
        let r = TcpStream::connect_timeout(&addr, TCP_TIMEOUT);
        if tcp_says_up(&r) {
            let ms = start.elapsed().as_millis() as u32;
            return Ping { up: Some(true), rtt_ms: Some(ms), tcp_port: Some(port) };
        }
    }
    Ping { up: Some(false), rtt_ms: None, tcp_port: None }
}

/// Ports to try: known-open ones first, then the common ones, deduplicated.
pub fn tcp_ports(known: &[u16]) -> Vec<u16> {
    let mut v: Vec<u16> = Vec::new();
    for p in known.iter().chain(COMMON_PORTS) {
        if *p != 0 && !v.contains(p) {
            v.push(*p);
        }
    }
    v.truncate(MAX_TCP_PROBES);
    v
}

/// A completed handshake or a RST (connection refused) both prove the host
/// is up; a timeout or unreachable error proves nothing.
pub fn tcp_says_up(r: &std::io::Result<TcpStream>) -> bool {
    match r {
        Ok(_) => true,
        Err(e) => e.kind() == std::io::ErrorKind::ConnectionRefused,
    }
}

fn resolve(host: &str, port: u16) -> Option<SocketAddr> {
    (host, port).to_socket_addrs().ok()?.next()
}

/// One ICMP ping via the system `ping`; Some(rtt) when it answered.
fn icmp(host: &str) -> Option<Option<u32>> {
    let o = Command::new("ping").args(["-c", "1", "-W", "1", host]).output().ok()?;
    if !o.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&o.stdout);
    Some(
        text.split("time=")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .and_then(|s| s.parse::<f32>().ok())
            .map(|f| f.round() as u32),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn tcp_port_order() {
        assert_eq!(tcp_ports(&[]), COMMON_PORTS.to_vec());
        assert_eq!(tcp_ports(&[8080, 445]), vec![8080, 445, 80, 443, 22, 3389]);
        assert_eq!(tcp_ports(&[1, 2, 3, 4, 5, 6, 7]).len(), MAX_TCP_PROBES);
    }

    #[test]
    fn tcp_open_and_refused_both_mean_up() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let open = l.local_addr().unwrap();
        assert!(tcp_says_up(&TcpStream::connect_timeout(&open, TCP_TIMEOUT)));
        drop(l);
        // nothing listens there any more: RST → still "up"
        assert!(tcp_says_up(&TcpStream::connect_timeout(&open, TCP_TIMEOUT)));
        let timeout = Err(std::io::Error::from(std::io::ErrorKind::TimedOut));
        assert!(!tcp_says_up(&timeout));
    }

    #[test]
    fn changing_target_clears_old_status() {
        let p = Pinger::idle();
        p.set_target("10.0.0.1");
        *p.state.lock().unwrap() = Ping { up: Some(true), rtt_ms: Some(30), tcp_port: None };
        p.set_target("10.0.0.1"); // same host: keep
        assert_eq!(p.get().up, Some(true));
        p.set_target("10.0.0.2");
        assert_eq!(p.get(), Ping::default());
    }
}
