//! `myoffice mcp --data D --space S --mode docker|local [--host H] [--readonly]`: hands the
//! office tools (tools.rs) to Claude Code as a stdio MCP server, one JSON-RPC message per line.
//! Its stdout carries JSON-RPC only; never print anything else here.

use crate::sandbox::{Mode, Sandbox};
use crate::sh;
use crate::tools::{defs, Toolbox};
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Command-line arguments for the MCP server (used to write Claude's --mcp-config).
pub fn args(sb: &Sandbox, space: &str, readonly: bool) -> Vec<String> {
    let mut v = vec!["mcp".into(), "--data".into(), sb.data.to_string_lossy().into_owned(), "--space".into(), space.into(), "--mode".into(), if sb.mode == Mode::Docker { "docker" } else { "local" }.into()];
    if let Some(h) = &sb.host {
        v.extend(["--host".into(), h.clone()]);
    }
    if readonly {
        v.push("--readonly".into());
    }
    v
}

pub fn parse(a: &[String]) -> Option<Toolbox> {
    let get = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let mode = if get("--mode")? == "docker" { Mode::Docker } else { Mode::Local };
    let sb = Sandbox { mode, data: PathBuf::from(get("--data")?), host: get("--host") };
    Some(Toolbox { sb, space: get("--space")?, readonly: a.iter().any(|x| x == "--readonly") })
}

pub async fn serve(t: Toolbox) {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Ok(Some(l)) = lines.next_line().await {
        let Ok(m) = serde_json::from_str::<Value>(&l) else { continue };
        if m.get("id").is_none() {
            continue; // notification, no answer
        }
        let r = handle(&t, &m).await;
        let _ = out.write_all(format!("{r}\n").as_bytes()).await;
        let _ = out.flush().await;
    }
}

async fn handle(t: &Toolbox, m: &Value) -> Value {
    let id = m["id"].clone();
    let res = match m["method"].as_str().unwrap_or("") {
        "initialize" => json!({"protocolVersion": m["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"), "capabilities": {"tools": {}}, "serverInfo": {"name": "office", "version": env!("CARGO_PKG_VERSION")}}),
        "tools/list" => json!({"tools": defs(t.readonly).into_iter().map(|(n, d, s)| json!({"name": n, "description": d, "inputSchema": s})).collect::<Vec<_>>()}),
        "tools/call" => {
            let name = m["params"]["name"].as_str().unwrap_or("");
            let a = if m["params"]["arguments"].is_object() { m["params"]["arguments"].clone() } else { json!({}) };
            match t.call(name, &a).await {
                Ok(x) => json!({"content": [{"type": "text", "text": sh::tail(&x, 100_000)}]}),
                Err(e) => json!({"content": [{"type": "text", "text": format!("error: {e}")}], "isError": true}),
            }
        }
        _ => json!({}),
    };
    json!({"jsonrpc": "2.0", "id": id, "result": res})
}
