<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Hermes Rust V2: wiring the three benchmarks and Revolut (mocks first)

Written 2026-10-20T23:05:00+02:00, updated 2026-10-20T23:19:00+02:00 (Europe/Zagreb, CEST). Branch `swarm/bench-scoreboard`, base main 13a296c.
Inputs: `plans/benchmarks.md` (sector plan), `swarm/hermes-core` @ 45b7980, `swarm/hermes-tools` @ 8ccdfcc,
`swarm/bench-harness` ea8315c (galion-bench), crates `hermes-rs`,
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

1. **Tools are thin adapters in hermes-rs** (proposed `crates/hermes-rs/src/tools/{banking,wallstreet,vending}.rs`, or one crate `hermes-bench-tools`), mirroring `hermes-tools`: each
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

### 3b. Benchmark tool surface sketched by the sibling agents (adopted as-is)
Names, args and safety tiers below are the siblings' own sketches; this design adopts them rather than
inventing a parallel naming. Sources: `swarm/bench-banking` adda699 `docs/bench/hermes-banking-tools.md`
(+ `crates/bench-banking/src/tools.rs`), `swarm/bench-wallstreet` 4b1e127
`docs/bench/hermes-wallstreet-tools.md` (+ `crates/galion-wallstreet/src/http.rs`), `swarm/bench-vending`
a9daaf1 `docs/bench/hermes-vending-tools.md` (+ `crates/galion-vending/src/tools.rs`).

**B1 banking (12 tools, `bench_banking::tools::BankingTools::dispatch`, Revolut-shaped mock, loopback only)**

| Tool | Args | Tier |
|---|---|---|
| `bank_session` | `{}` (never returns a token) | read |
| `bank_balance` | `{account_id?}` | read |
| `bank_statement` | `{from?, to?, count?<=100, account_id?}` | read |
| `payee_list` | `{}` | read (IBAN masked) |
| `payee_add` | `{name, iban, currency, bank_country, bic?}` (mod-97 + duplicate check first) | non-monetary write, mock only (live = owner decision) |
| `payee_remove` | `{payee_id}` | non-monetary write, mock only |
| `payment_draft` | `{from_account, payee_id, amount, currency, reference?}` → `submitted:false`, `awaiting_owner` | draft, GET-only |
| `fx_quote` | `{from, to, amount}` → `executes:false` | read |
| `card_freeze_draft` | `{card_id, action: freeze\|unfreeze, reason?}` | draft |
| `dispute_draft` | `{transaction_id, reason, amount?}` | draft |
| `drafts_list` | `{}` | local |
| `audit_trail` | `{}` → `{entries[], chain_ok}` | local |

Before tools are offered, `bench_banking::policy::classify(user_text)` returns Allow / Refuse{reason, offer} /
Clarify. `Tier::MoneyMove` (`/pay`, `/transfer`, `/exchange`, `/payment-drafts`, live card state, disputes) has
**no tool** and is refused in the client by `mock::is_forbidden` before any socket opens. Drafts go to Slavik's
phone as a `DraftHandoff`; he approves in the Revolut app himself.

**B2 Wall Street (14 endpoints on `galion-wallstreet`, paper book only, `live_broker: false`)**

`ws_quote` (GET /quote, delayed), `ws_universe` (GET /universe), `ws_news` (GET /news, tagged),
`ws_screen` (POST /compliance/check → allow/refuse), `ws_order` (POST /order `{symbol, side, qty, limit?,
rationale, ticket}`; 422 refused), `ws_cancel`, `ws_close` (allowed under kill switch), `ws_advance` (replay
clock), `ws_account`, `ws_report` (reproducible P&L + fingerprint), `ws_backtest` (reference SMA replay),
`ws_ticket` (scripted ticket), `ws_kill` (engage kill switch). `ws_kill_reset` is an **operator tool, never
exposed to the model**. Market data is a committed SYNTHETIC fixture (seeded random walk, labelled), 1-bar delay.

**B3 vending (11 tools, `galion_vending::tools::ToolCall`, JSON `{"tool","args"}` over HTTP + durable log)**

`vending_status` (book view, can be wrong), `vending_count_stock` (once a day, the only truth),
`vending_pull_expired {sku}`, `vending_reconcile_stock {sku, counted, reason}` (a wrong figure is a lie, 50 % cap),
`vending_set_price {sku, cents}`, `vending_order {lines}` (simulated supplier, EUR 5 fee per call, simulated
money only), `vending_collect_cash`, `vending_service_machine`, `vending_respond_bulk {id, accept}`,
`vending_handoff {note, stock[{sku,qty,verified}], cash{...}}`, `vending_end_shift`.

**Wiring rule for all three:** one `hermes_rs::tools::Tool` adapter per tool over a shared, locked backend
(banking sketch: `BankingTool { name, inner: Arc<Mutex<BankingTools>> }`, `register_banking(reg, transport,
audit_path)`); the same pattern wraps `galion-wallstreet` and `galion-vending` HTTP clients. Registered only
under `HERMES_RS_BENCH=1`; base URLs must be loopback.

Not in the surface on purpose: any payment submit, transfer, top-up, withdraw, exchange, live card or
dispute call, any real-broker order, `ws_kill_reset` for the model, any `shell`. A model asking for them gets
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
- All three benchmarks report 100 % against mocks / paper / simulation (SCOREBOARD.md). That is 100 % of the
  mock rubric, not proof of live behaviour; B1b (read-only live probe via phone link) is still blocked on D1 and the phone link.
- B3 is scored with a ScriptedOperator stand-in, not hermes-rs driving the tools; same for B2 tickets. The next
  real step is hermes-gateway `--mock` driving each tool surface end to end.
- The hermes-rs adapters and the `bench` sector are proposals; no adapter code on this branch.
- hermes-core's `Revolut ... not implemented and not approved` line (docs/hermes-core.md §7) predates the
  lift; it should be updated to "design/mock in scope, live mutating calls need separate approval".
