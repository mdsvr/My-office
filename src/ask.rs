//! "Ask the Office": you give a request, the agents investigate and answer whether it's possible
//! and what to do / not do. Nothing is changed or pushed: it ends with a report.
//!
//! Hunter investigates in a sandbox space of its own (reads the code, may run commands and tests,
//! commits nothing). Reviewer then checks the report against the code (read-only) and corrects
//! anything that is overclaimed. Without a repo it is a plain question with no tools.

use crate::db::Job;
use crate::llm::{self, Ask};
use crate::pipeline::{label, mirror, sync};
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
    let r = llm::ask(app, Ask { agent: "hunter", job: Some(id), system: prompts::ASK_INVESTIGATE, prompt, tools: tools.as_ref().map(clone_tb), resume: None, max_turns: 80, secs: 2400 }).await?;
    let mut report = r.json;
    if !readable(&report) && !r.session.is_empty() {
        // Same conversation: it keeps everything it found, and only has to restate it as JSON.
        app.log("hunter", Some(id), "answer wasn't valid JSON, asking for it again");
        let again = llm::ask(app, Ask { agent: "hunter", job: Some(id), system: prompts::ASK_INVESTIGATE, prompt: "Your answer couldn't be read. Reply again with ONLY the JSON report (no prose, no code fence), same content.".into(), tools: tools.as_ref().map(clone_tb), resume: Some(r.session.clone()), max_turns: 3, secs: 600 }).await;
        if let Ok(a) = again {
            report = a.json;
        }
    }
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
    let review = llm::ask(app, Ask { agent: "reviewer", job: Some(id), system: prompts::ASK_REVIEW, prompt, tools: ro, resume: None, max_turns: 40, secs: 1200 }).await;
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
