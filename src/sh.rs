use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

pub struct Out {
    pub code: i32,
    pub out: String,
    pub err: String,
}

/// Runs a program and waits for it. Exit code -1 means it could not start or timed out.
pub async fn run(prog: &str, args: &[&str], cwd: &Path, secs: u64) -> Out {
    let mut c = tokio::process::Command::new(prog);
    c.args(args).current_dir(cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let child = match c.spawn() {
        Ok(c) => c,
        Err(e) => return Out { code: -1, out: String::new(), err: format!("cannot start {prog}: {e}") },
    };
    // ponytail: kill_on_drop stops the direct child only; grandchildren (e.g. cargo under bash)
    // can outlive a timeout on Windows. Use a Job Object or Docker if that bites.
    match tokio::time::timeout(Duration::from_secs(secs), child.wait_with_output()).await {
        Ok(Ok(o)) => Out {
            code: o.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&o.stdout).into_owned(),
            err: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        Ok(Err(e)) => Out { code: -1, out: String::new(), err: e.to_string() },
        Err(_) => Out { code: -1, out: String::new(), err: format!("timed out after {secs}s") },
    }
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

/// Runs a shell script with stdout and stderr merged in order.
pub async fn bash(script: &str, cwd: &Path, secs: u64) -> Out {
    run(&bash_path(), &["-c", &format!("{{ {script}\n}} 2>&1")], cwd, secs).await
}

/// On Windows, plain `bash` may be WSL's, so prefer Git Bash.
fn bash_path() -> String {
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
