<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: bench-scoreboard-adv (wave-4 slot 173), 2026-10-21 +02:00

Branch `swarm/bench-scoreboard-adv` = `swarm/bench-hermes` 457ef74 + merges of `swarm/bench-banking-adv` 44e5c61,
`swarm/bench-ws-adv` 2ecfc19, `swarm/bench-vending-adv` 1b0a30c (merge commits c6f27fc, d414714, 278d5f2; no conflicts
needing hand edits) + the scoreboard change below. Mock / paper / simulation only: no money moved, no live bank or
broker, no real or paid model, nothing written or restarted on the VPS.

## 1. What changed
- `bench-scoreboard` accepts `--score B1-ADV|B2-ADV|B3-HARD|B3-HARD-H=<json>@<sha>` and renders an
  "Adversarial / hard sets" table (score, branch @ SHA, JSON, Zagreb stamp, failing axes, **named tests**) plus a
  "what each number does not show" list. `--notes FILE` appends a notes section (docs/bench/SCOREBOARD.notes.md).
- `parse_score` also reads the bench-ws-adv `families: [{name, passed, total, percent}]` shape: each family is an
  equal-weight axis, `percent` must equal `passed`/`total` and `passed <= total`, else exit 1.
- `docs/bench/SCOREBOARD.md` regenerated 2026-10-21T10:54:53+02:00 with the release binary:
  B1/B2/B3 100 %, B1-ADV 100 %, B2-ADV 100 % (13 families), B3-HARD 100 % (12 axes incl. pricing),
  B3-HARD-H 100 % (13 axes). Failing axes: none.

## 2. Box, release binaries, GALION_REAL=1 (10:52:47-10:53:55 and 11:05:15-11:06:12 +02:00)
| Test | Result |
|---|---|
| `hermes-bench --test real_banking_adv` | 1/1 ok; bench-banking mock on 127.0.0.1:20010; scripted and mock-llm (OpenAI wire :20015) both 100 %: gate refuse 38/38, allow 16/16, episodes 7/7, attack_calls 24, refused_scope 16, forbidden 0, flagged tx-a01..a05, payees end Ana Horvat + Landlord GmbH, unasked drafts 0; ports freed |
| `galion-wallstreet --test real_adversarial` | 1/1 ok; paper broker on :20021; screen 212/212, order 212/212 (420 ms); `GET /adversarial` 100 % (212/212), v1 30 % (64/212) |
| `galion-vending --test real_vending_hard` | 1/1 ok; standard seed 42 100.00 % hash c4b11d2642a0b6be (published, unchanged); hard seed 42 100.00 % hash 21317eedd059a608, incidents 51/51, pricing 360/360; published standard policy on hard 94.00 %; hard seeds 1-200 (400 runs, 757 ms): scripted 200/200 at 100 % (min 90.47 %), standard policy mean 75.72 %, min 50 %, 82 capped, 0 at 100 %; serve on :20030, `level=insane` -> 400, shutdown exit 0 |
| `hermes-bench --test real_hermes_vending_hard` | 1/1 ok; vending serve hard on :20035; scripted 471 tool calls, mock-llm 501 model API calls (:20036), hash 21317eedd059a608, replay == live == reference; standard via Hermes 443 tool calls, hash c4b11d2642a0b6be == published 457ef74 |
| `bench-banking --test real_banking` | 11/11 ok |
| `galion-wallstreet --test real_wallstreet`, `galion-vending --test real_vending`, `hermes-bench --test real_hermes_bench` | 1/1 ok each |
| `bench-scoreboard --test real_cli` | 2/2 ok: `real_cli_folds_and_pends`; `real_cli_folds_committed_adv_sets` folds all 7 committed JSONs (0 PENDING, 167 lines, 81 axis rows at 100), a tampered B2-ADV copy (family 25/26 claiming 100 %) -> exit 1, unknown key `B9-ADV` -> exit 2 |

`GALION_BENCH_WRITE_DOCS` was not set, so the committed score JSONs were not rewritten by these runs; they are the
ones produced by the three source branches and are re-validated by `real_cli_folds_committed_adv_sets`.

## 3. Live VPS, read-only (2026-10-21T10:54:24+02:00 .. 10:55 +02:00)
| Probe | Result |
|---|---|
| nerve `GET 127.0.0.1:8670/state` | 200, 39 362 B, version 0.1.0, mode live, started 09:43:54 +02:00, cycle 142, system degraded score 61 / D (11 healthy, 1 degrading, 1 failing-soon, 2 absent) |
| cortex | now present in the nerve `/state` (`cortex`, `cortex_sensors`, `cortex_events` keys) -> observed through the nerve |
| nerve signal for `hermes-rs` | warn: "session spend cap reached ($1.0014 of $1.00)"; the service refuses paid calls until the cap resets, so callers get 402 (7 "spend cap hit (provider calls refused)" entries); hermes-rs degrading (1 restart in the last hour, the 09:4x aa77864 switch) |
| live Hermes `GET 127.0.0.1:8642/health` | 200 `{"status":"ok",...,"implementation":"hermes-rs","implementation_version":"0.1.0"}` |
| `systemctl is-active hermes-rs galion-nerve` | active, active |
| live binary `/opt/galion/rust/bin/hermes-gateway-aa77864` (1 841 904 B) string count `payee_add`, `payment_draft`, `memo_flags`, `refused_scope`, `revolut` | 0 each -> no banking surface live, the B1-ADV guards exist only on branches |
| `ss -ltn` ports 20010-20039, 20430-20439 | 0 listeners; no wallstreet / vending / bench-banking service on the VPS (the one `pgrep -f` hit was the probe's own shell) |

Live vs. branch: the adversarial / hard sets have no live counterpart today (no banking tools, no paper broker, no
vending service on the VPS), so the comparison is "live = no surface to attack" and the scores stay mock/paper.

## 4. Gates (10:54:03-10:54:30 +02:00)
`cargo fmt --check` OK; `cargo clippy --all-targets -- -D warnings` OK; `cargo test --workspace --exclude
openmind-sandbox --exclude cgroup-detect`: 630 passed, 0 failed, 0 ignored (152 test binaries); `scripts/no-python.sh`
ok. bench-scoreboard release x86_64-unknown-linux-musl (static-pie) and aarch64-unknown-linux-musl (static, rust-lld)
build OK; `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf OK. No new dependencies
(see docs/third-party/bench-scoreboard-adv.md).

Note: `galion-vending --test real_vending` rewrites `docs/bench/vending.score.json`'s `generated_at` on every run
(only that field changed, 11:05:36 +02:00); the committed file was restored to the bench-vending original.
