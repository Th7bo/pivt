//! Hack The Box labs integration (`H`).
//!
//! Talks to the same API the HTB web app uses (labs.hackthebox.com/api), with
//! an App Token from Labs → Account Settings → App Tokens, read from
//! `~/.config/pivt/htb_token` or `$HTB_TOKEN`.
//!
//! Every request runs on a short-lived background thread that writes into a
//! shared [`HtbState`]; the UI only ever clones that state, so it never blocks
//! on the network. Endpoints (taken from the web app's own bundles):
//!
//! - `GET  v4/machine/active`                     → `{info: {id,…} | null}`
//! - `GET  v4/machine/profile/{id|name}`          → `{info: {…, synopsis, ip, …}}`
//! - `GET  v4/machines/{id}/tasks`                → `{data: [task…]}` (guided mode)
//! - `POST v4/machines/{id}/tasks/{task}/flag`    `{flag}`  (guided answers + user/root)
//! - `POST v5/machine/own`                        `{id, flag}` (adventure mode)
//! - `POST v4/vm/spawn` / `v4/vm/terminate`       `{machine_id}`

use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const API: &str = "https://labs.hackthebox.com/api";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    Task,
    User,
    Root,
}

#[derive(Debug, Clone)]
pub struct Task {
    /// 0 for the synthetic user/root rows of machines without guided mode.
    pub id: u64,
    pub title: String,
    pub question: String,
    pub hint: String,
    /// Answer shape, e.g. "number, such as 3, 17, or 4567" or "32 hex characters".
    pub format: String,
    pub completed: bool,
    pub kind: TaskKind,
    pub prereq: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Machine {
    pub id: u64,
    pub name: String,
    pub os: String,
    pub difficulty: String,
    pub points: u64,
    pub ip: Option<String>,
    pub synopsis: String,
    pub user_owned: bool,
    pub root_owned: bool,
    pub guided: bool,
    pub retired: bool,
    pub spawned: bool,
}

/// Something the app should react to once (flag burst, RHOST update, log line).
#[derive(Debug, Clone)]
pub enum HtbEvent {
    /// A user/root flag was accepted. `value` is what was submitted.
    Owned { which: &'static str, value: String },
    /// A guided task answer was accepted.
    TaskSolved { title: String, question: String, answer: String },
}

#[derive(Debug, Clone, Default)]
pub struct HtbState {
    pub machine: Option<Machine>,
    pub tasks: Vec<Task>,
    /// Human-readable description of the request in flight.
    pub busy: Option<String>,
    /// Last outcome: (ok, message).
    pub message: Option<(bool, String)>,
    pub events: Vec<HtbEvent>,
}

impl HtbState {
    /// A task is locked until its prerequisite is completed.
    pub fn locked(&self, t: &Task) -> bool {
        t.prereq
            .map(|p| self.tasks.iter().any(|o| o.id == p && !o.completed))
            .unwrap_or(false)
    }
}

pub struct Htb {
    token: Option<String>,
    state: Arc<Mutex<HtbState>>,
}

pub fn token_path() -> std::path::PathBuf {
    crate::model::config_dir().join("htb_token")
}

fn load_token() -> Option<String> {
    std::env::var("HTB_TOKEN")
        .ok()
        .or_else(|| std::fs::read_to_string(token_path()).ok())
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

impl Htb {
    pub fn new() -> Htb {
        Htb { token: load_token(), state: Arc::new(Mutex::new(HtbState::default())) }
    }

    /// For headless snapshots: a client with fixed state and no network.
    #[allow(dead_code)]
    pub fn with_state(s: HtbState) -> Htb {
        // Dummy token so the panel renders; snapshots never trigger requests.
        Htb { token: Some("offline".into()), state: Arc::new(Mutex::new(s)) }
    }

    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    pub fn snapshot(&self) -> HtbState {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Drain one-shot events for the app to act on.
    pub fn take_events(&self) -> Vec<HtbEvent> {
        self.state.lock().map(|mut s| std::mem::take(&mut s.events)).unwrap_or_default()
    }

    pub fn is_busy(&self) -> bool {
        self.state.lock().map(|s| s.busy.is_some()).unwrap_or(false)
    }

    /// Run `job` on a background thread with `label` shown as busy. Refuses
    /// (returning false) while another request is in flight.
    fn spawn<F>(&self, label: String, job: F) -> bool
    where
        F: FnOnce(&Api, &Arc<Mutex<HtbState>>) -> Result<String, String> + Send + 'static,
    {
        let Some(token) = self.token.clone() else {
            self.set_message(false, format!("no HTB token — put an App Token in {}", token_path().display()));
            return false;
        };
        {
            let Ok(mut s) = self.state.lock() else { return false };
            if s.busy.is_some() {
                return false;
            }
            s.busy = Some(label);
        }
        let state = self.state.clone();
        let _ = thread::Builder::new().name("pivt-htb".into()).spawn(move || {
            let api = Api::new(token);
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| job(&api, &state)))
                .unwrap_or_else(|_| Err("internal error".into()));
            if let Ok(mut s) = state.lock() {
                s.busy = None;
                s.message = Some(match res {
                    Ok(m) => (true, m),
                    Err(e) => (false, e),
                });
            }
        });
        true
    }

    fn set_message(&self, ok: bool, msg: String) {
        if let Ok(mut s) = self.state.lock() {
            s.message = Some((ok, msg));
        }
    }

    /// Load a machine by name/id, or the currently active one when `None`
    /// (falling back to `fallback`, e.g. the active room's name).
    pub fn load(&self, which: Option<String>, fallback: Option<String>) -> bool {
        let label = match &which {
            Some(n) => format!("loading {n}…"),
            None => "loading active machine…".into(),
        };
        self.spawn(label, move |api, state| {
            let id = match which {
                Some(n) => n,
                None => {
                    let v = api.get("v4/machine/active")?;
                    match (v["info"]["id"].as_u64(), fallback) {
                        (Some(id), _) => id.to_string(),
                        (None, Some(f)) => f,
                        (None, None) => return Err("no active machine — press m to pick one by name".into()),
                    }
                }
            };
            refresh(api, state, &id)?;
            let name = state.lock().ok().and_then(|s| s.machine.as_ref().map(|m| m.name.clone()));
            Ok(format!("loaded {}", name.unwrap_or(id)))
        })
    }

    /// Re-fetch the loaded machine and its tasks.
    pub fn refresh(&self) -> bool {
        let Some(id) = self.machine_id() else { return self.load(None, None) };
        self.spawn("refreshing…".into(), move |api, state| {
            refresh(api, state, &id.to_string())?;
            Ok("refreshed".into())
        })
    }

    fn machine_id(&self) -> Option<u64> {
        self.state.lock().ok().and_then(|s| s.machine.as_ref().map(|m| m.id))
    }

    /// Submit an answer for `task` (guided task, or user/root flag).
    pub fn submit(&self, task: Task, answer: String) -> bool {
        let Some(mid) = self.machine_id() else { return false };
        self.spawn(format!("submitting {}…", task.title), move |api, state| {
            let before = state.lock().ok().and_then(|s| s.machine.clone()).unwrap_or_default();
            let resp = if task.id == 0 {
                api.post("v5/machine/own", json!({ "id": mid, "flag": answer }))?
            } else {
                api.post(&format!("v4/machines/{mid}/tasks/{}/flag", task.id), json!({ "flag": answer }))?
            };
            refresh(api, state, &mid.to_string())?;
            let msg = resp["message"].as_str().unwrap_or("accepted").to_string();
            let Ok(mut s) = state.lock() else { return Ok(msg) };
            let after = s.machine.clone().unwrap_or_default();
            // Work out what was owned from the profile flipping, which also
            // covers the user/root rows of guided mode.
            if after.root_owned && !before.root_owned {
                s.events.push(HtbEvent::Owned { which: "root", value: answer.clone() });
            } else if after.user_owned && !before.user_owned {
                s.events.push(HtbEvent::Owned { which: "user", value: answer.clone() });
            } else if task.kind == TaskKind::Task {
                s.events.push(HtbEvent::TaskSolved { title: task.title, question: task.question, answer });
            }
            Ok(msg)
        })
    }

    /// Spawn the loaded machine, then poll until it has an IP (up to ~3 min).
    pub fn spawn_machine(&self) -> bool {
        let Some(mid) = self.machine_id() else { return false };
        self.spawn("spawning machine…".into(), move |api, state| {
            let resp = api.post("v4/vm/spawn", json!({ "machine_id": mid }))?;
            let msg = resp["message"].as_str().unwrap_or("spawning").to_string();
            if let Ok(mut s) = state.lock() {
                s.busy = Some(format!("{msg} — waiting for IP…"));
            }
            for _ in 0..18 {
                thread::sleep(Duration::from_secs(10));
                refresh(api, state, &mid.to_string())?;
                let ip = state.lock().ok().and_then(|s| s.machine.as_ref().and_then(|m| m.ip.clone()));
                if let Some(ip) = ip {
                    return Ok(format!("machine up at {ip}"));
                }
            }
            Ok(format!("{msg} (no IP yet — press r later)"))
        })
    }

    pub fn stop_machine(&self) -> bool {
        let Some(mid) = self.machine_id() else { return false };
        self.spawn("stopping machine…".into(), move |api, state| {
            let resp = api.post("v4/vm/terminate", json!({ "machine_id": mid }))?;
            refresh(api, state, &mid.to_string())?;
            Ok(resp["message"].as_str().unwrap_or("terminated").to_string())
        })
    }
}

/// Fetch profile (+ tasks) for `id` into the shared state.
fn refresh(api: &Api, state: &Arc<Mutex<HtbState>>, id: &str) -> Result<(), String> {
    let v = api.get(&format!("v4/machine/profile/{id}"))?;
    let m = parse_machine(&v["info"]).ok_or("unexpected profile response")?;
    let mut tasks = Vec::new();
    if m.guided {
        if let Ok(t) = api.get(&format!("v4/machines/{}/tasks", m.id)) {
            tasks = t["data"].as_array().map(|a| a.iter().filter_map(parse_task).collect()).unwrap_or_default();
        }
    }
    if tasks.is_empty() {
        tasks = vec![
            synthetic("Submit User Flag", TaskKind::User, m.user_owned),
            synthetic("Submit Root Flag", TaskKind::Root, m.root_owned),
        ];
    }
    if let Ok(mut s) = state.lock() {
        s.machine = Some(m);
        s.tasks = tasks;
    }
    Ok(())
}

fn synthetic(title: &str, kind: TaskKind, done: bool) -> Task {
    Task {
        id: 0,
        title: title.into(),
        question: String::new(),
        hint: String::new(),
        format: "32 hex characters".into(),
        completed: done,
        kind,
        prereq: None,
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

pub fn parse_machine(v: &Value) -> Option<Machine> {
    Some(Machine {
        id: v["id"].as_u64()?,
        name: s(&v["name"]),
        os: s(&v["os"]),
        difficulty: s(&v["difficultyText"]),
        points: v["points"].as_u64().unwrap_or(0),
        ip: v["ip"].as_str().filter(|i| !i.is_empty()).map(String::from),
        synopsis: s(&v["synopsis"]),
        user_owned: v["authUserInUserOwns"].as_bool().unwrap_or(false),
        root_owned: v["authUserInRootOwns"].as_bool().unwrap_or(false),
        guided: v["isGuidedEnabled"].as_bool().unwrap_or(false),
        retired: v["retired"].as_bool().unwrap_or(false),
        spawned: v["playInfo"]["isActive"].as_bool().unwrap_or(false),
    })
}

pub fn parse_task(v: &Value) -> Option<Task> {
    Some(Task {
        id: v["id"].as_u64()?,
        title: s(&v["title"]),
        question: s(&v["description"]),
        hint: s(&v["hint"]),
        format: s(&v["masked_flag"]),
        completed: v["completed"].as_bool().unwrap_or(false),
        kind: match v["type"]["text"].as_str() {
            Some("user") => TaskKind::User,
            Some("root") => TaskKind::Root,
            _ => TaskKind::Task,
        },
        prereq: v["prerequisite_id"].as_u64(),
    })
}

struct Api {
    agent: ureq::Agent,
    auth: String,
}

impl Api {
    fn new(token: String) -> Api {
        Api {
            agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(20)).build(),
            auth: format!("Bearer {token}"),
        }
    }

    fn req(&self, method: &str, path: &str) -> ureq::Request {
        self.agent
            .request(method, &format!("{API}/{path}"))
            .set("Authorization", &self.auth)
            .set("Accept", "application/json")
            .set("User-Agent", concat!("pivt/", env!("CARGO_PKG_VERSION")))
    }

    fn get(&self, path: &str) -> Result<Value, String> {
        read(self.req("GET", path).call())
    }

    fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        read(self.req("POST", path).send_json(body))
    }
}

/// Turn a response into JSON, surfacing HTB's `message` on errors.
fn read(r: Result<ureq::Response, ureq::Error>) -> Result<Value, String> {
    match r {
        Ok(resp) => resp.into_json::<Value>().map_err(|e| format!("bad response: {e}")),
        Err(ureq::Error::Status(code, resp)) => {
            let body: Value = resp.into_json().unwrap_or(Value::Null);
            let msg = body["message"].as_str().map(String::from);
            Err(match (code, msg) {
                (401, _) => "token rejected (401) — create a new App Token".into(),
                (_, Some(m)) => m,
                (c, None) => format!("HTTP {c}"),
            })
        }
        Err(e) => Err(format!("network: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile_and_tasks() {
        let p = json!({"id": 1, "name": "Lame", "os": "Linux", "difficultyText": "Easy", "points": 0,
            "ip": null, "synopsis": "first box", "authUserInUserOwns": true, "authUserInRootOwns": false,
            "isGuidedEnabled": true, "retired": true, "playInfo": {"isActive": false}});
        let m = parse_machine(&p).unwrap();
        assert_eq!((m.id, m.ip.clone(), m.user_owned, m.guided), (1, None, true, true));

        let t = json!({"id": 1239, "title": "Submit User Flag", "description": "", "hint": "",
            "masked_flag": "32 hex characters", "completed": false,
            "type": {"id": 1, "text": "user"}, "prerequisite_id": null});
        let t = parse_task(&t).unwrap();
        assert_eq!(t.kind, TaskKind::User);

        let a = Task { id: 1, prereq: None, completed: false, ..t.clone() };
        let b = Task { id: 2, prereq: Some(1), ..t };
        let st = HtbState { tasks: vec![a.clone(), b.clone()], ..Default::default() };
        assert!(st.locked(&b) && !st.locked(&a));
    }
}
