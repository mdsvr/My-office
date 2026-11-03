# MY OFFICE

A manga-style office of AI agents that turns GitHub issues into verified pull requests, on your own PC,
with your own AI accounts or local models. See [PLAN.md](PLAN.md) for the architecture.

## Run it

Needs: Rust, Node.js, `git`, the GitHub CLI logged in (`gh auth login`), and at least one AI provider
(Claude Code logged in, an API key, or a local model server such as Ollama). Docker Desktop is recommended.

```sh
cd web && npm install && npm run build && cd ..   # build the dashboard (once, and after UI changes)
cargo run --release                               # start the office
```

The terminal prints a **one-time login link**: open it. Need a new one? `myoffice login-link`.
Then **Repos → add `owner/repo`** with its test command, and press **⚡ Run Full Pipeline**
(or **+ New Issue** for one specific issue or bug).

Every fix waits in the QUEUE panel as **WAITING FOR YOU** until you approve it, unless you turn on
Auto-PR for that repository. You can edit the PR title and text before approving.

## AI providers and local models

Each agent has its own provider and model (bar under the office: pick an agent, then provider / model):

| Provider | What it needs |
|---|---|
| `claude` | Claude Code logged in (default) |
| `ollama`, `lmstudio` | the local server running; any model with tool support (qwen2.5/3.5, llama3.1+, ...) |
| `custom` | any OpenAI-compatible server (llama.cpp, vLLM, ...): set its address in ⚙ Settings |
| `openai`, `anthropic`, `gemini`, `groq`, `openrouter` | an API key in ⚙ Settings (stored in Windows Credential Manager) or the env variable |

Every provider gets the same office tools in the same sandbox. Small local models are slower and less reliable:
good for Scout/Guard/Vet, try bigger ones for Repro/Fixer.

## Ask the Office

**❓ Ask** in the header: type a request (about a repo, or a general question). HUNTER investigates in the sandbox
(reads the code, may run tests, changes nothing), REVIEWER checks the answer against the code, and you get a report:
**verdict** (possible / possible with caveats / not possible / need more information), the answer, **do** and
**don't** lists, a plan, effort, risks, evidence (file:line) and open questions. Nothing is changed or pushed.

At the end of the answer, **NEXT STEP**: add your own points to the plan (optional), then **Proceed** or **Not now**.
Proceed starts a build: FIXER implements the request + plan + your additions with tests, the full test suite and
REVIEWER check it (rejections go back to FIXER, up to 3 rounds), and it comes back as **WAITING FOR YOU** with the
diff and editable PR text. Only your approval opens the pull request.

## Workers

**Machines** popup: this PC plus other machines with Docker, reached as `ssh://user@host` with your SSH key.
Each machine runs **lanes**: jobs at the same time, each with its own working copy. More lanes use your AI plan
faster (rate limits).

## What the office picks up

- open GitHub issues (Scout → Guard → Vet); issues that already have a PR fixing them are skipped
- **Hunter** (per repo): reads the code once a day and reports up to 3 bugs
- **failed CI** on the default branch (per repo, on by default): the failure log becomes a job
- **your tools**: `POST /v1/report` with the API token from ⚙ Settings
  (`{"repo": "owner/repo", "title": "...", "body": "..."}`)
- **Sentry**: `POST /v1/hook/sentry?repo=owner/repo&token=...` (needs a tunnel, the office only listens on this PC)

Everything from outside goes through Guard first.

**Smart Watcher** follows the office's open PRs: new comments from the repo's maintainers become a follow-up job
(Guard → Fixer → CI → Reviewer → your approval → push + reply on the PR). Merge conflicts are rebased: pushed on
their own if the rebase was clean and the tests pass, otherwise after your approval.

## Settings (⚙)

Providers (address, key, optional price), a **daily budget** in USD (pauses AI calls), **ntfy** push
notifications, the API token, and the update check. Releases: tag `vX.Y.Z` and GitHub builds
`myoffice-windows-x64.exe`; the dashboard shows when a newer one exists (you replace the exe yourself, the files
aren't signed).

**MY OFFICE FM**: click **TUNE** on the boombox for internet radio by country, name or genre.

Environment variables: `MYOFFICE_PORT` (47400), `MYOFFICE_DATA` (`./data`), `MYOFFICE_CLAUDE`, `MYOFFICE_BASH`,
`MYOFFICE_SANDBOX=local`, `MYOFFICE_UPDATE_REPO`, and the provider key variables (`OPENAI_API_KEY`, ...).

## Sandbox

With Docker Desktop running, MY OFFICE builds a sandbox image on first start (about 2 minutes) and does all repo
work in the `myoffice-sandbox` container: checkouts, the agents' tools, tests and CI. The container has its own
volume, no access to your drives, and no GitHub, Claude or API credentials. Your PC only keeps a bare copy of each
repo (`data/mirror`) to fetch from and push to GitHub. The header shows **docker sandbox** or **local**.

Test commands run inside the sandbox, which is Linux: use `python3 -m pytest -q`, `npm test`, etc. (`py` also
works there). Without Docker everything runs on this PC instead, and the agents' shell is limited: no pushing,
no installs into the system, no paths outside the repo.

## Start with Windows

```sh
cd F:\MyOffice
target\release\myoffice.exe autostart on    # "off" to undo
```

## Safety

Every pull request and every PR reply waits for your approval unless you turn on Auto-PR for a repo. Agents never
see your claude.ai connectors, your settings or your files. In local mode (no Docker), only add repositories you
trust.
