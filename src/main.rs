mod ask;
mod claude;
mod db;
mod llm;
mod mcp;
mod pipeline;
mod prompts;
mod sandbox;
mod sh;
mod sources;
mod tools;
mod watcher;
mod web;
mod workers;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, Notify, OnceCell};

/// The 10 agents and their default model ("" = runs no model). Order matches the dashboard.
pub const AGENTS: &[(&str, &str)] = &[
    ("scout", "claude:haiku"),
    ("guard", "claude:haiku"),
    ("vet", "claude:haiku"),
    ("watcher", ""),
    ("hunter", "claude:sonnet"),
    ("repro", "claude:sonnet"),
    ("fixer", "claude:sonnet"),
    ("ci", ""),
    ("reviewer", "claude:opus"),
    ("submit", ""),
];

/// What an agent is doing right now (not saved; rebuilt from activity after a restart).
#[derive(Clone, Serialize)]
pub struct Live {
    pub status: String,
    pub job: Option<i64>,
    pub task: String,
    pub speech: String,
    pub tool: String,
}

pub struct App {
    pub db: db::Db,
    pub data: PathBuf,
    pub port: u16,
    /// Live messages for the dashboard (Server-Sent Events).
    pub tx: broadcast::Sender<String>,
    pub live: Mutex<BTreeMap<&'static str, Live>>,
    pub machine: Mutex<Value>,
    /// Wakes the job runner (job became ready or was approved).
    pub wake: Notify,
    /// Wakes the scout (repo added, issue added, "Run Full Pipeline").
    pub scan: Notify,
    /// This PC's sandbox; started on first use (building the Docker image can take minutes).
    pub sb: OnceCell<sandbox::Sandbox>,
    /// Sandboxes on other machines, opened on first use.
    pub remote: tokio::sync::Mutex<HashMap<String, sandbox::Sandbox>>,
    /// "ok" or the last error per machine, for the Machines popup.
    pub machine_status: Mutex<HashMap<String, String>>,
    /// Lanes that are running ("machine#n").
    pub lanes_up: Mutex<HashSet<String>>,
}

impl App {
    pub fn new(data: PathBuf, port: u16) -> anyhow::Result<App> {
        let db = db::Db::open(&data.join("office.db"))?;
        let (tx, _) = broadcast::channel(1024);
        let live = AGENTS
            .iter()
            .map(|(a, _)| {
                let status = if *a == "hunter" { "OFF" } else { "IDLE" };
                (*a, Live { status: status.into(), job: None, task: "—".into(), speech: String::new(), tool: String::new() })
            })
            .collect();
        let machine = json!({
            "name": std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "ME".into()),
            "os": format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
            "sandbox": "starting...",
        });
        Ok(App { db, data, port, tx, live: Mutex::new(live), machine: Mutex::new(machine), wake: Notify::new(), scan: Notify::new(), sb: OnceCell::new(), remote: Default::default(), machine_status: Default::default(), lanes_up: Default::default() })
    }

    pub async fn sandbox(&self) -> &sandbox::Sandbox {
        self.sb
            .get_or_init(|| async {
                self.log("watcher", None, "starting the sandbox...");
                let (sb, note) = sandbox::Sandbox::start(&self.data).await;
                self.machine.lock().unwrap()["sandbox"] = json!(note);
                self.log("watcher", None, &format!("sandbox ready: {note}"));
                sb
            })
            .await
    }

    /// The sandbox of a machine: this PC ("me") or another machine's Docker over SSH.
    pub async fn machine_sandbox(&self, id: &str) -> Result<sandbox::Sandbox, String> {
        if id == "me" {
            let sb = self.sandbox().await.clone();
            self.machine_status.lock().unwrap().insert(id.into(), "ok".into());
            return Ok(sb);
        }
        let mut remote = self.remote.lock().await;
        if let Some(sb) = remote.get(id) {
            return Ok(sb.clone());
        }
        let m = self.db.machines().into_iter().find(|m| m.id == id).ok_or("machine removed")?;
        match sandbox::Sandbox::remote(&self.data, &m.host).await {
            Ok(sb) => {
                self.machine_status.lock().unwrap().insert(id.into(), "ok".into());
                self.log("watcher", None, &format!("machine {} ready ({})", m.name, m.host));
                remote.insert(id.into(), sb.clone());
                Ok(sb)
            }
            Err(e) => {
                let first = self.machine_status.lock().unwrap().insert(id.into(), e.clone()).as_deref() != Some(e.as_str());
                if first {
                    self.log("watcher", None, &format!("machine {} not usable: {}", m.name, sh::head(&e, 200)));
                }
                Err(e)
            }
        }
    }

    /// Something an agent says: saved, shown in the LOG panel and in its speech bubble.
    pub fn log(&self, agent: &str, job: Option<i64>, msg: &str) {
        let msg = sh::head(msg.trim(), 500);
        self.db.event(agent, job, &msg);
        if let Some(l) = self.live.lock().unwrap().get_mut(agent) {
            l.speech = msg.clone();
        }
        let _ = self.tx.send(json!({"type": "log", "agent": agent, "job": job, "msg": msg, "t": db::now()}).to_string());
    }

    /// The agent's latest tool call, shown as the `$ ...` line in its bio strip.
    pub fn tool(&self, agent: &str, job: Option<i64>, line: &str) {
        if let Some(l) = self.live.lock().unwrap().get_mut(agent) {
            l.tool = sh::head(line, 200);
        }
        self.log(agent, job, line);
    }

    /// Status (IDLE / WORKING) and, optionally, the task text on the agent's card.
    pub fn set(&self, agent: &str, status: &str, job: Option<i64>, task: Option<&str>) {
        if let Some(l) = self.live.lock().unwrap().get_mut(agent) {
            l.status = status.into();
            l.job = job;
            if let Some(t) = task {
                l.task = sh::head(t, 120);
            }
        }
        self.changed();
    }

    /// Push notification through ntfy (set a topic in Settings). Never blocks the caller.
    pub fn notify(&self, title: &str, msg: &str) {
        let Some(topic) = self.db.get("ntfy_topic").filter(|t| !t.trim().is_empty()) else { return };
        let server = self.db.get("ntfy_server").filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "https://ntfy.sh".into());
        let url = format!("{}/{}", server.trim_end_matches('/'), topic.trim());
        let (title, msg) = (title.to_string(), msg.to_string());
        tokio::spawn(async move {
            let _ = reqwest::Client::new().post(url).header("Title", title).body(msg).send().await;
        });
    }

    /// A new one-time login link for the dashboard (the old one stops working).
    pub fn login_link(&self) -> String {
        let k = sources::new_token();
        self.db.set("login_key", &k);
        format!("http://127.0.0.1:{}/login?key={k}", self.port)
    }

    /// Tells dashboards to refetch /api/state.
    pub fn changed(&self) {
        let _ = self.tx.send(json!({"type": "changed"}).to_string());
    }

    pub fn paused(&self) -> bool {
        self.db.get("paused").as_deref() == Some("1")
    }

    /// "Pause office" stops before the next step; the step already running finishes.
    pub async fn wait_unpaused(&self) {
        while self.paused() {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    /// Daily budget in USD (0 = none). When today's cost reaches it, AI calls wait until
    /// tomorrow or until you raise the budget; nothing is dropped.
    pub async fn wait_budget(&self) {
        let mut told = false;
        loop {
            let budget: f64 = self.db.get("budget_usd").and_then(|b| b.parse().ok()).unwrap_or(0.0);
            let spent = self.db.usd_today();
            if budget <= 0.0 || spent < budget {
                return;
            }
            if !told {
                self.log("watcher", None, &format!("daily budget reached (${spent:.2} of ${budget:.2}): waiting until tomorrow or a higher budget"));
                self.changed();
                told = true;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    }

    /// "provider:model" for an agent: your choice from the dashboard, else the default.
    pub fn model(&self, agent: &str) -> String {
        self.db.get(&format!("model:{agent}")).unwrap_or_else(|| AGENTS.iter().find(|(a, _)| *a == agent).map(|(_, m)| m.to_string()).unwrap_or_default())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        // Started by Claude for each agent run; stdout is JSON-RPC only.
        Some("mcp") => {
            let o = mcp::parse(&args).ok_or_else(|| anyhow::anyhow!("usage: myoffice mcp --data D --space S --mode docker|local [--readonly]"))?;
            mcp::serve(o).await;
            return Ok(());
        }
        Some("login-link") => {
            let data = std::env::var_os("MYOFFICE_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
            let port: u16 = std::env::var("MYOFFICE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47400);
            println!("{}", App::new(data, port)?.login_link());
            return Ok(());
        }
        Some("autostart") => {
            println!("{}", autostart(args.get(1).map(String::as_str) == Some("on"))?);
            return Ok(());
        }
        Some(x) => anyhow::bail!("unknown command {x:?}. Run without arguments to start the office, or: myoffice autostart on|off"),
    }
    let port: u16 = std::env::var("MYOFFICE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47400);
    let data = std::env::var_os("MYOFFICE_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
    std::fs::create_dir_all(&data)?;
    // Not canonicalize(): on Windows it adds a \\?\ prefix that git and gh don't handle well.
    let data = if data.is_absolute() { data } else { std::env::current_dir()?.join(data) };
    let app = Arc::new(App::new(data, port)?);
    app.db.recover();

    let a = app.clone();
    tokio::spawn(async move {
        a.sandbox().await;
        a.changed();
    });
    tokio::spawn(pipeline::scout_loop(app.clone()));
    tokio::spawn(workers::supervise(app.clone()));
    tokio::spawn(watcher::watch_loop(app.clone()));

    // Only this PC can connect, and the dashboard needs the one-time login link below.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("\n  MY OFFICE is running.\n  Open this one-time login link: {}\n  (new link any time: myoffice login-link)\n  data: {}\n  stop with Ctrl+C\n", app.login_link(), app.data.display());
    axum::serve(listener, web::router(app)).await?;
    Ok(())
}

/// `myoffice autostart on|off`: start MY OFFICE (from the current folder) when you log in to Windows.
fn autostart(on: bool) -> anyhow::Result<String> {
    let appdata = std::env::var("APPDATA").map_err(|_| anyhow::anyhow!("autostart is only supported on Windows"))?;
    let f = PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup\MyOffice.cmd");
    if !on {
        let _ = std::fs::remove_file(&f);
        return Ok(format!("autostart off (removed {})", f.display()));
    }
    let (exe, dir) = (std::env::current_exe()?, std::env::current_dir()?);
    std::fs::write(&f, format!("@echo off\r\ncd /d \"{}\"\r\nstart \"MY OFFICE\" /min \"{}\"\r\n", dir.display(), exe.display()))?;
    Ok(format!("autostart on: MY OFFICE starts from {} when you log in ({})", dir.display(), f.display()))
}
