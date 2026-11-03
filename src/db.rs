use rusqlite::{params, Connection, OptionalExtension, Params, Row};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64
}

/// Days since 1970 (UTC). Used as the key for "tokens today".
pub fn now_ms() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()
}

pub fn day() -> i64 {
    now() / 86_400
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
  kind text not null default 'issue',
  title text not null,
  url text not null default '',
  state text not null,
  why text not null default '',
  data text not null default '{}',
  pr_url text not null default '',
  pr_state text not null default '',
  created integer not null,
  updated integer not null);
create index if not exists jobs_repo_issue on jobs(repo, issue);
create index if not exists jobs_state on jobs(state);
create table if not exists events(
  id integer primary key,
  t integer not null,
  agent text not null,
  job integer,
  msg text not null);
create table if not exists tokens(
  day integer not null,
  agent text not null,
  tok integer not null default 0,
  usd real not null default 0,
  primary key(day, agent));
create table if not exists settings(k text primary key, v text not null);
create table if not exists machines(
  id text primary key,
  name text not null,
  host text not null default '',
  lanes integer not null default 1,
  enabled integer not null default 1);
insert or ignore into machines(id, name, host, lanes) values('me', 'ME', '', 1);
create table if not exists sessions(token text primary key, used integer not null);
";

/// Logged-in browsers kept; past this, the one unused for the longest is logged out.
const SESSIONS: i64 = 20;

/// Where jobs run: this PC ("me", host "") or another machine's Docker (`ssh://user@host`).
#[derive(Clone, Serialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub host: String,
    /// Jobs it runs at the same time.
    pub lanes: i64,
    pub enabled: bool,
}

#[derive(Clone, Serialize)]
pub struct Repo {
    pub full: String,
    pub test_cmd: String,
    pub auto: bool,
    pub enabled: bool,
    /// Let Hunter read the code and look for bugs nobody reported.
    pub hunt: bool,
    /// Turn failed CI runs on the default branch into jobs.
    pub ci: bool,
}

/// One bug moving through the office. `state`:
/// vet → ready → repro → fix → ci → review → approve → approved → submit → done,
/// or skipped / blocked / dropped. PR status (OPEN/MERGED/CLOSED) lives in `pr_state`.
#[derive(Clone, Serialize)]
pub struct Job {
    pub id: i64,
    pub repo: String,
    /// GitHub issue number, 0 for a bug you described yourself ("direct").
    pub issue: i64,
    pub kind: String,
    pub title: String,
    pub url: String,
    pub state: String,
    pub why: String,
    /// Details: issue body, branch, base, proof, fix answer, diff, PR text, agents involved.
    pub data: Value,
    pub pr_url: String,
    pub pr_state: String,
    pub created: i64,
    pub updated: i64,
}

const COLS: &str = "id, repo, issue, kind, title, url, state, why, data, pr_url, pr_state, created, updated";

fn job_row(r: &Row) -> rusqlite::Result<Job> {
    Ok(Job {
        id: r.get(0)?,
        repo: r.get(1)?,
        issue: r.get(2)?,
        kind: r.get(3)?,
        title: r.get(4)?,
        url: r.get(5)?,
        state: r.get(6)?,
        why: r.get(7)?,
        data: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or(json!({})),
        pr_url: r.get(9)?,
        pr_state: r.get(10)?,
        created: r.get(11)?,
        updated: r.get(12)?,
    })
}

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> anyhow::Result<Db> {
        let c = Connection::open(path)?;
        c.execute_batch(SCHEMA)?;
        let _ = c.execute("alter table repos add column hunt integer not null default 0", []); // added after v0.1
        let _ = c.execute("alter table repos add column ci integer not null default 1", []);
        Ok(Db(Mutex::new(c)))
    }

    /// Only when the office itself starts (never from `login-link` and other commands, which may
    /// run next to a working office): work cut off by a restart starts over; a cut-off submit
    /// waits for approval again.
    pub fn recover(&self) {
        let c = self.c();
        let _ = c.execute("update jobs set state='ready', why='restarted' where state in ('repro','fix','ci','review')", []);
        let _ = c.execute("update jobs set state='approve', why='restarted while submitting, approve again' where state='submit'", []);
    }

    fn c(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap()
    }

    // ---------------------------------------------------------------- repos

    pub fn repos(&self) -> Vec<Repo> {
        let c = self.c();
        let mut s = c.prepare("select full, test_cmd, auto, enabled, hunt, ci from repos order by full").unwrap();
        let v = s
            .query_map([], |r| Ok(Repo { full: r.get(0)?, test_cmd: r.get(1)?, auto: r.get(2)?, enabled: r.get(3)?, hunt: r.get(4)?, ci: r.get(5)? }))
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
                "insert into repos(full, test_cmd, auto, enabled, hunt, ci) values(?1, ?2, ?3, ?4, ?5, ?6)
                 on conflict(full) do update set test_cmd=excluded.test_cmd, auto=excluded.auto, enabled=excluded.enabled, hunt=excluded.hunt, ci=excluded.ci",
                params![r.full, r.test_cmd, r.auto, r.enabled, r.hunt, r.ci],
            )
            .unwrap();
    }

    pub fn delete_repo(&self, full: &str) {
        self.c().execute("delete from repos where full=?1", params![full]).unwrap();
    }

    // ---------------------------------------------------------------- jobs

    pub fn find_job(&self, repo: &str, issue: i64) -> Option<i64> {
        self.c().query_row("select id from jobs where repo=?1 and issue=?2 and issue>0", params![repo, issue], |r| r.get(0)).optional().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_job(&self, repo: &str, issue: i64, kind: &str, title: &str, url: &str, state: &str, why: &str, data: &Value) -> i64 {
        let c = self.c();
        c.execute(
            "insert into jobs(repo, issue, kind, title, url, state, why, data, created, updated) values(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
            params![repo, issue, kind, title, url, state, why, data.to_string(), now()],
        )
        .unwrap();
        c.last_insert_rowid()
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

    pub fn jobs(&self, limit: i64) -> Vec<Job> {
        self.query_jobs("order by updated desc limit ?1", params![limit])
    }

    pub fn jobs_in(&self, state: &str) -> Vec<Job> {
        self.query_jobs("where state=?1 order by id", params![state])
    }

    /// Takes the next job for a lane in one locked step, so two lanes never get the same one.
    /// Approved jobs first, so a PR you approved doesn't wait behind new fixes.
    pub fn claim(&self, lane: &str) -> Option<Job> {
        let c = self.c();
        let mut s = c.prepare(&format!("select {COLS} from jobs where state in ('approved','ready') order by state='ready', id limit 1")).unwrap();
        let j = s.query_row([], job_row).optional().unwrap()?;
        let state = if j.state == "approved" { "submit" } else { "repro" };
        c.execute("update jobs set state=?2, why=?3, updated=?4 where id=?1", params![j.id, state, format!("taken by {lane}"), now()]).unwrap();
        Some(j)
    }

    pub fn machines(&self) -> Vec<Machine> {
        let c = self.c();
        let mut s = c.prepare("select id, name, host, lanes, enabled from machines order by id='me' desc, name").unwrap();
        let v = s
            .query_map([], |r| Ok(Machine { id: r.get(0)?, name: r.get(1)?, host: r.get(2)?, lanes: r.get(3)?, enabled: r.get(4)? }))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        v
    }

    pub fn save_machine(&self, m: &Machine) {
        self.c()
            .execute(
                "insert into machines(id, name, host, lanes, enabled) values(?1, ?2, ?3, ?4, ?5)
                 on conflict(id) do update set name=excluded.name, host=excluded.host, lanes=excluded.lanes, enabled=excluded.enabled",
                params![m.id, m.name, m.host, m.lanes, m.enabled],
            )
            .unwrap();
    }

    pub fn delete_machine(&self, id: &str) {
        self.c().execute("delete from machines where id=?1 and id<>'me'", params![id]).unwrap();
    }

    pub fn open_prs(&self) -> Vec<Job> {
        self.query_jobs("where pr_state='OPEN'", [])
    }

    pub fn set_state(&self, id: i64, state: &str, why: &str) {
        self.c().execute("update jobs set state=?2, why=?3, updated=?4 where id=?1", params![id, state, why, now()]).unwrap();
    }

    /// Read-modify-write of a job's `data` under one lock.
    pub fn update_data(&self, id: i64, f: impl FnOnce(&mut Value)) {
        let c = self.c();
        let Some(s) = c.query_row("select data from jobs where id=?1", params![id], |r| r.get::<_, String>(0)).optional().unwrap() else { return };
        let mut d: Value = serde_json::from_str(&s).unwrap_or(json!({}));
        f(&mut d);
        c.execute("update jobs set data=?2, updated=?3 where id=?1", params![id, d.to_string(), now()]).unwrap();
    }

    pub fn set_pr(&self, id: i64, url: &str, state: &str) {
        self.c().execute("update jobs set pr_url=?2, pr_state=?3, updated=?4 where id=?1", params![id, url, state, now()]).unwrap();
    }

    pub fn counts(&self) -> Value {
        let c = self.c();
        let n = |s: &str| c.query_row("select count(*) from jobs where state=?1", params![s], |r| r.get::<_, i64>(0)).unwrap_or(0);
        json!({"vet": n("vet"), "ready": n("ready"), "approve": n("approve"), "done": n("done")})
    }

    // ---------------------------------------------------------------- log, tokens, settings

    // ponytail: events grow forever; add "delete where t < now-30d" if the DB gets big.
    pub fn event(&self, agent: &str, job: Option<i64>, msg: &str) {
        self.c().execute("insert into events(t, agent, job, msg) values(?1, ?2, ?3, ?4)", params![now(), agent, job, msg]).unwrap();
    }

    pub fn events(&self, n: i64) -> Vec<Value> {
        let c = self.c();
        let mut s = c.prepare("select t, agent, job, msg from events order by id desc limit ?1").unwrap();
        let mut v: Vec<Value> = s
            .query_map(params![n], |r| Ok(json!({"t": r.get::<_, i64>(0)?, "agent": r.get::<_, String>(1)?, "job": r.get::<_, Option<i64>>(2)?, "msg": r.get::<_, String>(3)?})))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        v.reverse();
        v
    }

    pub fn add_tokens(&self, agent: &str, tok: i64, usd: f64) {
        self.c()
            .execute(
                "insert into tokens(day, agent, tok, usd) values(?1, ?2, ?3, ?4)
                 on conflict(day, agent) do update set tok=tok+excluded.tok, usd=usd+excluded.usd",
                params![day(), agent, tok, usd],
            )
            .unwrap();
    }

    pub fn tokens_today(&self) -> Map<String, Value> {
        let c = self.c();
        let mut s = c.prepare("select agent, tok from tokens where day=?1").unwrap();
        let m = s.query_map(params![day()], |r| Ok((r.get::<_, String>(0)?, json!(r.get::<_, i64>(1)?)))).unwrap().filter_map(Result::ok).collect();
        m
    }

    /// What today's Claude runs would cost at API prices (on a subscription you don't pay this per run).
    pub fn usd_today(&self) -> f64 {
        self.c().query_row("select coalesce(sum(usd), 0) from tokens where day=?1", params![day()], |r| r.get(0)).unwrap_or(0.0)
    }

    /// A new login. Keeps the 20 most recently used, so a browser you use is never pushed out.
    pub fn add_session(&self, token: &str) {
        let c = self.c();
        c.execute("insert into sessions(token, used) values(?1, ?2)", params![token, now()]).unwrap();
        c.execute("delete from sessions where token not in (select token from sessions order by used desc, rowid desc limit ?1)", params![SESSIONS]).unwrap();
    }

    /// Is this a logged-in browser? Records the use (at most once a minute).
    pub fn session_ok(&self, token: &str) -> bool {
        if token.is_empty() {
            return false;
        }
        let c = self.c();
        let t = now();
        let _ = c.execute("update sessions set used=?2 where token=?1 and used < ?2 - 60", params![token, t]);
        c.query_row("select count(*) from sessions where token=?1", params![token], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0
    }

    pub fn get(&self, k: &str) -> Option<String> {
        self.c().query_row("select v from settings where k=?1", params![k], |r| r.get(0)).optional().unwrap()
    }

    pub fn set(&self, k: &str, v: &str) {
        self.c().execute("insert into settings(k, v) values(?1, ?2) on conflict(k) do update set v=excluded.v", params![k, v]).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lanes claim jobs one at a time: approved first, never the same job twice.
    #[test]
    fn claim_is_exclusive() {
        let f = std::env::temp_dir().join(format!("myoffice-db-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let db = Db::open(&f).unwrap();
        let a = db.add_job("o/r", 1, "issue", "a", "", "ready", "", &json!({}));
        let b = db.add_job("o/r", 2, "issue", "b", "", "approved", "", &json!({}));
        let first = db.claim("ME/1").unwrap();
        assert_eq!(first.id, b, "approved goes first");
        assert_eq!(db.job(b).unwrap().state, "submit");
        assert_eq!(db.claim("ME/2").unwrap().id, a);
        assert!(db.claim("ME/3").is_none(), "nothing left to take");
        assert_eq!(db.machines()[0].id, "me", "this PC is always there");
        drop(db);
        let _ = std::fs::remove_file(&f);
    }
}

#[cfg(test)]
mod recover_tests {
    use super::*;

    /// Opening the database (e.g. `myoffice login-link` next to a running office) must not
    /// touch jobs that are being worked on; only the office's own start resets them.
    #[test]
    fn open_does_not_reset_running_jobs() {
        let f = std::env::temp_dir().join(format!("myoffice-recover-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let db = Db::open(&f).unwrap();
        let id = db.add_job("o/r", 1, "issue", "a", "", "fix", "", &json!({}));
        drop(db);
        let again = Db::open(&f).unwrap();
        assert_eq!(again.job(id).unwrap().state, "fix", "a second open leaves it alone");
        again.recover();
        assert_eq!(again.job(id).unwrap().state, "ready", "the office's own start restarts it");
        drop(again);
        let _ = std::fs::remove_file(&f);
    }
}

#[cfg(test)]
mod session_tests {
    use super::*;

    /// A browser in use stays logged in however many new logins come after it.
    #[test]
    fn used_browser_is_never_pushed_out() {
        let f = std::env::temp_dir().join(format!("myoffice-sess-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let db = Db::open(&f).unwrap();
        db.add_session("mine");
        db.c().execute("update sessions set used = used + 1000 where token='mine'", []).unwrap(); // recently used
        for i in 0..30 {
            db.add_session(&format!("test{i}"));
        }
        assert!(db.session_ok("mine"), "the browser you use stays");
        assert!(!db.session_ok("test0"), "the oldest unused one went");
        assert!(!db.session_ok(""));
        assert!(!db.session_ok("nope"));
        drop(db);
        let _ = std::fs::remove_file(&f);
    }
}
