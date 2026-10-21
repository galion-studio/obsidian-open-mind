<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life tests: console-port-merge (all 31 console routes in Rust)

Wave 4 slot 169. Branch `swarm/console-port-merge` = `swarm/landing-overview` 669dc18 + merges of console-port-1..5
(a3d65a6) + merge fixes. Box, Europe/Zagreb (CEST +02:00). Slot ports 20390-20399 (20390-20393 used). VPS reached
only through `ssh -N` forwards (ssh alias `slavik-vps`, key `~/.ssh/id_ed25519_vps`, IdentitiesOnly; the address is not
in the code or this doc). **GET only against the VPS: no POST reached it, nothing was started, written or changed on
the VPS.** No sandbox runs on the box.

## Merge fixes (this commit)
- `console-static/tests/ui.rs`: `/session` is ported now, so it must not show "Not in Rust yet"; all 28 rooms ported,
  `PORTED` has 30 entries and the overview says "31 of 31 routes in Rust".
- `tests/ui_port1.rs`, `ui_port2.rs`, `ui_port4.rs`: `UiConfig` literals gained the fields added by the other slices
  (`files`, `desk`).
- `console-ui/src/css.rs`: sheet budget 24 KiB -> 28 KiB (merged sheet is 26 480 B; each slice alone fit in 24 KiB).
- `console-ui/src/overview.rs`: Console sector status "31 of 31 routes in Rust on console-port-merge", facts and refs.
- New `console-static/tests/real_merge.rs` (`#[ignore]` + `GALION_REAL=1`): one merged binary
  `galion-console-static --ui rust --desk` on 127.0.0.1:20390, forwards 20391 -> 8080 (console API), 20392 -> 8646
  (galion-desk), 20393 -> 8670 (galion-nerve); GETs every `PORTED` route and every room: 200, no `<script>`, never the
  placeholder page; `/md/user` (stands for `/md/$name`) answers a clean 404 without `--memory-root` (the live file view
  is covered by `real_port4`); overview count 31 of 31; nerve `/state` before and after.

```text
GALION_REAL=1 cargo test --release -p console-static --test real_merge -- --ignored --nocapture
GALION_REAL=1 cargo test --release -p console-static --test real_port{2,3,4,5} -- --ignored --nocapture
GALION_REAL=1 PORT1_VPS=slavik-vps cargo test --release -p console-ui --test real_port1 -- --ignored --nocapture
```

## Runs, 2026-10-21 (+02:00), merged code
| test | ports | time (+02:00) | result |
|---|---|---|---|
| real_merge (first run) | 20390-20393 | 08:53:00-08:53:32 | FAIL: `/md/user` 404 (test expectation; front correct without `--memory-root`) -> test fixed |
| real_port2 | 20060-20063 | 08:53:32-08:54:08 | pass (/context, /gateway 9 endpoints, /guardian 401 stated, /hermes 214 threads) |
| real_port3 | 20070-20073 | 08:54:08-08:54:37 | pass (/instances 3 nodes, /keys 401 stated, /markers 0 stored = live) |
| real_port4 | 20080-20082 | 08:54:37-08:54:57 | pass (/memory 7 rows = VPS find, /md/persona + /md/user = VPS stat, /mesh idle) |
| real_port5 | 20090-20094 | 08:54:57-08:55:25 | pass (/node = live target, /security + /session 401 stated, paired view 30 files) |
| real_port1 | 20057-20059 | 08:55:25-08:55:57 | pass (/analytics 7 boards, /arena 12 rows = live, /bank, /cache; front RSS 3184 kB) |
| real_merge run 1 | 20390-20393 | 08:56:17-08:56:51 | pass: 29 routes 200 + /md/user 404, 0 placeholders, 31 of 31 |
| real_merge run 2 | 20390-20393 | 08:56:51-08:57:19 | pass (same) |

Per-route timing on the merged front (run of 08:53): 0-1248 ms per page (most ~790-840 ms, i.e. the forward round
trip to the live API); `/soon` 0 ms (static). Every page 0 `<script>`.

`real_port1` sends POST/PUT/DELETE only to the box-local Rust front, which refuses them itself with 405 before any
upstream call; nothing was forwarded to the VPS.

## Nerve (VPS 127.0.0.1:8670 `/state`, read-only)
Before the first run cycle 6997, after the last run cycle 7005; mode live, 15 components throughout. hermes-rs:8642,
galion-desk:8646 and console:8080 healthy before and after every run; galion-one:8091 "degrading" before and after
every run (planned restart for the t179 deploy at 08:39 +02:00, "1 restart in last hour"; not caused by these GETs).

## Box checks (08:51-08:58 +02:00)
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude
  cgroup-detect -- -D warnings` ok (`hermes-tools` is not a package in this workspace).
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect`: 645 passed, 0 failed, 7 ignored
  (134 suites; the 7th ignored is `real_merge`). console-ui lib 102/0.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 0 missing. No new crates (`Cargo.lock` unchanged).
- After the runs no listener left on 20057-20099 or 20390-20399.

## Known stale test (not run)
`console-ui/tests/real_console_ui.rs` (pre-port, ports 19375-19379) still asserts the unported rooms show
"Not in Rust yet" and that no page says "did not load"; after the merge there are no unported rooms and several
rooms state live 401s. It was not run; it should be retired or folded into `real_merge` (owner decision).
