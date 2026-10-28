<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# real-ports-batch-2 (wave-4 slot 375)

Follow-up to F-363-1: the next batch of `tests/real_*.rs` files with literal
ports (console, gaia, room, plus galion-health real_health and galion-one
real_one_probe) now read a port base from env through
`galion_realtest::ports`. Unset = the old default block, so earlier runs and
docs stay valid; a bad value panics naming the var (never a silent
fall-back). Branch `swarm/real-ports-batch-2` off `swarm/real-test-port-env`
0d545118. Box only; VPS read-only. Times Europe/Zagreb (CET +01:00).
Slot ports 22450-22459.

New helper: `ports::Block::new(VAR, default, span)` in a `static`; `addr(off)`
gives a cached `&'static str` `127.0.0.1:<base+off>`, `port(off)`, `range()`.
It prints one line `real ports: VAR -> 127.0.0.1:a-b` per test binary so a
run shows which block it used. Unit tests: default block, offset-outside-span
panic, plus the doc test.

## Env knobs

| Test | Env var | Default | Span | Layout |
|---|---|---|---|---|
| `console-static/tests/real_landing.rs` | `LANDING_REAL_PORT_BASE` | 19400 | 10 | bind +2, dead upstream +3 |
| `console-static/tests/real_merge.rs` | `MERGE_REAL_PORT_BASE` | 20390 | 10 | bind +0, API fwd +1, desk fwd +2, nerve fwd +3 |
| `console-static/tests/real_port2.rs` | `PORT2_REAL_PORT_BASE` | 20060 | 10 | bind +0, API +1, desk +2, nerve +3 |
| `console-static/tests/real_port3.rs` | `PORT3_REAL_PORT_BASE` | 20070 | 10 | bind +0, API +1, nerve +3 |
| `console-static/tests/real_port4.rs` | `PORT4_REAL_PORT_BASE` | 20080 | 10 | bind +0, API +1, nerve +2 |
| `console-static/tests/real_port5.rs` | `PORT5_REAL_PORT_BASE` | 20090 | 10 | bind +0, API +1, fake session +2, nerve +3, 2nd bind +4 |
| `console-static/tests/real_route31.rs` | `ROUTE31_REAL_PORT_BASE` | 20400 | 10 | bind +0, API +1, nerve +3 |
| `console-ui/tests/real_port1.rs` | `PORT1_REAL_PORT_BASE` | 20050 | 10 | front +0, nerve +7, desk +8, API +9 |
| `gaia/tests/real_gaia_poseidon.rs` | `GAIA_POSEIDON_REAL_PORT_BASE` | 19240 | 10 | queue +1 |
| `gaia/tests/real_poseidon_handle.rs` | `POSEIDON_HANDLE_REAL_PORT_BASE` | 20880 | 10 | queue +0, impostor +1, look-alike +2 |
| `gaia/tests/real_gaia_live.rs` | `GAIA_LIVE_REAL_PORT_BASE` | 19230 | 10 | nerve +3, dummy +4, `--allow-ports` = block; must stay in `gaia_live::heal::TEST_PORTS` 19000-19999 (guard not widened) |
| `room/tests/real_room.rs` | `ROOM_REAL_PORT` (existing; slot-08 range assert dropped) | 19081 | 2 | host +0, netns +1 |
| `galion-realtest/tests/real_sh_room.rs` | `REAL_SH_ROOM_PORT_BASE` | 19840 | 2 | host +0, netns +1 |
| `galion-health/tests/real_health.rs` | `HEALTH_REAL_PORT_BASE` | 19490 | 4 | health +0, upstream +1, closed +2, raw +3 |
| `galion-one/tests/real_one_probe.rs` | `ONE_PROBE_REAL_PORT_BASE` | 19057 | 3 | one +0, mouth +1, dead upstream +2 |

Already movable before this slot (proved on slot ports, no code change):
`console-static/tests/real_console_static.rs` (`CONSOLE_STATIC_ADDR`,
`CONSOLE_STATIC_UP`) and `console-ui/tests/real_console_ui.rs`
(`CONSOLE_UI_PORT`, `CONSOLE_UI_UPSTREAM`, `CONSOLE_UI_PULSE`).
`gaia/tests/real_cli.rs` binds no port (nothing to convert).

## Real runs (GALION_REAL=1, release, base 22450, 2026-10-28)

| Time (+01:00) | Test | Result |
|---|---|---|
| 08:16:32 / 08:17:03 | console-static real_landing | `real ports: LANDING_REAL_PORT_BASE -> 127.0.0.1:22450-22459`; /console 200 31952 B, 0 scripts, 9 cards; 1/1 pass |
| 08:16:40 | room real_room (`ROOM_REAL_PORT=22450`) | host session on 127.0.0.1:22450 (24 objects, 32 concurrent 200), torn down; netns session on 22451, host connect refused; 2/2 pass |
| 08:17:09 | gaia real_gaia_poseidon | queue block 22450-22459; 1/1 pass |
| 08:17:15 | gaia real_poseidon_handle | queue state `"addr":"127.0.0.1:22450"`, job 2 survived restart; 3/3 pass |
| 08:17:30 | console-static real_merge (ssh -N, GET only) | forward 22451->8080, 22452->8646, 22453->8670; nerve before/after cycle 67 live, hermes-rs/desk/one/console healthy; 1/1 pass (24.2 s) |
| 08:17:59 | console-static real_port2 | forward + binary on 22450-22453 up, /context /gateway /guardian checks passed; FAIL at live `GET /api/hermes/history` -> 404 (F-375-1) |
| 08:18:25 | console-static real_port3 | forward 22451/22453 up; FAIL live `GET /api/v1/markers` -> 404 (F-375-1) |
| 08:18:35 | console-static real_port4 | forward 22451/22452 up, /md rooms passed; FAIL live Node `GET /md/user` -> 404 (F-375-1) |
| 08:18:48 | console-static real_port5 | forward 22451/22453 up; FAIL live `/api/v1/health` has no `target` (F-375-1) |
| 08:18:58 | console-static real_route31 | forward 22451/22453 up, binary on 22450; FAIL live `GET /api/hermes/history` -> 404 (F-375-1) |
| 08:19:29 | console-ui real_port1 | forward 22459->8080, 22458->8646, 22457->8670; /analytics on 22450 200 14839 B; FAIL live `GET /api/v1/analytics` -> 404 (F-375-1) |
| 08:19:38 | galion-realtest real_sh_room | session 1 host 127.0.0.1:22450, session 2 netns 22451; 17 PASS / 0 FAIL; 1/1 pass |
| 08:20:09 | console-ui real_console_ui (existing knobs: 22450, up 22459, pulse 22458) | front listening 127.0.0.1:22450 upstream 22459; FAIL `/status: an API read failed` (live API, F-375-1) |
| 08:20:24 | console-static real_console_static (existing knobs: 22450/22451) | 16 pages 200, 16 probes 404, POST 405, 304; 1/1 pass |
| 08:21:06 | galion-health real_health | services JSON shows `"port":22451` up / 22452 down; 1/1 pass |
| 08:21:32 | galion-one real_one_probe | block 22450-22452; 1/1 pass |

Negatives (fail as intended):

- 08:19:51 `GAIA_LIVE_REAL_PORT_BASE=22450` -> panic `22450-22459 must lie inside the heal test range 19000-19999` (guard kept, D-363-1).
- 08:21:48 `LANDING_REAL_PORT_BASE=70000` -> panic `port base "70000" is not a number 1024-65535`.
- `ROOM_REAL_PORT=65535` -> panic `port base 65535 + 2 ports passes 65535`.
- 08:22:07 foreign listener (release galion-health) on 127.0.0.1:22452, landing base 22450 -> the test reached the foreign listener (`HTTP/1.1 404 Not Found`), proving the env picks the port. Listener killed; `ss` shows 22450-22459 free afterwards.

## Gates (08:22-08:26 +01:00)

`cargo fmt --check` ok; `cargo clippy --workspace --all-targets --exclude
openmind-sandbox --exclude cgroup-detect -- -D warnings` ok; `cargo test
--workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude
hermes-tools` 378 result blocks, 2011 passed / 0 failed / 18 ignored;
`scripts/no-python.sh` ok; `cargo check -p galion-realtest` for
aarch64-unknown-linux-musl, armv7-unknown-linux-musleabihf and
aarch64-linux-android ok. Test-only changes plus the std-only `ports::Block`.

## Findings

- **F-375-1** (pre-existing, not from this change) the live VPS :8080 is now
  the Rust `galion-console-static --local-api` (`console-rust-primary`,
  active since 2026-10-27 19:28 +01:00); read-only probe 08:19:19 +01:00:
  `/api/v1/health` 200 (`implementation":"rust","mode":"local-api"`), but
  `/api/hermes/history`, `/api/v1/markers`, `/md/user`, `/desk` all 404.
  real_port1..5, real_route31 and real_console_ui compare against Node-era
  endpoints and fail on those live reads after the ports, forwards and
  binary are up. They need re-targeting to the Rust console API (or a
  documented skip) - not in this row.
- **F-375-2** gaia real_gaia_live (like real_gaia_loop) cannot run in a
  22xxx slot range because of the 19000-19999 heal guard; only the negative
  check ran. Same owner decision as D-363-1.
- **F-375-3** remaining literal-port real tests after this batch: nerve
  real_cortex_* / real_wiring / real_score_v2 etc., tunnel-lint, bench-banking,
  openmind-discovery, hermes-gateway, voice-cpu, galion-one real_mouth_loading
  / real_one_hardening / real_one_no_shellout (about 45 files). Same Block
  pattern applies.

## Owner decisions

- **D-375-1** re-target the console live tests (F-375-1) to the Rust
  console API vs mark them as Node-era archives.
- **D-363-1** still open (heal guard vs slot ranges).

## VPS (read-only)

2026-10-28 08:26:33 +01:00: nerve `GET 127.0.0.1:8670/state` 200, cycle 67,
mode live, score 64 grade D (open galion-one mouth incident, known);
cortex `GET 127.0.0.1:8674/health` 200; 0 VPS listeners on 22450-22459.
Only ssh -N forwards with GETs were used; nothing written on the VPS.
