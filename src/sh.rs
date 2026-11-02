use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

pub struct Out {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// Raw output, for binary data such as git bundles.
pub struct Raw {
    pub code: i32,
    pub out: Vec<u8>,
    pub err: String,
}

/// Runs a program, optionally feeding it bytes on stdin. Exit code -1 = could not start or timed out.
pub async fn run_raw(prog: &str, args: &[&str], cwd: &Path, input: Option<Vec<u8>>, secs: u64) -> Raw {
    let mut c = tokio::process::Command::new(prog);
    c.args(args)
        .current_dir(cwd)
        .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = match c.spawn() {
        Ok(c) => c,
        Err(e) => return Raw { code: -1, out: vec![], err: format!("cannot start {prog}: {e}") },
    };
    if let Some(bytes) = input {
        let mut stdin = child.stdin.take().unwrap();
        tokio::spawn(async move {
            let _ = stdin.write_all(&bytes).await;
        });
    }
    // ponytail: kill_on_drop stops the direct child only; in local mode grandchildren can outlive
    // a timeout on Windows. Docker mode wraps commands in `timeout` inside the container.
    match tokio::time::timeout(Duration::from_secs(secs), child.wait_with_output()).await {
        Ok(Ok(o)) => Raw { code: o.status.code().unwrap_or(-1), out: o.stdout, err: String::from_utf8_lossy(&o.stderr).into_owned() },
        Ok(Err(e)) => Raw { code: -1, out: vec![], err: e.to_string() },
        Err(_) => Raw { code: -1, out: vec![], err: format!("timed out after {secs}s") },
    }
}

pub async fn run(prog: &str, args: &[&str], cwd: &Path, secs: u64) -> Out {
    let r = run_raw(prog, args, cwd, None, secs).await;
    Out { code: r.code, out: String::from_utf8_lossy(&r.out).into_owned(), err: r.err }
}

/// Like `run`, but returns stdout on success and a readable error otherwise.
pub async fn ok(prog: &str, args: &[&str], cwd: &Path, secs: u64) -> Result<String, String> {
    let o = run(prog, args, cwd, secs).await;
    if o.code == 0 {
        Ok(o.out)
    } else {
        Err(format!("`{prog} {}` failed: {}", args.iter().take(3).copied().collect::<Vec<_>>().join(" "), tail(&format!("{}{}", o.out, o.err), 800)))
    }
}

/// On Windows, plain `bash` may be WSL's, so prefer Git Bash.
pub fn bash_path() -> String {
    if let Ok(b) = std::env::var("MYOFFICE_BASH") {
        return b;
    }
    let git_bash = r"C:\Program Files\Git\bin\bash.exe";
    if cfg!(windows) && Path::new(git_bash).exists() {
        git_bash.into()
    } else {
        "bash".into()
    }
}

/// Single-quotes a value for bash.
pub fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

pub fn head(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n).collect::<String>() + "…"
    }
}

pub fn tail(s: &str, n: usize) -> String {
    let c = s.chars().count();
    if c <= n {
        s.to_string()
    } else {
        "…".to_string() + &s.chars().skip(c - n).collect::<String>()
    }
}
