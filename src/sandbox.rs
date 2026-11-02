//! Where repo work happens. Every checkout, commit, test run and agent tool call runs inside a
//! "space" (one working copy) in the sandbox: a Docker container when Docker is available,
//! otherwise a folder on this PC.
//!
//! The host never runs git in a space. It keeps a bare mirror per repo (data/mirror), which only
//! it writes: fetch from GitHub, push to GitHub. Code moves between mirror and space as git
//! bundles, which are plain data: nothing an agent writes (hooks, git config, filters) can run
//! on the host. The container holds no GitHub or Claude credentials.

use crate::sh::{self, Out, Raw};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const IMAGE: &str = "myoffice-sandbox:1";
pub const CONTAINER: &str = "myoffice-sandbox";

const DOCKERFILE: &str = r#"FROM node:22-bookworm
RUN apt-get update && apt-get install -y --no-install-recommends \
      python3 python3-pip python3-venv python3-pytest python3-dev build-essential git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && ln -sf /usr/bin/python3 /usr/local/bin/python \
    && printf '#!/bin/sh\n[ "$1" = "-3" ] && shift\nexec python3 "$@"\n' > /usr/local/bin/py && chmod +x /usr/local/bin/py
ENV PIP_BREAK_SYSTEM_PACKAGES=1 CI=1 NO_COLOR=1 GIT_TERMINAL_PROMPT=0
WORKDIR /work
"#;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Docker,
    Local,
}

#[derive(Clone)]
pub struct Sandbox {
    pub mode: Mode,
    pub data: PathBuf,
}

/// "work__owner__repo": the space Repro/Fixer/CI/Submit use. Hunter has its own ("hunt__...").
pub fn space(kind: &str, repo: &str) -> String {
    format!("{kind}__{}", repo.replace('/', "__"))
}

static SEQ: AtomicU64 = AtomicU64::new(0);

/// A fresh file name under data/tmp (for bundles and MCP configs).
pub fn tmp_file(data: &Path, ext: &str) -> PathBuf {
    let dir = data.join("tmp");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{}-{}.{ext}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)))
}

impl Sandbox {
    /// Docker if it's running (building the image and starting the container on first use),
    /// otherwise local. MYOFFICE_SANDBOX=local skips Docker. Returns a note for the dashboard.
    pub async fn start(data: &Path) -> (Sandbox, String) {
        let local = Sandbox { mode: Mode::Local, data: data.into() };
        if std::env::var("MYOFFICE_SANDBOX").as_deref() == Ok("local") {
            return (local, "local (MYOFFICE_SANDBOX=local)".into());
        }
        match docker_up(data).await {
            Ok(()) => (Sandbox { mode: Mode::Docker, data: data.into() }, "docker".into()),
            Err(e) => (local, format!("local, Docker not usable: {}", sh::head(&e, 200))),
        }
    }

    fn local_dir(&self, sp: &str) -> PathBuf {
        self.data.join("sandbox").join(sp)
    }

    /// Runs a bash script inside a space (cwd = its working copy), stdout/stderr kept apart.
    pub async fn exec(&self, sp: &str, script: &str, input: Option<Vec<u8>>, secs: u64) -> Raw {
        match self.mode {
            Mode::Docker => {
                let s = format!("mkdir -p /work/{sp} && cd /work/{sp} && {script}");
                let t = secs.to_string();
                sh::run_raw("docker", &["exec", "-i", CONTAINER, "timeout", "-k", "10", &t, "bash", "-c", &s], &self.data, input, secs + 30).await
            }
            Mode::Local => {
                let dir = self.local_dir(sp);
                let _ = std::fs::create_dir_all(&dir);
                sh::run_raw(&sh::bash_path(), &["-c", script], &dir, input, secs).await
            }
        }
    }

    /// Text output with stderr merged in order (for test logs).
    pub async fn sh(&self, sp: &str, script: &str, secs: u64) -> Out {
        let r = self.exec(sp, &format!("{{ {script}\n}} 2>&1"), None, secs).await;
        Out { code: r.code, out: String::from_utf8_lossy(&r.out).into_owned(), err: r.err }
    }

    /// stdout on success, a readable error otherwise.
    pub async fn ok(&self, sp: &str, script: &str, secs: u64) -> Result<String, String> {
        let r = self.exec(sp, script, None, secs).await;
        let out = String::from_utf8_lossy(&r.out).into_owned();
        if r.code == 0 {
            Ok(out)
        } else {
            Err(format!("`{}` failed: {}", sh::head(script, 80), sh::tail(&format!("{out}{}", r.err), 800)))
        }
    }

    /// Copies `branch` of the host mirror into the space as refs/remotes/origin/<branch>.
    pub async fn load(&self, sp: &str, mirror: &Path, branch: &str) -> Result<(), String> {
        let b = sh::run_raw("git", &["bundle", "create", "-", &format!("refs/heads/{branch}")], mirror, None, 600).await;
        if b.code != 0 {
            return Err(format!("git bundle failed: {}", sh::tail(&b.err, 400)));
        }
        let script = format!(
            "[ -d .git ] || git init -q . ; cat > .git/in.bundle && git fetch -q --force .git/in.bundle {} ; e=$?; rm -f .git/in.bundle; exit $e",
            sh::q(&format!("refs/heads/{branch}:refs/remotes/origin/{branch}"))
        );
        let r = self.exec(sp, &script, Some(b.out), 600).await;
        if r.code != 0 {
            return Err(format!("loading the code into the sandbox failed: {}", sh::tail(&r.err, 400)));
        }
        Ok(())
    }

    /// Copies the commits of `branch` after `base` from the space into the host mirror.
    pub async fn export(&self, sp: &str, mirror: &Path, branch: &str, base: &str) -> Result<(), String> {
        let r = self.exec(sp, &format!("git bundle create - {} ^{}", sh::q(&format!("refs/heads/{branch}")), sh::q(base)), None, 600).await;
        if r.code != 0 {
            return Err(format!("bundling the fix failed: {}", sh::tail(&r.err, 400)));
        }
        let f = tmp_file(&self.data, "bundle");
        std::fs::write(&f, &r.out).map_err(|e| e.to_string())?;
        let refspec = format!("+refs/heads/{branch}:refs/heads/{branch}");
        let o = sh::run("git", &["fetch", "-q", &f.to_string_lossy(), &refspec], mirror, 600).await;
        let _ = std::fs::remove_file(&f);
        if o.code != 0 {
            return Err(format!("importing the fix failed: {}", sh::tail(&o.err, 400)));
        }
        Ok(())
    }
}

/// Builds the image if needed and makes sure the container is running.
async fn docker_up(data: &Path) -> Result<(), String> {
    let ok = |o: Out, what: &str| if o.code == 0 { Ok(o.out) } else { Err(format!("{what}: {}", sh::tail(&format!("{}{}", o.out, o.err), 300))) };
    ok(sh::run("docker", &["info", "--format", "{{.ServerVersion}}"], data, 30).await, "docker is not running")?;
    if sh::run("docker", &["image", "inspect", IMAGE], data, 30).await.code != 0 {
        eprintln!("  building the sandbox image (first start only, a few minutes)...");
        let b = sh::run_raw("docker", &["build", "-q", "-t", IMAGE, "-"], data, Some(DOCKERFILE.as_bytes().to_vec()), 3600).await;
        if b.code != 0 {
            return Err(format!("image build failed: {}", sh::tail(&b.err, 400)));
        }
    }
    let state = sh::run("docker", &["inspect", "-f", "{{.Config.Image}} {{.State.Running}}", CONTAINER], data, 30).await;
    let s = state.out.trim().to_string();
    if state.code == 0 && !s.starts_with(IMAGE) {
        let _ = sh::run("docker", &["rm", "-f", CONTAINER], data, 60).await; // old image version
    }
    if state.code != 0 || !s.starts_with(IMAGE) {
        let cpus = std::thread::available_parallelism().map(|n| n.get().saturating_sub(1).max(1)).unwrap_or(1).to_string();
        // A named volume, not a folder from this PC: the agents can't reach your files.
        ok(sh::run("docker", &["run", "-d", "--name", CONTAINER, "--restart", "unless-stopped", "--cpus", &cpus, "--memory", "6g", "-v", "myoffice-work:/work", IMAGE, "sleep", "infinity"], data, 120).await, "starting the sandbox")?;
    } else if s.ends_with("false") {
        ok(sh::run("docker", &["start", CONTAINER], data, 60).await, "starting the sandbox")?;
    }
    // Commits in the sandbox are made in your name.
    for k in ["user.name", "user.email"] {
        let v = sh::run("git", &["config", "--global", k], data, 10).await.out.trim().to_string();
        let v = if v.is_empty() { if k == "user.name" { "MyOffice".into() } else { "myoffice@localhost".into() } } else { v };
        ok(sh::run("docker", &["exec", CONTAINER, "git", "config", "--global", k, &v], data, 30).await, "configuring git in the sandbox")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mirror → space → commit → back to the mirror, in local mode.
    #[tokio::test]
    async fn bundle_round_trip() {
        let root = std::env::temp_dir().join(format!("myoffice-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (src, mirror) = (root.join("src"), root.join("mirror.git"));
        std::fs::create_dir_all(&src).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            let o = std::process::Command::new("git").args(args).current_dir(dir).output().unwrap();
            assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        };
        git(&src, &["init", "-q", "-b", "main"]);
        std::fs::write(src.join("a.txt"), "one\n").unwrap();
        git(&src, &["add", "."]);
        git(&src, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-qm", "one"]);
        git(&root, &["clone", "-q", "--bare", &src.to_string_lossy(), &mirror.to_string_lossy()]);
        let base = git(&mirror, &["rev-parse", "refs/heads/main"]);

        let sb = Sandbox { mode: Mode::Local, data: root.join("data") };
        sb.load("work__t", &mirror, "main").await.unwrap();
        sb.ok("work__t", "git checkout -q -B fix origin/main && echo two >> a.txt && git -c user.name=t -c user.email=t@t commit -qam two", 60).await.unwrap();
        sb.export("work__t", &mirror, "fix", &base).await.unwrap();

        assert_eq!(git(&mirror, &["rev-list", "--count", &format!("{base}..refs/heads/fix")]), "1");
        assert_eq!(git(&mirror, &["show", "refs/heads/fix:a.txt"]), "one\ntwo");
        let _ = std::fs::remove_dir_all(&root);
    }
}
