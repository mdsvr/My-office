//! Bug sources besides GitHub issues. Everything enters as "vet", so Guard checks it first:
//! it's text the office didn't write.
//!
//! - failed CI on the default branch (polled with `gh`, no setup or tunnel needed)
//! - `POST /v1/report` from your own tools and scripts (token in the Authorization header)
//! - Sentry alerts at `POST /v1/hook/sentry?repo=owner/repo` (needs a tunnel to reach this PC)

use crate::db::Repo;
use crate::{sh, App};
use serde_json::{json, Value};

/// Called by Scout: the latest run of each workflow on the default branch; if it failed and is
/// new, it becomes a job with the failure log. The first look only records what's there.
pub async fn ci_failures(app: &App, r: &Repo) -> Result<(), String> {
    let default = sh::ok("gh", &["repo", "view", &r.full, "--json", "defaultBranchRef", "-q", ".defaultBranchRef.name"], &app.data, 60).await?.trim().to_string();
    let raw = sh::ok("gh", &["run", "list", "-R", &r.full, "-b", &default, "-L", "30", "--json", "databaseId,workflowName,conclusion,status,url,displayTitle"], &app.data, 60).await?;
    let runs: Vec<Value> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    let key = format!("ci_seen:{}", r.full);
    let first_look = app.db.get(&key).is_none();
    let seen: i64 = app.db.get(&key).and_then(|s| s.parse().ok()).unwrap_or(0);
    let newest = runs.iter().filter_map(|x| x["databaseId"].as_i64()).max().unwrap_or(seen).max(seen);
    app.db.set(&key, &newest.to_string());
    if first_look {
        return Ok(());
    }
    for run in latest_failures(&runs, seen) {
        let id = run["databaseId"].as_i64().unwrap_or(0);
        let workflow = run["workflowName"].as_str().unwrap_or("CI");
        let title = format!("CI failing: {workflow} on {default}");
        if app.db.jobs(300).iter().any(|j| j.repo == r.full && j.title == title && !matches!(j.state.as_str(), "done" | "dropped" | "skipped" | "blocked")) {
            continue;
        }
        let log = sh::run("gh", &["run", "view", &id.to_string(), "-R", &r.full, "--log-failed"], &app.data, 120).await;
        let body = format!("The `{workflow}` workflow fails on `{default}` ({}).\n\nCommit: {}\n\nFailing steps (last part of the log):\n```\n{}\n```", run["url"].as_str().unwrap_or(""), run["displayTitle"].as_str().unwrap_or(""), sh::tail(&log.out, 6000));
        let jid = app.db.add_job(&r.full, 0, "ci", &title, run["url"].as_str().unwrap_or(""), "vet", "from a failed CI run", &json!({"body": body, "agents": ["scout"]}));
        app.log("scout", Some(jid), &format!("{}: {title}", r.full));
    }
    Ok(())
}

/// Per workflow, its newest completed run, if that failed and is newer than `seen`.
/// (A failure followed by a green run was flaky or already fixed.)
fn latest_failures(runs: &[Value], seen: i64) -> Vec<Value> {
    let mut latest: std::collections::BTreeMap<String, &Value> = Default::default();
    for r in runs.iter().filter(|r| r["status"] == "completed") {
        let w = r["workflowName"].as_str().unwrap_or("").to_string();
        let newer = latest.get(&w).map_or(true, |x| r["databaseId"].as_i64() > x["databaseId"].as_i64());
        if newer {
            latest.insert(w, r);
        }
    }
    latest.into_values().filter(|r| r["conclusion"] == "failure" && r["databaseId"].as_i64().unwrap_or(0) > seen).cloned().collect()
}

/// A bug report from outside (API or webhook): enters at Guard like everything else.
pub fn report(app: &App, repo: &str, kind: &str, title: &str, body: &str, url: &str) -> Result<i64, String> {
    if app.db.repo(repo).is_none() {
        return Err(format!("{repo} is not one of the office's repositories"));
    }
    let title = sh::head(title.trim(), 120);
    if title.is_empty() {
        return Err("a report needs a title".into());
    }
    if app.db.jobs(300).iter().any(|j| j.repo == repo && j.title == title && !matches!(j.state.as_str(), "done" | "dropped" | "skipped" | "blocked")) {
        return Err("the same report is already being worked on".into());
    }
    let id = app.db.add_job(repo, 0, kind, &title, url, "vet", &format!("reported via {kind}"), &json!({"body": sh::head(body, 20_000), "agents": []}));
    app.log("scout", Some(id), &format!("new {kind} report for {repo}: {title}"));
    app.scan.notify_one();
    Ok(id)
}

/// Title, text and link from a Sentry alert (legacy webhook or integration platform payload).
pub fn sentry(b: &Value) -> (String, String, String) {
    let ev = if b["data"]["event"].is_object() { &b["data"]["event"] } else { &b["event"] };
    let title = ev["title"].as_str().or(b["message"].as_str()).or(b["data"]["issue"]["title"].as_str()).unwrap_or("Sentry error").to_string();
    let url = ev["web_url"].as_str().or(b["url"].as_str()).unwrap_or("").to_string();
    let mut text = format!("Sentry reports: {title}\n");
    if let Some(c) = ev["culprit"].as_str().or(b["culprit"].as_str()) {
        text.push_str(&format!("Culprit: {c}\n"));
    }
    for ex in ev["exception"]["values"].as_array().into_iter().flatten() {
        text.push_str(&format!("\n{}: {}\n", ex["type"].as_str().unwrap_or("Error"), ex["value"].as_str().unwrap_or("")));
        let frames = ex["stacktrace"]["frames"].as_array().cloned().unwrap_or_default();
        for f in frames.iter().rev().take(12) {
            text.push_str(&format!("  at {} ({}:{})\n", f["function"].as_str().unwrap_or("?"), f["filename"].as_str().unwrap_or("?"), f["lineno"].as_i64().unwrap_or(0)));
        }
    }
    (title, text, url)
}

/// A fresh random token for the report API.
pub fn new_token() -> String {
    let mut b = [0u8; 24];
    getrandom::getrandom(&mut b).expect("random");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_new_latest_failures() {
        let r = |id: i64, w: &str, c: &str| json!({"databaseId": id, "workflowName": w, "conclusion": c, "status": "completed"});
        let runs = vec![r(10, "CI", "failure"), r(9, "CI", "success"), r(8, "Lint", "failure"), r(11, "Lint", "success"), r(12, "Docs", "failure")];
        let got: Vec<i64> = latest_failures(&runs, 9).iter().map(|x| x["databaseId"].as_i64().unwrap()).collect();
        assert_eq!(got, vec![10, 12], "CI#10 and Docs#12 fail now (by workflow name); Lint recovered");
        assert!(latest_failures(&runs, 12).is_empty(), "already seen");
    }

    #[test]
    fn sentry_payload() {
        let b = json!({"data": {"event": {"title": "ZeroDivisionError: division by zero", "web_url": "https://sentry.io/x/1", "culprit": "stats.mean",
            "exception": {"values": [{"type": "ZeroDivisionError", "value": "division by zero", "stacktrace": {"frames": [{"function": "mean", "filename": "stats.py", "lineno": 3}]}}]}}}});
        let (t, text, url) = sentry(&b);
        assert_eq!(t, "ZeroDivisionError: division by zero");
        assert!(text.contains("at mean (stats.py:3)"));
        assert_eq!(url, "https://sentry.io/x/1");
        assert_eq!(new_token().len(), 48);
    }
}
