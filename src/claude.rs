use crate::{mcp, sandbox, sh, App};
use serde_json::{json, Value};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub struct Ask<'a> {
    pub agent: &'static str,
    pub job: Option<i64>,
    pub system: &'static str,
    pub prompt: String,
    /// The sandbox space whose office tools it gets; None = text answer only, no tools.
    pub space: Option<&'a str>,
    /// Only read/grep/ls (Hunter).
    pub readonly: bool,
    /// Continue an earlier conversation (Repro and Fixer keep their context between rounds).
    pub resume: Option<String>,
    pub max_turns: u32,
    pub secs: u64,
}

pub struct Reply {
    pub json: Value,
    pub session: String,
}

/// The real claude.exe. `claude` on PATH may be an older copy or a .cmd wrapper
/// (Rust can't pass multi-line arguments through .cmd files).
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

/// Runs `claude -p` with the agent's model and streams what it does to the dashboard.
///
/// Claude gets none of its built-in tools: only the office's MCP tools, which run inside the
/// sandbox space. It runs in an empty folder, so there is nothing of yours for it to see.
pub async fn ask(app: &App, a: Ask<'_>) -> Result<Reply, String> {
    let model = app.model(a.agent);
    let turns = a.max_turns.to_string();
    // --setting-sources= : load no settings files, so neither your hooks/plugins nor a cloned
    // repo's .claude/settings.json can run anything. Your login still works.
    let mut args: Vec<&str> = vec!["-p", "--setting-sources=", "--output-format", "stream-json", "--verbose", "--model", &model, "--max-turns", &turns, "--append-system-prompt", a.system, "--tools", ""];
    let mut cfg = None;
    if let Some(sp) = a.space {
        let sb = app.sandbox().await;
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let f = sandbox::tmp_file(&app.data, "json");
        let server = json!({"type": "stdio", "command": exe.to_string_lossy(), "args": mcp::args(sb, sp, a.readonly)});
        std::fs::write(&f, json!({"mcpServers": {"office": server}}).to_string()).map_err(|e| e.to_string())?;
        cfg = Some(f);
    }
    let cfg_s = cfg.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    if cfg.is_some() {
        args.extend(["--mcp-config", &cfg_s, "--strict-mcp-config", "--allowedTools", "mcp__office"]);
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
        .env_remove("CLAUDE_CODE_ENTRYPOINT");
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let result = match cmd.spawn() {
        Ok(child) => run(app, &a, child).await,
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
                            Some("tool_use") => app.tool(a.agent, a.job, &tool_line(c)),
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
    Ok(Reply { json: extract_json(text), session: r["session_id"].as_str().unwrap_or("").into() })
}

/// "sh npm test" instead of "mcp__office__sh {\"cmd\": ...}".
fn tool_line(c: &Value) -> String {
    let i = &c["input"];
    let arg = ["cmd", "path", "pattern"].iter().find_map(|k| i[k].as_str()).map(String::from).unwrap_or_else(|| i.to_string());
    let name = c["name"].as_str().unwrap_or("?");
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
    #[test]
    fn json_from_prose() {
        assert_eq!(super::extract_json("Done.\n```json\n{\"ok\": true}\n```")["ok"], true);
        assert!(super::extract_json("no json here").is_null());
    }
}
