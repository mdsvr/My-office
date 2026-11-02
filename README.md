# MY OFFICE

A manga-style office of AI agents that turns GitHub issues into verified pull requests, on your own PC,
with your own Claude Code login. See [PLAN.md](PLAN.md) for the architecture.

## Run it

Needs: Rust, Node.js, `git`, the GitHub CLI logged in (`gh auth login`), and Claude Code logged in.

```sh
cd web && npm install && npm run build && cd ..   # build the dashboard (once, and after UI changes)
cargo run --release                               # start the office
```

Open http://127.0.0.1:47400. Then **Repos → add `owner/repo`** with its test command, and press
**⚡ Run Full Pipeline** (or **+ New Issue** for one specific issue or bug).

Every fix waits in the QUEUE panel as **WAITING FOR YOU** until you approve it, unless you turn on
Auto-PR for that repository.

**Hunter**: tick "Let HUNTER look for unreported bugs" on a repo, and once a day it reads the code
(read-only) and reports up to 3 bugs, which go through Guard and Vet like issues.

**MY OFFICE FM**: click **TUNE** on the boombox to search internet radio (local stations by country code,
or by name, or Lo-fi/Jazz/Anime). Click the boombox to play or pause. Your station, volume and country are
remembered in this browser. Stations come from [radio-browser.info](https://www.radio-browser.info).

Settings via environment variables: `MYOFFICE_PORT` (47400), `MYOFFICE_DATA` (`./data`),
`MYOFFICE_CLAUDE` (path to `claude.exe`), `MYOFFICE_BASH` (path to bash).

UI development with hot reload: run the server, then `cd web && npm run dev` (it forwards `/api` and `/events`).

## ⚠ Safety

Until Docker isolation is added (PLAN.md step 5), Repro and Fixer run commands, and CI runs the repo's
tests, **directly on this PC**. Only add repositories you trust.
