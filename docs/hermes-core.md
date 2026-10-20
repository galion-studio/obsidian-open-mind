<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes core: the coordinator above the seven sectors

Written 2026-10-20T21:35:00+02:00 (Europe/Zagreb, CEST). Branch `swarm/hermes-core`.
Crate `crates/hermes-core` (binary `hermes`), registry `config/hermes/sectors.toml`.
Real-test record: `docs/real-tests/hermes-core.md`. Third-party: `docs/third-party/hermes-core.md`.

## 1. What Hermes is today (before this branch)

Hermes is the boss agent, the one agent the operator hears. `stack/hermes/SOUL.md` says so:
"You supervise every agent and every process", "You are the one agent the operator hears.
The others are yours. You send them." Today that role is spread over these pieces, and none
of them coordinates sectors.

| Piece | Where | What it does | What it lacks for the swarm |
|---|---|---|---|
| hermes-rs | `crates/hermes-rs` (about 6.3k lines) | OpenAI-compatible agent API (`/v1/chat/completions`, `/v1/runs*`, `/api/sessions*`, `/api/jobs`, `/api/spend`, `/health*`). It also has the agent loop, `delegate_task` (at most 3 children, depth 1, needs the user to ask), spend caps ($1 per session, $2 per day, 500 calls), a curl provider plus a MockProvider, cron, MCP, and sessions in SQLite and JSONL. `boss()` is the base tools plus DelegateTool. The default model is `anthropic/claude-opus-5.5`. | It has no sectors, no task queue, no governor and no retry ledger, and no HADES on its own actions. It caps children at 3. |
| hermes-gateway | `crates/hermes-gateway` | The binary for hermes-rs, with `--mock` and `--check-config`. It refuses to start without an `API_SERVER_KEY` of at least 16 characters. | Same as hermes-rs. |
| desk-hermes | `crates/desk-hermes` | Rust port of `desk-hermes.py` that reads `state.db` (read-only). | It only reads state. |
| stack/hermes | `SOUL.md`, `DESK.md`, `control.json`, `guard.json`, `loops.json`, `spawn-ledger.json`, `cron/jobs.json` (4 `no_agent` jobs), `hermes.service` | Persona and voice rules (one spoken sentence first). `control.json` holds the legacy laptop qwen model with guard true. `hermes.service` is the legacy Python unit and is disabled. | This is configuration, not a coordinator. |
| Docs | `docs/ws-hermes.md` (port plan), `TEACHER/BOSS.md` (Hermes is the boss on Opus, children on Sonnet, at most 3, $10 cap), `SOURCE-OF-TRUTH.md` (at most 3 agents) | The rules. | The 3-agent rule conflicts with the swarm's measured 16. Slavik decides (G14). |

On the VPS (read-only probe, 2026-10-20 21:06 CEST):
- `hermes-rs.service` is enabled and active since 2026-10-18 22:00:49 CEST. It runs `/opt/galion/rust/bin/hermes-gateway` on `127.0.0.1:8642` with the real provider (no `--mock`), uses 4.4M of a 64M MemoryMax, and has cron and MCP off.
- `hermes.service` (Python) is inactive and disabled.
- Two strays answer `/health` with the same 0.21.4 / hermes-rs 0.1.0 body:
  - port 8652: `hermes-gateway --mock`, PID 1395619, RSS about 53 MB against 4 MB for the live unit, up since 2026-10-18 22:15:53 CEST, cwd `/root/growth-20261018`, ppid 1;
  - port 8653: `/root/growth-20261018/bin/hermes-gateway.new --mock`, PID 1443105, RSS about 29 MB, up since 22:32:28 CEST.
- `/health/detailed`, `/v1/models` and `/v1/capabilities` return 401 on all three.

hermes-core does not replace hermes-rs. hermes-rs stays the conversational agent and API. hermes-core is the coordinator layer that hermes-rs (or the swarm) calls into: it holds the registry, the queue, the governor, the retry ledger, the HADES gate and the readiness gate.

## 2. Hierarchy

```
Slavik (owner, HADES admin key)
  └─ hermes            boss, 1 session, anthropic/claude-opus-5.5 (high reasoning), 1 reserved slot
       ├─ gaia-lead            ── gaia.<task>.a<attempt>        sub-agents, anthropic/claude-sonnet-5.5
       ├─ nerve-lead           ── nerve.<task>.a<n>
       ├─ cortex-lead          ── cortex.<task>.a<n>
       ├─ vps-ops-lead         ── vps-ops.<task>.a<n>
       ├─ integration-ci-lead  ── integration-ci.<task>.a<n>
       ├─ security-lead        ── security.<task>.a<n>
       └─ docs-whitepaper-lead ── docs-whitepaper.<task>.a<n>
```

- There is exactly one coordinator. Sector leads are on-demand sessions that Hermes opens for planning and review. They do not hold a standing slot. Sub-agents are one session per task attempt, named `<sector>.<task>.a<attempt>`, so a retry is a new session with a new name and the ledger keeps every attempt.
- Leads and sub-agents never dispatch. Only `Hermes::run` takes governor permits.
- `hermes agents` lists the standing agents (boss plus 7 leads) and every sub-agent session seen in the ledger with its state.

## 3. Registration (`config/hermes/sectors.toml`, `src/registry.rs`)

The registry is one TOML file with these parts: `[hermes]`, `[retry]`, `[hades]`, `[[readiness.require_merged]]`, `[[readiness.owner]]` and seven `[[sector]]`. Each sector has `id`, `name`, `lead`, `plan` (a bare file name under `plans_dir`), `max_concurrent`, `gaia_functions`, `gaia_systems` and the planner's own `planner_sessions/tasks/target/useful`.

Mapping to Gaia uses the gaia-land registry (`gaia_core`, swarm/gaia-land 9d3ec18, merged into this branch). `Registry::validate` refuses to load if:
- a system is not one of the 52 `gaia_core::System` names;
- a system's owning function is missing from that sector's `gaia_functions`;
- a function is not one of the 9;
- a lead is not named `<id>-lead`;
- a plan is not a bare `.md` name;
- a cap is 0, above the global cap, or the measured limits are inverted.

| Sector | Lead | Cap | Gaia functions | Systems (count) |
|---|---|---|---|---|
| gaia | gaia-lead | 4 | HADES, POSEIDON, DEMETER, ARTEMIS, AETHER, ELEUTHIA | 13 |
| nerve | nerve-lead | 3 | AETHER, MINERVA | nerve, guardian-watchdog, alerting |
| cortex | cortex-lead | 4 | AETHER, MINERVA | nerve, guardian-watchdog, alerting, vision, sandbox-policy-review |
| vps-ops | vps-ops-lead | 2 | DEMETER, HEPHAESTUS, AETHER, HADES | boot-units, survival, deploy-switch, release-artifacts, console, tunnels, origin, rollback |
| integration-ci | integration-ci-lead | 3 | HEPHAESTUS, MINERVA | cargo-build, release-artifacts, repo-check, secret-scan |
| security | security-lead | 3 | MINERVA, HADES, HEPHAESTUS | 7 |
| docs-whitepaper | docs-whitepaper-lead | 2 | APOLLO | teacher-docs, session-history, stack-schema |

The sectors cover 34 of the 52 systems. `hermes sectors` lists the 18 that are not covered (for example boss-node, which is Hermes itself, tuner, persona-plex and galion-one). Hermes itself is registered in Gaia as `hermes-core -> boss-node (ARTEMIS)`. That is one new row in `crates/gaia-core/src/ownership.rs`, and the row-count test goes from 35 to 36.

## 4. Task intake and dispatch (`src/plan.rs`, `src/action.rs`, `src/engine.rs`)

**Intake.** There are two sources.
1. **Plans**: the Markdown files in `/workspace/swarm/plans`. Each `### S<n>` heading is a session. Table columns are found from the header (target, useful, deps, and `#` first-column layouts). Task ids are `S1.1`, `T1.1`, `T1` (keyed `S<n>.T<m>`) or `S4.1`, and a task key is `<sector>:<id>`.
   - Alias rows are kept out of the counts: a row with `-` counts and `(= S1.1)`, "shared with" or "not double-counted" is an alias, not a task.
   - Dependencies come from the deps column (task or session ids; text after "blocks" is ignored), and cycles are detected.
   - The loader reproduces every planner's numbers exactly: 47 sessions, 200 tasks, 645 target agents, 407 useful.
2. **Operator intake**: `hermes intake --sector S --title T [--class C] [--deps K,K]` writes an `intake` event into the ledger. The task becomes `<sector>:H<n>` and is rebuilt on every load. The class is the classifier's or the operator's, whichever is more dangerous, so an operator cannot label a merge as code.

**Classification.** Every task gets an action class, `ReadOnly < Code < Merge < Deploy < Destructive`, from its title and session title.
- Keyword lists cover destructive, deploy, merge and release words. A strong read-only list (memo, dossier, proposal, design doc, runbook, verifier, docs:) overrides them.
- `kill -9` crash tests are rewritten as `sig9-test` before matching, so a test is not treated as a kill.
- Owner sessions ("owner-gated", "one service per approval", "live cutover") are at least Deploy.
- On the real plans the result is 40 read-only, 131 code, 17 deploy, 11 destructive and 0 merge. The plans leave merges to Slavik, and the one merge in the real test comes from operator intake.

**States.** `new → queued → running → done`. The other states are:
- `queued → held` (gated, no token; waits for Slavik) and `held → queued` (approved);
- `queued → refused` (HADES says no);
- `running → retry-wait → queued` (request-limit stop or stall);
- `running → failed` (retries or error budget used up);
- `queued → blocked` (a dependency was refused, failed or is out of scope).

Every transition is a ledger event, and `Hermes::apply` replays the ledger. After a crash, a task left `running` comes back as a stall and is retried, not lost.

**Dispatch** (`dry_run` and `run`):
- It works in session order and round-robin across sectors, so every sector gets work before any sector gets a second slot.
- A task needs `slots = min(useful, sector cap)` permits from both the global and the sector pool.
- Dependencies must be `done`.
- Gated tasks go through the HADES gate before they get a slot.
- `dispatch --dry-run` previews all of this and writes nothing (it has no side effect even on the HADES ledger). Live dispatch is refused: `run --simulate` uses the offline mock provider until credit and approval exist.

## 5. Concurrency governor (`src/governor.rs`)

- The governor is a weighted semaphore: a global pool of `concurrency_cap - reserved_slots` (16 - 1 = 15 worker slots) and one pool per sector (`max_concurrent`). Permits are released on `Drop`, so a panicking worker still gives its slots back.
- `try_acquire` is all-or-nothing across both pools. `acquire_timeout`, `clamp` (lower the cap at runtime without revoking permits that are already out) and `snapshot` (in use, peak, per sector) are also there. The snapshot is served at `/v1/governor`.
- **Why 16.** On 2026-10-20 the platform stopped agents at about 18–20 concurrent: 11 of 15 were lost at about 20:14, 16 survived from 20:34, and runs at 20 and 22 hit stops at 20:38 CEST. 16 leaves two slots under the measured floor. Readiness G6 fails if the cap is above `measured_limit_low - 2`.
- Real test: cap 16 against a limit of 18 gave **0** request-limit stops (peak 15). Cap 24 against 18 gave stops, and the retry loop drained them (section 6).

## 6. Retry loop and retry ledger (`src/retry.rs`, `src/engine.rs`)

- Stops are classed as `RequestLimit`, `Stall` or `Error`.
- Request-limit stops and stalls are retried until done, as Slavik asked: capped exponential backoff of 30, 60, 120, 240, 480 and then 600 s, up to `max_attempts = 8`.
- Plain errors get `max_error_attempts = 2` and then `failed`.
- Each retry is a new sub-agent session (`.a2`, `.a3`, …), and the wait is a `retry-wait` event with `after_secs`.
- `time_scale` compresses the waits in simulation only. The default is 0.001, so 30 s becomes 30 ms.
- `hermes ledger --retries` and `GET /v1/retries` show the retry ledger, one row per task: attempts, stops, request-limit, stalls, retries, outcome and latest start (Zagreb).

## 7. HADES approvals (`src/gate.rs`, hades-gate from swarm/hades-approval ccaab42)

- Merges, deploys and destructive work are gated. Read-only and code work is not.
- The hades-gate `Approver` verifies Ed25519 single-use tokens signed with **Slavik's** admin key. Each token is scoped to an action, a target `hermes.<sector>.<task>` and the SHA-256 of the exact ASCII plan bytes (`hermes-plan v1`, task, sector, session, class, action, title).
- Its ledger consumes nonces, so a replay is refused.
- Class mapping:
  - Deploy maps to `switch`.
  - Destructive maps to `rollback`, `heal-apply`, `rotate`, `kill` (only for "kill switch") or `destroy`.
  - Merge maps to `merge`.
- hades-gate allows only heal-apply, switch and rollback. kill and rotate are always refused, and unknown kinds (`merge`, `destroy`) are refused. So today **Hermes can never merge**, even with a signed token: merges stay with Slavik. Adding a `merge` kind is a Slavik decision (G14).
- With no HADES config (`[hades] config = ""`, the shipped default), every gated task is refused. `HADES disable` and `safety::review` win over any token.
- Decisions:
  - `held`: no token yet; Hermes writes the plan with `hermes gate --task K --plan-out F` for Slavik to sign.
  - `approved`: valid token, consumed.
  - `refused{code}`: codes are `action`, `wrong-scope`, `replayed`, `expired`, `not-yet-valid`, `lifetime`, `malformed`, `forged`, `ledger`, `disabled`, `admin-safety` (from hades-gate) and `no-hades` (from hermes-core).
- Revolut and any real-money access are not implemented and not approved.

## 8. Audit log (`src/ledger.rs`)

- The log is one JSONL file, `<state>/ledger.jsonl`. Each line has `seq`, `ts` (Zagreb with offset, for example `2026-10-20T21:28:03+02:00`, never `Z`), `unix`, `kind`, `task`, `sector`, `agent`, `session`, `attempt`, `slots`, `state`, `detail` and `prev`, where `prev` is the SHA-256 of the previous line.
- `Ledger::open` takes an exclusive `File::try_lock`, so two Hermes instances cannot interleave.
- `open` refuses a broken chain and truncates a torn last line (a crash mid-write).
- Every append is `write` plus `sync_data`.
- `hermes ledger --verify` reports chain, stamps, torn tail and head.
- Event kinds:
  - task events: intake, queued, held, approved, refused, dispatch, done, stop, retry-wait, failed, blocked;
  - run events: run-start, run-end, recover.

## 9. Reporting (`src/report.rs`)

- Every report is a `Report { spoken, detail }`. `spoken` is one sentence a voice can say:
  - at most 32 words;
  - ASCII;
  - no paths, flags, markup, or hashes;
  - small numbers written as words.
- `spoken_ok` enforces this and readiness G15 checks it. For example: "Hermes is not ready, eleven of fifteen checks pass, and what blocks it is the HADES key, the model credit, the unmerged branches and your open decisions."
- `detail` carries the evidence.
- Sub-agent briefs (`hermes brief --task K`) follow the `TEACHER/BOSS.md` report format and rules: Rust only, no secrets, HADES for gated work.

## 10. Interfaces

- **CLI** (`hermes`): `sectors`, `agents`, `plan [--sector]`, `dispatch --dry-run [--first-sessions N] [--cap N]`, `run --simulate [...]`, `intake`, `gate --task`, `brief --task`, `ledger [--tail N|--retries|--verify]`, `readiness [--repo]`, `report`, `serve --port`. Global flags are `--config`, `--plans`, `--state` and `--json`. Exit code 3 means not ready or refused.
- **HTTP status API** (`hermes serve`):
  - binds loopback only; non-loopback addresses are refused;
  - GET and HEAD only, so every other method gets 405;
  - at most 16 connections and an 8 KiB request head.
  - Routes: `/health`, `/v1/sectors`, `/v1/agents`, `/v1/tasks[?sector=]`, `/v1/dispatch`, `/v1/ledger[?tail=]`, `/v1/retries`, `/v1/governor`, `/v1/readiness`, `/v1/report`.
- **Provider trait** (`src/provider.rs`): `run(&Brief) -> RunOutcome`.
  - `OpenRouter` always refuses with the 402 message and makes no network call.
  - `MockProvider(limit, work)` models the platform request limit by counting active slots and can inject stalls.
  - A real provider is a later, approved change (G11).

## 11. Readiness gate (`hermes readiness`, `src/readiness.rs`)

Hermes is ready to coordinate the swarm only when all 15 checks pass. Results are as of 2026-10-20 21:28 CEST:
- **A** is the shipped config (`[hades] config = ""`).
- **B** is the real test, which points HADES at a throwaway test key.

| # | Check | A | B | Who fixes |
|---|---|---|---|---|
| G1 | Registry loads; every sector maps to real Gaia systems and owners | pass | pass | Hermes |
| G2 | hermes-core is in the Gaia ownership registry | pass | pass | Hermes |
| G3 | Seven plans load, no dependency cycles | pass | pass | Hermes |
| G4 | Loader reproduces every planner's task and session counts (200/47) | pass | pass | Hermes |
| G5 | Every sector has dispatchable work in its first session | pass | pass | Hermes |
| G6 | Cap at least 2 under the measured limit (16 ≤ 18−2) | pass | pass | Hermes |
| G7 | Request-limit stops retried until every runnable task completes (self-test on the real first sessions) | pass | pass | Hermes |
| G8 | Ledger hash-chained, Zagreb-stamped, survives a torn write | pass | pass | Hermes |
| G9 | HADES configured with an admin public key | **fail** | pass (test key) | Slavik (key custody) |
| G10 | Unapproved merge and deploy are not dispatchable | pass | pass | Hermes |
| G11 | A real model provider is usable (Opus boss, Sonnet subs) | **fail** | **fail** | Slavik (credit, spend) |
| G12 | integration-1 8fd26f6, hades-approval ccaab42, gaia-land 9d3ec18 merged into main | **fail** | **fail** | Slavik (merges) |
| G13 | Status API loopback-only and answers | pass | pass | Hermes |
| G14 | Slavik's open decisions recorded as decided (7 open) | **fail** | **fail** | Slavik |
| G15 | Operator report opens with one speakable sentence | pass | pass | Hermes |

The totals are 11/15 for A and 12/15 for B. Every check that fails needs Slavik, and Hermes can fix none of them alone.

## 12. What Slavik decides (the open items of G14)

1. OpenRouter credit (402 today) and spend approval for the Opus boss and Sonnet sub-agents.
2. HADES admin key custody: he generates the Ed25519 seed on his own device, and only the public key goes on the Hermes host.
3. Wave-1 merge order (ucpu before edge-boot, tuner-cutover before common, nerve main.rs over gaia-live).
4. POSEIDON vs HEPHAESTUS ownership of compute.
5. Whether HADES gets `merge` (and a dedicated deploy) action kinds so Hermes may act on a signed approval. Today merges are owner-only.
6. Stray VPS processes: hermes-gateway `--mock` on 8652 and 8653, plus openmind-memory 8431–8433. Keep or stop, one port per approval.
7. The max-agents rule: SOURCE-OF-TRUTH.md says 3 and the swarm runs 16. Record the new cap.

When a decision is recorded, set `decided = true` for that item in `sectors.toml`. G14 then passes once all seven are true.

## 13. Overlaps with other branches

- **hermes-enroll (f2121a8)**: touches `hermes-rs/{Cargo.toml, api.rs, http.rs, lib.rs, enroll/*}`, `hermes-gateway/src/main.rs` and Cargo.lock. hermes-core touches no hermes-rs file. Only Cargo.lock overlaps, and that conflict is mechanical.
- **nerve-wiring**: has uncommitted edits to hermes-rs `api.rs`, `http.rs`, `spend.rs`, `Cargo.toml` and hermes-gateway `main.rs` (a nerve_client observe hook). There is no file overlap with hermes-core.
- **integration-1 (8fd26f6)**: carries hermes-rs and hermes-gateway changes from hermes-tools. A merge-tree of this branch against integration-1 conflicts in `Cargo.lock`, `crates/gaia/src/lib.rs` and `crates/gaia/src/main.rs`. All of these are inherited from gaia-land vs integration-1, not from hermes-core. The hermes-core-specific overlap is the `ownership.rs` row and its 36-row count test.
