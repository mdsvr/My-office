//! Workers: every machine (this PC, or another machine's Docker over SSH) runs `lanes` jobs at
//! the same time. Each lane has its own working copy per repo, so lanes never share files, and
//! takes jobs with one locked database step (`Db::claim`), so two lanes never take the same job.

use crate::sandbox::{self, Sandbox};
use crate::{pipeline, App};
use std::sync::Arc;
use std::time::Duration;

pub struct Lane {
    pub sb: Sandbox,
    pub n: i64,
    /// "ME/1", "build-box/2": shown in the log.
    pub name: String,
}

impl Lane {
    /// The lane's working copy of a repo, e.g. "l2__owner__repo".
    pub fn space(&self, repo: &str) -> String {
        sandbox::space(&format!("l{}", self.n), repo)
    }
}

/// Starts lanes for machines and lane counts as you configure them (checked every 30 s).
/// Lanes you remove finish their current job and stop.
pub async fn supervise(app: Arc<App>) {
    loop {
        for m in app.db.machines().into_iter().filter(|m| m.enabled) {
            for n in 1..=m.lanes.clamp(1, 16) {
                let key = format!("{}#{n}", m.id);
                if app.lanes_up.lock().unwrap().insert(key.clone()) {
                    tokio::spawn(lane(app.clone(), m.id.clone(), n, key));
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(30)).await;
    }
}

fn wanted(app: &App, machine: &str, n: i64) -> bool {
    app.db.machines().iter().any(|m| m.id == machine && m.enabled && n <= m.lanes)
}

async fn lane(app: Arc<App>, machine: String, n: i64, key: String) {
    let sb = loop {
        if !wanted(&app, &machine, n) {
            app.lanes_up.lock().unwrap().remove(&key);
            return;
        }
        match app.machine_sandbox(&machine).await {
            Ok(sb) => break sb,
            // An unreachable machine is retried; its status shows in the Machines popup.
            Err(_) => tokio::time::sleep(Duration::from_secs(60)).await,
        }
    };
    let name = app.db.machines().into_iter().find(|m| m.id == machine).map(|m| format!("{}/{n}", m.name)).unwrap_or(key.clone());
    let ln = Lane { sb, n, name };
    loop {
        if !wanted(&app, &machine, n) {
            app.lanes_up.lock().unwrap().remove(&key);
            return;
        }
        app.wait_unpaused().await;
        match app.db.claim(&ln.name) {
            Some(job) => pipeline::run_job(&app, &ln, job).await,
            None => {
                tokio::select! {
                    _ = app.wake.notified() => {}
                    _ = tokio::time::sleep(Duration::from_secs(20)) => {}
                }
            }
        }
    }
}
