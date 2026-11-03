//! "Ask the Office": you give a request, the agents investigate and answer whether it's possible
//! and what to do / not do. Nothing is changed or pushed: it ends with a report.
//!
//! Hunter investigates in a sandbox space of its own (reads the code, may run commands and tests,
//! commits nothing). Reviewer then checks the report against the code (read-only) and corrects
//! anything that is overclaimed. Without a repo it is a plain question with no tools.

use crate::db::Job;
use crate::llm::{self, Ask};
use crate::pipeline::{ci_and_review, commits_since, dirty, involve, label, mirror, prepare, sync};
use crate::sandbox::space;
use crate::sh::{self, q};
use crate::tools::Toolbox;
use crate::workers::Lane;
use crate::{prompts, App};
use serde_json::{json, Value};

pub async fn run(app: &App, lane: &Lane, job: &Job) {
    if let Err(e) = investigate(app, lane, job).await {
        app.db.set_state(job.id, "dropped", &sh::head(&e, 300));
        app.log("hunter", Some(job.id), &format!("{}: couldn't answer: {e}", label(job)));
    }
    for a in ["hunter", "reviewer"] {
        app.set(a, "IDLE", None, None);
    }
}

async fn investigate(app: &App, lane: &Lane, job: &Job) -> Result<(), String> {
    let id = job.id;
    let request = job.data["body"].as_str().unwrap_or(&job.title).to_string();
    let mut tools = None;
    let mut where_ = String::from("General question, no repository.");
    if !job.repo.is_empty() {
        app.db.set_state(id, "repro", "loading the code");
        let sp = space("ask", &job.repo);
        let default = sync(app, &job.repo, "hunter", Some(id)).await?;
        lane.sb.load(&sp, &mirror(app, &job.repo), &default).await?;
        lane.sb.ok(&sp, &format!("git checkout -q -f --detach {} && git clean -fdq", q(&format!("origin/{default}"))), 120).await?;
        tools = Some(Toolbox { sb: lane.sb.clone(), space: sp, readonly: false });
        where_ = format!("Repository {} (default branch `{default}`, checked out in your working directory).", job.repo);
    }

    app.db.set_state(id, "fix", "investigating");
    app.set("hunter", "WORKING", Some(id), Some(&format!("investigating {}", label(job))));
    app.log("hunter", Some(id), &format!("investigating: {}", sh::head(&job.title, 120)));
    let prompt = format!("{where_}\n\nThe owner's request:\n<request>\n{request}\n</request>\n\nInvestigate and answer with the JSON report.");
    let r = llm::ask_json(app, Ask { agent: "hunter", job: Some(id), system: prompts::ASK_INVESTIGATE, prompt, tools: tools.as_ref().map(clone_tb), resume: None, max_turns: 80, secs: 2400 }, "verdict").await?;
    let mut report = r.json;
    if !readable(&report) {
        // Never lose the work: show what it wrote.
        if r.text.trim().is_empty() {
            return Err("Hunter gave no answer".into());
        }
        report = json!({"verdict": "see answer", "summary": "The answer below couldn't be split into sections; it's Hunter's full text.", "answer": r.text});
    }
    crate::pipeline::involve(app, id, "hunter");
    app.set("hunter", "IDLE", None, Some(&format!("verdict: {}", report["verdict"].as_str().unwrap_or("?"))));

    // Second opinion: Reviewer checks the report against the code.
    app.db.set_state(id, "review", "second opinion");
    app.set("reviewer", "WORKING", Some(id), Some(&format!("checking the answer to {}", label(job))));
    let prompt = format!("{where_}\n\nThe owner's request:\n<request>\n{request}\n</request>\n\nHunter's report:\n{}\n\nCheck it and answer with the JSON.", serde_json::to_string_pretty(&report).unwrap_or_default());
    let ro = tools.as_ref().map(|t| Toolbox { sb: t.sb.clone(), space: t.space.clone(), readonly: true });
    let review = llm::ask_json(app, Ask { agent: "reviewer", job: Some(id), system: prompts::ASK_REVIEW, prompt, tools: ro, resume: None, max_turns: 40, secs: 1200 }, "agree").await;
    crate::pipeline::involve(app, id, "reviewer");
    match review {
        Ok(v) if v.json.is_object() => {
            if v.json["agree"] == false {
                // The reviewer's verdict and corrections win; Hunter's stays visible.
                report["hunter_verdict"] = report["verdict"].clone();
                if let Some(vd) = v.json["verdict"].as_str() {
                    report["verdict"] = json!(vd);
                }
            }
            report["review"] = v.json;
            app.set("reviewer", "IDLE", None, Some(if report["review"]["agree"] == true { "agrees" } else { "corrected the answer" }));
        }
        Ok(_) | Err(_) => {
            report["review"] = json!({"agree": null, "notes": "the second opinion failed; this is Hunter's answer alone"});
            app.set("reviewer", "IDLE", None, Some("no second opinion"));
        }
    }

    let verdict = report["verdict"].as_str().unwrap_or("?").to_string();
    app.db.update_data(id, |d| d["report"] = report);
    app.db.set_state(id, "answered", &format!("answer: {verdict}"));
    app.log("hunter", Some(id), &format!("{}: answered ({verdict})", label(job)));
    app.notify("MY OFFICE: your answer is ready", &format!("{}: {verdict}", sh::head(&job.title, 80)));
    Ok(())
}

fn readable(r: &Value) -> bool {
    r.is_object() && r["verdict"].is_string()
}

fn clone_tb(t: &Toolbox) -> Toolbox {
    Toolbox { sb: t.sb.clone(), space: t.space.clone(), readonly: t.readonly }
}

/// Your request from the dashboard. Your own text, so it skips Guard and Vet.
pub fn add(app: &App, repo: &str, text: &str) -> Result<i64, String> {
    let text = text.trim();
    if text.len() < 8 {
        return Err("describe your request in at least a few words".into());
    }
    if !repo.is_empty() && app.db.repo(repo).is_none() {
        return Err("pick one of the office's repositories, or none for a general question".into());
    }
    let title = sh::head(text.lines().next().unwrap_or(text), 100);
    let id = app.db.add_job(repo, 0, "ask", &title, "", "ready", "your request, waiting for a free lane", &json!({"body": text, "agents": []}));
    app.log("scout", Some(id), &format!("new request: {title}"));
    app.wake.notify_one();
    Ok(id)
}

/// What a report looks like, for the tests and as documentation of the contract.
#[allow(dead_code)]
pub fn example() -> Value {
    json!({
        "verdict": "possible with caveats",
        "summary": "one or two sentences",
        "answer": "the full explanation",
        "do": ["concrete things to do"],
        "dont": ["things to avoid, and why"],
        "plan": ["ordered steps"],
        "effort": "small | medium | large",
        "risks": ["what could go wrong"],
        "evidence": ["src/file.py:42 what it shows"],
        "questions": ["what the owner should clarify, if anything"]
    })
}

// ---------------------------------------------------------------- Proceed: build the plan

/// "Proceed" on an answered request: a build job from the request, the plan and your additions.
pub fn proceed(app: &App, ask_id: i64, notes: &str) -> Result<i64, String> {
    let a = app.db.job(ask_id).ok_or("no such request")?;
    if a.kind != "ask" || a.state != "answered" {
        return Err("only an answered request can go ahead".into());
    }
    if a.repo.is_empty() {
        return Err("a general question has no repository to build in".into());
    }
    if let Some(b) = a.data["build"].as_i64() {
        return Err(format!("already going ahead as job {b}"));
    }
    let r = &a.data["report"];
    let plan: Vec<String> = r["plan"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(String::from)).collect();
    let id = app.db.add_job(&a.repo, 0, "build", &a.title, "", "ready", "approved by you, waiting for a free lane", &json!({
        "body": a.data["body"], "plan": plan, "notes": notes.trim(), "do": r["do"], "dont": r["dont"], "parent_ask": ask_id, "agents": ["hunter", "reviewer"],
    }));
    app.db.update_data(ask_id, |d| {
        d["build"] = json!(id);
        d["notes"] = json!(notes.trim());
    });
    app.log("scout", Some(id), &format!("going ahead with: {}", sh::head(&a.title, 100)));
    app.wake.notify_one();
    Ok(id)
}

pub fn decline(app: &App, ask_id: i64) -> Result<(), String> {
    let a = app.db.job(ask_id).ok_or("no such request")?;
    if a.kind != "ask" || a.state != "answered" || a.data["build"].is_i64() {
        return Err("nothing to decline".into());
    }
    app.db.update_data(ask_id, |d| d["declined"] = json!(true));
    app.changed();
    Ok(())
}

/// A build job on a lane: Fixer implements, the full test suite and Reviewer check it,
/// problems go back to Fixer, and the result waits for your approval like any fix.
pub async fn build(app: &App, lane: &Lane, job: &Job) {
    if let Err(e) = build_inner(app, lane, job).await {
        app.db.set_state(job.id, "dropped", &sh::head(&e, 300));
        app.log("fixer", Some(job.id), &format!("{} stopped: {e}", label(job)));
    }
    for a in ["fixer", "ci", "reviewer"] {
        app.set(a, "IDLE", None, None);
    }
}

async fn build_inner(app: &App, lane: &Lane, job: &Job) -> Result<(), String> {
    let id = job.id;
    let repo = app.db.repo(&job.repo).ok_or("this repository was removed")?;
    let (sb, sp) = (&lane.sb, lane.space(&job.repo));
    app.db.set_state(id, "repro", "preparing the repository");
    let (base, branch, default) = prepare(app, sb, job, &sp).await?;
    app.db.update_data(id, |d| {
        d["base"] = json!(base);
        d["branch"] = json!(branch);
        d["default"] = json!(default);
    });
    let d = &job.data;
    let lines = |k: &str| d[k].as_array().into_iter().flatten().filter_map(|x| x.as_str()).map(|x| format!("- {x}")).collect::<Vec<_>>().join("\n");
    let notes = d["notes"].as_str().unwrap_or("").trim();
    let context = format!(
        "Repository: {}\nBranch `{branch}` is checked out (from `{default}`).\n\nThe owner's request:\n<request>\n{}\n</request>\n\nThe approved plan:\n{}\n\nDo:\n{}\n\nDon't:\n{}\n\nThe owner's additions to the plan:\n{}",
        job.repo,
        d["body"].as_str().unwrap_or(&job.title),
        lines("plan"),
        lines("do"),
        lines("dont"),
        if notes.is_empty() { "(none)" } else { notes }
    );
    let tb = || Some(Toolbox { sb: sb.clone(), space: sp.clone(), readonly: false });
    let mut prompt = format!("{context}\n\nImplement it.");
    let mut session = None;
    let mut last = String::new();
    for round in 1..=3 {
        app.wait_unpaused().await;
        app.db.set_state(id, "fix", &format!("building, round {round} of 3"));
        app.set("fixer", "WORKING", Some(id), Some(&format!("building {}", label(job))));
        let r = llm::ask(app, Ask { agent: "fixer", job: Some(id), system: prompts::BUILD, prompt: prompt.clone(), tools: tb(), resume: session.clone(), max_turns: 150, secs: 5400 }).await?;
        if !r.session.is_empty() {
            session = Some(r.session);
        }
        let f = r.json;
        if let Some(g) = f["give_up"].as_str() {
            return Err(format!("Fixer: {g}"));
        }
        involve(app, id, "fixer");
        app.set("fixer", "IDLE", None, Some(&format!("built: {}", f["title"].as_str().unwrap_or(&job.title))));
        let problem = if commits_since(sb, &sp, &base).await? < 1 {
            Some("No commit on top of the branch.".to_string())
        } else if let Some(p) = dirty(sb, &sp).await? {
            Some(p)
        } else {
            let explain = format!("Summary: {}\nTests: {}\nLeft out: {}", f["summary"].as_str().unwrap_or(""), f["tests"].as_str().unwrap_or(""), f["not_done"].as_str().unwrap_or(""));
            ci_and_review(app, sb, job, &repo, &sp, &base, &context, &explain, prompts::BUILD_REVIEW).await?
        };
        match problem {
            None => return ready(app, job, &repo, sb, &sp, &base, &branch, f).await,
            Some(p) => {
                app.log("fixer", Some(id), &format!("round {round} rejected: {}", sh::head(&p, 300)));
                let fb = format!("Your change was rejected:\n\n{p}\n\nFix that, keep everything committed, and answer again with the same JSON.");
                prompt = if session.is_some() { fb } else { format!("{context}\n\n{fb}") };
                last = p;
            }
        }
    }
    Err(format!("no accepted build after 3 rounds: {}", sh::head(&last, 200)))
}

#[allow(clippy::too_many_arguments)]
async fn ready(app: &App, job: &Job, repo: &crate::db::Repo, sb: &crate::sandbox::Sandbox, sp: &str, base: &str, branch: &str, f: Value) -> Result<(), String> {
    sb.export(sp, &mirror(app, &job.repo), branch, base).await?;
    let range = q(&format!("{base}..HEAD"));
    let diff = sb.ok(sp, &format!("git diff {range}"), 60).await?;
    let files: Vec<String> = sb.ok(sp, &format!("git diff --name-only {range}"), 60).await?.lines().map(String::from).collect();
    let title = f["title"].as_str().unwrap_or(&job.title).to_string();
    let not_done = f["not_done"].as_str().unwrap_or("").trim().to_string();
    let body = format!(
        "{}\n\n## Tests\n{}{}\n\n---\n🤖 Generated with AI ({}) by MyOffice{}.",
        f["summary"].as_str().unwrap_or(""),
        f["tests"].as_str().unwrap_or(""),
        if not_done.is_empty() { String::new() } else { format!("\n\n## Not included\n{not_done}") },
        app.model("fixer"),
        if repo.auto { "" } else { ", approved by a human before submitting" }
    );
    app.db.update_data(job.id, |d| {
        d["diff"] = json!(sh::head(&diff, 200_000));
        d["files"] = json!(files);
        d["pr_title"] = json!(title);
        d["pr_body"] = json!(body);
        d["fix"] = json!({"cause": f["summary"], "fix": f["tests"]});
    });
    if repo.auto {
        app.db.set_state(job.id, "approved", "auto-submit is on for this repo");
        app.wake.notify_one();
    } else {
        app.db.set_state(job.id, "approve", "built and checked, waiting for your approval");
        app.log("submit", Some(job.id), &format!("{} is built and checked, waiting for your approval", label(job)));
        app.notify("MY OFFICE: build ready for you", &title);
    }
    Ok(())
}
