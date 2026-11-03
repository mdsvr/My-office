//! AI providers. Every agent call goes through `ask`, which picks the backend from the agent's
//! model setting, written "provider:model" (e.g. "claude:sonnet", "ollama:qwen2.5:3b"):
//!
//! - Claude Code CLI (your login), tools over MCP (claude.rs)
//! - any OpenAI-compatible chat API: local (Ollama, LM Studio, llama.cpp, vLLM, ...) or hosted
//!   (OpenAI, Anthropic, Gemini, Groq, OpenRouter). The loop below runs on this PC and hands the
//!   model the same office tools (tools.rs), so it works in the same sandbox.
//!
//! API keys come from an environment variable or Windows Credential Manager; never the database,
//! never the sandbox, never a command line.

use crate::tools::{defs, Toolbox};
use crate::{claude, sh, App};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Ask<'a> {
    pub agent: &'static str,
    pub job: Option<i64>,
    pub system: &'a str,
    pub prompt: String,
    /// The office tools in a sandbox space; None = text answer only.
    pub tools: Option<Toolbox>,
    /// Continue an earlier conversation (Repro and Fixer keep their context between rounds).
    pub resume: Option<String>,
    pub max_turns: u32,
    pub secs: u64,
}

pub struct Reply {
    pub json: Value,
    pub session: String,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    ClaudeCli,
    OpenAi,
}

pub struct Provider {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    /// Default API base URL (can be changed in the dashboard, e.g. Ollama on another PC).
    pub base: &'static str,
    /// Environment variable checked before Windows Credential Manager; "" = no key needed.
    pub key_env: &'static str,
    pub local: bool,
}

pub const PROVIDERS: &[Provider] = &[
    Provider { id: "claude", name: "Claude Code (your login)", kind: Kind::ClaudeCli, base: "", key_env: "", local: false },
    Provider { id: "ollama", name: "Ollama (local)", kind: Kind::OpenAi, base: "http://127.0.0.1:11434/v1", key_env: "", local: true },
    Provider { id: "lmstudio", name: "LM Studio (local)", kind: Kind::OpenAi, base: "http://127.0.0.1:1234/v1", key_env: "", local: true },
    Provider { id: "custom", name: "Custom OpenAI-compatible (llama.cpp, vLLM, ...)", kind: Kind::OpenAi, base: "http://127.0.0.1:8080/v1", key_env: "CUSTOM_API_KEY", local: true },
    Provider { id: "openai", name: "OpenAI API", kind: Kind::OpenAi, base: "https://api.openai.com/v1", key_env: "OPENAI_API_KEY", local: false },
    Provider { id: "anthropic", name: "Anthropic API", kind: Kind::OpenAi, base: "https://api.anthropic.com/v1", key_env: "ANTHROPIC_API_KEY", local: false },
    Provider { id: "gemini", name: "Google Gemini API", kind: Kind::OpenAi, base: "https://generativelanguage.googleapis.com/v1beta/openai", key_env: "GEMINI_API_KEY", local: false },
    Provider { id: "groq", name: "Groq", kind: Kind::OpenAi, base: "https://api.groq.com/openai/v1", key_env: "GROQ_API_KEY", local: false },
    Provider { id: "openrouter", name: "OpenRouter", kind: Kind::OpenAi, base: "https://openrouter.ai/api/v1", key_env: "OPENROUTER_API_KEY", local: false },
];

pub fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// "ollama:qwen2.5:3b" → (ollama, "qwen2.5:3b"). A bare name ("opus", from older settings) is Claude.
pub fn split(spec: &str) -> (&'static Provider, String) {
    if let Some((p, m)) = spec.split_once(':') {
        if let Some(p) = provider(p) {
            return (p, m.to_string());
        }
    }
    (&PROVIDERS[0], spec.to_string())
}

pub fn base_url(app: &App, p: &Provider) -> String {
    app.db.get(&format!("base:{}", p.id)).filter(|b| !b.trim().is_empty()).unwrap_or_else(|| p.base.to_string()).trim_end_matches('/').to_string()
}

const KEYRING: &str = "MyOffice";

pub fn key(p: &Provider) -> Option<String> {
    if p.key_env.is_empty() && p.id != "custom" {
        return None;
    }
    std::env::var(p.key_env).ok().filter(|k| !k.is_empty()).or_else(|| keyring::Entry::new(KEYRING, p.id).ok()?.get_password().ok())
}

pub fn set_key(p: &Provider, k: &str) -> Result<(), String> {
    let e = keyring::Entry::new(KEYRING, p.id).map_err(|e| e.to_string())?;
    if k.is_empty() {
        let _ = e.delete_credential();
        Ok(())
    } else {
        e.set_password(k).map_err(|e| e.to_string())
    }
}

/// The models a provider offers (for the dashboard's model picker).
pub async fn models(app: &App, p: &Provider) -> Result<Vec<String>, String> {
    if p.kind == Kind::ClaudeCli {
        return Ok(vec!["haiku".into(), "sonnet".into(), "opus".into()]);
    }
    let mut r = http().get(format!("{}/models", base_url(app, p)));
    if let Some(k) = key(p) {
        r = r.bearer_auth(k);
    }
    let v: Value = r.send().await.map_err(|e| reach(p, &e))?.error_for_status().map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    let mut m: Vec<String> = v["data"].as_array().into_iter().flatten().filter_map(|x| x["id"].as_str()).map(|s| s.trim_start_matches("models/").to_string()).collect();
    m.sort();
    Ok(m)
}

fn http() -> reqwest::Client {
    reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build().unwrap()
}

fn reach(p: &Provider, e: &reqwest::Error) -> String {
    if p.local && e.is_connect() {
        format!("{} is not running (start it, or check its address in Providers)", p.name)
    } else {
        e.to_string()
    }
}

/// Runs one agent turn-set with the agent's model and streams what it does to the dashboard.
pub async fn ask(app: &App, a: Ask<'_>) -> Result<Reply, String> {
    app.wait_budget().await;
    let (p, model) = split(&app.model(a.agent));
    match p.kind {
        Kind::ClaudeCli => claude::ask(app, &a, &model).await,
        Kind::OpenAi => chat(app, &a, p, &model).await,
    }
}

static SESSIONS: Mutex<Option<HashMap<String, Vec<Value>>>> = Mutex::new(None);

/// The OpenAI-compatible tool loop: ask, run the tools it calls in the sandbox, repeat.
async fn chat(app: &App, a: &Ask<'_>, p: &Provider, model: &str) -> Result<Reply, String> {
    let url = format!("{}/chat/completions", base_url(app, p));
    let key = key(p);
    if !p.local && key.is_none() {
        return Err(format!("{} needs an API key: set {} or add it under Providers", p.name, p.key_env));
    }
    let tools: Vec<Value> = a.tools.as_ref().map(|t| defs(t.readonly).into_iter().map(|(n, d, s)| json!({"type": "function", "function": {"name": n, "description": d, "parameters": s}})).collect()).unwrap_or_default();
    let mut messages = a.resume.as_ref().and_then(|s| SESSIONS.lock().unwrap().as_ref().and_then(|m| m.get(s).cloned())).unwrap_or_else(|| vec![json!({"role": "system", "content": a.system})]);
    messages.push(json!({"role": "user", "content": a.prompt}));
    let session = a.resume.clone().unwrap_or_else(|| format!("{}-{}-{}", p.id, std::process::id(), crate::db::now_ms()));
    let deadline = Instant::now() + Duration::from_secs(a.secs);
    let (mut tin, mut tout) = (0i64, 0i64);
    let result = async {
        for _ in 0..a.max_turns.max(1) {
            let mut body = json!({"model": model, "messages": messages, "temperature": 0.2});
            if !tools.is_empty() {
                body["tools"] = json!(tools);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("{} timed out after {} minutes", p.name, a.secs / 60));
            }
            let mut r = http().post(&url).json(&body).timeout(left);
            if let Some(k) = &key {
                r = r.bearer_auth(k);
            }
            let resp = r.send().await.map_err(|e| reach(p, &e))?;
            let status = resp.status();
            let v: Value = resp.json().await.map_err(|e| format!("{}: unreadable answer ({e})", p.name))?;
            if !status.is_success() {
                let m = v["error"]["message"].as_str().map(String::from).unwrap_or_else(|| v.to_string());
                if !tools.is_empty() && m.to_lowercase().contains("tool") {
                    return Err(format!("{model} can't use tools ({}). Pick a model with tool support for this agent.", sh::head(&m, 200)));
                }
                return Err(format!("{} error {status}: {}", p.name, sh::head(&m, 300)));
            }
            tin += v["usage"]["prompt_tokens"].as_i64().unwrap_or(0);
            tout += v["usage"]["completion_tokens"].as_i64().unwrap_or(0);
            let msg = v["choices"][0]["message"].clone();
            let text = msg["content"].as_str().unwrap_or("").trim().to_string();
            let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
            messages.push(json!({"role": "assistant", "content": msg["content"], "tool_calls": if calls.is_empty() { Value::Null } else { json!(calls) }}));
            if calls.is_empty() {
                return Ok(text);
            }
            if !text.is_empty() && !text.starts_with('{') {
                app.log(a.agent, a.job, &text);
            }
            let tb = a.tools.as_ref().ok_or("the model called a tool, but this agent has none")?;
            for c in calls {
                let name = c["function"]["name"].as_str().unwrap_or("");
                let args: Value = serde_json::from_str(c["function"]["arguments"].as_str().unwrap_or("{}")).unwrap_or(json!({}));
                app.tool(a.agent, a.job, &tool_line(name, &args));
                let out = tb.call(name, &args).await.unwrap_or_else(|e| format!("error: {e}"));
                messages.push(json!({"role": "tool", "tool_call_id": c["id"], "content": sh::tail(&out, 30_000)}));
            }
        }
        Err(format!("{model} used all {} turns without a final answer", a.max_turns))
    }
    .await;
    let usd = price(app, p, model).map(|(i, o)| (tin as f64 * i + tout as f64 * o) / 1e6).unwrap_or(0.0);
    app.db.add_tokens(a.agent, tin + tout, usd);
    SESSIONS.lock().unwrap().get_or_insert_with(HashMap::new).insert(session.clone(), messages);
    let text = result?;
    Ok(Reply { json: extract_json(&text), session })
}

/// Optional USD per million input/output tokens for a provider ("price:<id>" = "3,15").
fn price(app: &App, p: &Provider, _model: &str) -> Option<(f64, f64)> {
    let s = app.db.get(&format!("price:{}", p.id))?;
    let (i, o) = s.split_once(',')?;
    Some((i.trim().parse().ok()?, o.trim().parse().ok()?))
}

/// "sh npm test" instead of a JSON blob.
pub fn tool_line(name: &str, args: &Value) -> String {
    let arg = ["cmd", "path", "pattern"].iter().find_map(|k| args[k].as_str()).map(String::from).unwrap_or_else(|| args.to_string());
    format!("{} {}", name.strip_prefix("mcp__office__").unwrap_or(name), sh::head(&arg, 160))
}

/// Models sometimes wrap the JSON in prose or a code fence; take the outermost {...}.
pub fn extract_json(t: &str) -> Value {
    let t = t.trim();
    if let Ok(v) = serde_json::from_str::<Value>(t) {
        return v;
    }
    if let (Some(a), Some(b)) = (t.find('{'), t.rfind('}')) {
        if a < b {
            if let Ok(v) = serde_json::from_str::<Value>(&t[a..=b]) {
                return v;
            }
        }
    }
    Value::Null
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_from_prose() {
        assert_eq!(extract_json("Done.\n```json\n{\"ok\": true}\n```")["ok"], true);
        assert!(extract_json("no json here").is_null());
    }

    use crate::sandbox::Sandbox;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    fn app(name: &str) -> (Arc<App>, std::path::PathBuf) {
        let data = std::env::temp_dir().join(format!("myoffice-llm-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        (Arc::new(App::new(data.clone(), 0).unwrap()), data)
    }

    /// A fake OpenAI-compatible server: first asks for the `write` tool, then answers with JSON.
    /// Proves the loop runs the tool in the sandbox and feeds the result back.
    #[tokio::test]
    async fn openai_tool_loop() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            for turn in 0..2 {
                let (mut s, _) = l.accept().await.unwrap();
                let mut buf = vec![0u8; 65536];
                let mut got = Vec::new();
                loop {
                    let n = s.read(&mut buf).await.unwrap();
                    got.extend_from_slice(&buf[..n]);
                    let t = String::from_utf8_lossy(&got).to_string();
                    if let Some(h) = t.find("\r\n\r\n") {
                        let len: usize = t.lines().find_map(|x| x.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap())).unwrap_or(0);
                        if got.len() >= h + 4 + len {
                            break;
                        }
                    }
                }
                let req = String::from_utf8_lossy(&got).to_string();
                let body = if turn == 0 {
                    assert!(req.contains("\"tools\""), "tools offered");
                    json!({"choices": [{"message": {"content": null, "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "write", "arguments": "{\"path\":\"hello.txt\",\"text\":\"hi\"}"}}]}}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}})
                } else {
                    assert!(req.contains("\"role\":\"tool\"") && req.contains("\"content\":\"ok\""), "tool result sent back");
                    json!({"choices": [{"message": {"content": "Done.\n{\"ok\": true}"}}], "usage": {"prompt_tokens": 20, "completion_tokens": 5}})
                };
                let b = body.to_string();
                let r = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{b}", b.len());
                s.write_all(r.as_bytes()).await.unwrap();
            }
        });
        let (app, data) = app("fake");
        app.db.set("base:custom", &format!("http://127.0.0.1:{port}/v1"));
        app.db.set("model:fixer", "custom:fake-model");
        let tb = Toolbox { sb: Sandbox::local(&data), space: "work__t".into(), readonly: false };
        let r = ask(&app, Ask { agent: "fixer", job: None, system: "sys", prompt: "go".into(), tools: Some(tb), resume: None, max_turns: 5, secs: 60 }).await.unwrap();
        assert_eq!(r.json["ok"], true);
        assert_eq!(std::fs::read_to_string(data.join("sandbox/work__t/hello.txt")).unwrap(), "hi", "tool ran in the sandbox space");
        let _ = std::fs::remove_dir_all(&data);
    }

    /// Real local model through Ollama: `cargo test ollama -- --ignored`.
    /// Any tool-capable model you have: MYOFFICE_TEST_MODEL (default llama3.2:3b).
    #[tokio::test]
    #[ignore]
    async fn ollama_local_model() {
        let (app, data) = app("ollama");
        let model = std::env::var("MYOFFICE_TEST_MODEL").unwrap_or_else(|_| "llama3.2:3b".into());
        app.db.set("model:fixer", &format!("ollama:{model}"));
        let tb = Toolbox { sb: Sandbox::local(&data), space: "work__t".into(), readonly: false };
        let r = ask(&app, Ask { agent: "fixer", job: None, system: "You are a careful assistant. Use the tools when asked.", prompt: "Use the write tool to create the file note.txt containing exactly: local model works. Then reply with only this JSON: {\"done\": true}".into(), tools: Some(tb), resume: None, max_turns: 6, secs: 600 }).await.unwrap();
        let note = std::fs::read_to_string(data.join("sandbox/work__t/note.txt")).unwrap_or_default();
        println!("reply: {} | note.txt: {note:?}", r.json);
        assert!(note.contains("local model works"));
        let _ = std::fs::remove_dir_all(&data);
    }

    #[test]
    fn model_specs() {
        assert_eq!(split("ollama:qwen2.5:3b").0.id, "ollama");
        assert_eq!(split("ollama:qwen2.5:3b").1, "qwen2.5:3b");
        assert_eq!(split("opus").0.id, "claude", "old bare setting means Claude");
        assert_eq!(split("claude:sonnet").1, "sonnet");
        assert_eq!(split("nope:x").0.id, "claude");
    }
}
