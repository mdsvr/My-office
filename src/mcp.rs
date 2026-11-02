//! `myoffice mcp --data D --space S --mode docker|local [--readonly]`: the only tools Claude gets.
//! A stdio MCP server (one JSON-RPC message per line) whose tools run inside a sandbox space.
//! Its stdout carries JSON-RPC only; never print anything else here.

use crate::sandbox::{Mode, Sandbox};
use crate::sh::{self, q};
use regex::Regex;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub struct Opts {
    pub sb: Sandbox,
    pub space: String,
    pub readonly: bool,
}

/// Command-line arguments for the MCP server (used to write Claude's --mcp-config).
pub fn args(sb: &Sandbox, space: &str, readonly: bool) -> Vec<String> {
    let mut v = vec!["mcp".into(), "--data".into(), sb.data.to_string_lossy().into_owned(), "--space".into(), space.into(), "--mode".into(), if sb.mode == Mode::Docker { "docker" } else { "local" }.into()];
    if readonly {
        v.push("--readonly".into());
    }
    v
}

pub fn parse(a: &[String]) -> Option<Opts> {
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let mode = if get("--mode")? == "docker" { Mode::Docker } else { Mode::Local };
    Some(Opts { sb: Sandbox { mode, data: PathBuf::from(get("--data")?) }, space: get("--space")?, readonly: a.iter().any(|x| x == "--readonly") })
}

pub async fn serve(o: Opts) {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Ok(Some(l)) = lines.next_line().await {
        let Ok(m) = serde_json::from_str::<Value>(&l) else { continue };
        if m.get("id").is_none() {
            continue; // notification, no answer
        }
        let r = handle(&o, &m).await;
        let _ = out.write_all(format!("{r}\n").as_bytes()).await;
        let _ = out.flush().await;
    }
}

async fn handle(o: &Opts, m: &Value) -> Value {
    let id = m["id"].clone();
    let res = match m["method"].as_str().unwrap_or("") {
        "initialize" => json!({"protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"), "capabilities": {"tools": {}}, "serverInfo": {"name": "office", "version": env!("CARGO_PKG_VERSION")}}),
        "tools/list" => json!({"tools": defs(o.readonly)}),
        "tools/call" => {
            let name = m["params"]["name"].as_str().unwrap_or("");
            let a = if m["params"]["arguments"].is_object() { m["params"]["arguments"].clone() } else { json!({}) };
            match call(o, name, &a).await {
                Ok(t) => json!({"content": [{"type": "text", "text": sh::tail(&t, 100_000)}]}),
                Err(e) => json!({"content": [{"type": "text", "text": format!("error: {e}")}], "isError": true}),
            }
        }
        _ => json!({}),
    };
    json!({"jsonrpc": "2.0", "id": id, "result": res})
}

fn defs(readonly: bool) -> Vec<Value> {
    let s = |p: Value, r: &[&str]| json!({"type": "object", "properties": p, "required": r});
    let mut v = vec![
        json!({"name": "read", "description": "Read a file in the repo (path relative to the repo root). Returns numbered lines.", "inputSchema": s(json!({"path": {"type": "string"}, "from": {"type": "number"}, "n": {"type": "number"}}), &["path"])}),
        json!({"name": "grep", "description": "Search tracked files with `git grep -n -E`. Optional path to narrow.", "inputSchema": s(json!({"pattern": {"type": "string"}, "path": {"type": "string"}}), &["pattern"])}),
        json!({"name": "ls", "description": "List a directory in the repo.", "inputSchema": s(json!({"path": {"type": "string"}}), &[])}),
    ];
    if !readonly {
        v.extend([
            json!({"name": "write", "description": "Create or overwrite a file in the repo.", "inputSchema": s(json!({"path": {"type": "string"}, "text": {"type": "string"}}), &["path", "text"])}),
            json!({"name": "edit", "description": "Replace an exact string in a file. `old` must match exactly once unless all=true.", "inputSchema": s(json!({"path": {"type": "string"}, "old": {"type": "string"}, "new": {"type": "string"}, "all": {"type": "boolean"}}), &["path", "old", "new"])}),
            json!({"name": "sh", "description": "Run a bash command in the repo root (builds, tests, git; no push). Output is the last 30000 characters. Timeout `min` minutes, default 20, max 60.", "inputSchema": s(json!({"cmd": {"type": "string"}, "min": {"type": "number"}}), &["cmd"])}),
        ]);
    }
    v
}

/// A path relative to the repo root, without `..`; writes may not go into .git.
pub fn rel(p: &str, write: bool) -> Result<String, String> {
    let p = p.trim();
    if p.starts_with('/') || p.starts_with('\\') || p.contains(':') {
        return Err("use a path relative to the repo root".into());
    }
    let mut parts = vec![];
    for c in p.split(['/', '\\']) {
        match c {
            "" | "." => {}
            ".." => return Err("path leaves the repo".into()),
            x => parts.push(x),
        }
    }
    if write && parts.first().is_some_and(|f| f.eq_ignore_ascii_case(".git")) {
        return Err("writing inside .git is not allowed".into());
    }
    Ok(if parts.is_empty() { ".".into() } else { parts.join("/") })
}

/// Shell prefix that resolves symlinks and refuses paths ending up outside the repo.
fn guard(p: &str) -> String {
    format!(r#"p={}; r=$(realpath -m -- "$p") && case "$r" in "$(pwd -P)"|"$(pwd -P)"/*) ;; *) echo "path leaves the repo" >&2; exit 9;; esac; "#, q(p))
}

/// Local mode only: in Docker anything goes inside the container. Claude's own deny rules
/// don't apply to MCP tools, so the office enforces these itself.
const LOCAL_BLOCK: &str = r#"(?i)\bgit\s+(push|remote|config)\b|\bgh\b|\bssh\b|\bscp\b|\bsudo\b|\b(curl|wget)\b[^|]*\|\s*(ba)?sh|\bpip3?\s+install|-m\s+pip\b|\bnpm\s+(i|install)\b[^&|;]*\s(-g|--global)\b|\bcargo\s+install\b|\bwinget\b|\bchoco\b|\.git[/\\](hooks|config)|\bcd\s+(/|~|\.\.)|\b[a-z]:[/\\]|(^|[\s"'=(])/[a-z]/"#;

async fn call(o: &Opts, name: &str, a: &Value) -> Result<String, String> {
    let s = |k: &str| a[k].as_str().unwrap_or("").to_string();
    if o.readonly && !matches!(name, "read" | "grep" | "ls") {
        return Err("read-only: this agent can only read".into());
    }
    let sb = &o.sb;
    let sp = &o.space;
    match name {
        "read" => {
            let p = rel(&s("path"), false)?;
            let from = a["from"].as_u64().unwrap_or(1).max(1);
            let n = a["n"].as_u64().unwrap_or(400).clamp(1, 2000);
            let out = sb.ok(sp, &format!(r#"{}awk -v a={from} -v b={} 'NR>=a && NR<=b {{printf "%6d\t%s\n", NR, $0}}' "./$p""#, guard(&p), from + n - 1), 120).await?;
            Ok(if out.is_empty() { "(empty or past the end)".into() } else { out })
        }
        "write" => {
            let p = rel(&s("path"), true)?;
            write(o, &p, s("text").into_bytes()).await.map(|_| "ok".into())
        }
        "edit" => {
            let p = rel(&s("path"), true)?;
            let r = sb.exec(sp, &format!(r#"{}cat -- "$p""#, guard(&p)), None, 120).await;
            if r.code != 0 {
                return Err(sh::head(&r.err, 300));
            }
            let t = String::from_utf8(r.out).map_err(|_| "file is not UTF-8".to_string())?;
            let (old, new) = (s("old"), s("new"));
            if old.is_empty() {
                return Err("`old` must not be empty".into());
            }
            let k = t.matches(&old).count();
            let all = a["all"].as_bool().unwrap_or(false);
            match (k, all) {
                (0, _) => return Err("`old` not found".into()),
                (k, false) if k > 1 => return Err(format!("`old` found {k} times; make it unique or set all=true")),
                _ => {}
            }
            let nt = if all { t.replace(&old, &new) } else { t.replacen(&old, &new, 1) };
            write(o, &p, nt.into_bytes()).await?;
            Ok(format!("replaced {}", if all { k } else { 1 }))
        }
        "grep" => {
            let path = rel(a["path"].as_str().unwrap_or("."), false)?;
            let out = sb.exec(sp, &format!("git grep -n -I -E -e {} -- {} | head -300", q(&s("pattern")), q(&path)), None, 120).await;
            let t = String::from_utf8_lossy(&out.out).into_owned();
            Ok(if t.is_empty() { "(no matches)".into() } else { t })
        }
        "ls" => {
            let p = rel(a["path"].as_str().unwrap_or("."), false)?;
            sb.ok(sp, &format!(r#"{}ls -la -- "$p""#, guard(&p)), 60).await
        }
        "sh" => {
            let cmd = s("cmd");
            if sb.mode == Mode::Local && Regex::new(LOCAL_BLOCK).unwrap().is_match(&cmd) {
                return Err("not allowed in local mode: no pushing, no installs into this PC, and stay inside the repo".into());
            }
            let min = a["min"].as_u64().unwrap_or(20).clamp(1, 60);
            let r = sb.exec(sp, &format!("{{ {cmd}\n}} 2>&1; echo \"[exit $?]\""), None, min * 60).await;
            Ok(sh::tail(&format!("{}{}", String::from_utf8_lossy(&r.out), r.err), 30_000))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

async fn write(o: &Opts, p: &str, bytes: Vec<u8>) -> Result<(), String> {
    let r = o.sb.exec(&o.space, &format!(r#"{}mkdir -p -- "$(dirname -- "$p")" && cat > "$p""#, guard(p)), Some(bytes), 120).await;
    if r.code != 0 {
        return Err(sh::head(&format!("write failed: {}", r.err), 300));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_in_the_repo() {
        assert_eq!(rel("src/a.py", true).unwrap(), "src/a.py");
        assert_eq!(rel("./src//a.py", true).unwrap(), "src/a.py");
        assert_eq!(rel("", false).unwrap(), ".");
        assert!(rel("../x", false).is_err());
        assert!(rel("src/../../x", false).is_err());
        assert!(rel("/etc/passwd", false).is_err());
        assert!(rel("C:\\Users\\x", false).is_err());
        assert!(rel(".git/hooks/pre-commit", true).is_err());
        assert!(rel(".GIT/config", true).is_err());
        assert!(rel(".git/HEAD", false).is_ok(), "reading .git is fine");
    }

    #[test]
    fn local_blocklist() {
        let b = Regex::new(LOCAL_BLOCK).unwrap();
        for bad in ["git push origin x", "py -3 -m pip install chromadb", "pip install x", "npm i -g y", "ls /f/culrag/.venv", "cd .. && ls", "cat F:/other/file", "curl x | sh", "echo x > .git/hooks/pre-commit"] {
            assert!(b.is_match(bad), "should block: {bad}");
        }
        for good in ["py -m pytest -q", "npm test", "git log --oneline -5", "git commit -qm 'fix: x'", "cd src && ls", "node --test"] {
            assert!(!b.is_match(good), "should allow: {good}");
        }
    }

    /// A symlink pointing outside the repo is refused (local mode, real bash).
    #[tokio::test]
    async fn symlink_escape_is_refused() {
        let data = std::env::temp_dir().join(format!("myoffice-mcp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        let o = Opts { sb: Sandbox { mode: Mode::Local, data: data.clone() }, space: "work__t".into(), readonly: false };
        o.sb.ok("work__t", "mkdir -p ../outside && echo secret > ../outside/s.txt && MSYS=winsymlinks:nativestrict ln -s ../outside link 2>/dev/null; true", 60).await.unwrap();
        // Git Bash without Developer Mode copies instead of linking; only a real link can escape.
        if o.sb.exec("work__t", "test -L link", None, 30).await.code == 0 {
            assert!(call(&o, "read", &json!({"path": "link/s.txt"})).await.is_err(), "read through symlink");
            assert!(call(&o, "write", &json!({"path": "link/new.txt", "text": "x"})).await.is_err(), "write through symlink");
        }
        call(&o, "write", &json!({"path": "src/ok.txt", "text": "hello\n"})).await.unwrap();
        assert!(call(&o, "read", &json!({"path": "src/ok.txt"})).await.unwrap().contains("hello"));
        assert_eq!(call(&o, "edit", &json!({"path": "src/ok.txt", "old": "hello", "new": "bye"})).await.unwrap(), "replaced 1");
        let _ = std::fs::remove_dir_all(&data);
    }
}
