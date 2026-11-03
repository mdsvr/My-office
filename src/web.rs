use crate::db::{Job, Repo};
use crate::{llm, pipeline, App, AGENTS};
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, Method, StatusCode, Uri};
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
        .route("/api/machine", post(save_machine))
        .route("/api/machine/delete", post(delete_machine))
        .route("/api/token", post(api_token))
        .route("/v1/report", post(v1_report))
        .route("/v1/hook/sentry", post(v1_sentry))
        .route("/api/agent/test", post(test_agent))
        .route("/api/office/pause", post(pause))
        .route("/api/settings", get(get_settings).post(save_settings))
        .route("/api/notify/test", post(notify_test))
        .route("/api/update", get(update_check))
        .route("/api/job/pr-text", post(pr_text))
        .route("/login", get(login))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

/// Only this computer's browser may talk to the office: the Host must be localhost
/// (stops DNS rebinding), POSTs need a custom header (stops other websites' forms), and the
/// browser needs the session cookie from the one-time login link.
async fn guard(State(app): S, req: Request, next: Next) -> Response {
    // The report API and webhooks may come from other machines (through a tunnel); they check
    // the API token themselves.
    if req.uri().path().starts_with("/v1/") {
        return next.run(req).await;
    }
    let host = req.headers().get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let host_ok = host == format!("127.0.0.1:{}", app.port) || host == format!("localhost:{}", app.port);
    if !host_ok || (req.method() == Method::POST && !req.headers().contains_key("x-myoffice")) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if req.uri().path() == "/login" || logged_in(&app, req.headers()) {
        return next.run(req).await;
    }
    if req.uri().path().starts_with("/api/") || req.uri().path() == "/events" {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        "<!doctype html><meta charset=utf-8><title>MY OFFICE</title><body style='font:16px system-ui;padding:40px;background:#fdfbf7'><h2>MY OFFICE</h2><p>Open the one-time login link printed in the terminal where MY OFFICE runs.</p><p>New link: <code>myoffice login-link</code></p>",
    )
        .into_response()
}

fn same(a: &str, b: &str) -> bool {
    !a.is_empty() && a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The last 5 logins stay valid (PC browser, phone, ...); a 6th logs out the oldest.
const SESSIONS: usize = 5;

fn sessions(app: &App) -> Vec<String> {
    app.db.get("sessions").unwrap_or_default().split(',').filter(|s| !s.is_empty()).map(String::from).collect()
}

fn logged_in(app: &App, h: &HeaderMap) -> bool {
    let all = sessions(app);
    h.get_all(header::COOKIE).iter().filter_map(|v| v.to_str().ok()).flat_map(|v| v.split(';')).any(|c| c.trim().strip_prefix("myoffice=").is_some_and(|v| all.iter().any(|s| same(v, s))))
}

/// The one-time login link: trades its key for a session cookie, then the key is used up.
async fn login(State(app): S, h: HeaderMap, Query(q): Query<HashMap<String, String>>) -> Response {
    let key = app.db.get("login_key").unwrap_or_default();
    if !same(q.get("key").map(String::as_str).unwrap_or(""), &key) {
        // An old link in a browser that is already logged in: just open the office.
        if logged_in(&app, &h) {
            return (StatusCode::SEE_OTHER, [(header::LOCATION, "/")]).into_response();
        }
        return (
            StatusCode::FORBIDDEN,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<!doctype html><meta charset=utf-8><title>MY OFFICE</title><body style='font:16px system-ui;padding:40px;background:#fdfbf7'><h2>MY OFFICE</h2><p>This login link was already used (each link works once).</p><p>Get a new one: open a terminal in <code>F:\\MyOffice</code> and run <code>target\\release\\myoffice.exe login-link</code></p>",
        )
            .into_response();
    }
    let session = crate::sources::new_token();
    let mut all = sessions(&app);
    all.push(session.clone());
    let keep = all.len().saturating_sub(SESSIONS);
    app.db.set("sessions", &all[keep..].join(","));
    app.db.set("login_key", "");
    (
        StatusCode::SEE_OTHER,
        [(header::SET_COOKIE, format!("myoffice={session}; HttpOnly; SameSite=Strict; Path=/; Max-Age=31536000")), (header::LOCATION, "/".to_string())],
    )
        .into_response()
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
        "machines": machines(&app),
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
    app.log("scout", None, if app.paused() { "scan requested, but the office is paused: press Resume office" } else { "scan requested from the dashboard" });
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
    app.db.save_repo(&Repo { full: full.clone(), test_cmd, auto: b["auto"].as_bool().unwrap_or(false), enabled: true, hunt: b["hunt"].as_bool().unwrap_or(false), ci: b["ci"].as_bool().unwrap_or(true) });
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
    app.log("submit", Some(id), &format!("{}: {why}{}", pipeline::label(&j), if app.paused() { " (the office is paused: press Resume office)" } else { "" }));
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
    let login = crate::claude::login_status(&app.data).await;
    Json(json!(llm::PROVIDERS
        .iter()
        .map(|p| json!({
            "login": if p.kind == llm::Kind::ClaudeCli { login.clone() } else { Value::Null },
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

// ---------------------------------------------------------------- machines (workers)

fn machines(app: &App) -> Vec<Value> {
    let status = app.machine_status.lock().unwrap().clone();
    let up = app.lanes_up.lock().unwrap().clone();
    app.db
        .machines()
        .into_iter()
        .map(|m| {
            let running = up.iter().filter(|k| k.starts_with(&format!("{}#", m.id))).count();
            json!({"id": m.id, "name": m.name, "host": m.host, "lanes": m.lanes, "enabled": m.enabled, "running": running,
                   "status": status.get(&m.id).cloned().unwrap_or_else(|| "starting...".into())})
        })
        .collect()
}

/// Add or change a machine. Remote ones are another computer's Docker, reached over SSH with a key.
async fn save_machine(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let lanes = b["lanes"].as_i64().unwrap_or(1);
    if !(1..=16).contains(&lanes) {
        return err("lanes must be 1 to 16");
    }
    let id = b["id"].as_str().unwrap_or("").trim().to_string();
    let existing = app.db.machines().into_iter().find(|m| m.id == id);
    let host = if id == "me" { String::new() } else { b["host"].as_str().unwrap_or("").trim().to_string() };
    if id != "me" && !valid_host(&host) {
        return err("use ssh://user@host (the machine needs Docker and your SSH key)");
    }
    let name = b["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).map(String::from).or(existing.as_ref().map(|m| m.name.clone())).unwrap_or_else(|| host.trim_start_matches("ssh://").into());
    let id = if id.is_empty() { format!("m{}", crate::db::now_ms()) } else { id };
    app.db.save_machine(&crate::db::Machine { id: id.clone(), name: name.clone(), host, lanes, enabled: b["enabled"].as_bool().unwrap_or(true) });
    // A changed address is reconnected on next use.
    app.remote.lock().await.remove(&id);
    app.machine_status.lock().unwrap().remove(&id);
    app.log("watcher", None, &format!("machine {name}: {lanes} lane(s)"));
    app.changed();
    ok()
}

async fn delete_machine(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let id = b["id"].as_str().unwrap_or("");
    if id == "me" {
        return err("this PC can't be removed; set it to fewer lanes instead");
    }
    app.db.delete_machine(id);
    app.remote.lock().await.remove(id);
    app.changed();
    ok()
}

/// `ssh://user@host` or `ssh://user@host:port`, nothing else (it ends up in a docker command).
fn valid_host(h: &str) -> bool {
    Regex::new(r"^ssh://[\w.-]+@[\w.-]+(:\d+)?$").unwrap().is_match(h)
}

#[cfg(test)]
mod tests {
    #[test]
    fn machine_hosts() {
        assert!(super::valid_host("ssh://me@build-box"));
        assert!(super::valid_host("ssh://me@10.0.0.5:2222"));
        assert!(!super::valid_host("build-box"));
        assert!(!super::valid_host("ssh://me@box; rm -rf /"));
        assert!(!super::valid_host("tcp://box:2375"));
    }
}

// ---------------------------------------------------------------- outside bug sources

/// Constant-time comparison with the stored API token.
fn token_ok(app: &App, given: &str) -> bool {
    let Some(t) = app.db.get("api_token").filter(|t| !t.is_empty()) else { return false };
    t.len() == given.len() && t.bytes().zip(given.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
}

fn bearer(h: &HeaderMap) -> String {
    h.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).and_then(|v| v.strip_prefix("Bearer ")).unwrap_or("").trim().to_string()
}

fn v1_answer(r: Result<i64, String>) -> Response {
    match r {
        Ok(id) => (StatusCode::OK, Json(json!({"ok": true, "id": id}))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({"error": e}))).into_response(),
    }
}

/// POST /v1/report  {"repo": "owner/repo", "title": "...", "body": "...", "url": "..."}
async fn v1_report(State(app): S, h: HeaderMap, Json(b): Json<Value>) -> Response {
    if !token_ok(&app, &bearer(&h)) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing or wrong API token"}))).into_response();
    }
    let s = |k: &str| b[k].as_str().unwrap_or("").to_string();
    v1_answer(crate::sources::report(&app, &s("repo"), "report", &s("title"), &s("body"), &s("url")))
}

/// POST /v1/hook/sentry?repo=owner/repo&token=...  (Sentry alert webhook)
async fn v1_sentry(State(app): S, h: HeaderMap, Query(q): Query<HashMap<String, String>>, Json(b): Json<Value>) -> Response {
    let given = q.get("token").cloned().unwrap_or_else(|| bearer(&h));
    if !token_ok(&app, &given) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "missing or wrong API token"}))).into_response();
    }
    let (title, body, url) = crate::sources::sentry(&b);
    v1_answer(crate::sources::report(&app, q.get("repo").map(String::as_str).unwrap_or(""), "sentry", &title, &body, &url))
}

/// Create a new API token (shown once) or revoke it.
async fn api_token(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    if b["op"] == "revoke" {
        app.db.set("api_token", "");
        app.log("watcher", None, "API token revoked");
        return ok();
    }
    let t = crate::sources::new_token();
    app.db.set("api_token", &t);
    app.log("watcher", None, "new API token created (old one stops working)");
    Json(json!({"ok": true, "token": t}))
}

// ---------------------------------------------------------------- settings, update check, PR text

async fn get_settings(State(app): S) -> Json<Value> {
    let g = |k: &str| app.db.get(k).unwrap_or_default();
    Json(json!({
        "budget_usd": g("budget_usd"), "ntfy_topic": g("ntfy_topic"), "ntfy_server": g("ntfy_server"),
        "has_api_token": !g("api_token").is_empty(), "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn save_settings(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    if let Some(v) = b["budget_usd"].as_str() {
        let v = v.trim();
        if !v.is_empty() && v.parse::<f64>().map_or(true, |x| x < 0.0) {
            return err("the budget is a number of USD per day, or empty for none");
        }
        app.db.set("budget_usd", v);
    }
    for k in ["ntfy_topic", "ntfy_server"] {
        if let Some(v) = b[k].as_str() {
            app.db.set(k, v.trim());
        }
    }
    app.log("watcher", None, "settings saved");
    app.changed();
    ok()
}

async fn notify_test(State(app): S) -> Json<Value> {
    if app.db.get("ntfy_topic").unwrap_or_default().is_empty() {
        return err("set an ntfy topic first");
    }
    app.notify("MY OFFICE", "Test notification: it works.");
    ok()
}

/// Compares this version with the newest GitHub release. Never installs anything by itself:
/// the files aren't signed, so you download and replace the exe yourself.
async fn update_check(State(app): S) -> Json<Value> {
    let repo = std::env::var("MYOFFICE_UPDATE_REPO").unwrap_or_else(|_| "mdsvr/My-office".into());
    let current = env!("CARGO_PKG_VERSION");
    match crate::sh::ok("gh", &["api", &format!("repos/{repo}/releases/latest"), "-q", "[.tag_name, .html_url] | @tsv"], &app.data, 30).await {
        Ok(out) => {
            let mut it = out.trim().split('\t');
            let (tag, url) = (it.next().unwrap_or("").to_string(), it.next().unwrap_or("").to_string());
            Json(json!({"current": current, "latest": tag, "newer": newer(&tag, current), "url": url}))
        }
        Err(_) => Json(json!({"current": current, "latest": null, "newer": false, "note": "no release published yet"})),
    }
}

/// "v0.3.0" vs "0.2.9" → true.
fn newer(tag: &str, current: &str) -> bool {
    let v = |s: &str| s.trim_start_matches('v').split('.').map(|x| x.parse::<u64>().unwrap_or(0)).collect::<Vec<_>>();
    v(tag) > v(current)
}

/// Your edits to the PR title/text (or a follow-up's reply) before approving.
async fn pr_text(State(app): S, Json(b): Json<Value>) -> Json<Value> {
    let id = b["id"].as_i64().unwrap_or(0);
    let Some(j) = app.db.job(id) else { return err("no such job") };
    if j.state != "approve" {
        return err("only a fix waiting for your approval can be edited");
    }
    if let Some(body) = b["pr_body"].as_str() {
        // Many projects require it, and hiding it can get you banned.
        if !body.contains("Generated with AI") {
            return err("keep the line saying it was generated with AI");
        }
    }
    if b["pr_title"].as_str().is_some_and(|t| t.trim().is_empty()) {
        return err("the title can't be empty");
    }
    app.db.update_data(id, |d| {
        for k in ["pr_title", "pr_body", "reply"] {
            if let Some(v) = b[k].as_str() {
                d[k] = json!(v);
            }
        }
    });
    app.changed();
    ok()
}

#[cfg(test)]
mod more_tests {
    #[test]
    fn versions() {
        assert!(super::newer("v0.2.0", "0.1.0"));
        assert!(super::newer("0.1.10", "0.1.9"));
        assert!(!super::newer("v0.1.0", "0.1.0"));
        assert!(!super::newer("", "0.1.0"));
    }

    #[test]
    fn constant_time_compare() {
        assert!(super::same("abc", "abc"));
        assert!(!super::same("abc", "abd"));
        assert!(!super::same("", ""), "an empty secret never matches");
    }
}
