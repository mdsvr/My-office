mod claude;
mod db;
mod pipeline;
mod prompts;
mod sh;
mod web;

use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::{broadcast, Notify};

pub const WORKERS: &[&str] = &["scout", "fixer", "verifier", "ci", "reviewer", "submit", "watcher"];

#[derive(Clone, Serialize, Default)]
pub struct Worker {
    pub status: String,
    pub job: Option<i64>,
    pub said: String,
}

/// Everything the server, the pipeline and the dashboard share.
pub struct App {
    pub db: db::Db,
    pub data: PathBuf,
    pub port: u16,
    /// Live messages for the dashboard (Server-Sent Events).
    pub tx: broadcast::Sender<String>,
    pub workers: Mutex<BTreeMap<&'static str, Worker>>,
    /// Wakes the job runner (new job queued or approved).
    pub wake: Notify,
    /// Wakes the scout (repo added or "scan now").
    pub scan: Notify,
}

impl App {
    /// Records what a worker is doing: saved to the DB and pushed to the dashboard.
    pub fn log(&self, worker: &str, job: Option<i64>, msg: &str) {
        let msg = sh::head(msg, 500);
        self.db.event(worker, job, &msg);
        if let Some(w) = self.workers.lock().unwrap().get_mut(worker) {
            w.said = msg.clone();
        }
        let _ = self.tx.send(json!({"worker": worker, "job": job, "msg": msg, "t": db::now()}).to_string());
    }

    pub fn set(&self, worker: &str, status: &str, job: Option<i64>) {
        if let Some(w) = self.workers.lock().unwrap().get_mut(worker) {
            w.status = status.into();
            w.job = job;
        }
        let _ = self.tx.send(json!({"kind": "workers"}).to_string());
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let port: u16 = std::env::var("MYOFFICE_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(47400);
    let data = std::env::var_os("MYOFFICE_DATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("data"));
    std::fs::create_dir_all(data.join("work"))?;
    let data = std::fs::canonicalize(&data)?;
    let db = db::Db::open(&data.join("office.db"))?;
    let (tx, _) = broadcast::channel(512);
    let workers = WORKERS.iter().map(|w| (*w, Worker { status: "idle".into(), ..Default::default() })).collect();
    let app = Arc::new(App { db, data, port, tx, workers: Mutex::new(workers), wake: Notify::new(), scan: Notify::new() });

    tokio::spawn(pipeline::scout_loop(app.clone()));
    tokio::spawn(pipeline::run_loop(app.clone()));
    tokio::spawn(pipeline::watch_loop(app.clone()));

    // ponytail: localhost only, no login. Add a login link before exposing it to a network.
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    println!("MyOffice is running: http://127.0.0.1:{port}   (data: {})", app.data.display());
    axum::serve(listener, web::router(app)).await?;
    Ok(())
}
