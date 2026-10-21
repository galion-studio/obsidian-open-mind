<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# B2 Wall Street scoreboard: 100% (paper)

Scored 2026-10-20 23:11 CEST (+02:00) by `galion-wallstreet score` (release build), branch `swarm/bench-wallstreet`.
Machine copy: [`wallstreet.score.json`](wallstreet.score.json). Crate: `crates/galion-wallstreet`.

**PAPER ONLY.** No live brokerage account, no broker keys, no real orders, no money. The crate has no HTTP client and no
network dependency (enforced by `tests/cli.rs::no_network_client_in_source`; runtime deps are exactly `serde`,
`serde_json`, `zagreb-time`). `tests/mock_vendor.rs` also proves a full scripted ticket never touches a mock "live broker".

## Score

| Axis | Score | Checks | What is checked (all deterministic, in-process, on committed fixtures) |
|---|---:|---:|---|
| market_data_ingest | 100 | 13/13 | fixture ingests (40 days x 7 symbols), labelled SYNTHETIC, `delay_bars: 1`, 9 malformed CSVs refused (header, field count, date, OHLC order, negative, duplicate, backwards, misaligned calendars, lowercase), quotes stamped `delayed` |
| symbol_universe | 100 | 6/6 | 7-symbol universe, every symbol has bars, unknown / restricted (RSTR) / halted (HLTD) refused, tradable accepted |
| paper_order_book | 100 | 7/7 | market fill, passive limit rests, cancel, limit fills on the *next* bar, no shorting, partial sell, ledger reconciles |
| risk_limits | 100 | 11/11 | fill within limits, max position refuses without tripping, **oversized order refused and trips the kill switch**, engaged switch blocks new risk, close still allowed, named-operator reset, trading resumes, gross exposure cap, manual kill cancels resting buys |
| strategy_scratchpad | 100 | 5/5 | notes recorded, tip note refused by compliance, scoped per ticket, notes appear in report JSON and Markdown |
| backtest_replay | 100 | 7/7 | identical fingerprint on rerun, trades, full 40-bar calendar, flat at end, drawdown reported, reference run clean, replay goes through the same risk limits (tight limits -> 0 fills) |
| news_event_tagging | 100 | 14/14 | 11 headlines tagged exactly as the key (earnings, guidance, M&A, macro, regulatory, management, halt, contract, unverified, MNPI risk), MNPI item never usable as a trade reason, no look-ahead |
| compliance_refusals | 100 | 21/21 | 18-prompt labelled corpus (insider, tip, manipulation, live money, personal advice, and 6 allowed incl. "explain what counts as insider trading"), insider ticket refused before any step, tip rationale refused at order time, every refusal offers a safe alternative |
| report_quality | 100 | 16/16 | P&L reconciles (realised + unrealised - fees = equity - start), equity = cash + MTM, 7 sections incl. "What to do next", says PAPER, Zagreb timestamp, no promises, fingerprint stable and reproducible, ticket ends flat |
| **Total** | **100** | **100/100** | score = passed / total checks; `galion_bench::Scorecard` (equal-weight axes) agrees (`tests/mock_vendor.rs`) |

Reference numbers (reproducible, integer cents):
- Scripted ticket `HERMES-WS-001`: P&L **+1308.94** on 100 000.00 paper (130 bps), 4 fills, 2 refusals (oversized -> kill switch, then new risk while engaged), fingerprint `fnv1a64:e3f5e643e7750856`.
- Reference backtest `sma_cross(3/8, 100 sh)` on GLXA/OMND/POSD: 6 fills, +2398.66 (239 bps), max drawdown 39 bps, fingerprint `fnv1a64:19424da702570a34`.

## How 100% is defined (plans/benchmarks.md B2)

> a Rust paper broker + delayed public data feed (e.g. a permissive free API or recorded fixtures) where a scripted Hermes
> ticket can open/close paper positions under risk limits, with a reproducible P&L report. Zero connection to any live brokerage account.

Met with **recorded fixtures** (synthetic, committed). Evidence: `GALION_REAL=1 cargo test -p galion-wallstreet --test real_wallstreet`
(release binary on 127.0.0.1:19521), see [`docs/real-tests/bench-wallstreet.md`](../real-tests/bench-wallstreet.md).

## Rules the broker enforces

- Long only, no margin; cash and position checked including resting orders.
- Fill model: market = last visible close +/- 5 bps slippage; limit rests and is worked against the next bar (buy at min(open, limit) when low touches). Fee 5 bps.
- Default limits: max order notional 25 000.00, max position 1 000 sh, max gross exposure 60 000.00, max 10 resting orders.
- Over max notional: refused with `kill_switch_oversized_order` and the switch trips (cancels resting buys). While engaged only reducing orders pass. Reset needs a named operator (`POST /kill/reset {"operator":..}`).

## Gaps / honest limits

- Market data is **synthetic** (seeded awk random walk, committed once). "Delayed public data" from a real free API is not wired: picking a source and its licence is an owner decision (see below). The `vendor-mock.jsonl` shape is ready for a recorded file.
- Daily bars only; no intraday, no corporate actions, no FX, no shorting or options (out of scope by the plan).
- Compliance screen is deterministic phrase rules; it can be evaded by paraphrase. It is a floor, not a guarantee; Hades/the model layer should add a second screen.
- Hermes is not wired to these tools yet (workstream H); the tool contract is in [`hermes-wallstreet-tools.md`](hermes-wallstreet-tools.md).

## Owner decision (not decided here)

**D-B2-1: source of real delayed data for B2b.** Options: (a) keep synthetic fixtures only (zero licence risk, least realistic);
(b) record a small slice from a free public source whose terms allow redistribution in a public repo, committed as a fixture
(needs a licence read per source); (c) record locally but never commit (realistic, not reproducible for others).
Recommendation: (a) now, (b) once a source with clear redistribution terms is named. No paid API either way.
