mod claude;
mod db;
mod pipeline;
mod prompts;
mod sh;
mod web;

use serde::Serialize;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, Notify};

/// The 10 agents and their default model ("" = runs no model). Order matches the dashboard.
pub const AGENTS: &[(&str, &str)] = &[
    ("scout", "haiku"),
    ("guard", "haiku"),
    ("vet", "haiku"),
    ("watcher", ""),
    ("hunter", "sonnet"),
    ("repro", "sonnet"),
    ("fixer", "sonnet"),
    ("ci", ""),
    ("reviewer", "opus"),
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
}

impl App {
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

    /// Model alias for an agent: your choice from the dashboard, else the default.
    pub fn model(&self, agent: &str) -> String {
        self.db.get(&format!("model:{agent}")).unwrap_or_else(|| AGENTS.iter().find(|(a, _)| *a == agent).map(|(_, m)| m.to_string()).unwrap_or_default())
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::var("MYOFFICE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47400);
    let data = std::env::var_os("MYOFFICE_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
    std::fs::create_dir_all(data.join("work"))?;
    let data = std::fs::canonicalize(&data)?;
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
        "docker": Value::Null,
    });
    let app = Arc::new(App { db, data, port, tx, live: Mutex::new(live), machine: Mutex::new(machine), wake: Notify::new(), scan: Notify::new() });

    let a = app.clone();
    tokio::spawn(async move {
        let o = sh::run("docker", &["info", "--format", "{{.ServerVersion}}"], &a.data, 30).await;
        a.machine.lock().unwrap()["docker"] = if o.code == 0 { json!(o.out.trim()) } else { json!(false) };
    });
    tokio::spawn(pipeline::scout_loop(app.clone()));
    tokio::spawn(pipeline::run_loop(app.clone()));
    tokio::spawn(pipeline::watch_loop(app.clone()));

    // ponytail: localhost only, no login. Add a login link before exposing it to a network.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("\n  MY OFFICE is running: http://127.0.0.1:{port}\n  data: {}\n  stop with Ctrl+C\n", app.data.display());
    axum::serve(listener, web::router(app)).await?;
    Ok(())
}
