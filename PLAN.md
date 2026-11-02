# MyOffice — architecture plan

Goal: the dashboard in `manga_animator_multi_agent_studio.tsx`, unchanged in look, driven by a real
Rust backend that turns GitHub issues into verified pull requests with Claude Code.

## 1. Big picture

```
 Browser (React + Tailwind, your TSX design)
   │  GET /api/state        snapshot on load
   │  GET /events  (SSE)    live: agent status, log lines, job/PR changes
   │  POST /api/...         buttons: scan, new issue, approve, merge, pause, model switch
   ▼
 Rust server (axum, 127.0.0.1:47400)  ── serves the built UI from web/dist
   │
   ├── SQLite (data/office.db)   repos, jobs, prs, events, tokens, settings
   │
   └── Pipeline (tokio tasks)
         Scout ─► Guard ─► Vet ─► Repro ─► Fixer ─► CI ─► Reviewer ─► [you approve] ─► Submit ─► Watcher
                                    ▲ Hunter (optional: finds bugs without an issue)
         runs:  claude -p (your login)  ·  git  ·  gh  ·  the repo's own test command
```

## 2. The 10 agents: what each one really does

| Agent | Model (default) | Real job | What the card shows |
|---|---|---|---|
| SCOUT | Haiku | `gh issue list` on each repo, skips features/docs/assigned, asks Claude which are real bugs | "checking owner/repo", "3 new issues" |
| GUARD | Haiku | Checks the issue text for prompt injection ("ignore your instructions…"); blocks it | "checking #12", "blocked #12: …" |
| VET (x3) | Haiku | Is it a real, focused, testable bug? Is there already a PR for it? | "vetting #12" |
| HUNTER | Sonnet | Optional, off by default: reads the code and files its own bug reports | "hunting in src/…" |
| REPRO | Sonnet | Writes commit 1: a regression test only. The pipeline runs it and it **must fail** | "[local] takes owner/repo#12" |
| FIXER | Sonnet | Writes commit 2: the fix. The pipeline runs the test again and it **must pass** | "round 1/3: patching #12" |
| CI | no model | Runs the repo's full test command on the fix | "green after 3 min" |
| REVIEWER | Opus | Reads issue + diff, answers ok / problems; problems go back to Fixer | "ok" |
| SUBMIT | no model | Pre-push gate (default branch moved? re-verify), push branch (your fork if needed), `gh pr create` | "PR opened: …" |
| WATCHER | no model | Every 5 min: open PRs merged? closed? new review comments? | "#7 merged" |

The design's split of Repro and Fixer is better than my first draft: two separate Claude runs, each with one
job, and the red/green proof sits between them.

Retry loop: if CI or Reviewer rejects, the feedback goes back to Fixer (max 3 rounds), then the job is dropped.

## 3. Wiring each part of the design to real data

| Design element | Today (simulated) | Real version |
|---|---|---|
| Header `online/paused` | local state | `office.paused` from server; **Pause office** stops new work after the current step |
| Header `local` stamp | static | machine name (later: `docker` when isolation is on) |
| Counter `vet · ready · approve · done` | fake numbers | live job counts per state |
| **⚡ Run Full Pipeline** | plays the animation | `POST /api/scan`: Scout checks all repos now |
| **+ New Issue** modal | free text → animation | enter an issue URL / `#number` **or** a free-text bug description (job without a GitHub issue; PR says "found by MyOffice") |
| **Repos** modal | hardcoded ao-testbed | list + add/remove repo, test command, "submit without asking" toggle |
| **Machines** modal + ME rack | fake CPU/VRAM | real OS, CPU count, Docker yes/no, current CI command + elapsed time; blades light up while CI runs |
| Roster cards (10) | fake status/tokens | `status`, `taskText`, `model`, tokens today (from Claude's usage report) |
| Desk speech bubble | canned text | the agent's latest real log line |
| Bio strip: **Model** select | local only | `POST /api/agent/model`, saved, used on the next run |
| Bio strip: **Test Agent** | fake | runs a tiny `claude -p` ping with that agent's model ("ok in 2.1s") |
| Bio strip: `$ promptSnippet` | canned | the agent's last real tool call (e.g. `→ Bash cargo test parse_empty`) |
| QUEUE panel | fake | in progress job + **waiting for approval** cards + recent done/dropped |
| PULL REQUESTS panel | fake | real PRs; OPEN / MERGED / CLOSED from Watcher |
| Diff modal | one removed/added line | real unified diff (all files), cause, fix, red/green proof, "Verified by: scout → … → submit" |
| **Merge** button | fake | `gh pr merge` — only shown when you have write access to that repo |
| LOG panel + filter | fake | live SSE stream, saved history |
| Themes, boombox, plants, matcha | fun | stay client-side (theme saved in localStorage) |

**One thing the design is missing: the approval step.** By default nothing is pushed until you approve it.
I'll add "WAITING FOR YOU" cards in the QUEUE panel with **Diff**, **Approve & open PR** and **Reject**, in the same manga style.

## 4. Backend

**Crates:** axum, tokio, rusqlite (bundled), serde/serde_json, regex, anyhow, tokio-stream, rust-embed (serve UI).

**Files**
```
src/main.rs       start server + pipeline tasks
src/db.rs         SQLite schema and queries            (already written)
src/sh.rs         run git/gh/bash with timeouts        (already written)
src/claude.rs     run `claude -p`, stream tool calls & tokens to the log   (already written)
src/prompts.rs    system prompts per agent             (already written, add guard/vet/repro)
src/agents.rs     live agent state (status, task, said, tokens, model)
src/pipeline.rs   scout/guard/vet → repro → fixer → ci → reviewer → submit, plus watcher
src/web.rs        HTTP API + SSE + embedded UI
```

**Tables**
- `repos(full, test_cmd, auto, enabled)`
- `jobs(id, repo, issue, kind[fix|hunt|direct], title, url, state, why, data json, created, updated)`
  - states: `vet → ready → repro → fix → ci → review → approve → approved → submit → done` / `dropped` / `blocked` / `skipped`
  - `data`: issue body, branch, base commit, test/fix commits, proof, diff, PR text, agents involved
- `prs(repo, num, url, title, state, job_id)`
- `events(t, agent, job, msg)` · `tokens(day, agent, input, output, usd)` · `settings(key, value)` (models, paused)

**API**
```
GET  /api/state            agents, counts, queue, prs, last 200 log lines, repos, machine, paused
GET  /api/job?id=          full job: diff, proof, cause/fix
GET  /events               SSE: {"type":"log"|"agent"|"job"|"pr", ...}
POST /api/scan             run Scout now
POST /api/issue            {repo, issue} or {repo, text}
POST /api/repo             {full, test_cmd, auto}      POST /api/repo/delete {full}
POST /api/job/approve|reject|retry {id}
POST /api/pr/merge         {repo, num}
POST /api/agent/model      {agent, model}               POST /api/agent/test {agent}
POST /api/office/pause     {paused}
```

**Safety (kept from ApolloOffice's ideas, written fresh)**
- Server only on 127.0.0.1; POSTs need an `x-myoffice` header + Host check (blocks other websites).
- Issue text is wrapped as untrusted; Guard blocks injection attempts.
- Claude gets only Read/Edit/Write/Glob/Grep/Bash; `git push`, `git remote/config`, `gh`, `curl`, `ssh` denied.
- Only Submit pushes, and only after your approval (unless you turn on auto for a repo).
- ⚠ Without Docker, Claude's Bash and the test commands run on your PC. Phase 4 adds Docker.

## 5. Frontend

```
web/                    Vite + React + TypeScript + Tailwind + lucide-react
  src/App.tsx           your layout, simulation removed
  src/agents.ts         static flavour per agent (jpName, color, role, sfx, accessory) from your file
  src/api.ts            fetch + EventSource, one `useOffice()` hook returning live state
  src/components/       ChibiBotDesk, MangaPlant, StudioBoombox, RosterCard, QueuePanel,
                        PrPanel, LogPanel, DiffModal, ReposModal, MachinesModal, NewIssueModal
```
Your TSX gets split into these pieces as-is: same classes, same SVGs. Only the data source changes.
`npm run build` → `web/dist`, embedded into the Rust binary, so you run a single `myoffice.exe`.
During development: `npm run dev` (hot reload) proxies `/api` and `/events` to the Rust server.

## 6. Build order

1. **Toolchain**: install Rust + VS C++ Build Tools (you run the commands, see README).
2. **UI shell**: Vite project from your TSX, split into components, still on fake data. You see it in the browser.
3. **Backend core**: server, DB, SSE, repos, manual "New Issue", Repro → Fixer → red/green proof → CI → Reviewer → approve → Submit.
   Connect UI to `/api/state` + `/events`. **First real PR.**
4. **Scout, Guard, Vet, Watcher, Merge button, model switching, Test Agent, token counts.**
5. **Docker isolation** for Repro/Fixer/CI (the `local` stamp becomes `docker`).
6. Optional: Hunter, cost tracking, notifications, autostart.

Test target: a small repo of your own with planted bugs (like ApolloOffice's `ao-testbed`), so PRs don't go to strangers while testing.
