<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes V2 tool sketch: Wall Street (paper)

Written 2026-10-20 23:15 CEST (+02:00). Backing service: `galion-wallstreet serve --bind 127.0.0.1:<port>` (loopback; default 19520).
Every tool is a thin HTTP call; no tool can reach a live broker because the service has no outbound code at all.

## Tools

| Tool | HTTP | Args | Returns | Side effects |
|---|---|---|---|---|
| `ws_quote` | `GET /quote?symbol=` | symbol | delayed quote: price, as_of, delay_bars, source | none |
| `ws_universe` | `GET /universe` | – | tickers, sector, status (tradable/restricted/halted) | none |
| `ws_news` | `GET /news?symbol=` | symbol or `*` | tagged public news up to the session date; `usable_as_reason` flag | none |
| `ws_screen` | `POST /compliance/check` | `{text}` | `allow` or `refuse{category,code,reason,alternative}` | none |
| `ws_order` | `POST /order` | `{symbol, side, qty, limit?, rationale, ticket}` | `filled` / `resting` / `refused{code,message}` (422) | paper book only |
| `ws_cancel` | `POST /cancel` | `{order_id}` | cancelled order | paper book only |
| `ws_close` | `POST /close` | `{symbol, ticket}` | fill (allowed even with kill switch engaged) | paper book only |
| `ws_advance` | `POST /advance` | `{days}` | new as_of + resting orders worked | replay clock |
| `ws_account` | `GET /account` | – | cash, equity, P&L, positions, open orders, kill switch | none |
| `ws_report` | `GET /report?ticket=` / `/report.md` | ticket | reproducible P&L report + fingerprint + next steps | none |
| `ws_backtest` | `GET /backtest` | – | reference SMA replay with equity curve | none |
| `ws_ticket` | `POST /ticket` | ticket JSON (`?fresh=0` to run on session state) | per-step results + report; 422 if the request is refused | paper only |
| `ws_kill` | `POST /kill` | `{reason}` | kill switch engaged | paper only |
| `ws_kill_reset` | `POST /kill/reset` | `{operator}` | **operator tool; not exposed to the model** | paper only |

## Rules for the Hermes side

1. Call `ws_screen` on the user's request before anything else; on `refuse`, relay `reason` + `alternative`, do no further calls.
2. Every `ws_order` carries a `rationale`; it is screened again server-side (a tip in the rationale is refused with `compliance_*`).
3. Never call `ws_kill_reset`; when the kill switch is engaged, tell the user which order tripped it and offer `ws_close`.
4. End every trading turn with `ws_report` and quote the bottom line, the fingerprint, and the first "What to do next" item.
5. Treat news with `usable_as_reason: false` as a warning only.

## Error codes the model must handle

`unknown_symbol`, `restricted_symbol`, `halted_symbol`, `invalid_qty`, `invalid_side`, `invalid_limit`, `no_short`,
`insufficient_cash`, `max_position`, `max_gross_exposure`, `max_open_orders`, `kill_switch_oversized_order`,
`kill_switch_engaged`, `no_position`, `compliance_insider_information`, `compliance_stock_tip`,
`compliance_market_manipulation`, `compliance_live_money`, `compliance_personal_advice`.

## Example ticket

`crates/galion-wallstreet/fixtures/tickets/scripted-open-close.json`: quote -> news -> note -> buy 200 GLXA -> limit buy OMND
-> advance 10 -> oversized buy (refused, kill switch trips) -> buy HRMS (refused, engaged) -> close GLXA (filled) -> note
-> operator reset -> close OMND. Result: +1308.94 paper, fingerprint `fnv1a64:e3f5e643e7750856`.

## Open for workstream H

- Port the tool list into `hermes-rs`'s tool registry (MCP-style JSON schema per row above).
- Decide per-user paper accounts vs one shared session (today: one in-memory session per server process; `/ticket` is isolated by default).
