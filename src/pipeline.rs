//! The agents' work. One job runs at a time:
//! Scout → Guard → Vet pick issues; Repro writes a failing test; Fixer writes the fix;
//! CI runs the suite; Reviewer reads the diff; you approve; Submit opens the PR; Watcher follows it.

use crate::claude::{self, Ask};
use crate::db::{Job, Repo};
use crate::{prompts, sh, App};
use regex::Regex;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FIX_ROUNDS: u32 = 3;
const REPRO_TRIES: u32 = 2;
const SKIP_LABELS: &str = r"(?i)\b(enhancement|feature|question|docs?|documentation|discussion|wontfix|duplicate|invalid)\b";
/// Output that means the test never ran properly, so it can't count as "red".
const NOT_A_TEST_FAILURE: &str = r"error\[E\d+\]|could not compile|SyntaxError|ModuleNotFoundError|ImportError|Cannot find module|error TS\d+|No tests found|no tests ran|command not found|ClassNotFoundException";

fn workdir(app: &App, repo: &str) -> PathBuf {
    app.data.join("work").join(repo.replace('/', "__"))
}

/// "ao-testbed#6", or "ao-testbed (report 12)" for a bug you described.
pub fn label(j: &Job) -> String {
    let name = j.repo.rsplit('/').next().unwrap_or(&j.repo);
    if j.issue > 0 {
        format!("{name}#{}", j.issue)
    } else {
        format!("{name} (report {})", j.id)
    }
}

fn untrusted(j: &Job) -> String {
    format!("<untrusted-issue>\n{}\n\n{}\n</untrusted-issue>", j.title, sh::head(j.data["body"].as_str().unwrap_or(""), 8000))
}

fn involve(app: &App, id: i64, agent: &str) {
    app.db.update_data(id, |d| {
        let mut v: Vec<Value> = d["agents"].as_array().cloned().unwrap_or_default();
        if !v.iter().any(|x| x == agent) {
            v.push(json!(agent));
        }
        d["agents"] = json!(v);
    });
}

// ================================================================ Scout, Guard, Vet

pub async fn scout_loop(app: Arc<App>) {
    loop {
        app.wait_unpaused().await;
        if let Err(e) = scout(&app).await {
            app.log("scout", None, &format!("error: {e}"));
        }
        app.set("scout", "IDLE", None, None);
        if let Err(e) = vet_all(&app).await {
            app.log("vet", None, &format!("error: {e}"));
        }
        for a in ["guard", "vet"] {
            app.set(a, "IDLE", None, None);
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(600)) => {}
            _ = app.scan.notified() => {}
        }
    }
}

async fn scout(app: &App) -> Result<(), String> {
    let skip = Regex::new(SKIP_LABELS).unwrap();
    for r in app.db.repos().into_iter().filter(|r| r.enabled) {
        app.set("scout", "WORKING", None, Some(&format!("scanning {}", r.full)));
        let raw = sh::ok("gh", &["issue", "list", "-R", &r.full, "--state", "open", "--limit", "50", "--json", "number,title,body,labels,assignees,url"], &app.data, 120).await?;
        let issues: Vec<Value> = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let fresh: Vec<&Value> = issues
            .iter()
            .filter(|i| {
                app.db.find_job(&r.full, i["number"].as_i64().unwrap_or(0)).is_none()
                    && i["assignees"].as_array().map_or(true, |a| a.is_empty())
                    && !i["labels"].as_array().map_or(false, |ls| ls.iter().any(|l| skip.is_match(l["name"].as_str().unwrap_or(""))))
            })
            .collect();
        if fresh.is_empty() {
            app.log("scout", None, &format!("{}: horizon is clear, no new issues", r.full));
            continue;
        }
        app.log("scout", None, &format!("{}: {} new issue(s), checking which are real bugs", r.full, fresh.len()));
        for chunk in fresh.chunks(10) {
            let prompt = chunk
                .iter()
                .map(|i| format!("<untrusted-issue number=\"{}\">\n{}\n\n{}\n</untrusted-issue>", i["number"], i["title"].as_str().unwrap_or(""), sh::head(i["body"].as_str().unwrap_or(""), 3000)))
                .collect::<Vec<_>>()
                .join("\n\n");
            let reply = claude::ask(app, Ask { agent: "scout", job: None, system: prompts::SCOUT, prompt, cwd: &app.data, tools: false, resume: None, max_turns: 1, secs: 300 }).await?;
            for i in chunk {
                let n = i["number"].as_i64().unwrap_or(0);
                let v = &reply.json[n.to_string()];
                let why = v["why"].as_str().unwrap_or("no verdict").to_string();
                let state = if v["pick"].as_bool() == Some(true) { "vet" } else { "skipped" };
                let id = app.db.add_job(&r.full, n, "issue", i["title"].as_str().unwrap_or(""), i["url"].as_str().unwrap_or(""), state, &why, &json!({"body": i["body"], "agents": ["scout"]}));
                app.log("scout", Some(id), &format!("#{n} {}: {why}", if state == "vet" { "spotted" } else { "skipped" }));
            }
        }
    }
    Ok(())
}

/// Every job waiting in "vet": Guard checks for prompt injection, then Vet judges the bug.
async fn vet_all(app: &App) -> Result<(), String> {
    for j in app.db.jobs_in("vet") {
        app.wait_unpaused().await;
        let id = j.id;
        let tag = label(&j);
        let issue = untrusted(&j);

        app.set("guard", "WORKING", Some(id), Some(&format!("checking {tag}")));
        let g = claude::ask(app, Ask { agent: "guard", job: Some(id), system: prompts::GUARD, prompt: issue.clone(), cwd: &app.data, tools: false, resume: None, max_turns: 1, secs: 300 }).await?;
        app.set("guard", "IDLE", None, Some("—"));
        involve(app, id, "guard");
        if g.json["injection"].as_bool() != Some(false) {
            let why = g.json["why"].as_str().unwrap_or("no clear verdict, blocked to be safe");
            app.db.set_state(id, "blocked", &format!("Guard: {why}"));
            app.log("guard", Some(id), &format!("blocked {tag}: {why}"));
            continue;
        }
        app.log("guard", Some(id), &format!("{tag} is safe to work on"));

        if j.data["manual"].as_bool() == Some(true) {
            app.db.set_state(id, "ready", "added by you");
            app.wake.notify_one();
            continue;
        }
        app.set("vet", "WORKING", Some(id), Some(&format!("vetting {tag}")));
        let prs = sh::ok("gh", &["pr", "list", "-R", &j.repo, "--state", "open", "--limit", "50", "--json", "number,title"], &app.data, 60).await.unwrap_or_default();
        let prompt = format!("Repository {}.\n\n{issue}\n\nOpen pull requests (number, title):\n{}", j.repo, sh::head(&prs, 6000));
        let v = claude::ask(app, Ask { agent: "vet", job: Some(id), system: prompts::VET, prompt, cwd: &app.data, tools: false, resume: None, max_turns: 1, secs: 300 }).await?;
        app.set("vet", "IDLE", None, Some("—"));
        involve(app, id, "vet");
        let why = v.json["why"].as_str().unwrap_or("no verdict").to_string();
        if v.json["ok"].as_bool() == Some(true) {
            app.db.set_state(id, "ready", &format!("Vet: {why}"));
            app.log("vet", Some(id), &format!("triage passed for {tag}: {why}"));
            app.wake.notify_one();
        } else {
            app.db.set_state(id, "skipped", &format!("Vet: {why}"));
            app.log("vet", Some(id), &format!("{tag} skipped: {why}"));
        }
    }
    Ok(())
}

/// A GitHub issue you picked yourself: Guard still checks it, Vet doesn't second-guess you.
pub async fn add_issue(app: &App, repo: &str, n: i64) -> Result<(), String> {
    if let Some(id) = app.db.find_job(repo, n) {
        app.db.set_state(id, "vet", "added again by you");
        app.db.update_data(id, |d| d["manual"] = json!(true));
    } else {
        let raw = sh::ok("gh", &["issue", "view", &n.to_string(), "-R", repo, "--json", "title,body,url"], &app.data, 60).await?;
        let i: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        let id = app.db.add_job(repo, n, "issue", i["title"].as_str().unwrap_or(""), i["url"].as_str().unwrap_or(""), "vet", "added by you", &json!({"body": i["body"], "agents": [], "manual": true}));
        app.log("scout", Some(id), &format!("you added #{n}: {}", i["title"].as_str().unwrap_or("")));
    }
    app.scan.notify_one();
    Ok(())
}

// ================================================================ Runner

pub async fn run_loop(app: Arc<App>) {
    loop {
        app.wait_unpaused().await;
        let Some(job) = app.db.next_job() else {
            tokio::select! {
                _ = app.wake.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
            }
            continue;
        };
        let id = job.id;
        if job.state == "approved" {
            if let Err(e) = submit(&app, &job).await {
                // A failed push shouldn't throw the verified fix away.
                app.db.set_state(id, "approve", &format!("submit failed: {}", sh::head(&e, 300)));
                app.log("submit", Some(id), &format!("submit failed: {e}"));
            }
        } else if let Err(e) = work(&app, &job).await {
            app.db.set_state(id, "dropped", &sh::head(&e, 300));
            app.log("fixer", Some(id), &format!("{} dropped: {e}", label(&job)));
        }
        for a in ["repro", "fixer", "ci", "reviewer", "submit"] {
            app.set(a, "IDLE", None, None);
        }
    }
}

/// Fresh branch from the latest default branch. Returns (base commit, branch, default branch).
async fn prepare(app: &App, job: &Job, dir: &Path) -> Result<(String, String, String), String> {
    let default = sh::ok("gh", &["repo", "view", &job.repo, "--json", "defaultBranchRef", "-q", ".defaultBranchRef.name"], &app.data, 60).await?.trim().to_string();
    if !dir.join(".git").exists() {
        app.log("repro", Some(job.id), &format!("cloning {}", job.repo));
        sh::ok("gh", &["repo", "clone", &job.repo, &dir.to_string_lossy()], &app.data, 1800).await?;
    }
    sh::ok("git", &["fetch", "-q", "origin"], dir, 600).await?;
    let branch = if job.issue > 0 { format!("myoffice/issue-{}", job.issue) } else { format!("myoffice/report-{}", job.id) };
    sh::ok("git", &["checkout", "-q", "-f", "-B", &branch, &format!("origin/{default}")], dir, 120).await?;
    // ponytail: -fd keeps ignored build caches (target/, node_modules/) so rebuilds stay fast.
    sh::ok("git", &["clean", "-fdq"], dir, 120).await?;
    let base = rev(dir, "HEAD").await?;
    Ok((base, branch, default))
}

async fn rev(dir: &Path, r: &str) -> Result<String, String> {
    Ok(sh::ok("git", &["rev-parse", r], dir, 30).await?.trim().to_string())
}

async fn commits_since(dir: &Path, base: &str) -> Result<i64, String> {
    Ok(sh::ok("git", &["rev-list", "--count", &format!("{base}..HEAD")], dir, 30).await?.trim().parse().unwrap_or(-1))
}

async fn dirty(dir: &Path) -> Result<Option<String>, String> {
    let s = sh::ok("git", &["status", "--porcelain"], dir, 30).await?;
    Ok((!s.trim().is_empty()).then(|| format!("The working tree is not clean. Commit or delete these:\n{}", sh::head(&s, 1500))))
}

/// Runs a command on the checked-out commit, then removes what it left behind.
async fn run_here(dir: &Path, cmd: &str, secs: u64) -> sh::Out {
    let o = sh::bash(cmd, dir, secs).await;
    let _ = sh::ok("git", &["checkout", "-q", "-f", "--", "."], dir, 60).await;
    let _ = sh::ok("git", &["clean", "-fdq"], dir, 120).await;
    o
}

struct Proof {
    cmd: String,
    tests: Vec<String>,
    test_commit: String,
}

async fn work(app: &App, job: &Job) -> Result<(), String> {
    let id = job.id;
    let tag = label(job);
    let repo = app.db.repo(&job.repo).ok_or("this repository was removed")?;
    let dir = workdir(app, &job.repo);
    app.db.set_state(id, "repro", "preparing the repository");
    app.set("repro", "WORKING", Some(id), Some(&format!("[local] takes {}", job.repo)));
    app.log("repro", Some(id), &format!("[local] takes {}", tag));
    let (base, branch, default) = prepare(app, job, &dir).await?;
    app.db.update_data(id, |d| {
        d["base"] = json!(base);
        d["branch"] = json!(branch);
        d["default"] = json!(default);
    });
    let suite = if repo.test_cmd.is_empty() { String::new() } else { format!("The project's full test command is `{}`; don't run all of it.\n", repo.test_cmd) };
    let context = format!("Repository: {}\nBase commit: {base}\nBranch `{branch}` is checked out.\n{suite}\n{}", job.repo, untrusted(job));

    let proof = repro(app, id, &tag, &dir, &base, &context).await?;
    fix_rounds(app, job, &repo, &dir, &base, &context, &proof).await
}

/// Repro writes one test commit; the office checks it really fails.
async fn repro(app: &App, id: i64, tag: &str, dir: &Path, base: &str, context: &str) -> Result<Proof, String> {
    let first = format!("{context}\n\nWrite the regression test for this bug.");
    let mut prompt = first.clone();
    let mut session = None;
    for attempt in 1..=REPRO_TRIES {
        app.db.set_state(id, "repro", &format!("writing a failing test (try {attempt})"));
        app.set("repro", "WORKING", Some(id), Some(&format!("sketching test for {tag}")));
        let reply = claude::ask(app, Ask { agent: "repro", job: Some(id), system: prompts::REPRO, prompt: prompt.clone(), cwd: dir, tools: true, resume: session.clone(), max_turns: 80, secs: 2400 }).await?;
        if !reply.session.is_empty() {
            session = Some(reply.session);
        }
        let f = reply.json;
        if let Some(g) = f["give_up"].as_str() {
            return Err(format!("Repro gave up: {g}"));
        }
        let problem = match check_red(app, id, dir, base, &f).await? {
            Ok(p) => {
                involve(app, id, "repro");
                app.set("repro", "IDLE", None, Some(&format!("red: {}", p.tests.join(", "))));
                return Ok(p);
            }
            Err(p) => p,
        };
        app.log("repro", Some(id), &format!("test rejected: {}", sh::head(&problem, 300)));
        let feedback = format!("Your test was rejected:\n\n{problem}\n\nFix that. Keep exactly one commit (the test) on top of {base}, then answer again with the same JSON.");
        prompt = if session.is_some() { feedback } else { format!("{first}\n\n{feedback}") };
    }
    Err("could not reproduce the bug with a failing test".into())
}

/// Outer Ok(Err(..)) is feedback for Repro; outer Err is a real failure.
async fn check_red(app: &App, id: i64, dir: &Path, base: &str, f: &Value) -> Result<Result<Proof, String>, String> {
    let cmd = f["test_cmd"].as_str().unwrap_or("").trim().to_string();
    let tests: Vec<String> = f["tests"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).filter(|t| !t.is_empty()).map(String::from).collect();
    if cmd.is_empty() || tests.is_empty() {
        return Ok(Err("Your answer is missing `test_cmd` or `tests`.".into()));
    }
    let n = commits_since(dir, base).await?;
    if n != 1 {
        return Ok(Err(format!("Expected exactly 1 commit (the test) on top of {base}, found {n}.")));
    }
    if let Some(p) = dirty(dir).await? {
        return Ok(Err(p));
    }
    app.log("repro", Some(id), &format!("running `{cmd}` (must fail)"));
    let out = run_here(dir, &cmd, 1800).await;
    if let Some(p) = red_problem(&tests, &out) {
        return Ok(Err(p));
    }
    let test_commit = rev(dir, "HEAD").await?;
    app.log("repro", Some(id), &format!("reproduced: {} fails without a fix", tests.join(", ")));
    app.db.update_data(id, |d| {
        d["proof"] = json!({"test_cmd": cmd, "tests": tests, "test_commit": test_commit, "red_exit": out.code, "red_tail": sh::tail(&out.out, 1500), "summary": f["summary"]});
    });
    Ok(Ok(Proof { cmd, tests, test_commit }))
}

/// Fixer writes the fix; the office checks green, CI and review. Rejections go back to Fixer.
async fn fix_rounds(app: &App, job: &Job, repo: &Repo, dir: &Path, base: &str, context: &str, proof: &Proof) -> Result<(), String> {
    let id = job.id;
    let tag = label(job);
    let first = format!(
        "{context}\n\nThe last commit ({}) is a regression test that fails because of this bug. `{}` runs it (tests: {}). Fix the bug with one commit on top.",
        proof.test_commit,
        proof.cmd,
        proof.tests.join(", ")
    );
    let mut prompt = first.clone();
    let mut session = None;
    let mut last = String::new();
    for round in 1..=FIX_ROUNDS {
        app.wait_unpaused().await;
        app.db.set_state(id, "fix", &format!("round {round} of {FIX_ROUNDS}"));
        app.set("fixer", "WORKING", Some(id), Some(&format!("patching {tag}...")));
        let reply = claude::ask(app, Ask { agent: "fixer", job: Some(id), system: prompts::FIXER, prompt: prompt.clone(), cwd: dir, tools: true, resume: session.clone(), max_turns: 100, secs: 3600 }).await?;
        if !reply.session.is_empty() {
            session = Some(reply.session);
        }
        let f = reply.json;
        if let Some(g) = f["give_up"].as_str() {
            return Err(format!("Fixer gave up: {g}"));
        }
        app.set("fixer", "IDLE", None, Some(&format!("done: {}", f["title"].as_str().unwrap_or(&job.title))));
        involve(app, id, "fixer");
        match check_fix(app, job, repo, dir, base, proof, &f).await? {
            None => return ready(app, id, repo, dir, base, f).await,
            Some(problem) => {
                app.log("fixer", Some(id), &format!("round {round} rejected: {}", sh::head(&problem, 300)));
                let feedback = format!("Your fix was rejected:\n\n{problem}\n\nFix that. Keep the test commit unchanged and exactly one fix commit on top of it, then answer again with the same JSON.");
                prompt = if session.is_some() { feedback } else { format!("{first}\n\n{feedback}") };
                last = problem;
            }
        }
    }
    Err(format!("no verified fix after {FIX_ROUNDS} rounds: {}", sh::head(&last, 200)))
}

/// None = green, CI passed and the reviewer approved. Some(problem) = feedback for Fixer.
async fn check_fix(app: &App, job: &Job, repo: &Repo, dir: &Path, base: &str, proof: &Proof, f: &Value) -> Result<Option<String>, String> {
    let id = job.id;
    let n = commits_since(dir, base).await?;
    if n != 2 {
        return Ok(Some(format!("Expected exactly 2 commits on top of {base} (the test, then your fix), found {n}.")));
    }
    if rev(dir, "HEAD~1").await? != proof.test_commit {
        return Ok(Some(format!("The test commit {} was changed or replaced. Leave it as it is and put only the fix on top.", proof.test_commit)));
    }
    if let Some(p) = dirty(dir).await? {
        return Ok(Some(p));
    }

    app.log("fixer", Some(id), &format!("running `{}` with the fix (must pass)", proof.cmd));
    let out = run_here(dir, &proof.cmd, 1800).await;
    if let Some(p) = green_problem(&out) {
        return Ok(Some(p));
    }
    app.db.update_data(id, |d| {
        d["proof"]["green_exit"] = json!(out.code);
        d["proof"]["green_tail"] = json!(sh::tail(&out.out, 1500));
    });
    app.log("fixer", Some(id), "proof ok: the test fails without the fix and passes with it");

    if !repo.test_cmd.is_empty() {
        app.wait_unpaused().await;
        app.db.set_state(id, "ci", "running the full test suite");
        app.set("ci", "WORKING", Some(id), Some(&format!("{} running...", sh::head(&repo.test_cmd, 40))));
        app.log("ci", Some(id), &format!("running `{}` for {}", repo.test_cmd, label(job)));
        let t = Instant::now();
        let o = run_here(dir, &repo.test_cmd, 3600).await;
        let mins = t.elapsed().as_secs() / 60;
        involve(app, id, "ci");
        if o.code != 0 {
            app.set("ci", "IDLE", None, Some(&format!("red after {mins} min")));
            app.log("ci", Some(id), &format!("red after {mins} min (exit {})", o.code));
            return Ok(Some(format!("The project's test command `{}` fails (exit {}):\n{}", repo.test_cmd, o.code, sh::tail(&format!("{}{}", o.out, o.err), 3000))));
        }
        app.set("ci", "IDLE", None, Some(&format!("green after {mins} min")));
        app.log("ci", Some(id), &format!("green after {mins} min"));
    }

    app.wait_unpaused().await;
    app.db.set_state(id, "review", "reviewing the change");
    app.set("reviewer", "WORKING", Some(id), Some(&format!("reviewing {}", label(job))));
    let diff = sh::ok("git", &["diff", &format!("{base}..HEAD")], dir, 60).await?;
    let prompt = format!(
        "{}\n\nAuthor's explanation:\nCause: {}\nFix: {}\n\n<diff>\n{}\n</diff>",
        untrusted(job),
        f["cause"].as_str().unwrap_or(""),
        f["fix"].as_str().unwrap_or(""),
        sh::head(&diff, 100_000)
    );
    let r = claude::ask(app, Ask { agent: "reviewer", job: Some(id), system: prompts::REVIEWER, prompt, cwd: dir, tools: false, resume: None, max_turns: 1, secs: 900 }).await?;
    involve(app, id, "reviewer");
    match r.json["ok"].as_bool() {
        Some(true) => {
            app.set("reviewer", "IDLE", None, Some("ok"));
            app.log("reviewer", Some(id), "ok");
            Ok(None)
        }
        Some(false) => {
            let p = r.json["problems"].as_str().unwrap_or("").to_string();
            app.set("reviewer", "IDLE", None, Some("changes requested"));
            app.log("reviewer", Some(id), &format!("changes requested: {}", sh::head(&p, 300)));
            Ok(Some(format!("The reviewer rejected the change:\n{p}")))
        }
        None => Err("the reviewer gave no readable verdict".into()),
    }
}

fn short(name: &str) -> &str {
    name.rsplit(|c: char| ":./ >".contains(c)).find(|s| !s.is_empty()).unwrap_or(name)
}

fn both(o: &sh::Out) -> String {
    sh::tail(&format!("{}{}", o.out, o.err), 2500)
}

/// None = a real failing test. Some(reason) otherwise.
fn red_problem(tests: &[String], o: &sh::Out) -> Option<String> {
    if o.code == -1 {
        return Some(format!("The test command could not run or timed out:\n{}", both(o)));
    }
    if o.code == 0 {
        return Some(format!("The test passes on the current code. It must fail there because of the bug:\n{}", both(o)));
    }
    if Regex::new(NOT_A_TEST_FAILURE).unwrap().is_match(&o.out) {
        return Some(format!("The test doesn't compile or load. That doesn't count; it must fail an assertion:\n{}", both(o)));
    }
    if !tests.iter().any(|n| o.out.contains(short(n))) {
        return Some(format!("None of the named tests ({}) appear in the output:\n{}", tests.join(", "), both(o)));
    }
    None
}

fn green_problem(o: &sh::Out) -> Option<String> {
    (o.code != 0).then(|| format!("The regression test still fails with your fix (exit {}):\n{}", o.code, both(o)))
}

async fn ready(app: &App, id: i64, repo: &Repo, dir: &Path, base: &str, f: Value) -> Result<(), String> {
    let job = app.db.job(id).ok_or("job vanished")?;
    let diff = sh::ok("git", &["diff", &format!("{base}..HEAD")], dir, 60).await?;
    let files: Vec<String> = sh::ok("git", &["diff", "--name-only", &format!("{base}..HEAD")], dir, 60).await?.lines().map(String::from).collect();
    let title = f["title"].as_str().unwrap_or(&job.title).to_string();
    let body = pr_body(&job, &f, repo.auto);
    app.db.update_data(id, |d| {
        d["diff"] = json!(sh::head(&diff, 200_000));
        d["files"] = json!(files);
        d["pr_title"] = json!(title);
        d["pr_body"] = json!(body);
        d["fix"] = f;
    });
    if repo.auto {
        app.db.set_state(id, "approved", "auto-submit is on for this repo");
        app.wake.notify_one();
    } else {
        app.db.set_state(id, "approve", "verified, waiting for your approval");
        app.log("submit", Some(id), &format!("{} is ready and verified, waiting for your approval", label(&job)));
    }
    Ok(())
}

fn pr_body(job: &Job, f: &Value, auto: bool) -> String {
    let p = &job.data["proof"];
    let tests = p["tests"].as_array().into_iter().flatten().filter_map(|t| t.as_str()).map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", ");
    let reference = if job.issue > 0 { format!("Fixes #{}", job.issue) } else { "Bug found and reported directly (no GitHub issue).".into() };
    format!(
        "{reference}\n\n## Cause\n{}\n\n## Fix\n{}\n\n## Proof\nRegression test {tests}, run with `{}`:\n- fails on the test commit, without the fix\n- passes on the fix commit\n\n---\n🤖 Generated with AI (Claude Code) by MyOffice{}.",
        f["cause"].as_str().unwrap_or(""),
        f["fix"].as_str().unwrap_or(""),
        p["test_cmd"].as_str().unwrap_or(""),
        if auto { "" } else { ", approved by a human before submitting" }
    )
}

// ================================================================ Submit

async fn submit(app: &App, job: &Job) -> Result<(), String> {
    let id = job.id;
    let s = |k: &str| job.data[k].as_str().unwrap_or("").to_string();
    let (branch, default, base) = (s("branch"), s("default"), s("base"));
    let dir = workdir(app, &job.repo);
    app.db.set_state(id, "submit", "pre-push gate");
    app.set("submit", "WORKING", Some(id), Some(&format!("pre-push gate {}", label(job))));
    sh::ok("git", &["checkout", "-q", "-f", &branch], &dir, 60).await?;

    // Pre-push gate: if the default branch moved, rebase and prove it again.
    sh::ok("git", &["fetch", "-q", "origin"], &dir, 600).await?;
    let tip = rev(&dir, &format!("origin/{default}")).await?;
    if tip != base {
        app.log("submit", Some(id), "default branch moved, rebasing and re-checking the proof");
        let rb = sh::run("git", &["rebase", "-q", &format!("origin/{default}")], &dir, 600).await;
        if rb.code != 0 {
            let _ = sh::run("git", &["rebase", "--abort"], &dir, 60).await;
            return Err("rebase conflict with the current default branch; reject it or retry the job".into());
        }
        let cmd = job.data["proof"]["test_cmd"].as_str().unwrap_or("").to_string();
        let tests: Vec<String> = job.data["proof"]["tests"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect();
        sh::ok("git", &["checkout", "-q", "-f", "HEAD~1"], &dir, 60).await?;
        let red = run_here(&dir, &cmd, 1800).await;
        sh::ok("git", &["checkout", "-q", "-f", &branch], &dir, 60).await?;
        let green = run_here(&dir, &cmd, 1800).await;
        if let Some(p) = red_problem(&tests, &red).or_else(|| green_problem(&green)) {
            return Err(format!("after rebasing: {}", sh::head(&p, 300)));
        }
        app.db.update_data(id, |d| d["base"] = json!(tip));
    } else {
        app.log("submit", Some(id), "default branch unchanged, proofs still valid");
    }

    let perm = sh::ok("gh", &["repo", "view", &job.repo, "--json", "viewerPermission", "-q", ".viewerPermission"], &dir, 60).await?;
    let can_write = matches!(perm.trim(), "ADMIN" | "MAINTAIN" | "WRITE");
    let (url, head) = if can_write {
        (format!("https://github.com/{}.git", job.repo), branch.clone())
    } else {
        let me = sh::ok("gh", &["api", "user", "-q", ".login"], &dir, 60).await?.trim().to_string();
        app.log("submit", Some(id), &format!("no write access to {}, pushing to your fork", job.repo));
        let _ = sh::run("gh", &["repo", "fork", &job.repo, "--clone=false"], &dir, 120).await; // fine if it already exists
        // ponytail: assumes your fork has the same name as the original.
        let name = job.repo.rsplit('/').next().unwrap_or("");
        (format!("https://github.com/{me}/{name}.git"), format!("{me}:{branch}"))
    };

    let refspec = format!("HEAD:refs/heads/{branch}");
    let mut pushed = Err(String::new());
    for _ in 0..3 {
        // Push with gh's login, whatever credential helper git has configured.
        pushed = sh::ok("git", &["-c", "credential.helper=", "-c", "credential.helper=!gh auth git-credential", "push", "-f", "-q", &url, &refspec], &dir, 600).await;
        if pushed.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_secs(10)).await; // a fresh fork takes a moment
    }
    pushed?;

    let body_file = app.data.join(format!("pr-{id}.md"));
    std::fs::write(&body_file, s("pr_body")).map_err(|e| e.to_string())?;
    let out = sh::ok("gh", &["pr", "create", "-R", &job.repo, "--base", &default, "--head", &head, "--title", &s("pr_title"), "--body-file", &body_file.to_string_lossy()], &dir, 120).await;
    let _ = std::fs::remove_file(&body_file);
    let out = out?;
    let pr = out.lines().rev().find(|l| l.starts_with("http")).unwrap_or("").to_string();
    app.db.update_data(id, |d| d["can_merge"] = json!(can_write));
    app.db.set_pr(id, &pr, "OPEN");
    app.db.set_state(id, "done", "PR opened");
    involve(app, id, "submit");
    app.set("submit", "IDLE", None, Some(&format!("PR opened: {pr}")));
    app.log("submit", Some(id), &format!("PR opened: {pr}"));
    Ok(())
}

pub async fn merge(app: &App, job: &Job) -> Result<(), String> {
    if job.pr_state != "OPEN" {
        return Err("this PR is not open".into());
    }
    if job.data["can_merge"].as_bool() != Some(true) {
        return Err("you don't have write access to this repository; the maintainers merge it".into());
    }
    sh::ok("gh", &["pr", "merge", &job.pr_url, "--squash", "--delete-branch"], &app.data, 120).await?;
    app.db.set_pr(job.id, &job.pr_url, "MERGED");
    app.log("submit", Some(job.id), &format!("merged {} ドーン! 🎉", job.pr_url));
    app.changed();
    Ok(())
}

// ================================================================ Watcher, Test Agent

pub async fn watch_loop(app: Arc<App>) {
    loop {
        tokio::time::sleep(Duration::from_secs(300)).await;
        for j in app.db.open_prs() {
            app.set("watcher", "WORKING", Some(j.id), Some(&format!("watching {}", label(&j))));
            if let Ok(st) = sh::ok("gh", &["pr", "view", &j.pr_url, "--json", "state", "-q", ".state"], &app.data, 60).await {
                let st = st.trim();
                if !st.is_empty() && st != "OPEN" {
                    app.db.set_pr(j.id, &j.pr_url, st);
                    app.log("watcher", Some(j.id), &format!("{} is now {}", j.pr_url, st.to_lowercase()));
                }
            }
        }
        app.set("watcher", "IDLE", None, None);
    }
}

pub async fn test_agent(app: &App, agent: &str) {
    let Some((id, _)) = crate::AGENTS.iter().find(|(a, _)| *a == agent) else { return };
    app.set(id, "WORKING", None, None);
    let t = Instant::now();
    let r = claude::ask(app, Ask { agent: id, job: None, system: "Answer only with JSON.", prompt: r#"Reply with exactly {"ok": true}"#.into(), cwd: &app.data, tools: false, resume: None, max_turns: 1, secs: 120 }).await;
    let msg = match r {
        Ok(r) if r.json["ok"] == true => format!("test ok: {} answered in {:.1}s", app.model(id), t.elapsed().as_secs_f32()),
        Ok(_) => "test: answered, but not with the expected JSON".into(),
        Err(e) => format!("test failed: {e}"),
    };
    app.log(id, None, &msg);
    app.set(id, if *id == "hunter" { "OFF" } else { "IDLE" }, None, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(code: i32, s: &str) -> sh::Out {
        sh::Out { code, out: s.into(), err: String::new() }
    }

    #[test]
    fn red_and_green_rules() {
        let t = vec!["tests::parses_empty".to_string()];
        assert!(red_problem(&t, &out(0, "test result: ok")).is_some(), "red must fail");
        assert!(red_problem(&t, &out(101, "error[E0425]: cannot find value")).is_some(), "compile error isn't red");
        assert!(red_problem(&t, &out(1, "test other ... FAILED")).is_some(), "named test must appear");
        assert!(red_problem(&t, &out(-1, "")).is_some(), "timeouts don't count");
        assert!(red_problem(&t, &out(101, "test tests::parses_empty ... FAILED")).is_none(), "real red");
        assert!(green_problem(&out(101, "")).is_some());
        assert!(green_problem(&out(0, "ok")).is_none());
    }
}
