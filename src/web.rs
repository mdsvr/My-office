use crate::db::{Job, Repo};
use crate::{llm, pipeline, App, AGENTS};
use axum::extract::{Query, Request, State};
use axum::http::{header, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use regex::Regex;
use rust_embed::RustEmbed;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

/// The built dashboard (web/dist), compiled into the binary.
#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Ui;

type S = State<Arc<App>>;

pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/events", get(events))
        .route("/api/state", get(state))
        .route("/api/job", get(job))
        .route("/api/scan", post(scan))
        .route("/api/issue", post(add_issue))
        .route("/api/repo", post(save_repo))
        .route("/api/repo/delete", post(delete_repo))
        .route("/api/job/action", post(job_action))
        .route("/api/pr/merge", post(merge))
        .route("/api/agent/model", post(set_model))
        .route("/api/providers", get(providers))
        .route("/api/models", get(models))
        .route("/api/provider", post(save_provider))
        .route("/api/agent/test", post(test_agent))
        .route("/api/office/pause", post(pause))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

/// Only this computer's browser may talk to the office: the Host must be localhost
/// (stops DNS rebinding) and POSTs need a custom header (stops other websites' forms).
async fn guard(State(app): S, req: Request, next: Next) -> Response {
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_ok = host == format!("127.0.0.1:{}", app.port) || host == format!("localhost:{}", app.port);
    if !host_ok || (req.method() == Method::POST && !req.headers().contains_key("x-myoffice")) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}

async fn static_file(uri: Uri) -> Response {
    let p = uri.path().trim_start_matches('/');
    let (p, f) = match Ui::get(p) {
        Some(f) if !p.is_empty() => (p, f),
        _ => match Ui::get("index.html") {
            Some(f) => ("index.html", f),
            None => return (StatusCode::NOT_FOUND, "Dashboard not built yet: run `npm run build` in web/, then rebuild.").into_response(),
        },
    };
    let ct = match p.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, ct)], f.data.into_owned()).into_response()
}

fn ok() -> Json<Value> {
    Json(json!({"ok": true}))
}

fn err(m: impl Into<String>) -> Json<Value> {
    Json(json!({"error": m.into()}))
}

/// "Sonnet" for Claude, "ollama · qwen2.5:3b" for everything else.
fn model_label(spec: &str) -> String {
    if spec.is_empty() {
        return "no model".into();
    }
    let (p, m) = llm::split(spec);
    if p.id == "claude" && !m.is_empty() {
        m[..1].to_uppercase() + &m[1..]
    } else {
        format!("{} · {m}", p.id)
    }
}

/// Small view of a job for the Queue and Pull Requests panels.
fn summary(j: &Job) -> Value {
    let d = &j.data;
    json!({
        "id": j.id, "repo": j.repo, "issue": j.issue, "kind": j.kind, "title": j.title, "url": j.url,
        "state": j.state, "why": j.why, "pr_url": j.pr_url, "pr_state": j.pr_state, "updated": j.updated,
        "label": pipeline::label(j),
        "pr_title": d["pr_title"], "file": d["files"][0], "summary": d["fix"]["cause"],
        "agents": d["agents"], "can_merge": d["can_merge"],
    })
}

async fn state(State(app): S) -> Json<Value> {
    let tokens = app.db.tokens_today();
    let live = app.live.lock().unwrap().clone();
    let agents: Map<String, Value> = AGENTS
        .iter()
        .map(|(id, _)| {
            let l = &live[id];
            let tok = tokens.get(*id).and_then(Value::as_i64).unwrap_or(0);
            (id.to_string(), json!({"status": l.status, "job": l.job, "task": l.task, "speech": l.speech, "tool": l.tool, "tokens": (tok + 500) / 1000, "model": model_label(&app.model(id)), "spec": app.model(id)}))
        })
        .collect();
    let machine = app.machine.lock().unwrap().clone();
    Json(json!({
        "paused": app.paused(),
        "machine": machine,
        "counts": app.db.counts(),
        "usd_today": app.db.usd_today(),
        "agents": agents,
        "jobs": app.db.jobs(200).iter().map(summary).collect::<Vec<_>>(),
        "events": app.db.events(200),
        "repos": app.db.repos(),
    }))
}

async fn job(State(app): S, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let id = q.get("id").and_then(|s| s.parse().ok()).unwrap_or(0);
    match app.db.job(id) {
        Some(j) => {
            let mut v = summary(&j);
            v["data"] = j.data;
            Json(v)
        }
        None => err("no such job"),
    }
}

async fn events(State(app): S) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let s = BroadcastStream::new(app.tx.subscribe()).filter_map(|m| m.ok().map(|m| Ok::<_, Infallible>(Event::default().data(m))));
    Sse::new(s).keep_alive(KeepAlive::default())
}

async fn scan(State(app): S) -> Json<Value> {
    app.log("scout", None, "scan requested from the dashboard");
    app.scan.notify_one();
    ok()
}

/// "New Issue": a GitHub issue number/URL, or a bug you describe yourself.
async fn add_issue(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let repo = b["repo"].as_str().unwrap_or("").to_string();
    if app.db.repo(&repo).is_none() {
        return err("add this repository first (Repos button)");
    }
    let input = b["text"].as_str().unwrap_or("").trim().to_string();
    let num = Regex::new(r"^(?:#|.*/issues/)?(\d+)$").unwrap().captures(&input).and_then(|c| c[1].parse::<i64>().ok());
    match num {
        Some(n) => match pipeline::add_issue(&app, &repo, n).await {
            Ok(()) => ok(),
            Err(e) => err(e),
        },
        None if input.len() >= 10 => {
            let title = crate::sh::head(input.lines().next().unwrap_or(""), 100);
            let id = app.db.add_job(&repo, 0, "direct", &title, "", "ready", "described by you", &json!({"body": input, "agents": []}));
            app.log("scout", Some(id), &format!("new bug from you: {title}"));
            app.wake.notify_one();
            ok()
        }
        None => err("enter an issue number or URL, or describe the bug in at least a sentence"),
    }
}

async fn save_repo(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let full = b["full"].as_str().unwrap_or("").trim().trim_end_matches(".git").replace("https://github.com/", "");
    if !Regex::new(r"^[\w.-]+/[\w.-]+$").unwrap().is_match(&full) {
        return err("use the form owner/repo");
    }
    let test_cmd = b["test_cmd"].as_str().unwrap_or("").trim().to_string();
    app.db.save_repo(&Repo { full: full.clone(), test_cmd, auto: b["auto"].as_bool().unwrap_or(false), enabled: true, hunt: b["hunt"].as_bool().unwrap_or(false) });
    app.log("scout", None, &format!("added {full}"));
    app.scan.notify_one();
    ok()
}

async fn delete_repo(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    app.db.delete_repo(b["full"].as_str().unwrap_or(""));
    app.changed();
    ok()
}

async fn job_action(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let id = b["id"].as_i64().unwrap_or(0);
    let Some(j) = app.db.job(id) else { return err("no such job") };
    let (allowed, state, why) = match b["action"].as_str().unwrap_or("") {
        "approve" => (j.state == "approve", "approved", "approved by you"),
        "reject" => (matches!(j.state.as_str(), "approve" | "ready" | "vet"), "dropped", "rejected by you"),
        // Back through Guard (never skip the injection check); `manual` makes Vet accept your call.
        "retry" => (matches!(j.state.as_str(), "dropped" | "skipped" | "blocked"), "vet", "retry requested, Guard checks it first"),
        _ => (false, "", ""),
    };
    if !allowed {
        return err(format!("can't do that while the job is '{}'", j.state));
    }
    app.db.set_state(id, state, why);
    if state == "vet" {
        app.db.update_data(id, |d| d["manual"] = json!(true));
        app.scan.notify_one();
    }
    app.log("submit", Some(id), &format!("{}: {why}", pipeline::label(&j)));
    app.wake.notify_one();
    ok()
}

async fn merge(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let Some(j) = app.db.job(b["id"].as_i64().unwrap_or(0)) else { return err("no such job") };
    match pipeline::merge(&app, &j).await {
        Ok(()) => ok(),
        Err(e) => err(e),
    }
}

async fn set_model(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let agent = b["agent"].as_str().unwrap_or("");
    let model = b["model"].as_str().unwrap_or("").trim().to_string();
    let Some((_, default)) = AGENTS.iter().find(|(a, _)| *a == agent) else { return err("unknown agent") };
    if default.is_empty() {
        return err("this agent runs no model");
    }
    // "provider:model"; a bare Haiku/Sonnet/Opus means Claude.
    let model = if model.contains(':') { model } else { format!("claude:{}", model.to_lowercase()) };
    let (p, m) = llm::split(&model);
    if m.is_empty() || !model.starts_with(&format!("{}:", p.id)) {
        return err("pick a provider and a model");
    }
    app.db.set(&format!("model:{agent}"), &model);
    app.log(agent, None, &format!("switched model to {}", model_label(&model)));
    app.changed();
    ok()
}

async fn test_agent(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let agent = b["agent"].as_str().unwrap_or("").to_string();
    if !AGENTS.iter().any(|(a, m)| *a == agent && !m.is_empty()) {
        return err("this agent runs no model");
    }
    tokio::spawn(async move { pipeline::test_agent(&app, &agent).await });
    ok()
}

async fn pause(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let p = b["paused"].as_bool().unwrap_or(false);
    app.db.set("paused", if p { "1" } else { "0" });
    app.log("watcher", None, if p { "office paused: finishing the current step, then waiting" } else { "office resumed" });
    app.changed();
    ok()
}

// ---------------------------------------------------------------- AI providers

async fn providers(State(app): S) -> Json<Value> {
    Json(json!(llm::PROVIDERS
        .iter()
        .map(|p| json!({
            "id": p.id, "name": p.name, "local": p.local, "cli": p.kind == llm::Kind::ClaudeCli,
            "base": llm::base_url(&app, p), "key_env": p.key_env,
            // Never the key itself: only whether one is there.
            "has_key": llm::key(p).is_some(),
            "price": app.db.get(&format!("price:{}", p.id)).unwrap_or_default(),
        }))
        .collect::<Vec<_>>()))
}

async fn models(State(app): S, Query(q): Query<HashMap<String, String>>) -> Json<Value> {
    let Some(p) = llm::provider(q.get("provider").map(String::as_str).unwrap_or("")) else { return err("unknown provider") };
    match llm::models(&app, p).await {
        Ok(m) => Json(json!(m)),
        Err(e) => err(e),
    }
}

/// Address, API key (to Windows Credential Manager) and optional price of a provider.
async fn save_provider(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let Some(p) = llm::provider(b["id"].as_str().unwrap_or("")) else { return err("unknown provider") };
    if let Some(base) = b["base"].as_str() {
        let base = base.trim();
        if !base.is_empty() && !(base.starts_with("http://") || base.starts_with("https://")) {
            return err("the address must start with http:// or https://");
        }
        app.db.set(&format!("base:{}", p.id), base);
    }
    if let Some(k) = b["key"].as_str() {
        if let Err(e) = llm::set_key(p, k.trim()) {
            return err(format!("could not store the key in Windows Credential Manager: {e}"));
        }
    }
    if let Some(pr) = b["price"].as_str() {
        app.db.set(&format!("price:{}", p.id), pr.trim());
    }
    app.log("watcher", None, &format!("provider {} updated", p.name));
    app.changed();
    ok()
}
