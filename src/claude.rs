use crate::{sh, App};
use serde_json::Value;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

pub struct Ask<'a> {
    pub worker: &'static str,
    pub job: Option<i64>,
    pub model: &'a str,
    pub system: &'static str,
    pub prompt: String,
    pub cwd: &'a Path,
    /// true: may read/edit files and run commands in `cwd`. false: text answer only.
    pub tools: bool,
    /// Continue an earlier conversation (the Fixer keeps its context between rounds).
    pub resume: Option<String>,
    pub max_turns: u32,
    pub secs: u64,
}

pub struct Reply {
    pub text: String,
    pub json: Value,
    pub session: String,
}

const TOOLS: &str = "Read,Edit,Write,Glob,Grep,Bash";
// ponytail: Claude Code's permission rules are the only fence here, and Bash runs on your own PC.
// ApolloOffice runs builds in Docker; do the same before pointing this at repos you don't trust.
const DENY: &[&str] = &["Bash(git push:*)", "Bash(git remote:*)", "Bash(git config:*)", "Bash(gh:*)", "Bash(curl:*)", "Bash(wget:*)", "Bash(ssh:*)"];

/// Runs `claude -p` and streams what it does to the dashboard.
pub async fn ask(app: &App, a: Ask<'_>) -> Result<Reply, String> {
    let turns = a.max_turns.to_string();
    let mut args: Vec<&str> = vec!["-p", "--output-format", "stream-json", "--verbose", "--model", a.model, "--max-turns", turns.as_str(), "--append-system-prompt", a.system];
    if a.tools {
        args.extend(["--tools", TOOLS, "--allowedTools", TOOLS, "--disallowedTools"]);
        args.extend(DENY.iter().copied());
    } else {
        args.extend(["--tools", ""]);
    }
    if let Some(s) = &a.resume {
        args.extend(["--resume", s.as_str()]);
    }
    let bin = std::env::var("MYOFFICE_CLAUDE").unwrap_or_else(|_| "claude".into());
    let mut cmd = tokio::process::Command::new(&bin);
    cmd.args(&args)
        .current_dir(a.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_ENTRYPOINT");
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {bin}: {e}"))?;

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
                                    app.log(a.worker, a.job, t);
                                }
                            }
                            Some("tool_use") => app.log(a.worker, a.job, &format!("→ {} {}", c["name"].as_str().unwrap_or("?"), sh::head(&c["input"].to_string(), 200))),
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
    let text = r["result"].as_str().unwrap_or("").to_string();
    if r["is_error"].as_bool() == Some(true) {
        return Err(format!("claude error: {}", sh::head(&text, 300)));
    }
    if let Some(usd) = r["total_cost_usd"].as_f64() {
        app.log(a.worker, a.job, &format!("finished (${usd:.3})"));
    }
    Ok(Reply { json: extract_json(&text), session: r["session_id"].as_str().unwrap_or("").into(), text })
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
