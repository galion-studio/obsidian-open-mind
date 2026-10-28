<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# gaia-test-ports-override (wave-4 slot 423, D-383-2 option b, F-383-7)

Branch `swarm/gaia-test-ports-override` off `real-ports-batch-3` 26a526c1.
Box runs 2026-10-28 21:05-21:13 +01:00 (Europe/Zagreb), ports 22930-22939.
NO deploy, no heal-apply on the VPS, no live heal-tick / hades-gate install.

## Problem (F-383-7)

`gaia_live::heal::TEST_PORTS` is 19000-19999, so `gaia heal-tick`,
`heal-plan` and `gaia-dummy` refuse any slot range above 19999. GAIA_BIN-driven
real tests (`real_event_feed`, `real_gaia_loop`) could only run on defaults.

## Change

- `gaia_live::heal`: `TEST_PORTS` stays 19000-19999 (production default).
  New `GAIA_TEST_PORTS=LO-HI` override, read by `test_ports()`:
  - unset/empty: default 19000-19999;
  - set without `GALION_REAL=1`: refused (exit 2 / scope refusal), so a stray
    env var can never move a production heal-tick;
  - set with `GALION_REAL=1`: must parse, `LO <= HI`, both inside
    `TEST_PORTS_OVERRIDE_BOUNDS` 19000-22999, else refused.
  - It REPLACES the box range (never unions), and `check_target_in` refuses
    any test range outside 19000-22999 even when a caller passes one in
    directly. Live ports (8080, 8091, 8420, 8642, 8650, 8670) are always
    refused.
- `check_target` = `check_target_in(t, allowed, test_ports()?)`.
- `heal_plan` names the range in force; default plan line (and so every
  signed plan hash) is byte-identical to before.
- `gaia-dummy` uses the same range; usage text and `gaia` help updated.
- `real_event_feed.rs`: heal-tick now passes `--hades-config` (the old
  `--trust-file` flag no longer exists, so the GAIA_BIN branch printed
  "stdout not JSON" before) and sets `GAIA_TEST_PORTS` to its block.
- `real_gaia_loop.rs`: dummy block checked against `test_ports()`; nerve may
  share the 10-port block but never a dummy port (needed to fit one slot range).

## Tests

Unit (`crates/gaia-live/tests/test_ports_override.rs`, 7 tests, pass): default
when unset/empty; refusal without GALION_REAL; parse inside bounds; refusal of
reversed, below, above, live, 1-65535, junk; `check_target_in` follows the
override and replaces (not widens) the default; wide ranges passed straight in
are refused; `check_target` = default without env.

Real (GALION_REAL=1, release):

1. `GALION_REAL=1 GAIA_BIN=$T/release/gaia CORTEX_EVENT_FEED_REAL_PORT_BASE=22930
   cargo test --release -p openmind-nerve --test real_event_feed` 21:05-21:06: PASS
   ```
   real ports: CORTEX_EVENT_FEED_REAL_PORT_BASE -> 127.0.0.1:22930-22939
   1. gaia heal-tick exit Some(4) outcome "refused" detail "hades-gate unavailable, failing closed: .../no-hades-config.toml: entity not found"
      cycle 2: "t106 is down (Gaia service probe)" | "t106: Gaia heal refused at stage hades-approval"
   2. cycle 3: ... incident "inc-ea04675c" "critical" | score 57 | cycle_ms 22 event-feed us 40 rss 4156 KiB
   3. gaia heal-tick exit Some(0) outcome "healthy" detail "service is up; nothing to do"
   4. cycle 5: "guardian probe disk is failing" ; cycle 6 cleared; 4 records
   ```
   (The real gaia binary drove steps 1 and 3; before this change the slot
   block was refused at scope and the heal-tick call itself was malformed.)
2. `GALION_REAL=1 cargo test --release -p gaia --test real_test_ports` 21:07: PASS
   ```
   A. default: exit 2 | gaia: allowed range 22930-22939 is outside the test range 19000-19999
   B. override w/o GALION_REAL: exit 2 | ... needs GALION_REAL=1; unset it (production range 19000-19999)
   C. override: exit 0 | restart box test service t423 on 127.0.0.1:22931 (box test ports 22930-22939 only)
   D. default plan line unchanged, sha256 eb3136d8...941439
   E. 23000-23009 / 18990-19009 / 8000-8999 / 1-65535: exit 2 | outside the override bounds 19000-22999
   E. live nerve 8670 under 19000-22999: exit 2
   F. heal-tick default: exit 4 refused at scope; override: scope ok, stops at "nerve ... not healthy" (no restart)
   G. gaia-dummy :22932 default exit 2; override serves {"status":"ok",...}
   ```
3. `GALION_REAL=1 GAIA_TEST_PORTS=22930-22939 GAIA_LOOP_DUMMY_PORT_BASE=22930
   GAIA_LOOP_NERVE_PORT=22939 GAIA_NERVE_BIN=... cargo test --release -p gaia
   --test real_gaia_loop` 21:12:53-21:13:18: PASS - real signed hades-gate tokens
   and real restarts on 22931-22933: hang 3 apply-failed + 3 capped, flap 3
   restarted then capped, crash 1 restarted then healthy; one-shot new
   --state exit 5 capped; nerve events 156 (heal.approved 7, applied 4,
   failed 3, capped 37, refused 0); teardown 0 dummies, ports closed.
4. aarch64 musl `gaia` under qemu: override plan sha 99a10c9d... (same as
   x86_64); default refuses 22930-22939.

Gates: `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets
-D warnings` ok; `cargo test --workspace --exclude openmind-sandbox --exclude
cgroup-detect` 387 suites 2041/0/18 (base 2033 + 8 new); gaia+gaia-live+nerve
61 suites 428/0/3 after the last edit; no-python ok; spdx 0 missing; x86_64
musl gaia 1,053,568 B / gaia-dummy 475,936 B; aarch64 musl 895,120 /
420,160 B; aarch64-linux-android + armv7-unknown-linux-musleabihf check ok.

## VPS (read-only, 2026-10-28 21:11-21:12 +01:00)

- nerve `GET 127.0.0.1:8670/state` 200 (32,483 B, score 68, cycle_ms ~750-980).
  Cortex live (gaia-cortex-58a...).
- No `gaia` CLI on PATH, no gaia heal-tick / hades unit (only gaia-cortex,
  gaia-hermes-handoff, galion-health). `GAIA_TEST_PORTS` count 0 in the env of
  the live cortex and nerve processes. So the override has no live surface
  today; a future heal-tick install inherits the default 19000-19999 and is
  refused if anyone exports GAIA_TEST_PORTS without GALION_REAL=1.

## Owner decisions

- D-423-1: accept option (b) as implemented (GALION_REAL-gated override,
  bounds 19000-22999, replaces never unions). Recommended yes.
- D-423-2: should a set-but-not-real `GAIA_TEST_PORTS` refuse (current, fail
  closed) or be silently ignored? Recommended keep refuse.
