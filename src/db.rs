use rusqlite::{params, Connection, Params, Row};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

const SCHEMA: &str = "
pragma journal_mode=wal;
create table if not exists repos(
  full text primary key,
  test_cmd text not null default '',
  auto integer not null default 0,
  enabled integer not null default 1);
create table if not exists jobs(
  id integer primary key,
  repo text not null,
  issue integer not null,
  title text not null,
  url text not null default '',
  state text not null,
  why text not null default '',
  data text not null default '{}',
  pr_url text not null default '',
  pr_state text not null default '',
  created integer not null,
  updated integer not null,
  unique(repo, issue));
create table if not exists events(
  id integer primary key,
  t integer not null,
  worker text not null,
  job integer,
  msg text not null);
";

#[derive(Clone, Serialize)]
pub struct Repo {
    pub full: String,
    pub test_cmd: String,
    pub auto: bool,
    pub enabled: bool,
}

/// One GitHub issue moving through the pipeline. `state` is the stage:
/// queued → preparing → fixing → verifying → ci → reviewing → approve → approved
/// → submitting → pr_open → merged/closed, or skipped/blocked/dropped.
#[derive(Clone, Serialize)]
pub struct Job {
    pub id: i64,
    pub repo: String,
    pub issue: i64,
    pub title: String,
    pub url: String,
    pub state: String,
    pub why: String,
    /// Free-form details: issue body, branch, base commit, fix answer, proof, diff, PR text.
    pub data: Value,
    pub pr_url: String,
    pub pr_state: String,
    pub created: i64,
    pub updated: i64,
}

const COLS: &str = "id, repo, issue, title, url, state, why, data, pr_url, pr_state, created, updated";

fn job_row(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        repo: r.get(1)?,
        issue: r.get(2)?,
        title: r.get(3)?,
        url: r.get(4)?,
        state: r.get(5)?,
        why: r.get(6)?,
        data: serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or(json!({})),
        pr_url: r.get(8)?,
        pr_state: r.get(9)?,
        created: r.get(10)?,
        updated: r.get(11)?,
    })
}

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        let c = Connection::open(path)?;
        c.execute_batch(SCHEMA)?;
        // Work that was cut off by a restart starts over; a cut-off submit waits for approval again.
        c.execute("update jobs set state='queued', why='restarted' where state in ('preparing','fixing','verifying','ci','reviewing')", [])?;
        c.execute("update jobs set state='approve', why='restarted while submitting, approve again' where state='submitting'", [])?;
        Ok(Db(Mutex::new(c)))
    }

    fn c(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }

    pub fn repos(&self) -> Vec<Repo> {
        let c = self.c();
        let mut s = c.prepare("select full, test_cmd, auto, enabled from repos order by full").unwrap();
        let v = s
            .query_map([], |r| Ok(Repo { full: r.get(0)?, test_cmd: r.get(1)?, auto: r.get(2)?, enabled: r.get(3)? }))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        v
    }

    pub fn repo(&self, full: &str) -> Option<Repo> {
        self.repos().into_iter().find(|r| r.full == full)
    }

    pub fn save_repo(&self, r: &Repo) {
        self.c()
            .execute(
                "insert into repos(full, test_cmd, auto, enabled) values(?1, ?2, ?3, ?4)
                 on conflict(full) do update set test_cmd=excluded.test_cmd, auto=excluded.auto, enabled=excluded.enabled",
                params![r.full, r.test_cmd, r.auto, r.enabled],
            )
            .unwrap();
    }

    pub fn delete_repo(&self, full: &str) {
        self.c().execute("delete from repos where full=?1", params![full]).unwrap();
    }

    pub fn seen(&self, repo: &str, issue: i64) -> bool {
        self.c().query_row("select count(*) from jobs where repo=?1 and issue=?2", params![repo, issue], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0
    }

    pub fn add_job(&self, repo: &str, issue: i64, title: &str, url: &str, state: &str, why: &str, data: &Value) -> i64 {
        let c = self.c();
        c.execute(
            "insert into jobs(repo, issue, title, url, state, why, data, created, updated) values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
             on conflict(repo, issue) do update set title=excluded.title, state=excluded.state, why=excluded.why, data=excluded.data, updated=excluded.updated",
            params![repo, issue, title, url, state, why, data.to_string(), now()],
        )
        .unwrap();
        c.query_row("select id from jobs where repo=?1 and issue=?2", params![repo, issue], |r| r.get(0)).unwrap()
    }

    fn query_jobs(&self, tail: &str, p: impl Params) -> Vec<Job> {
        let c = self.c();
        let mut s = c.prepare(&format!("select {COLS} from jobs {tail}")).unwrap();
        let v = s.query_map(p, job_row).unwrap().filter_map(Result::ok).collect();
        v
    }

    pub fn job(&self, id: i64) -> Option<Job> {
        self.query_jobs("where id=?1", params![id]).pop()
    }

    pub fn jobs(&self) -> Vec<Job> {
        self.query_jobs("order by updated desc limit 300", [])
    }

    /// Approved jobs go first so a PR you approved doesn't wait behind new fixes.
    pub fn next_job(&self) -> Option<Job> {
        self.query_jobs("where state in ('approved','queued') order by state='queued', id limit 1", []).pop()
    }

    pub fn open_prs(&self) -> Vec<Job> {
        self.query_jobs("where pr_state='OPEN'", [])
    }

    pub fn set_state(&self, id: i64, state: &str, why: &str) {
        self.c().execute("update jobs set state=?2, why=?3, updated=?4 where id=?1", params![id, state, why, now()]).unwrap();
    }

    pub fn set_data(&self, id: i64, d: &Value) {
        self.c().execute("update jobs set data=?2, updated=?3 where id=?1", params![id, d.to_string(), now()]).unwrap();
    }

    pub fn set_pr(&self, id: i64, url: &str, state: &str) {
        self.c().execute("update jobs set pr_url=?2, pr_state=?3, updated=?4 where id=?1", params![id, url, state, now()]).unwrap();
    }

    // ponytail: events grow forever; add "delete where t < now-30d" if the DB gets big.
    pub fn event(&self, worker: &str, job: Option<i64>, msg: &str) {
        self.c().execute("insert into events(t, worker, job, msg) values(?1, ?2, ?3, ?4)", params![now(), worker, job, msg]).unwrap();
    }

    pub fn events(&self, n: i64) -> Vec<Value> {
        let c = self.c();
        let mut s = c.prepare("select t, worker, job, msg from events order by id desc limit ?1").unwrap();
        let mut v: Vec<Value> = s
            .query_map(params![n], |r| Ok(json!({"t": r.get::<_, i64>(0)?, "worker": r.get::<_, String>(1)?, "job": r.get::<_, Option<i64>>(2)?, "msg": r.get::<_, String>(3)?})))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        v.reverse();
        v
    }
}
