<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: gaia-loop-decisions (wave-4 slot 175)

Branch `swarm/gaia-loop-decisions`, based on `swarm/gaia-tick-loop` e4631a6. All times are Europe/Zagreb, CEST (+02:00), 2026-10-21.
This slot implements Mate's accepted decisions D-L1..D-L4 for slot 160 (docs/gaia-heal-loop.md).

**Box only**, with release binaries. D-L1: nothing was installed, written or restarted on the VPS. The VPS was used only for read-only GETs of the nerve and read-only `systemctl` listings.

## Ports
Slot 175 owns 20750-20759: fake nerve 20750, real `openmind-nerve serve` 20751 (decisions test), and 20752 (`real_gaia_loop`, moved off slot 160's 20300).
The `gaia-dummy` services use **19750-19757**, because the heal apply path only ever touches 127.0.0.1:19000-19999 (`gaia_live::heal::TEST_PORTS`, a guard these tests do not widen). This is the same deviation slot 160 made with 19601-19603.
The hermetic tests use 19960-19989 (heal_decisions) and 19758 (tokens; nothing listens there).

## Commands
```
export CARGO_TARGET_DIR=/workspace/swarm/target/gaia-loop-decisions
cargo-slot build -j 2 --release -p openmind-nerve -p gaia
GALION_REAL=1 GAIA_NERVE_BIN=$CARGO_TARGET_DIR/release/openmind-nerve \
  cargo-slot test -j 2 --release -p gaia --test real_gaia_decisions -- --nocapture   # also real_gaia_loop, real_gaia_live
```

## Runs
| Run | Start | Test | Result |
|---|---|---|---|
| 1 | 08:32:50 | real_gaia_decisions | **Hung in the test's own summary line, then killed at 08:39.** Every D-L2/D-L3/D-L4 check and the real-nerve heal had already passed (log 08:32:50-08:33:14). Cause: a test-side deadlock, with two `Mutex` guards on the fake nerve's log inside one `eprintln!`. Fixed, then re-run. Not a product bug. |
| 2 | 08:40:00 | real_gaia_decisions | pass, 23.8 s |
| 3 | 08:40:37 | real_gaia_loop (updated for D-L4) | pass, 11.8 s |
| 4 | 08:40:49 | real_gaia_live | pass, 1.3 s |
| 5 | 08:40:50 | real_gaia_decisions | pass, 23.8 s |
| 6 | 08:41:14 | real_gaia_decisions | pass, 23.9 s |
| 7 | 08:45:40 | real_gaia_decisions (final code, incl. `gaia nerve-restarts`) | pass, 23.4 s |
| 8 | 08:46:12 | real_gaia_loop (final code) | pass, 11.6 s |
| 9 | 08:46:31 | real_gaia_live (final code) | pass, 1.3 s |

real_gaia_decisions: 4 of 4 runs that finished passed (5 started; run 1 was killed by the test bug). real_gaia_loop: 2/2 passed. real_gaia_live: 2/2 passed. After every run: 0 dummies left, ports 20751-20752 and 19750-19757 closed (`ss -ltn` count 0 at 08:46:39), and 0 processes left from this target dir.

## real_gaia_decisions output (run 2, 08:40; runs 5-7 gave the same numbers)
```
[08:40:07] fake nerve on 127.0.0.1:20750
[08:40:07] D-L3: batch of 3 minted (ttl 900 s), 4th refused (no stockpiling), --count 4 / --ttl-s 3601 refused
[08:40:12] D-L2: nerve 2 -> 1 Gaia restart, then capped (8 capped ticks, combined 1+2=3) after 3.25 s; consumed tokens 1
[08:40:16] D-L2: nerve -> 0, cap lifted after 3.00 s: Gaia restarts 3 total, then capped 3+0;
           loop summary ticks 26 outcomes {"capped":12,"healthy":11,"restarted":3} tokens_spent 3
[08:40:16] D-L2: nerve 3 -> one-shot exit 5: "restart cap reached: 0 Gaia + 3 nerve restarts of dummy-ncap in the last hour
           (combined cap 3); next allowed in at most 3600 s"
[08:40:16] D-L2: /state HTTP 500 -> exit 2 fail closed, token kept: gaia: nerve restart count (D-L2): nerve GET /state: HTTP 500
[08:40:27] D-L4: 4 ticks in 10.22 s -> 4 x gaia.heal.hung (listener pid 673481, probes ["read: operation would block",
           "read: operation would block"]), 0 restarts, 0 tokens, dummy alive
[08:40:30] D-L3: ttl 2 s batch expired; loop moved 3 aside (tokens_expired 3), 0 consumed, outcomes {"refused":2}; new batch of 3 accepted
[08:40:30] real openmind-nerve /state after 0.9 ms: cycle 1 components [openmind-memory=0, hermes-rs=0, link-watch-rs=0, galion-desk=0,
           tuner=0, galion-one=0, galion-one-mouth=0, console=0, galion-health=0, cloudflared=0, tuner-quick=0, voice-worker=0, gaia=0,
           lxc-host=0, galion-nerve=0]
  nerve restarts for hermes-rs: {component hermes-rs, count 0, cycle 1, reflex_live_1h 0, restarts_1h 0, unit hermes-rs.service}
  nerve restarts for dummy-real: {component null, count 0, ...}
[08:40:30] real nerve: dummy-real restarted (exit 0), cap step nerve {"component":null,"count":0,"cycle":1,...}
fake nerve: 65 events; /state GETs 22; applied 3 capped 13 hung 4 failed 0 refused 2 loop.started 3; replay ledger consumed 4
[08:40:31] teardown: 0 dummies, 20751 + 19750-19754 closed
```
What the test asserts, beyond the lines above:
- every `gaia tokens` output and every nerve event is free of token text and of the `hades1.` prefix;
- minted files are mode 0600 and their signatures verify;
- `status --hades-config` reports `signature_ok: true` and `consumed_in_ledger: false`;
- the replay ledger has exactly 4 consumptions (3 flap + 1 real-nerve heal);
- the hung dummy is still running and still listening after the loop;
- each `gaia.heal.hung` event has `listener_pids == [hung dummy pid]`, `kill: false`, `restart: false` and two probe details.

## real_gaia_loop (slot 160's test, updated for D-L4; run 3, 08:40)
```
dummy-hang:  ticks 4  restarts 0 outcomes {"hung":4} tokens_spent 0 errors 0   (each tick ~2.33 s: two 1 s probes + 300 ms)
dummy-flap:  ticks 23 restarts 3 outcomes {"capped":11,"healthy":9,"restarted":3} tokens_spent 3 reaped 3
dummy-crash: ticks 22 restarts 1 outcomes {"healthy":21,"restarted":1} tokens_spent 1
hang: 4 gaia.heal.hung events, listener pid 673666 (the hung dummy), still alive: true
one-shot heal-tick with a new --state dir: exit 5 capped, token not consumed
second flap loop on a new --state dir: outcomes {"capped":3} (no restart, no token)
nerve events: 88 total; loop.started 4 loop.stopped 4 heal.applied 4 heal.failed 0 heal.capped 15 heal.approved 4
```
Compared with slot 160: the hang service used to burn **3 tokens and 3 budget slots** on restarts that could never take the port (3 x `gaia.heal.failed`). Now it uses 0 of each, and the replay ledger has 4 consumptions instead of 7.

## VPS (read-only), live nerve 127.0.0.1:8670
| Time (CEST) | Probe | Result |
|---|---|---|
| 08:16:47 | `GET /state` keys + reflex | cycle 6925, `interval_s` 30; `reflex.limits` {restarts_per_hour_default 3, cloudflared_per_hour 1, max_actions_per_cycle 2, fail_cycles 3, rollback_per_unit_per_day 1}; `reflex.budgets` for 10 units all `live_1h 0`; `recent []`; 15 components, all `restarts_1h 0` |
| 08:25:05 | `GET /state` (trimmed, committed as `crates/gaia-live/tests/fixtures/vps-nerve-state-20261021.json`; name/unit/status/restart fields and `reflex` only; no address) | cycle 6941; the D-L2 parser test reads it: 9 live units matched by name and unit, all 0; `galion-voice` matches `voice-worker` by unit; `dummy-flap` unmatched = 0 |
| 08:41:54 | mode, restarts, cortex, install check | cycle 6974, mode `live`; `restarts_1h` sum **2** (`galion-one` 1 since 08:39:35, `cloudflared` 1 since 08:32:35); `live_1h` sum 0 (these were not reflex restarts); `/api/v1/cortex` **404** (cortex not live); no `gaia*` unit, timer or binary (D-L1 holds) |
| 08:44:43 | `GET /state` saved to the box target dir (not committed), then `gaia nerve-restarts --state-file` (release build, box) | cycle 6980: `galion-one` count 1 (restarts_1h 1, reflex_live_1h 0), **2 left for Gaia**; `cloudflared` count 1, 2 left (the nerve's own cloudflared limit is 1/h: owner point D-L2b); hermes-rs, openmind-memory, galion-voice (voice-worker) 0, 3 left; dummy-flap unmatched, 3 left |

Real data shows one more thing. `restarts_1h` counts restarts the reflex did not make: `galion-one` and `cloudflared` were restarted with `live_1h 0`. Gaia counts the larger of the two fields, so those count against the combined cap too.
The VPS `/state` body is 13,091 bytes, well under the client's 1 MiB read cap.

## Box test numbers
- `cargo fmt --all --check`: ok.
- `cargo clippy --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools --all-targets -- -D warnings`: ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` (debug, GALION_REAL unset): **1255 passed, 0 failed, 6 ignored**. Base e4631a6 had 1234/0/6.
- New: gaia-live `decisions` 6, gaia `heal_decisions` 7, gaia `tokens` 7, plus `real_gaia_decisions` (skip-pass without GALION_REAL). Existing gaia `heal_tick` 13, `heal_loop` 11, gaia-live `heal` 12 still pass (the `heal_tick` fake nerve now answers `GET /state`).
- `scripts/no-python.sh`: ok. No sandbox test was run on the box. openmind-sandbox, cgroup-detect and hermes-tools were excluded from every local run.
- Cross targets (gaia + gaia-live): `cargo check` aarch64-linux-android and armv7-unknown-linux-musleabihf ok; `cargo build --release` of the `gaia` binary for x86_64-unknown-linux-musl and aarch64-unknown-linux-musl (rust-lld) ok (08:48).
- No new dependencies (docs/third-party/gaia-loop-decisions.md); Cargo.lock unchanged.
