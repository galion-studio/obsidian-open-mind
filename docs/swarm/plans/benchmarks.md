<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Snapshot of /workspace/swarm/plans/benchmarks.md copied from the swarm box on 2026-10-20 23:29 CEST +02:00 by swarm/swarm-record. The box copy is the working copy; this is the durable record. -->

# Sector plan: benchmarks (banking, Wall Street, vending machine)
# Written 2026-10-20 22:57 CEST (+02:00). Planning only; no live money moves.

## 0. Hard constraints
- Revolut is in SCOPE for the Hermes Rust V2 redesign (Slavik typed lift, 2026-10-20, relayed by Mate): design, UI, phone link, and paper/sandbox benchmarks. NOT for acting on Slavik's behalf.
- No agent may place an order, send a transfer, top up, withdraw, buy, sell, or otherwise move money. Ever. Sandbox / paper / recorded fixtures only.
- Rust only. No Python. No VPS deploys from this sector without a separate owner go-ahead.
- Concurrent budget for this sector tonight: **5 agents** (box already at 13; measured limit 18–20). Rest queue.

## 1. What each benchmark measures

### B1. Banking (target 100%)
Measures Hermes's ability to **operate a personal bank account as a tool**, end to end, without ever sending a real payment.
- Axes (each scored 0–100, equal weight): auth & session, balance & statement read, payee management, payment *draft* (never submit), FX quote, card freeze/unfreeze *draft*, dispute *draft*, audit trail, refusal of out-of-scope asks, recovery from API errors.
- **100% means:** every axis has a GALION_REAL=1 Rust test against a **local mock bank** (fixture HTTP server) that returns scripted Revolut-shaped JSON, plus a read-only live probe that confirms the real Revolut app/API is reachable from the phone link *without* calling any mutating endpoint. Live mutating calls are forbidden until Slavik signs a separate "live banking" approval.
- Out of scope tonight: any live Revolut write, Open Banking PSD2 consent capture, or storing card PANs.

### B2. Wall Street (target 100%)
Measures Hermes's ability to **research and paper-trade**, not to manage real capital.
- Axes: market data ingest (delayed/public), symbol universe, paper order book, risk limits (max notional, max position, kill switch), strategy scratchpad, backtest replay, news/event tagging, compliance refusals (no tip, no insider framing), report quality ("leave with more than you came in with").
- **100% means:** a Rust paper broker + delayed public data feed (e.g. a permissive free API or recorded fixtures) where a scripted Hermes ticket can open/close paper positions under risk limits, with a reproducible P&L report. Zero connection to any live brokerage account.
- Out of scope: live broker keys, margin, options writing, anything that can spend euros.

### B3. Vending machine (target 100%)
Measures long-horizon **operating a small business** under scarcity (the classic agent "vending-bench" style task: inventory, pricing, restock, failure recovery).
- Axes: inventory truth, pricing, supplier orders (simulated), cash drawer reconciliation, theft/spoilage events, multi-day continuity (durable log), human handoff notes, refusal to invent stock.
- **100% means:** a Rust simulation (`galion-vending` crate) with a seeded scenario of ≥30 simulated days; Hermes (or a local scripted operator) keeps the shop solvent and ends with cash ≥ starting cash and no silent inventory lies. Deterministic seed → identical score. Score formula written in the crate docs.
- Out of scope: any real payment terminal or real supplier API.

## 2. Workstreams

| ID | Workstream | Agents useful | Depends on | Output |
|---|---|---|---|---|
| P0 | Sector plan + scoring rubric (this file) | 1 | – | plans/benchmarks.md |
| P1 | Shared mock harness (`galion-bench`) | 2 | P0 | crate + fixtures |
| B1a | Banking axes + Revolut-shaped fixtures | 2 | P1 | tests + docs/bench/banking.md |
| B1b | Phone/read-only reachability (no writes) | 1 | phone-link | probe report |
| B2a | Paper broker + risk limits | 2 | P1 | crate + tests |
| B2b | Public/delayed data + replay | 1 | B2a | fixtures |
| B3a | Vending sim + score formula | 2 | P0 | galion-vending |
| B3b | 30-day seeded runs + report | 1 | B3a | evidence/bench/vending/ |
| H | Hermes V2 redesign wiring (tools that call the mocks) | 2 | B1a,B2a,B3a | hermes-rs tool surface |
| R | Record Revolut rule lift in settings + memos | 1 | – | settings + docs/decisions |

Realistic tonight at 5 concurrent: R + P1 + B3a first (no phone needed), then B1a/B2a, then H. B1b waits on the phone link.

## 3. Ceiling math (2026-10-20 22:57 +02:00)
- Measured limit: ~18–20 concurrent.
- Running before this sector: 13.
- Headroom: 5–7.
- This sector launches **5** now, queues the rest (B1b, B2b, B3b, H×2 = 5 more).

## 4. Definition of done for the sector
A scoreboard at `docs/bench/SCOREBOARD.md` with three percentages, each backed by named tests and a Zagreb timestamp. Anything short of 100% lists the failing axis. No live money. Ever.
