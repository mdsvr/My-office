//! Claude Code CLI backend (your Claude login). Claude gets none of its built-in tools: only the
//! office tools over MCP (mcp.rs), which run in the sandbox. It runs in an empty folder with no
//! settings files and no account connectors, so there is nothing of yours for it to see.

use crate::llm::{extract_json, tool_line, Ask, Reply};
use crate::{mcp, sandbox, sh, App};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// The real claude.exe. `claude` on PATH may be an older copy or a .cmd wrapper
/// (Rust can't pass multi-line arguments through .cmd files).
/// Is Claude Code logged in? For the Settings popup: logged_in, method (claude.ai = subscription),
/// plan. Never the email or org.
pub async fn login_status(data: &Path) -> Value {
    let o = sh::run(&claude_bin(), &["auth", "status"], data, 30).await;
    let v: Value = serde_json::from_str(o.out.trim()).unwrap_or(Value::Null);
    if v.is_null() {
        return json!({"logged_in": false, "note": format!("Claude Code didn't answer: {}", sh::head(&format!("{}{}", o.out, o.err), 200))});
    }
    json!({"logged_in": v["loggedIn"], "method": v["authMethod"], "plan": v["subscriptionType"]})
}

fn claude_bin() -> String {
    if let Ok(b) = std::env::var("MYOFFICE_CLAUDE") {
        return b;
    }
    if let Ok(appdata) = std::env::var("APPDATA") {
        let npm = Path::new(&appdata).join(r"npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe");
        if npm.exists() {
            return npm.to_string_lossy().into_owned();
        }
    }
    "claude".into()
}

pub async fn ask(app: &App, a: &Ask<'_>, model: &str) -> Result<Reply, String> {
    let turns = a.max_turns.to_string();
    // --setting-sources= : load no settings files, so neither your hooks/plugins nor a cloned
    // repo's .claude/settings.json can run anything. Your login still works.
    let mut args: Vec<&str> = vec!["-p", "--setting-sources=", "--output-format", "stream-json", "--verbose", "--model", model, "--max-turns", &turns, "--append-system-prompt", a.system, "--tools", ""];
    let mut cfg = None;
    if let Some(t) = &a.tools {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let f = sandbox::tmp_file(&app.data, "json");
        let server = json!({"type": "stdio", "command": exe.to_string_lossy(), "args": mcp::args(&t.sb, &t.space, t.readonly)});
        std::fs::write(&f, json!({"mcpServers": {"office": server}}).to_string()).map_err(|e| e.to_string())?;
        cfg = Some(f);
    }
    // Always strict: otherwise Claude also loads the connectors of your claude.ai account
    // (Gmail, Drive, Notion, ...) while reading issue text written by strangers.
    let cfg_s = cfg.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| r#"{"mcpServers":{}}"#.into());
    args.extend(["--strict-mcp-config", "--mcp-config", &cfg_s]);
    if cfg.is_some() {
        args.extend(["--allowedTools", "mcp__office"]);
    }
    if let Some(s) = &a.resume {
        args.extend(["--resume", s.as_str()]);
    }
    let bin = claude_bin();
    let empty = app.data.join("empty");
    let _ = std::fs::create_dir_all(&empty);
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args(&args)
        .current_dir(&empty)
        .env("MCP_TOOL_TIMEOUT", "3700000") // long test runs: up to the sh tool's 60 min
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT")
        // Always your Claude Code login (subscription), never API billing: an API key in the
        // environment would take precedence over the login.
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("ANTHROPIC_AUTH_TOKEN");
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let result = match cmd.spawn() {
        Ok(child) => run(app, a, child).await,
        Err(e) => Err(format!("cannot start {bin}: {e}")),
    };
    if let Some(f) = cfg {
        let _ = std::fs::remove_file(f);
    }
    result
}

async fn run(app: &App, a: &Ask<'_>, mut child: tokio::process::Child) -> Result<Reply, String> {

    // The prompt goes through stdin: issue texts can be longer than a command line allows.
    let mut stdin = child.stdin.take().unwrap();
    let prompt = a.prompt.clone();
    tokio::spawn(async move {
        let _ = stdin.write_all(prompt.as_bytes()).await;
    });
    let mut stderr = child.stderr.take().unwrap();
    let err_task = tokio::spawn(async move {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s).await;
        s
    });

    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut result: Option<Value> = None;
    let read = async {
        while let Ok(Some(l)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&l) else { continue };
            match v["type"].as_str() {
                Some("assistant") => {
                    for c in v["message"]["content"].as_array().into_iter().flatten() {
                        match c["type"].as_str() {
                            Some("text") => {
                                let t = c["text"].as_str().unwrap_or("").trim();
                                if !t.is_empty() && !t.starts_with('{') && !t.starts_with("```") {
                                    app.log(a.agent, a.job, t);
                                }
                            }
                            Some("tool_use") => app.tool(a.agent, a.job, &tool_line(c["name"].as_str().unwrap_or("?"), &c["input"])),
                            _ => {}
                        }
                    }
                }
                Some("result") => result = Some(v),
                _ => {}
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(a.secs), read).await.is_err() {
        let _ = child.kill().await;
        return Err(format!("claude timed out after {} minutes", a.secs / 60));
    }
    let _ = child.wait().await;
    let stderr = err_task.await.unwrap_or_default();

    let r = result.ok_or_else(|| format!("claude ended without an answer: {}", sh::tail(&stderr, 400)))?;
    let u = &r["usage"];
    // New input + output. Cache reads/writes are left out: they re-count the same context every turn.
    let tok = ["input_tokens", "output_tokens"].iter().map(|k| u[k].as_i64().unwrap_or(0)).sum();
    app.db.add_tokens(a.agent, tok, r["total_cost_usd"].as_f64().unwrap_or(0.0));
    let text = r["result"].as_str().unwrap_or("");
    if r["is_error"].as_bool() == Some(true) {
        return Err(format!("claude error: {}", sh::head(text, 300)));
    }
    Ok(Reply { json: extract_json(text), session: r["session_id"].as_str().unwrap_or("").into(), text: text.to_string() })
}
