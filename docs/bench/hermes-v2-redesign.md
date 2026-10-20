<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes Rust V2: wiring the three benchmarks and Revolut (mocks first)

Written 2026-10-20T23:05:00+02:00 (Europe/Zagreb, CEST). Branch `swarm/bench-scoreboard`, base main 13a296c.
Inputs: `plans/benchmarks.md` (sector plan), `swarm/hermes-core` @ 45b7980, `swarm/hermes-tools` @ 8ccdfcc,
`swarm/bench-harness` (galion-bench, uncommitted at time of writing), crates `hermes-rs`,
`hermes-gateway`, `desk-hermes` on main 13a296c. Scores: `docs/bench/SCOREBOARD.md`.

**Status: design only. Nothing here is built, deployed or wired on the VPS.**

## 0. Revolut scope (verbatim effect of the 2026-10-20 ~22:56 CEST lift)
- IN scope: design, mock, read and wire Revolut into the Hermes Rust V2 redesign and the admin-phone link;
  Revolut-shaped fixtures; read-only reachability probes.
- OUT of scope until Slavik signs a separate written "live banking" approval: placing orders,
  transfers, top-ups, withdrawals, buy/sell, card freeze/unfreeze, disputes, payee creation on
  the real account — any live mutating call. Agents never move money on Slavik's behalf.
- Enforced in code, not only in prose (section 4).

## 1. V1 → V2 at a glance

| | V1 Hermes (Python) | V2 Hermes (Rust) |
|---|---|---|
| Agent / API | `hermes.service` (Python, now inactive + disabled on VPS) | `hermes-rs` lib + `hermes-gateway` bin; live as `hermes-rs.service` on 127.0.0.1:8642 since 2026-10-18 22:00:49 CEST |
| Coordinator | none | `hermes-core` (swarm/hermes-core 45b7980): sectors registry, governor (cap 16), retry ledger, HADES gate, readiness, hash-chained JSONL ledger |
| Desk / state read | `desk-hermes.py` | `desk-hermes` (Rust port, `state.db` opened `SQLITE_OPEN_READ_ONLY`) |
| Tools | Python toolset | `Tool` trait in `crates/hermes-rs/src/tools.rs` (`name/parameters/description/call`), `Registry::schemas()` → OpenAI `tools` array |
| Money | ad hoc / none | bench tools behind a `MoneyGuard` (section 4); mock backends only |

## 2. Where the benchmarks plug in

```
phone (admin link) ──HTTPS──► hermes-gateway (hermes-rs agent loop, spend caps, sessions)
                                   │  Tool registry: clock, delegate_task, MCP, fs_read/fs_write/git/cargo,
                                   │                 + bank.*, market.*/paper.*, vend.*   (this design)
                                   ▼
                          hermes-core (coordinator): sector `bench`, HADES gate, ledger
                                   │
         ┌─────────────────────────┼──────────────────────────┐
         ▼                         ▼                          ▼
  galion-bench mock bank   paper broker (B2a) +        galion-vending sim (B3a)
  (Revolut-shaped JSONL    delayed/recorded feed       seeded ≥30 days, deterministic
   fixtures, loopback)     (B2b), no live broker       score formula in crate docs
```

1. **Tools live in a new crate `hermes-bench-tools`** (proposed), mirroring `hermes-tools`: each
   tool implements `hermes_rs::tools::Tool`, is registered by `hermes-gateway` only when
   `HERMES_RS_BENCH=1`, and targets a base URL that must be loopback (`127.0.0.1`/`::1`) unless a
   live-read flag is set (section 4).
2. **Backends are the sibling crates**: `galion-bench` (swarm/bench-harness: std-only mock HTTP
   server from JSONL fixtures, loopback by default, hit journal with Zagreb stamps, `Scorecard`
   0–100 equal-weight, empty card = 0), the paper broker (swarm/bench-wallstreet) and
   `galion-vending` (swarm/bench-vending).
3. **hermes-core gets one more sector `bench`** in `config/hermes/sectors.toml` (cap 2, Gaia
   function MINERVA for scoring, HADES for the money guard). Every benchmark ticket is a task
   `bench:<id>`; drafts classify as `Code`/`ReadOnly`; anything that would submit is classified
   `Destructive` and therefore refused (hades-gate has no `money` kind; adding one is an owner decision).
4. **Scoring**: each benchmark's GALION_REAL=1 Rust test drives the real `hermes-gateway --mock`
   (or a scripted operator) on the slot's port range against the mock, builds a `Scorecard`, and
   commits its JSON; `SCOREBOARD.md` folds it in with SHA + Zagreb time.
5. **Audit**: every bench tool call writes an `allowed`/`refused`/`ran` line like `hermes-tools`,
   and the coordinator ledger (`<state>/ledger.jsonl`, SHA-256 chained, `ts` with `+02:00`) records the
   task; the banking "audit trail" axis is scored from these two logs.

## 3. Tool surface

### 3a. Already sketched / built by other agents
| Tool / endpoint | Owner branch | Notes |
|---|---|---|
| `clock` | main (`hermes-rs/src/tools.rs`) | Zagreb RFC 3339 |
| `delegate_task` | main (`hermes-rs/src/delegate.rs`) | ≤3 children, depth 1 |
| MCP tools | main (`hermes-rs/src/mcp.rs`) | off on the live unit |
| `fs_read`, `fs_write`, `git` (status/diff/commit), `cargo` (build/check/test/clippy) | swarm/hermes-tools 8ccdfcc | all inside `openmind-sandbox`, no network, no shell tool |
| hermes-rs HTTP: `/health`, `/v1/models`, `/v1/chat/completions` (SSE), `/v1/runs*`, `/api/sessions*`, `/api/jobs`, `/api/spend` | main | gateway refuses to start without `API_SERVER_KEY` ≥16 chars |
| hermes-core HTTP: `/health`, `/v1/sectors`, `/v1/agents`, `/v1/tasks`, `/v1/dispatch` (dry-run), `/v1/ledger`, `/v1/retries`, `/v1/governor`, `/v1/readiness`, `/v1/report` | swarm/hermes-core 45b7980 | live dispatch refused; `run --simulate` only |
| `galion-bench` lib: `fixture`, `server`, `client`, `score::Scorecard` + bin `galion-bench-mock` | swarm/bench-harness (uncommitted at 23:05 CEST) | JSON `{"pct":N,"axes":[{"name","score"}]}` |

### 3b. Benchmark tools (proposed here; derived 1:1 from the rubric axes)
`M` = mock-only now; `R` = may later get a live **read** behind a flag; `D` = draft only, never submits.

| Tool | Axis (plans/benchmarks.md) | Mode |
|---|---|---|
| `bank.session` `{op: open\|status\|close}` | B1 auth & session | M, R |
| `bank.balance` / `bank.statement {from,to,account?}` | B1 balance & statement | M, R |
| `bank.payee` `{op: list\|draft_add}` | B1 payee management | M (list R), D |
| `bank.payment_draft {payee,amount,ccy,ref}` | B1 payment draft | D — returns a draft id + human-readable summary for the phone; no submit verb exists |
| `bank.fx_quote {from,to,amount}` | B1 FX quote | M, R |
| `bank.card_draft {op: freeze\|unfreeze}` | B1 card | D |
| `bank.dispute_draft {txn,reason}` | B1 dispute | D |
| (refusal + error recovery are scored on all `bank.*` calls) | B1 refusals, recovery | — |
| `market.quote {symbol}` / `market.bars {symbol,from,to}` | B2 data ingest, replay | M (recorded / delayed public) |
| `market.universe` | B2 symbol universe | M |
| `paper.order {symbol,side,qty,limit?}` / `paper.cancel` / `paper.positions` | B2 paper order book | paper only, never a live broker |
| `paper.risk {op: get\|set_limits\|kill}` | B2 risk limits, kill switch | paper only |
| `paper.backtest {strategy_id,from,to,seed}` / `paper.report` | B2 backtest, report quality | M |
| `vend.state` / `vend.inventory` | B3 inventory truth | sim |
| `vend.set_price {sku,price}` | B3 pricing | sim |
| `vend.order {supplier,sku,qty}` | B3 supplier orders | sim (simulated suppliers only) |
| `vend.cash {op: count\|reconcile}` | B3 cash drawer | sim |
| `vend.advance_day` / `vend.log` / `vend.handoff_note {text}` | B3 continuity, handoff | sim, durable log |

Not in the surface on purpose: `bank.payment_submit`, `bank.transfer`, `bank.topup`,
`bank.withdraw`, `paper.*` against a real broker, any `shell`. A model asking for them gets
`Unknown tool: ...` (same mechanism as hermes-tools).

## 4. Money guard (how "no money moves" is enforced)
1. **No mutating verb is compiled in.** Draft tools return a draft object; there is no code path
   that sends it. Live submit would be a *new* tool behind a new approval, not a flag.
2. **Base URL check**: bench tools refuse non-loopback hosts unless `HERMES_RS_BANK_LIVE_READ=1`,
   and even then only HTTP GET to an allowlist of read paths; any other method is refused before
   a socket is opened.
3. **Classification**: hermes-core classifies submit/transfer/buy/sell titles as `Destructive`;
   hades-gate refuses unknown kinds, so even a signed token cannot authorise money movement today.
4. **Secrets**: no Revolut credential is read by bench tools in mock mode; the live-read mode
   (later) takes the token from the gateway environment and never logs or returns it, matching the
   hermes-tools rule that keys never reach a tool's child process.
5. **Tests**: each benchmark includes a refusal test proving `payment_draft` never produces an
   outbound request to anything but the loopback mock (checked via the galion-bench hit journal).

## 5. Order of work (mocks first)
1. galion-bench lands (bench-harness) → 2. bank fixtures + `bank.*` against the mock (bench-banking),
   paper broker (bench-wallstreet), `galion-vending` (bench-vending) → 3. `hermes-bench-tools` crate
   registers the surface in hermes-gateway under `HERMES_RS_BENCH=1` → 4. `bench` sector in
   hermes-core → 5. scores to SCOREBOARD.md → 6. (owner) read-only phone-link probe B1b →
   7. (owner, separate approval) anything live and mutating.

## 6. Owner decisions (not decided here)
| # | Decision | Options | Evidence |
|---|---|---|---|
| D1 | Live Revolut **read** via phone link | (a) never; (b) read-only GET allowlist behind `HERMES_RS_BANK_LIVE_READ=1`; (c) Open Banking AIS consent later | rubric B1 needs a read-only reachability probe for 100 % |
| D2 | Live Revolut **mutating** calls | (a) never; (b) separate written approval per action class with HADES `money` kind | standing rule: forbidden until signed |
| D3 | Add a `money` kind to hades-gate | (a) no (today: refused as unknown); (b) yes, single-use per draft id, plan hash = draft bytes | hermes-core §7: unknown kinds refused |
| D4 | Delayed public market data source | recorded fixtures only vs. a permissive free API | B2b; no paid API without approval |

## 7. Open issues
- Sibling score JSONs were not available when this was written (see SCOREBOARD.md).
- `hermes-bench-tools` and the `bench` sector are proposals; no code on this branch.
- hermes-core's `Revolut ... not implemented and not approved` line (docs/hermes-core.md §7) predates the
  lift; it should be updated to "design/mock in scope, live mutating calls need separate approval".
