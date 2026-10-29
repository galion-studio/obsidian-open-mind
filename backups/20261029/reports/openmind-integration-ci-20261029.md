# OpenMind on Rust: Hermes <-> Obsidian/OpenMind integration (area 3) and tests/CI/deploy (area 4)

Written 2026-10-29 ~12:40 (+01:00, Europe/Zagreb). Read-only research. Nothing was changed on the VPS,
GitHub or the box (except this file). No git was run on the VPS. No paid model calls. VPS IP and secrets
are not in this file. All times are Europe/Zagreb unless marked.

## 0. Caveats about the box (facts)

- On arrival (~12:13) `/workspace/swarm` had been re-created at ~12:12 (+01:00) and was still being
  filled while I worked: `reports/` did not exist, then appeared holding only `kept-docs/ws-voice.md`.
  **The area 1/2 reports and the other *-20261029.md reports named in the brief are not on the box**
  (`find / -name "*20261029*.md"` -> nothing). So areas 1-2 in the checklist below are re-checked
  from live facts, not copied from those reports.
- `/workspace/swarm/base.git` has no `7ba85b60`, `ccca5545`, `1d7164b4`, `8887a0cb`, `1010eb7c`
  (`git cat-file -t` -> "Not a valid object name"); its `main` is `13a296c`. `/workspace/oom-rust`
  has `fatal: bad object HEAD`. Its remote is the VPS working copy, so I did not fetch it.
  All repo facts below come from the GitHub API (cursor-github connector, read-only) instead.
- GitHub `galion-studio/obsidian-open-mind` `main` = `7ba85b60` (confirmed via list_branches).

## Area 3: Hermes <-> Obsidian vault / OpenMind memory

### 3.1 Live services (VPS, `systemctl cat/show`, `ss -ltn`, read-only)

| Unit | Binary in use | Port | Active since | Notes |
|---|---|---|---|---|
| hermes-rs | `/opt/galion/rust/bin/hermes-gateway-b7fbbc78` (drop-in 90-deploy-t291u) | 127.0.0.1:8642 | 2026-10-27 19:46 | NRestarts 0, MemoryMax 64M. `GET /health` 200 `implementation: hermes-rs`. Cron OFF, `HERMES_RS_MCP=0`, coding runtime OFF (no `HERMES_RS_CODING_WORKTREE`) |
| openmind-memory | `/opt/galion/rust/bin/openmind-memory-aa77864` (drop-in 50) | 127.0.0.1:8420 | 2026-10-22 23:46 (+02:00) | `GET /health` 200, `read_only:false`, `fts:true`, `embeddingService:false`. Data `/var/lib/memory-tencentdb` |
| openmind-queue (POSEIDON) | `/usr/local/bin/openmind-queue` | 127.0.0.1:19120 | 2026-10-22 | not Hermes-linked |
| galion-nerve | `openmind-nerve-f66974c` (drop-in 92-merge9) | 127.0.0.1:8670 | - | binary contains `NERVE_MEMFRESH` strings, but no memfresh drop-in is installed -> sensor not registered |
| galion-one | `/opt/galion/rust/bin/galion-one` | 8091 / 8002 | 2026-10-22 | `ONE_NO_WARM=1` |

### 3.2 How live Hermes reads/writes OpenMind memory today: it does not

- `strings hermes-gateway-b7fbbc78`: 0 hits for `MEMORY_TENCENTDB_GATEWAY`, `TDAI_GATEWAY`, `OPENMIND_MEMORY`,
  `HERMES_RS_MEMORY`, `8420`, `/recall`-style memory routes, `vault`, `OBSIDIAN`. The env names it does read
  are HERMES_RS_{HOST,PORT,STATE_DB,LEGACY_STATE_DB,SPEND_FILE,caps,CRON*,MCP,PERSONA,ENROLL*,CODING*}, NERVE_URL.
- `ss` shows **no established connection to :8420**.
- `/opt/galion/hermes/.env` still carries `MEMORY_TENCENTDB_GATEWAY_HOST/PORT` and `TDAI_GATEWAY_API_KEY`
  (names only checked) and `config.yaml` still says `memory: provider: memory_tencentdb, memory_enabled: true`.
  These are Python-era keys; the live Rust binary ignores them.
- Memory store freshness (file mtimes, `find -printf`, no SQLite open): newest write
  `instances/galion/vectors.db-wal` and `conversations/2026-10-19.jsonl` = **2026-10-19 14:57**.
  Nothing has written long-term memory for ~10 days. (The branch docs attribute this to Python Hermes
  being retired 2026-10-18; that attribution is from the branch doc, not re-verified here.)
- Hermes's own state: `/var/lib/hermes-rs/rs-state.db` (sessions), `rs-spend.json`; legacy
  `/opt/galion/hermes/state.db` opened read-only.

### 3.3 How live Hermes reads/writes the Obsidian vault today: it does not

- Unit sandboxing: `ProtectSystem=strict`, `ProtectHome=read-only`, `ReadOnlyPaths=/opt/galion/hermes`,
  only `ReadWritePaths=/var/lib/hermes-rs`. So Hermes cannot write any vault path.
- Built-in tools in the Rust gateway (main-era source on box + binary strings): `clock`, `delegate_task`,
  MCP-proxied tools; no `read_file`/`write_file`/`search_files` (0 hits in the live binary). Coding tools
  (hermes-tools) are compiled in but the runtime is OFF.
- MCP: config.yaml has one server `galion-stack` -> `/usr/local/lib/hermes-agent/venv/bin/python
  /workspace/galion-light/mcp/stack-read.py` (**a Python fallback**). That interpreter path does **not exist**
  on the VPS, and `HERMES_RS_MCP=0`, so it is inert. The Rust replacement `crates/openmind-mcp-stack` on main
  is a documented **stub** ("Read-only stdio MCP stub", one tool `stack_status` returning fixture JSON).
- Obsidian-shaped directories found on the VPS (`find -maxdepth 7`, read-only):
  `/opt/galion/galion-light/docs/build-session/.obsidian` (51 .md, newest 2026-10-16 09:16) and
  `/opt/galion/hermes/vault` (empty except `.vault.lock`, 2026-09-22). Nothing in the Rust stack reads either.
- Repo code (GitHub tree of main, plus box worktree grep): "Obsidian" appears only as branding strings
  (galion-origin HTML, galion-one persona, gaia-core system list, galion-boot). **There is no vault
  reader/writer/indexer crate on main.** GitHub code search for `vault`/`obsidian` returned 0 (index
  marked `incomplete_results:true`, so that search alone is not conclusive; the tree listing is).
- Whether the GitHub repo `obsidian-open-mind` itself is "the vault" (markdown docs, TEACHER/) is an owner
  definition question; unverified.

### 3.4 What the four branches add (GitHub compare vs main 7ba85b60)

All four share merge-base `3f015986` and are **120 commits behind main** (status `diverged`). None is in
main, none has an open PR (open PRs: #14 hermes-offhost, #7 brand, #1/#2/#4/#5/#6 dependabot).

| Branch @ head | Ahead | Adds |
|---|---|---|
| swarm/memory-db-stale-diag @1d7164b4 | 5 | `openmind-memory freshness` read-only CLI (`src/freshness.rs`), Zagreb local-time rendering of stored UTC, tests `real_freshness`, `real_local_time`, `local_time`, docs/ops/memory-db-stale-diag.md |
| swarm/hermes-rs-memory-client @8887a0cb | 7 (= above + 2) | `crates/hermes-rs/src/memory.rs` (728 lines): `/recall` before each turn (3 s), `/capture` in background (10 s, max 4 inflight), fence/sanitize as the Python `memory_tencentdb` plugin. **Off unless `HERMES_RS_MEMORY=1`** or `hermes_rs.memory.enabled`; Python `memory.provider` key ignored on purpose. Loopback-only URL, default `http://127.0.0.1:8420`; key from `MEMORY_TENCENTDB_GATEWAY_API_KEY`/`TDAI_GATEWAY_API_KEY`. `/health/detailed` memory counters. Real test `real_memory_client` (doc: 9 steps, 3x PASS, negative control FAIL) |
| swarm/hermes-memory-session-scope @ccca5545 | 8 (= above + 1) | recall scoped to session by default (`scope:"session"`), cross-session only via `HERMES_RS_MEMORY_CROSS_SESSION=1`; openmind-memory server honours/echoes `scope` (store.rs, engine.rs, memory-api.md); **fail closed** when the server does not echo scope (the live `aa77864` server does not: strings 0 hits) -> no memory block, `recall_unscoped`+1. So client and server must ship together. Branch doc gates: fmt ok, clippy ok, `cargo test` (excl. openmind-sandbox, cgroup-detect, hermes-tools) 2032 passed / 0 failed / 18 ignored, no-python ok (box run, doc-reported, not re-run by me) |
| swarm/memory-freshness-sensor @1010eb7c | 7 (= stale-diag + 2) | nerve sensor `sensors/memory_freshness.rs` running `openmind-memory freshness` as a child; staged drop-in `deploy/staged/memory-freshness/galion-nerve-memfresh.conf` (NOT installed; needs `/opt/galion/rust/bin/openmind-memory-freshness`, which does not exist on the VPS). Doc: VPS throwaway run on a copy of the live store reported stale 219.7 h |

Related: `swarm/openmind-write-guard` @679d404f (ahead 5, behind 121): `galion-report-push` crate
(refuses git writes into `/opt/galion/openmind`) + repo-check `openmind` rule + detach runbook. Not on main.

### 3.5 Wired / stubbed / missing (area 3)

- Wired live: hermes-rs on 8642 (chat, spend caps, nerve signals); openmind-memory on 8420 (serving, nobody calling it).
- Built but off / on branches only: memory client + session scope (ccca5545), freshness CLI + nerve sensor.
- Stubbed: openmind-mcp-stack (fixture), MCP disabled in hermes-rs, Python MCP entry in config.yaml.
- Missing: any Obsidian vault read/index/write path in the Rust stack; a write path to the vault for Hermes
  (unit is read-only by design); a decision on which directory/repo is the canonical vault.

## Area 4: tests / CI / deploy

### 4.1 GitHub Actions (obsidian-open-mind; other galion-studio repos not covered)

Workflows on main: self-eval, realtest, launch-gates, miri, release, sbom (draft), scorecard,
ui-check (registered, no file on main), Dependabot.

| Workflow | Triggers | What it runs | What it skips |
|---|---|---|---|
| self-eval | push main, PR to main / swarm/main-staging, call | fmt (+omnode), clippy -D warnings, `cargo test --workspace --locked`, replay-corpus, omnode, ucpu tier-1 (x86_64+aarch64 musl static, 2 MiB budget), tier-2 check (armv7, android), ucpu-qemu (GALION_REAL real run of galion-one, tuner, openmind-memory, hermes-gateway `--mock` under qemu), repo-check, no-python, CJK, Zagreb timestamps (warn only), gitleaks, MIT license | all other `GALION_REAL=1` real tests (real_memory, real_memory_client, real_freshness, galion-deploy real_*, ...) are not run; no live-VPS probe |
| realtest | PR to main / main-staging, dispatch | builds galion-realtest, galion-boot, galion-one, openmind-memory, tuner, galion-desk, link-watch, guardian-tick, persona-plex-relay; runs `galion-realtest stack` on ports 19500-19551 | **hermes-gateway is not in the build list**; "live probe ... stays manual" |
| launch-gates | PR, dispatch | public-export tree + gitleaks, cargo-deny, REUSE (comply), CycloneDX SBOM | - |
| miri | every push (any branch), nightly 00:17 UTC, dispatch | galion-common, hades-gate, openmind-schema only | everything else |
| release | v* tag only; release job also needs repo public AND `RELEASE_ENABLED=true` | build, SBOM, cosign, GH release | locked off |

Results (GitHub API):
- main `7ba85b60` check-runs: every self-eval job **success** (completed 2026-10-28 18:09-18:18 +01:00),
  miri-short success; nightly **miri full failed 2026-10-29 06:57** (+01:00).
- Swarm branch pushes trigger **only miri** (self-eval/realtest/launch-gates need a PR). Of the last 40 runs,
  almost all miri push runs are `failure`, including `swarm/hermes-memory-session-scope` ccca5545
  (2026-10-28 21:59). Those jobs ran **~2 s with no runner name and no steps**, i.e. they never executed
  code. Cause unverified (runner/billing/queue issue is a guess, not checked). One success at 2026-10-28
  20:22 (console-studio-tls-diag) shows it is intermittent.
- So **the four memory branches have no GitHub test result at all**; their only evidence is box/VPS-throwaway
  runs written into their own docs.
- Dependabot PRs: rusqlite 0.40.2 -> self-eval + realtest failure; toml 1.1.8 -> self-eval failure.
- Test counts: main tree has 282 `crates/*/tests/*.rs` files and 197 `docs/real-tests/*` docs. CI pass/fail
  counts per test were not retrievable (no log download tool); the only count I have is the branch-doc
  box run: 2032 passed / 0 failed / 18 ignored (382 suites) on ccca5545, unverified by me.

### 4.2 Deploy mechanism (as found)

- **Drop-ins**: every live Rust swap is an `/etc/systemd/system/<unit>.service.d/NN-*.conf` overriding
  `ExecStart=` to a sha-suffixed binary in `/opt/galion/rust/bin/`, old binary kept, rollback = rm drop-in +
  daemon-reload + restart (comments in each drop-in). hermes-rs has 50/60/70/90, openmind-memory 50/60,
  galion-nerve 50..92.
- **galion-deploy** (Rust, `/opt/galion/rust/bin/galion-deploy`, 2026-10-21): `galion-deploy <service>
  <new-binary> [health-url]` = backup, atomic install, restart, health loop (20x1 s), automatic rollback,
  journal/lock, optional fail-fast; `galion-deploy backup` pre-cutover snapshot. It replaces the binary at the
  unit's ExecStart path; it does not create drop-ins. Unit tests `cargo test -p galion-deploy` (28 + backend
  scenarios) run in CI; its real_* rehearsals do not.
- **push-branch.sh** (`/workspace/swarm/push-branch.sh`): bundles one `swarm/*` branch, scp to the VPS,
  runs `remote-push.sh` there so the GitHub token stays on the VPS. Only path from box to GitHub. Refuses
  non-`swarm/*` branches.
- Staged-not-installed artefacts on main: `deploy/staged/hermes-rs.service(.d/coding-tools.conf)`,
  `deploy/staged/openmind-memory.service`; on branches: memfresh drop-in.
- Sandbox: hermes-gateway b7fbbc78 has openmind-sandbox compiled in (coding off; Landlock absent on the PVE
  host per the 90-deploy-t291u comment). Branch real tests use throwaway instances on the VPS
  (ports 22xxx, `runuser -u nobody`), which is the de facto "sandbox live test".

### 4.3 Missing for a sandbox-first OpenMind-on-Rust deploy

1. Rebase/merge the memory branches onto main (120 behind) and open a PR so self-eval/realtest/launch-gates run.
2. Fix/explain the 2-second, no-runner miri failures (and nightly miri on main) so branch CI is meaningful.
3. CI does not run `GALION_REAL` memory tests (real_memory_client, real_memory_session_scope, real_freshness,
   real_memory_freshness_sensor) nor builds hermes-gateway in realtest; a Hermes+memory scenario in
   `galion-realtest stack` is absent.
4. No single scripted "sandbox deploy -> security checks -> promote" flow: today it is manual throwaway runs +
   hand-written drop-ins, then galion-deploy/systemctl. No automated gate binds the promote step to a sandbox pass.
5. Deploy order constraint: openmind-memory with `scope` support must go live before (or with) a hermes-rs
   build that has `HERMES_RS_MEMORY=1`, else recall fails closed (capture still works).
6. Owner decisions pending per branch docs: D-403-1 (turn memory client on), D-403-2 (session scope),
   D-388-3 / D-405-1 (freshness sensor), D-391-2 follow-up (write guard). Status of each decision: unverified.

## Commands used (all read-only)

`ssh slavik-vps` with: `systemctl list-units/cat/show`, `ss -ltn`, `ss -tnp state established`,
`curl -s http://127.0.0.1:{8642,8420}/health`, `strings <binary> | grep -c`, `ls -la`, `find -printf`
(mtimes only), `grep` on config.yaml (non-secret lines), `sed 's/=.*//'` on env files (names only),
`journalctl -u hermes-rs -o cat | grep`. GitHub API: list_branches, compare_commits (x5), get_git_tree,
get_file_contents (5 workflows, galion-deploy README), list_workflows, list_workflow_runs,
list_check_runs_for_ref, list_workflow_run_jobs (x2), search_pull_requests, search_code (x2).
