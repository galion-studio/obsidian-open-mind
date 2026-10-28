<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: console-realtests-rust-routes (wave 4 slot 384, D-375-1 prep)

Branch `swarm/console-realtests-rust-routes` off `real-ports-batch-2` 7c3de266.
All times Europe/Zagreb (CET, +01:00), 2026-10-28. Ports 22540-22549 only.
VPS read-only (ssh -N forwards + GETs); nothing deployed; live console untouched.

## Problem (slot 375, D-375-1)

Seven console real tests (real_port1-5, real_route31, real_console_ui) assert
Node-era console API routes. Since the Rust cutover (console.service and
console-rust-primary.service active 2026-10-27 19:28 CET) the live :8080 is
`galion-console-serve-2d846fb8 --ui rust --local-api`, which serves only
`/api/v1/health` (200, no `target`), `/api/v1/session` + `session-pack`
(401 unpaired) and `/api/v1/ping`. Every other Node-era read is 404.

## Read-only live probe (11:11 CET)

| Owner | Port | Probe | Answer |
|---|---|---|---|
| Rust console 2d846fb8 | :8080 | /api/v1/health | 200, `implementation` rust, no `target` |
| Rust console | :8080 | /api/v1/session, /session-pack | 401 |
| Rust console | :8080 | /md/user | 404, 886 B, "USER.md is not shown: not configured on this console." |
| Rust console | :8080 | every other row below | 404 |
| galion-one | :8091 | /api/v1/analytics | 401 `{"error":"pair"}` |
| galion-one | :8091 | /api/v1/awake | 200 |
| hermes-rs | :8642 | /health | 200 (122 B) |
| hermes-rs | :8642 | /v1/models, /api/sessions | 401 (bearer key; never sent) |
| galion-desk | :8646 | /desk | 200 |

## Route map (`galion_realtest::console_routes::MAP`, 32 rows)

| Node-era path | Rust owner | Rust path | Live now |
|---|---|---|---|
| /api/v1/health | console :8080 | same (no `target`) | 200 |
| /api/v1/session, /session-pack | console :8080 | same | 401 |
| /md/user | console :8080 | same (room) | 404 "not configured" |
| /desk | galion-desk :8646 | /desk | 200 |
| /api/v1/analytics | galion-one :8091 | /api/v1/analytics | 401 pair |
| /api/v1/awake | galion-one :8091 | /api/v1/awake | 200 |
| /api/hermes/health | hermes-rs :8642 | /health | 200 |
| /api/hermes/models | hermes-rs :8642 | /v1/models | 401 (key) |
| /api/hermes/history | hermes-rs :8642 | /api/sessions | 401 (key) |
| /api/v1/services, catalog, domains, repos, ledger, docs, protocol, markers, admin; /api/arena | candidate 5aa31927 (swarm/console-deploy-candidate 4f763ade) | same | not live (404) |
| /api/v1/bank | retired (candidate only proxies `--data-upstream`) | — | 404 |
| /api/v1/instances, laptop, keys, security, cloudflare, pack; /api/v3/guardian/status, incidents; /api/mesh; /api/hermes/guard, modes | retired (no Rust provider on any base.git ref) | — | 404 |

## Options staged (one switch, `CONSOLE_LIVE_ROUTES`)

* **A (`rust`, default):** each live read goes through `routes::decide`.
  Mapped route answers -> old comparison. A 404 on a non-console-owned row ->
  "Gone": the Rust room must state the miss honestly (`page_says_unavailable`),
  show 0 invented rows, no script, and where a Rust owner exists the test also
  reads it (galion-one analytics 401/200, hermes-rs /health 200). A 404 on a
  console-owned row whose map says otherwise, or any unmapped path, fails.
  When the candidate lands, the same tests compare again with no code edit.
* **B (`retire`):** every non-console/desk row is treated as Gone regardless
  of the live answer (Node-era asserts retired; no live comparison for them).
* **`node`:** the old strict asserts (negative control).
* Gate: `routes::gate_ok` — with /api/v1/admin gone the gate must be the
  fail-closed 503 "Console API is not answering"; with it present, the old
  pairing page.
* `tests/console_routes_drift.rs` (normal gate, no network): every `/api/...`
  literal in the seven real tests must be in MAP (31 literals found).

## Runs (GALION_REAL=1, release, base 22540, ssh alias slavik-vps)

| Mode | Test | Start-End CET | Result |
|---|---|---|---|
| A | real_port1 | 11:15:48-11:16:10 | ok (22 s): analytics 404 Gone + galion-one 401 pair; arena/bank/admin Gone; gate 503 fail-closed; desk /cache equal; nerve 200 |
| A | real_console_ui | 11:17:57-11:18:26 | ok (29 s): 30 Rust pages; board rooms state the miss; health no target |
| A | real_port2 | ~11:19 | ok (24.9 s): /context = live desk; gateway statuses equal; guardian 404 shown; hermes history Gone -> 0 threads; nerve healthy before/after |
| A | real_port3 | ~11:20 | ok (21.8 s): instances/laptop/keys/markers Gone, rooms state it, 0 cards/rows |
| A | real_port4 | ~11:20 | ok (13.4 s): /memory = VPS find (7 rows); /md = VPS stat; live /md/user "not configured"; mesh Gone; gate 503 x3 |
| A | real_port5 | ~11:21 | ok (21.9 s): /node states no target; security+guardian 404 stated; session 401 compared; session-pack 401 = live; 30 session files equal |
| A | real_route31 | 11:22:25-11:22:56 | ok (25.8 s): /hermes Rust room, 0 threads, CSP/XFO kept; 29 Rust-first + 2 SPA; proxy /api/hermes/health 404 = direct; hermes-rs /health 200 |
| B | real_port1 | 11:23:14-11:23:36 | ok (21.3 s) |
| B | real_console_ui | 11:23:36-11:24:03 | ok (27.0 s) |
| B | real_port2 | 11:24:03-11:24:29 | ok (26.1 s) |
| B | real_port3 | 11:24:29-11:24:49 | ok (20.4 s) |
| B | real_port4 | 11:24:49-11:25:03 | ok (13.3 s) |
| B | real_port5 | 11:25:03-11:25:25 | ok (22.0 s) |
| B | real_route31 | 11:25:25-11:25:50 | ok (25.5 s) |
| node | real_port2 | 11:25:50-11:26:11 | FAILED as expected: live 404 vs 200 (the D-375-1 failure) |
| node | real_route31 | 11:26:11-11:26:15 | FAILED as expected: live /api/hermes/history 404 |

Gates (11:27-11:31 CET): `cargo fmt --all --check` ok; clippy
`--workspace --exclude openmind-sandbox --exclude cgroup-detect --all-targets
-D warnings` ok; workspace tests (also excluding hermes-tools) 2020 passed,
0 failed, 18 ignored; galion-realtest 24 unit + 2 drift ok;
`scripts/no-python.sh` ok.

## Recommendation: option A

A keeps live coverage of every Rust-owned route (console, galion-one,
hermes-rs, desk), fails on a regression of those or on unmapped drift, and
turns the board rows back into full comparisons by itself once candidate
5aa31927 is live. B passes too but stops checking anything outside the
console/desk, so a broken candidate deploy would go unnoticed.

## Owner decisions

* **D-384-1:** adopt option A (default) for the seven console real tests, or
  B. Recommendation A.
* **D-384-2:** confirm the 13 "retired" rows (bank, instances, laptop, keys,
  security, guardian status/incidents, mesh, cloudflare, pack, hermes
  guard/modes) are really retired, or name a Rust owner to port them to.
* **D-384-3:** /hermes thread list: keep it gone, or let the console read
  hermes-rs `/api/sessions` (needs the bearer key server-side).

## Findings

* **F-384-1:** console.galion.app rooms answer the fail-closed 503 "Console API
  is not answering" because live :8080 has no /api/v1/admin (reconfirms slot
  303). Fixed when the candidate (with --admin-store) lands.
* **F-384-2:** live /api/v1/health has no `target`, so /node shows no ssh
  line / Last IP.
* **F-384-3:** real_route31 also had a stale count (`PORTED + 1`) since
  console-port-merge made /arena, /bank SPA-first; now subtracts SPA_FIRST,
  as unit test ui_route31 already did.
* **F-384-4:** real_port2's /gateway endpoint list GETs live /api/v1/ping
  (pre-existing, read-only).
* **F-384-5:** `page_says_unavailable` accepts a bare "404" marker; broad but
  every Gone branch also asserts 0 invented rows. Tighten later if wanted.
