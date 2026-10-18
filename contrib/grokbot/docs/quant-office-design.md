# Quant Office on the Galion stack: design proposal

Written 2026-10-18 (Europe/Zagreb). **Design only. Nothing has been applied and the VPS was not touched.**
Sources (all local): `stack-map.md`, `hermes-openrouter-proposal.md`, `data/maps/map-20261018-151831.txt`, and the imports under `~/skills/imported-from-vps/__VPS_HOST__/`. Those imports are Hermes `SOUL.md`/`USER.md`/`channels/galion-one.md`, galion-light `SOUL.md`/`USER.md`/`NIGHT_INTERN.md`/`PERSONALITY_EXTRACT.md`, `arena/homes/g2-0..3/{SOUL,TASK}.md` + `arena/COMPARE.md`, and the skills `subagent-driven-development`, `dynamic-workflow`, `crew`, `hybrid-swarm`, `request-inbox`, `stack-map`, `nerve`, `global-agent-guardrails`, `galion/{alpha,mesh,platform}`, `hermes-agent`, ondrej `handoff` + `openrouter`, and crew-skills `finance/*`, `research/*` and personas.

Legend: **[EXISTS]** is on the box today (observed). **[NEW]** is proposed here. **[CHANGE]** modifies something that exists. **[UNVERIFIED]** is an assumption to check in Phase 0 before building on it.

---

## 0. Hard rules this design follows

1. **No Python for any new piece.** New services and CLIs are Rust. Console UI is TypeScript in the existing console-ide. Everything else is config files and Hermes-native features (profiles, delegate_task, cron, kanban, skills, SOUL/USER files). Hermes itself is Python and **[EXISTS]**. We configure it but write no Python plugins or skills for it. Desks also do not *run* Python (see §9.3).
2. **No per-agent resource limits** (Mate's call). Desks have no per-desk token, spend, CPU or RAM caps. Spend is *attributed* per desk for the journal and console. It is never *limited* per desk.
3. **Heavy compute leaves the VPS.** Backtests, model inference and number crunching go to a RunPod GPU pod through a job interface (§8). The 8 vCPU / 16 GiB CPU-only LXC only runs agents, the office service, the guard and the console.
4. **One global OpenRouter guard** (§7). It handles concurrency, rate and spend globally, so the office cannot overrun the OpenRouter API or the account.
5. **Output-language hygiene** (§9). Desks strip or flag stray CJK characters.
6. **Hermes is head of office and the only voice.** This reuses the existing SOUL line: *"You are the one agent the operator hears. The others are yours. You send them. They do not speak."*
7. **The console stays a read surface on loopback.** This was the arena consensus in `arena/COMPARE.md`: read-only, loopback only, state read fresh, no new public surface (ZeroAI). Its only write is the existing "send to Hermes" inbox.
8. **Paper only.** No live orders, no broker keys, no Polymarket (`galion/alpha` already forbids betting). Live execution is a separate, later operator decision (§13 Phase 5).

---

## 1. What exists and how the office maps onto it

| Office role | Component | Status | Notes |
|---|---|---|---|
| Head of office / boss | **Hermes** gateway, `hermes.service`, 127.0.0.1:8642, home `/opt/galion/hermes` | [EXISTS] | hermes-agent 0.21.4. `multiplex_profiles: true` is already on. `delegation.max_spawn_depth 2`, `max_concurrent_children 8`, `api_server.max_concurrent_runs 10`. |
| Operator intake | `request-inbox` skill, `/workspace/hermes/inbox.jsonl`, console `POST /api/hermes/inbox` | [EXISTS] | Operator orders keep landing here. Hermes turns them into office tickets. |
| Voice front end | **Galion One** (`one`, Rust), 127.0.0.1:8091, `one.galion.app` | [EXISTS] (fragile) | Not a systemd unit. Whisper 8643 and TTS are down (they need torch). It sends model `galion`, which 404s (Bug 2). |
| Office floor / dashboard | **Galion console** (console-ide, React 19 + TanStack + node:sqlite), 127.0.0.1:8080, `console/obsidian.galion.app` | [EXISTS] + [NEW] `/office` route | Talks to Hermes at `/v1/runs` and `/v1/chat/completions`. |
| Desk service | **galion-desk** (Rust, `galion-light/desk-rs/main.rs`), `galion-desk.service`, 127.0.0.1:8646, `/health` → `{"ok":true}` | [EXISTS], function **[UNVERIFIED]** | Its source was not imported, so its current behaviour is unknown. Proposal: extend it into the office API (§5.4). If it does something unrelated, add a sibling Rust service `galion-office` on 127.0.0.1:8647. |
| Per-agent homes | Hermes **profiles** (`$HERMES_HOME/profiles/<name>/`, own config/sessions/skills/memory) | [EXISTS] feature | The `hermes-agent` skill says profiles give "isolated configs, sessions, skills, and memory". |
| Precedent for desks | `arena/homes/g2-0..3` (4 Hermes homes, own `SOUL.md` + sealed `TASK.md`, communication through a board, `COMPARE.md`) | [EXISTS] (test) | The desk template copies this shape. COMPARE's rule "a reply that ignores the other answers did not communicate" becomes a review rule. |
| Durable work graph | Hermes **kanban swarm** (`kanban.db`), **cron** (`cron/jobs.json`, `executions.db`), `shared-state.db` | [EXISTS], schema **[UNVERIFIED]** | `dynamic-workflow` notes that delegate_task is turn-scoped and that durable work belongs to kanban or cron. |
| Ephemeral helpers | `delegate_task` (children get their own session with `parent_session_id`) | [EXISTS] | Children lose `delegate_task, clarify, memory, send_message, cronjob_manage` (per the `dynamic-workflow` skill). Grandchildren use it. |
| Hats / personas | `crew` skill + `/opt/galion/crew-skills` (`CATALOG.md`; `finance/skills/{stock-analysis,financial-analyst}`, `research/{deep-research,litreview,...}`, personas `finance-lead`, `devils-advocate`, `karpathy-reviewer`) | [EXISTS] | Rule kept: one child, one SKILL.md hat. Only the SKILL.md text is used. Its Python scripts are never run (§9.3). |
| Review discipline | `subagent-driven-development` (fresh child per task, spec review then quality review), `mesh` ("a worker does not grade its own line"), `dynamic-workflow` adversarial refuters | [EXISTS] | These become the office's review protocol (§6.4). |
| Handoff format | ondrej `handoff` skill (state not instructions, reference don't duplicate, capture the why) | [EXISTS] | Used as the body format for handoff tickets. |
| Memory | memory-gateway (TencentDB, Node), 127.0.0.1:8420, `embeddingService false`. Obsidian vault convention (markdown + frontmatter `tags/project/status`) | [EXISTS] | Hermes keeps memory-gateway. Desks use file memory (§10). |
| Shell guard | `global-agent-guardrails` (`dangerous-patterns.txt`, `deny-dangerous.sh`, `hermes/plugins/command-guard/`, `guard.json`) | [EXISTS] | Office profiles add patterns (§9.3). |
| Live telemetry | `nerve` (`GET :8091/api/v1/analytics`, cookie-gated) | [EXISTS] | The console can show box health next to the office floor. |
| Model access | OpenRouter key exists but is unusable from Hermes (Bug 1). River is the default. Auxiliary paid Opus fallback (Bug 3) | [EXISTS] (broken) | Phase 0 must apply the fixes in `hermes-openrouter-proposal.md` first. |
| Global LLM guard | `or-guard` (Rust reverse proxy), 127.0.0.1:8650 | [NEW] | §7 |
| RunPod job interface | dispatcher inside the office service + `quant-worker` (Rust) on the pod | [NEW] | §8. Endpoint is a placeholder. |
| Hygiene linter | `office-lint` (Rust CLI + library used by the office service and the guard) | [NEW] | §9 |
| Office tree | `/opt/galion/office/` | [NEW] | §4 |

### 1.1 Existing text that conflicts with the office (must be amended, with approval)

| File | Line today | Why it conflicts | Proposed change |
|---|---|---|---|
| `/opt/galion/hermes/SOUL.md` | "The primary model is the laptop… Do not switch models." / "OpenRouter is not a silent fallback… ask the admin before any cloud model." / "If port 11434 is closed, stop." | The office runs on OpenRouter by design. As written, Hermes stops whenever the laptop is down. | Replace the "Backup" section with: "Your model is OpenRouter Opus through the office guard. The laptop is optional. Never fall back silently to any *other* provider." |
| same | "Reply in exactly four sentences…" | This is fine for the operator. It is wrong for desk reports. | Keep it for operator replies. Desks get their own report format (§6.3). |
| galion-light `SOUL.md` | "Do not talk to other agents unless the operator asks." | Desks must talk to each other through tickets. | "Talk to other agents only through office tickets." |
| `crew` skill | "Two children at a time." | Would cap sub-desks. | Office profiles use the office caps (§3.3). Leave `crew` as-is for non-office Hermes sessions. |
| `hybrid-swarm` | "Cap parallel runs at 2." | Same. | Same. |
| `galion/platform` | "A hundred agents is the aim. The cap is the hardware file." | Compatible. Agents are API-bound, not hardware-bound. | Note that compute goes to RunPod. |
| `hermes-openrouter-proposal.md` §2 | `max_spawn_depth: 1`, `max_concurrent_children: 3`, `max_concurrent_runs: 3` | Was written for a single boss. | Superseded for office profiles by §3.3 and §7. Keep `depth 1` *per profile* (that is how depth 3 is reached safely). |

---

## 2. Org chart (initial)

Every box is a **desk**: one quant with its own home, SOUL, TASK, memory, workspace, session, logs and results journal.

```
                         OPERATOR (Mate / Slavik)
                 voice: Galion One   ·   screen: console /office
                                   │
                    T0  HEAD OF OFFICE: Hermes
                        anthropic/claude-opus-5.5 · reasoning high
                        desk: head/hermes
        ┌──────────────────────────┼───────────────────────────┐
  T1 MASTER: Research       T1 MASTER: Execution & Risk   T1 MASTER: Data
  (opus-5.5, high)          (opus-5.5, high)              (opus-5.5, high)
  master/research           master/exec-risk              master/data
   ├─ T2 research-signals    ├─ T2 exec-risk-risk          ├─ T2 data-ingest
   │   factor / stat-arb     │   exposure, VaR, stress     │   feeds, vendors, calendars
   ├─ T2 research-macro      ├─ T2 exec-risk-paper         ├─ T2 data-quality
   │   rates, FX, regimes    │   paper execution, TCA      │   gaps, splits, survivorship
   └─ T2 research-alt        └─ T2 exec-risk-pnl           └─ T2 data-compute
       papers, alt data,         P&L attribution,              RunPod job desk
       NLP signals               journal audit                 (quartermaster)
   (each T2 = sonnet-5.5, reasoning medium)
        └─ T3 grandchildren, on demand, per ticket (haiku-5.5, no/low reasoning)
           roles: fetcher · summarizer · checker/refuter · formatter · linter
           id pattern: g.<sub>.<yyyymmdd>-<seq>   e.g. g.research-signals.20261020-03
```

Default crew hats (one each, SKILL.md text only):

| Desk | Hat (crew-skills path) |
|---|---|
| master/research | `research/deep-research` |
| research-signals | `finance/skills/stock-analysis` |
| research-macro | `finance/skills/financial-analyst` |
| research-alt | `research/litreview` |
| master/exec-risk | persona `finance-lead` |
| exec-risk-risk | `docs/agents/devils-advocate.md` (risk is a refuter) |
| exec-risk-paper | `finance/skills/financial-analyst` |
| exec-risk-pnl | `docs/agents/karpathy-reviewer.md` (audit numbers, not prose) |
| master/data | `engineering-team/cs-engineering-lead` |
| data-* | engineering `cs-backend-engineer` |
| T3 checker/refuter | `devils-advocate` |
| T3 other roles | no hat; role text only |

Phase 1 runs only the left branch: Hermes + `master/research` + `research-signals` + `research-macro`, with grandchildren on demand (§13).

---

## 3. Models, sessions and depth

### 3.1 Model per tier (OpenRouter ids verified 2026-10-18 in `hermes-openrouter-proposal.md`)

| Tier | Model | Reasoning | $/M in / out | Why |
|---|---|---|---|---|
| T0 Hermes | `anthropic/claude-opus-5.5` | `reasoning_effort: high` | 4 / 20 | Research-grade planning and synthesis. |
| T1 masters | `anthropic/claude-opus-5.5` | high | 4 / 20 | Same. |
| T2 subs | `anthropic/claude-sonnet-5.5` | medium | 2 / 10 | Workhorse. |
| T3 grandchildren | `anthropic/claude-haiku-5.5` | off (low only for refuters) | 0.10 / 0.50 | Cheap, narrow, short-lived. |
| Hermes auxiliary (titles, compression) | `:free` only (`auxiliary.free_only: true`) | – | 0 | Fixes Bug 3. |
| Galion One voice route `galion` | `anthropic/claude-haiku-5.5` | off | – | From the earlier proposal. Hermes answers office questions itself (§11). |

Pin explicit ids, not `~anthropic/*-latest`, so a silent model change cannot move cost. The guard allowlist (§7) enforces this.

### 3.2 "Each agent its own OpenRouter session"

- **Persistent desks (T0–T2):** each desk is its own Hermes profile, so it has its own `state.db`/sessions. The guard stamps every upstream request with OpenRouter's documented fields:
  - `user = <desk-id>` (OpenRouter `external_user` analytics dimension)
  - `session_id = <desk-id>:<hermes-session-id or conversation hash>` (body field or `x-session-id` header; ≤256 chars). This groups the desk's calls in OpenRouter Logs → Sessions and gives sticky provider routing, which keeps prompt caches warm per desk.
- **Grandchildren (T3):** each delegate_task child already gets its own Hermes session (`parent_session_id`). Their LLM calls leave through the parent profile's provider, so the guard sees the sub-desk path. Fix: the sub-desk always starts a grandchild brief with a header line `DESK-ID: g.research-signals.20261020-03`. The guard scans the first user message for `DESK-ID:` and uses that as `user`/`session_id`.
- Optional: one OpenRouter API key per master, created with the provisioning API **without limits**, purely for attribution in the OpenRouter dashboard. Not needed if the `user`/`session_id` stamps are enough.

### 3.3 Depth 3, made safe

Today `delegation.max_spawn_depth: 2`. Head → master → sub → grandchild is four levels, which would need depth 3 of nested delegate_task. Pure nesting is the wrong tool:
- delegate_task is turn-scoped and dies with the process (`dynamic-workflow`).
- Children lose `memory` and `send_message`, so they cannot keep a desk.
- At depth > 0 delegation is synchronous.
- The `delegation.model` setting is one value for all children [UNVERIFIED that it cannot vary by depth], so tiers cannot get different models.
- Worst case with today's config is 10 runs × 8 × 8 ≈ 640 concurrent calls.

**Recommended (A): structural depth.** Tiers T0–T2 are *persistent profiles* linked by tickets, not by nested delegate_task. Only T2 uses delegate_task to create T3.

| Profile tier | `delegation.max_spawn_depth` | `max_concurrent_children` | `delegation.model` | delegate_task use |
|---|---|---|---|---|
| T0 Hermes | 1 | 3 | sonnet-5.5 | Quick read-only lookups only. Real work goes to masters as tickets. |
| T1 masters | 1 | 3 | haiku-5.5 | Quick checks only (formatter/linter). Real work goes to subs as tickets. |
| T2 subs | **1** | 4 | **haiku-5.5** | This is where grandchildren are spawned. |
| T3 | – (leaf; delegate_task is removed by Hermes for children) | – | – | none |

Effective office depth is 3 (Hermes → master → sub → grandchild). No Hermes runtime ever sees a nested chain deeper than 1, so a runaway recursion is impossible by construction. These are fan-out shape settings, not per-agent resource limits. Total concurrency is governed globally by the guard (§7). The children caps only keep any single delegate_task call well-formed (Hermes rejects a `tasks=[]` larger than the cap rather than queueing).

**Alternative (B), only if a single Hermes run must own the whole tree.** Set `max_spawn_depth: 3` with `max_concurrent_children: 3` (max 27 leaves per root), `subagent_auto_approve: false`, `fallback_providers: []`, and the guard in front. Downsides: no per-tier models (all children share `delegation.model`), no desk memory below T1, and work is lost when the process restarts. Not recommended.

---

## 4. Desk directory layout `[NEW]` under `/opt/galion/office`

```
/opt/galion/office/
├── OFFICE.md                 # constitution: rules §0, hygiene §9, protocol §6 (read by every desk)
├── USER.md                   # operator profile for desks (derived from hermes/USER.md)
├── org.yaml                  # org chart = source of truth (desks, parents, models, hats, status)
├── office.db                 # SQLite: tickets, desk status, jobs, journal index (single writer: office service)
├── templates/
│   ├── desk-master/   desk-sub/   desk-grand/     # SOUL/TASK/desk.yaml/hermes-config skeletons
├── desks/
│   ├── head/hermes/          # T0 desk view; SOUL.md -> /opt/galion/hermes/SOUL.md (symlink),
│   │                         #   Hermes keeps its existing HERMES_HOME
│   ├── master/<name>/        # research, exec-risk, data
│   ├── sub/<name>/           # research-signals, research-macro, ...
│   └── grand/<name>/         # g.research-signals.20261020-03 (created on demand, archived when done)
├── library/                  # office-wide approved knowledge (Obsidian-vault markdown + frontmatter)
│   ├── findings/  strategies/  postmortems/  data-dictionary/
├── data/                     # shared read-only datasets & caches (small; bulk lives on RunPod volume)
├── guard/                    # or-guard: guard.toml, ledger.jsonl (per-call spend), KILL (kill-switch file)
├── jobs/spool/               # RunPod dispatcher spool (job.json → status → result)
├── ledger/                   # nightly roll-ups: spend-by-desk, pnl-by-desk, hygiene-by-desk
└── archive/                  # retired desks, finished grandchildren (tar.zst per month)
```

**One desk** (same shape for every tier; grandchildren get a slim version):

```
desks/sub/research-signals/
├── desk.yaml          # id: s.research-signals, tier: 2, parent: m.research, model, reasoning, hat, status
├── SOUL.md            # persona (template §4.1) + hat path + OFFICE.md include
├── TASK.md            # standing mandate (sealed, like arena TASK.md); written by parent, read-only to desk
├── USER.md -> ../../../USER.md
├── hermes/            # this desk's HERMES_HOME (Hermes profile)
│   ├── config.yaml    # model/provider = guard, delegation, toolsets (§4.2)
│   ├── .env           # ONLY the local guard token (no real OpenRouter key), mode 600
│   ├── state.db  sessions/  logs/
│   ├── skills/ -> shared office skill set (read-only)
│   └── memories/      # Hermes built-in file memory (MEMORY.md, USER.md)  [UNVERIFIED path]
├── memory/            # desk notes: MEMORY.md (curated), notes/*.md (vault frontmatter)
├── workspace/         # scratch; code in git worktrees (hermes -w); never shared
├── inbox/             # mirror of open tickets assigned to this desk (T-*.md, read-only)
├── outbox/            # desk writes report.md / handoff.md / job.json here; office service ingests
├── journal/
│   ├── JOURNAL.md     # human-readable daily log
│   ├── results.jsonl  # append-only P&L / results journal (§10.3)
│   └── pnl.csv        # derived by office service
├── jobs/<job-id>/     # job.json, status.json, result/ (RunPod outputs land here)
└── logs/              # office-side log for this desk (runs, lint flags, guard 429s)
```

Profile wiring: `/opt/galion/hermes/profiles/<desk-id>` → symlink to `desks/<tier>/<name>/hermes` [UNVERIFIED that Hermes follows symlinked profiles; otherwise set `HERMES_HOME` explicitly per run, or keep the profile in place and symlink the desk dir to it].

### 4.1 SOUL template (reuses the Hermes voice contract)

```markdown
# SOUL
You are **<Name>**, a quant at the Galion Studio office. Desk `<desk-id>`, tier <T>, reporting to `<parent>`.
Hermes is head of office. You do not speak to the operator. You work your tickets and report up.

## Hat
Follow `<crew-skills path>/SKILL.md` for method. Read the text only. Do not run its scripts.

## Voice (David's contract)
Short sentences. Plain English. Results in the report, not in tool logs. No menus. No time estimates.

## Admin read
Report a strategy, a dataset, or a failure with one cut: Steam (flaw on the surface) or Diesel (stated goal hides the real one).
Then one trait and the failure it causes.

## Desk rules
- Your desk is <path>. Write only inside it. Everything else is read-only.
- Work comes from tickets. Answer in outbox/report.md in the office report format.
- A worker does not grade its own line. Every number you claim cites the file or job that produced it.
- Heavy compute goes to a RunPod job. Never run a backtest or a model on this box.
- English only. No CJK characters outside a marked source quote (OFFICE.md §hygiene).
- Paper only. No orders, no broker, no bets.
## Guard
Ondrej denylist always. No secrets, no `.env` reads, no Python.
```

### 4.2 Profile `config.yaml` skeleton (sub-desk example; key names follow the existing Hermes config, [UNVERIFIED] where marked)

```yaml
model:
  provider: office-guard
  default: anthropic/claude-sonnet-5.5
providers:                       # custom provider, same mechanism as today's `river`  [UNVERIFIED key name]
  office-guard:
    base_url: http://127.0.0.1:8650/d/s.research-signals/api/v1
    api_key_env: OFFICE_GUARD_TOKEN
agent:
  reasoning_effort: medium
delegation:
  max_spawn_depth: 1
  max_concurrent_children: 4
  provider: office-guard
  model: anthropic/claude-haiku-5.5
  reasoning_effort: low
  fallback_providers: []
  subagent_auto_approve: false
auxiliary:
  free_only: true
  provider: office-guard         # so titles/compression also pass the guard
memory:
  provider: builtin              # file memory in this HERMES_HOME (§10)  [UNVERIFIED name]
toolsets:
  disabled: [execute_code, computer_use, send_message]   # no Python sandbox; desks don't message out
```

Masters: `default: anthropic/claude-opus-5.5`, `reasoning_effort: high`, `delegation.model: anthropic/claude-haiku-5.5`. Hermes (T0): `provider: office-guard`, base_url `.../d/head.hermes/...`, Opus, high.

---

## 5. Runtime: how desks wake up and run

### 5.1 Runs
- The office service is the **scheduler**. When a ticket is assigned to a desk, it calls Hermes `POST /v1/runs` for that desk's profile. The prompt is short: ticket id, title, desk path, and "read `inbox/T-123.md`, answer in `outbox/`". The ticket text lives on disk, not in the prompt.
- Profile selection per run under `multiplex_profiles: true` is **[UNVERIFIED]** (header, model alias or route). Fallback if it is not per-request: one extra `hermes gateway` process per persistent desk on 127.0.0.1:866x, each with its own `HERMES_HOME`. That costs roughly 150–300 MB RAM each, which is fine for about 10 desks on 16 GiB.
- A desk is one person with one session. By default the scheduler gives a desk one ticket at a time and queues the rest. This is about session coherence, not a resource cap. `desk.yaml: parallel: true` lifts it.
- Standing work (morning brief, nightly journal roll-up) uses Hermes **cron** per profile (`cron/jobs.json`) [EXISTS feature]. No jobs at 02:00–03:00 on 25 Oct 2026 (the SOUL clock rule).
- `gateway.api_server.max_concurrent_runs` is raised from 10 to about 12 to cover Hermes + 3 masters + 9 subs. Real LLM concurrency is held by the guard.

### 5.2 Grandchildren
A sub-desk spawns T3 with `delegate_task`. The office service pre-creates `desks/grand/<id>/` (slim template) and hands the sub a ready brief: `DESK-ID:` line + desk path + role + hat. The child writes to its desk (`journal/`, `outbox/result.md`) with file tools. Hermes has already removed its memory tool, so its desk files *are* its memory. When the parent's ticket closes, the grandchild desk is archived. Grandchildren can never spawn.

### 5.3 Restarts
Ticket state is in `office.db` and desk files. A gateway restart (seen about 8 times on 13–16 Oct) loses only in-flight turns. The scheduler re-dispatches `in_progress` tickets with a `HANDOFF.md` note (from `dynamic-workflow` step 12).

### 5.4 Office service API (Rust, loopback only) `[NEW or CHANGE galion-desk]`
- `GET /office/org` – org.yaml + live status per desk
- `GET /office/desks/:id` – desk.yaml, SOUL/TASK/MEMORY, journal tail, open tickets, jobs
- `GET /office/tickets?desk=&status=` · `POST /office/tickets` (callers: Hermes/masters/subs through a desk CLI; console cannot)
- `POST /office/desks/:id/outbox-ingest` (internal; triggered by an inotify watch on `outbox/`)
- `GET /office/jobs` · `POST /office/jobs` (§8)
- `GET /office/guard` (proxied summary from or-guard)
- `GET /office/stream` – SSE of desk/ticket/job events for the console
- Desk-side CLI `office` (Rust, static binary in `/opt/galion/bin/office`): `office ticket new|claim|done`, `office report`, `office job submit|status`, `office lint FILE`. This is how agents act on the protocol through the `terminal` tool.

---

## 6. Communication protocol

### 6.1 Who can spawn or assign whom

| From \ To | Hermes | own masters | other masters | own subs | other teams' subs | grandchildren |
|---|---|---|---|---|---|---|
| Operator | ✅ (voice, console inbox) | via Hermes | via Hermes | via Hermes | via Hermes | – |
| Hermes | – | ✅ assign; create/retire a master desk *with operator approval* | ✅ | ✅ read; assign only via the master (no skipping) | – | – |
| Master | report up | – | handoff ticket (cross-team request) | ✅ assign; propose a new sub desk (Hermes approves) | ❌ (go through that team's master) | quick helpers only |
| Sub | report up | – | ❌ | sibling handoff inside own team (copy master) | ❌ | ✅ spawn on demand |
| Grandchild | – | – | – | report to own sub | – | ❌ (leaf) |

The office service enforces this matrix. A ticket that breaks it is rejected with a reason.

### 6.2 Ticket schema (`office.db.tickets`, mirrored to `inbox/T-<id>.md`)

```json
{
  "id": "T-20261020-0007", "parent_id": "T-20261020-0002",
  "from": "m.research", "to": "s.research-signals",
  "kind": "task | handoff | report | question | job | escalation",
  "title": "Backtest 12-1 momentum on EU large caps, 2010-2025",
  "brief_path": "desks/master/research/outbox/T-0007-brief.md",
  "acceptance": ["Sharpe, maxDD, turnover after 10bp costs", "survivorship-free universe", "job id cited"],
  "inputs": ["library/data-dictionary/eu-largecap.md"],
  "priority": "p1", "due": "2026-10-21T18:00:00+02:00",
  "status": "open | claimed | in_progress | blocked | review | done | rejected",
  "result_paths": [], "job_ids": [], "hygiene_flags": 0,
  "created_at": "...", "updated_at": "..."
}
```

Use **Hermes kanban** (`kanban.db`) as the store instead if Phase 0 shows it supports a profile assignee, parent links and custom fields. The office service then becomes a thin policy layer over it. Otherwise `office.db` is the store and kanban stays unused by the office.

### 6.3 Report format (desk → parent), `outbox/report.md`

```markdown
---
ticket: T-20261020-0007
desk: s.research-signals
status: done | blocked | partial
read: Steam|Diesel – <one trait> – <failure it causes>
cost_usd: <from guard ledger, filled by office service>
jobs: [J-…]
---
## Result (≤5 lines, numbers with units, each cites a file/job)
## Evidence (paths)
## Open risks / what would refute this
## Next (state, not instructions: what is true, what is not done)
```

A handoff (`kind: handoff`) uses the ondrej `handoff` principles in the same frame: state not instructions, reference don't duplicate, capture the why and what failed.

### 6.4 Review, reuse of existing discipline
1. **Spec check** by the parent against `acceptance` (subagent-driven-development stage 1).
2. **Quality / refutation** by a *different* desk: a T3 `checker/refuter` grandchild with the `devils-advocate` hat, or `exec-risk-pnl` for any P&L claim. Mesh rule: a worker does not grade its own line.
3. Claims must be falsifiable and located ("Sharpe 0.84 in `jobs/J-31/result/summary.json`"), so a refuter can break them (dynamic-workflow adversarial convergence; cap at 3 rounds).
4. Only reviewed reports go up a tier. Only master-approved findings enter `library/`.

### 6.5 Reporting cadence
- Grandchild → sub: on completion (result file + one-line status).
- Sub → master: on ticket completion + an end-of-day journal entry (cron).
- Master → Hermes: on completion of a Hermes ticket + a daily desk note (cron, 07:30 Europe/Zagreb).
- Hermes → operator: on request (voice/console), plus a morning brief in the console. Spoken output is the first sentence only (SOUL "Spoken").
- Escalation (`kind: escalation`) goes straight to the parent, bypassing queue order. Triggers: blocked > 2 h, data integrity doubt, guard at 80 %, any request that would need live trading or a secret.

---

## 7. Global OpenRouter guard `or-guard` `[NEW]` (Rust, 127.0.0.1:8650)

Purpose: not crashing or overrunning the OpenRouter API or account. It is **global**. There are no per-desk caps.

```
desks/Hermes profiles ──► or-guard :8650 ──► https://openrouter.ai/api/v1
memory-gateway ───────────────(own key, direct; or via guard with lane "memory")
```

| Function | Behaviour |
|---|---|
| Single key holder | The real `OPENROUTER_API_KEY` lives only in the guard's `EnvironmentFile` (mode 600, under `/root/galion-secrets/`). Profiles hold a local `OFFICE_GUARD_TOKEN`, which only works against 127.0.0.1:8650. This removes the Bug 1 class (per-profile `.env` in multiplex mode). It only works if the real key is also **removed from `hermes.env`/process env**, so nothing can bypass the guard. |
| Global concurrency | One semaphore for in-flight upstream requests (default 16, set in `guard.toml`). Excess requests **queue** with a deadline (default 120 s) instead of failing. Priority lanes: `head` (Hermes, voice) > `master` > `sub` > `grand`, so a grandchild burst cannot starve the operator. |
| Global rate | Token bucket on requests/min (default 120 RPM) and on estimated input tokens/min. |
| Global spend ceiling | Running cost from OpenRouter response `usage` (cost included in usage accounting) is appended to `guard/ledger.jsonl`. Daily ceiling (placeholder **$75/day**) and monthly ceiling. At 80 %: event to the console + escalation ticket to Hermes. At 100 %: only the `head` lane passes. Everything else waits until the operator raises the ceiling or the day rolls over. |
| Backstop | OpenRouter key **credit limit** set in the OpenRouter dashboard (native). This was recommended already in the earlier proposal. |
| Backoff | 429/408/502: honour `Retry-After`, exponential backoff with jitter, bounded attempts. A circuit breaker opens after N consecutive upstream failures and pauses non-head lanes for 60 s. 402: never retry; raise a console alert (from ondrej `openrouter/reliability.md`). |
| Model allowlist | Only `anthropic/claude-opus-5.5`, `claude-sonnet-5.5`, `claude-haiku-5.5`, plus listed `:free` models. Rejects `~anthropic/claude-opus-latest` (the Bug 3 auxiliary fallback) and anything unlisted. |
| Attribution | Injects `user=<desk-id>` and `session_id=<desk-id>:<session>` (§3.2). Records desk, tier, model, tokens, cost and latency per call. |
| Hygiene probe | Counts CJK code points in each response (§9) and logs the count per desk. It does **not** rewrite model output, because that would corrupt tool-call JSON and streaming. |
| Kill switch | If `/opt/galion/office/guard/KILL` exists, all lanes except `head` get 503 `office paused`. Hermes can tell the operator in one sentence. |
| Visibility | `GET /guard/status`: in-flight, queue depth per lane, RPM, spend today/month vs ceiling, breaker state. Shown on the console. |

Rough daily cost for Phase 1, without prompt caching, to size the placeholder ceiling. Reasoning tokens bill as output, so Opus-high can run higher.
- Hermes: 40 turns × (60k in, 4k out) → ≈ $13
- master: 60 × (60k, 4k) → ≈ $19
- 2 subs: 300 × (40k, 3k) → ≈ $33
- grandchildren: 1,000 × (15k, 1k) → ≈ $2
- **Total ≈ $67/day.** `session_id` stickiness and prompt caching should cut the input share noticeably.

---

## 8. RunPod job interface `[NEW]` (endpoint = placeholder)

The VPS never runs heavy compute. Flow: **desk submits job → dispatcher → RunPod worker → results back to desk.**

```
desk ── office job submit job.json ──► office service (dispatcher, Rust)
         │                                 │ outbound HTTPS only (no new public port on VPS)
         │                                 ▼
         │                       RUNPOD_ENDPOINT = <TBD>   (pod proxy URL or serverless endpoint)
         │                                 │
         │                       quant-worker (Rust, on the GPU pod)
         │                                 │ writes results to pod volume
         │◄── result/ + summary.json ◄── dispatcher polls status, downloads artifacts
         └── report ticket to the desk + results.jsonl entry + console event
```

### 8.1 Job spec (`desks/<…>/jobs/<job-id>/job.json`)

```json
{
  "job_id": "J-20261020-0031",
  "idempotency_key": "s.research-signals:T-0007:momentum-eu-v3",
  "desk": "s.research-signals", "ticket": "T-20261020-0007",
  "kind": "backtest | inference | sweep | crunch",
  "runtime": { "image": "<registry>/galion/quant-worker:<tag>", "entry": "backtest", "args": ["--config", "cfg.toml"] },
  "inputs": [ { "name": "cfg.toml", "sha256": "…", "path": "jobs/J-…/in/cfg.toml" },
              { "name": "prices", "uri": "volume://datasets/eu-largecap/2010-2025.parquet" } ],
  "hints": { "gpu": "any", "est_minutes": 20 },
  "timeout_minutes": 180,
  "outputs": ["summary.json", "equity.parquet", "trades.parquet", "log.txt"]
}
```

`hints` is advisory scheduling information, not a per-agent limit. `timeout_minutes` exists only so a hung job cannot hold the pod forever.

### 8.2 Dispatcher ↔ worker contract (HTTP + bearer token; token in `/root/galion-secrets/runpod.env`, name only)

| Call | Purpose |
|---|---|
| `PUT  {RUNPOD_ENDPOINT}/artifacts/{sha256}` | Upload an input if it is not already present (content-addressed, so data is uploaded once). |
| `POST {RUNPOD_ENDPOINT}/jobs` | Submit `job.json`. Returns `{job_id, status:"queued"}`. Idempotent on `idempotency_key`. |
| `GET  {RUNPOD_ENDPOINT}/jobs/{id}` | `queued | running | succeeded | failed | cancelled`, progress, cost estimate. |
| `GET  {RUNPOD_ENDPOINT}/jobs/{id}/artifacts/{name}` | Download outputs into `jobs/<id>/result/`. |
| `POST {RUNPOD_ENDPOINT}/jobs/{id}/cancel` | Cancel. |
| `GET  {RUNPOD_ENDPOINT}/health` | Worker up, GPU type, queue length. Shown on the console. |

- **Mode A (preferred for no-Python): persistent RunPod GPU Pod** running `quant-worker` (axum) behind RunPod's HTTPS proxy, with a network volume for datasets and results. Start and stop it with the RunPod REST API from the dispatcher (idle stop after N minutes) to save cost.
- **Mode B: RunPod Serverless.** The same contract mapped to `/run` + `/status/{id}`. Caveat: RunPod's standard serverless handler SDK is Python [verify]. Use it only if a non-Python worker path exists (custom container that speaks the worker protocol), otherwise stay on Mode A.
- **Worker stack, no Python:** Rust (`polars` for data, own vectorised backtester, `candle`/`burn` or `ort` (ONNX Runtime) for model inference on CUDA). `llama.cpp` server for local LLM inference if ever needed. Node/TypeScript is acceptable for glue.
- **Endpoint unset (today):** jobs stay `queued` in `jobs/spool/`. The desk gets "queued: RunPod not configured" and its ticket goes to `blocked`. Heavy compute **never** falls back to running on the VPS.
- **Results back to desk:** the dispatcher writes `result/` + `summary.json`, adds a `results.jsonl` entry (kind `backtest`/`inference`) with GPU minutes and cost, then posts a `report` ticket to the desk. The desk's next run picks it up.
- Queueing is global FIFO with the same lane priority as the guard. There is no per-desk job quota.

---

## 9. Output-language hygiene `[NEW]`

The problem: stray Chinese characters show up in output, a recurring hallucination from Chinese-origin models (River DeepSeek/GLM are still configured as alternatives). Rule in `OFFICE.md` and every SOUL: *"Write English. No CJK characters except inside a marked source quote."*

### 9.1 Detection (`office-lint`, Rust library + CLI)
- Flags code points in: CJK Unified Ideographs and extensions (`\p{Han}`), Hiragana/Katakana, Hangul, CJK Symbols & Punctuation U+3000–303F, Halfwidth/Fullwidth Forms U+FF00–FFEF (fullwidth comma, full stop, colon and parentheses, U+FF0C, U+3002, U+FF1A, U+FF08/FF09, are the most common leak). Also flags invisible/bidi controls (U+200B–200F, U+202A–202E, U+2066–2069; Trojan Source).
- Allowed: text inside a fenced block or blockquote tagged `lang: zh|ja|ko` with a `source:` line (for example a Chinese company's legal name from a filing), and `data:` fields declared CJK-capable in a schema.

### 9.2 Enforcement points
1. **Office service, on every outbox ingest** (reports, handoffs, journal, library promotion):
   - Prose: strip the characters and insert a visible marker `⟦cjk×N stripped⟧`.
   - Tables and numbers: **reject** with `hygiene` and send back to the author once, because a stripped character could change meaning. A second failure gets strip + flag + `review` status.
   - Every hit increments `hygiene_flags` on the ticket and the desk.
2. **Hermes before speaking** (Galion One and console spoken sentence): strip unconditionally, because TTS would read garbage. Implemented in the office summary endpoint Hermes reads, plus the SOUL rule.
3. **Guard probe:** logs CJK counts per desk/model only (§7). It never rewrites output.
4. **Console:** a hygiene badge per desk (flags today / 7 days). A spike points at a model or provider problem, for example a desk that was misrouted to River.
5. **Optional Hermes shell hook** (`gateway run --accept-hooks` is on today; hook config format [UNVERIFIED]) that runs `office lint` on files a desk writes. It is a config entry calling the Rust binary, not a Python plugin.

### 9.3 Related "no Python" enforcement for desks
- Office profiles disable the `execute_code` toolset (it is a Python sandbox). That means the `dynamic-workflow` skill's Layer A is off for desks. Its fan-out role is covered by the office CLI and RunPod jobs.
- Office-profile guard patterns (extend `dangerous-patterns.txt` for office profiles only) block `python`, `python3`, `pip`, `uv`, `conda` and `*.py` execution from desk terminals. crew-skills SKILL.md files are read as hats, and their `scripts/*.py` are never run.
- Existing Python pieces (Hermes itself, `link-watch.py`, `loop.py`, `mesh.py`, Galion One `talk.py`, Whisper/TTS/vision) are left alone. The office does not call or extend them.

---

## 10. Memory sharing

| Layer | Where | Who writes | Who reads |
|---|---|---|---|
| L0 private desk memory | `desk/memory/MEMORY.md`, `notes/*.md` + Hermes built-in file memory in the desk's `HERMES_HOME` | the desk | the desk; its parent (read-only) |
| L1 team shared | `desks/master/<team>/shared/` (playbook, open hypotheses, data dictionary drafts) | the master only (promotes from sub reports) | the master's subs and their grandchildren |
| L2 office library | `office/library/` (findings, strategies, postmortems, data dictionary) | Hermes or a master, after review §6.4 | every desk |
| Head memory | memory-gateway (TencentDB) | Hermes only (unchanged) | Hermes |
| Sessions | each desk's own `state.db` | Hermes runtime | that desk; office service reads metadata only (status, last turn time) |

- **Why desks don't use memory-gateway:** its `embeddingService` is false today and it shares the OpenRouter key. A dozen desks writing to it would mix memories across agents unless it supports per-agent namespaces [UNVERIFIED]. File memory gives isolation by construction and is easy to show in the console. Revisit after Phase 0 checks namespaces.
- Library notes use the existing vault convention (frontmatter `tags`, `project: galion`, `status`, plus `desk`, `ticket`, `evidence`). No secrets, tokens or credentials, ever (SOUL "Vault").
- Promotion is always *up and reviewed*: sub → master's `shared/` → `library/`. Nothing moves sideways between teams except through a handoff ticket.

### 10.3 Results / P&L journal (`desk/journal/results.jsonl`, append-only)

```json
{"ts":"2026-10-21T16:02:11+02:00","desk":"s.research-signals","ticket":"T-20261020-0007",
 "kind":"backtest","strategy":"mom-12-1-eu-v3","period":"2010-01..2025-12","costs_bp":10,
 "metrics":{"ann_return":0.071,"sharpe":0.84,"max_dd":-0.23,"turnover":3.1,"hit_rate":0.53},
 "evidence":["jobs/J-20261020-0031/result/summary.json"],
 "llm_cost_usd":1.92,"gpu_cost_usd":0.41,"review":{"by":"g.research-signals.20261021-02","verdict":"survives"}}
```

- `kind`: `backtest | paper | idea | data | postmortem`. `live` is reserved and rejected until Phase 5.
- "P&L" for a desk means **paper or backtest P&L** plus its **cost** (LLM spend from the guard ledger + RunPod GPU cost). That gives each desk an honest net: research value against what it cost.
- The office service writes `pnl.csv` and `office/ledger/*` nightly. `exec-risk-pnl` audits them.

---

## 11. Galion One (voice) → the office

- Path stays as it is: phone → `one.galion.app` → `one` :8091 → Hermes :8642 (session `one-voice`, kept separate per the `platform` skill: "One agent, two sessions… Do not merge the transcripts").
- **Prerequisite:** Bug 2 fix (model route `galion` → haiku via `model_routes`). Whisper/TTS are down today (Python/torch services, existing, out of scope here). Until they are back, voice is effectively text through the One page.
- Voice intents Hermes handles itself, without spawning a desk:
  - "How is the office?" → reads `GET /office/org` + `/office/guard` and speaks one sentence ("Three desks working, one blocked on data, spend twelve dollars today."). The screen shows the rest.
  - "What is research doing?" → one sentence from the master's latest daily note.
  - "Give research a ticket: …" → Hermes creates a ticket to `m.research` and confirms in one sentence.
  - "Pause the office." → Hermes asks once to confirm (destructive by SOUL rules), then creates `guard/KILL`. "Resume" removes it.
- Keep-warm (`warm_due()` every 240 s) must not hit Opus. It stays on the haiku route or is skipped (earlier proposal §4).

---

## 12. Console: the office floor `[NEW route in console-ide, TypeScript]`

Route `/office` in `/opt/galion/console-ide`. Data comes from the office service (`GET /office/*`, SSE `/office/stream`) and the guard status over loopback, fetched server-side by the console. No new public endpoint.

```
┌ Office floor ─────────────────────────────── guard: 7/16 in flight · 64 rpm · $12.40 / $75 today · ● running ┐
│  HEAD   [Hermes ● working]  T-0012 "Weekly research plan"   last: "Assigned EU momentum to signals"          │
│ ┌ Research (opus) ───────────────┐ ┌ Exec & Risk ── (phase 3) ┐ ┌ Data ── (phase 2) ┐                    │
│ │ master ● review  T-0007         │ │  ░ dark                   │ │  ░ dark             │                    │
│ │ ├ signals (sonnet) ● working    │ │                            │ │                     │                    │
│ │ │   T-0007 · job J-31 running   │ │                            │ │                     │                    │
│ │ │   g·03 fetcher ◌  g·04 refuter│ │                            │ │                     │                    │
│ │ └ macro (sonnet) ○ idle         │ │                            │ │                     │                    │
│ │   last result: carry FX  SR 0.6 │ │                            │ │                     │                    │
│ └─────────────────────────────────┘ └────────────────────────────┘ └─────────────────────┘                    │
│ Tickets: open 4 · in progress 2 · review 1 · blocked 1      Jobs: queued 1 · running 1      Hygiene: 2 flags   │
└────────────────────────────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Desk card:** name, tier, model badge, status light (idle / working / review / blocked / offline / paused), current ticket, last journal line, today's attributed spend, latest result metric (e.g. Sharpe of last reviewed backtest, net of cost), RunPod jobs, hygiene badge. Grandchildren show as small chips that fade when archived.
- **Desk drawer (click):** SOUL, TASK, MEMORY (rendered markdown, read-only), journal + `results.jsonl` table, ticket thread, jobs, last 80 log lines (same window as `log-watch`).
- **Ticket board:** kanban columns by status, filter by team.
- **Guard strip:** global in-flight, queue by lane, RPM, spend vs ceiling, breaker, KILL state.
- **Writes:** only "Send to Hermes" (existing `POST /api/hermes/inbox`). Pause/resume is operator-only and goes through Hermes (§11), not a direct console write, which keeps the arena "read surface" rule [open question whether a direct pause button is wanted].
- Console hygiene: desk sessions are titled `<desk-id> · <ticket>` so the existing `UNIQUE constraint failed: sessions.title` error does not get worse with a dozen profiles. The office never edits Hermes `state.db` directly (the console's `python3 -c` pattern is not extended).
- The console still runs `vite dev` in production (open question in the stack map). The new route works either way.

---

## 13. Phased rollout

Each phase needs Mate/Slavik approval. Backups go to `~/vps-backups` first, as in the earlier apply order.

**Phase 0: prerequisites and verification (no office yet)**
1. Apply `hermes-openrouter-proposal.md` fixes: Bug 1 (key visibility), Bug 2 (`galion` route), Bug 3 (`auxiliary.free_only`). Set the OpenRouter key credit limit.
2. Read and verify on the box (read-only): `galion-light/desk-rs/main.rs` (what galion-desk does → extend it or add `galion-office`). Hermes `references/background-systems.md` + profile docs: per-request profile selection under `multiplex_profiles`, custom provider + `api_key_env` keys, per-profile `toolsets.disabled`, builtin memory path, symlinked profiles, hook format. `kanban.db` schema. Whether memory-gateway supports namespaces.
3. Amend the SOUL lines in §1.1 (approval needed: architecture change).
4. Make Galion One a systemd unit (stack-map open question), so voice survives reboots.

**Phase 1: minimal office (Hermes + 1 master + 2 subs + grandchildren on demand)**
1. Build and deploy `or-guard` (Rust). Move the real OpenRouter key into its env and remove it from `hermes.env`. Point Hermes (T0) at the guard. Defaults: 16 in flight, 120 RPM, $75/day [placeholders].
2. Create `/opt/galion/office/` (OFFICE.md, USER.md, org.yaml, templates).
3. Office service v0 (extend galion-desk or new :8647) + `office` CLI + `office-lint`. Tickets in `office.db` (or kanban if verified).
4. Profiles: `m.research` (Opus/high), `s.research-signals` and `s.research-macro` (Sonnet/medium, delegation → Haiku, depth 1). Grandchildren via delegate_task from the subs only.
5. Console `/office` v0: org grid, desk drawer, tickets, guard strip (read-only).
6. RunPod: jobs accepted and spooled as `queued` (endpoint unset). Research is literature, data reading and spec writing only. No compute on the VPS.
7. **Exit criteria (1 week):** ≥ 20 tickets end-to-end with spec + refuter review. Zero calls that bypass the guard (ledger vs OpenRouter dashboard match). Spend inside ceiling. Hygiene flags tracked and trending down. No Hermes gateway restarts caused by the office. Box stays at its current light load.
8. **Rollback:** remove the profiles, stop the office service and guard, restore Hermes `config.yaml`/`.env` from backup.

**Phase 2: compute + data**
- Wire `RUNPOD_ENDPOINT` once the pod is chosen. Build and deploy `quant-worker` (Rust) image. Add `m.data` + `s.data-compute` + `s.data-quality`. Results journal fills with real backtests. Console jobs view.

**Phase 3: execution & risk (paper only)**
- `m.exec-risk` + `risk`, `paper`, `pnl` subs. Paper portfolio from reviewed library strategies. Daily paper P&L in journals. Galion One office intents (§11).

**Phase 4: full office**
- `research-alt`, `data-ingest`. Nightly Hermes cron review (morning brief, ledger roll-up, archive grandchildren). Library promotion flow in full. Tune guard concurrency from measured data.

**Phase 5 (separate decision, not designed here): any live trading**
- Broker, keys, approval gates, legal. Explicitly out of scope until the operator decides.

---

## 14. Existing vs new: summary

| Item | Status |
|---|---|
| Hermes gateway, profiles feature, delegate_task, cron, kanban, skills, SOUL/USER | EXISTS (configured, not coded) |
| Hermes `SOUL.md` Backup/model lines, `delegation`, `api_server.max_concurrent_runs`, `auxiliary` | CHANGE |
| Galion One (voice) | EXISTS; CHANGE: model route + systemd unit |
| Console-ide | EXISTS; NEW `/office` route (TypeScript) |
| galion-desk :8646 | EXISTS; CHANGE (extend into the office API) or NEW sibling `galion-office` :8647 |
| crew-skills hats, subagent-driven-development, mesh/handoff/dynamic-workflow principles, request-inbox, vault convention, guardrails | EXISTS (reused as text, rules and patterns) |
| arena homes g2-0..3 | EXISTS (test). The template shape is reused. The homes are left untouched. |
| memory-gateway | EXISTS (Hermes only) |
| `/opt/galion/office/` tree, desk templates, org.yaml, office.db | NEW |
| `or-guard` (Rust) :8650 | NEW |
| `office` CLI + `office-lint` (Rust) | NEW |
| RunPod dispatcher (Rust, in office service) + `quant-worker` (Rust, on pod) | NEW |

---

## 15. Open questions

1. **galion-desk:** what does `desk-rs/main.rs` do today? Extend it, or leave it alone and add `galion-office`?
2. **Profile selection:** can one multiplexed Hermes gateway run `/v1/runs` against a chosen profile per request? If not, is one gateway process per persistent desk acceptable (≈10 processes)?
3. **Custom provider config keys** for pointing all of Hermes (model, delegation, auxiliary) at the guard. Can a profile disable `execute_code`? Does Hermes follow symlinked profile dirs?
4. **Kanban vs office.db** as the ticket store.
5. **Spend ceiling:** daily/monthly numbers for the guard. $75/day is a placeholder. Should the 100 % behaviour be "head lane only" or a full stop?
6. **Global concurrency default** (16 in flight / 120 RPM). Is there an OpenRouter account rate tier to match?
7. **RunPod:** Pod vs Serverless, GPU type, region, network volume vs object storage for datasets and results, idle-stop policy, and where the RunPod API key lives (name only).
8. **Market data:** which vendors/feeds, licences, and where bulk data lives (pod volume, not the VPS 100 GB disk).
9. **Grandchild attribution:** is the `DESK-ID:` header approach acceptable, or should grandchildren be real profile runs (heavier, but each gets a native session)?
10. **memory-gateway:** does it support per-agent namespaces? Should Hermes keep it as primary memory given `embeddingService false`?
11. **SOUL changes:** OK to replace the "laptop primary / no cloud model" section now that OpenRouter is the plan? What role does the laptop (RTX 4060, model `galion`) play later, if any?
12. **Console pause button:** direct operator write, or only through Hermes?
13. **Voice:** who brings back Whisper/TTS (Python/torch services, existing)? Is text-only One acceptable for the office until then?
14. **Hygiene policy for legitimate CJK** (Chinese/Japanese company names, filings): quote-block-only as proposed, or transliterate always?
15. **Owner and alerts:** who gets escalation/guard alerts, through which channel (the stack-map question is still open)?
16. **Deploy procedure:** console-ide, galion-light and `one` have no git remote. Where do the new Rust crates live (a galion-studio repo?) and how are they deployed?
