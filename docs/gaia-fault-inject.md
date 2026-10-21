<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Gaia S3.2: heal-tick fault injection (slot 189, 2026-10-21)

Base: `swarm/gaia-loop-decisions` b40208a (D-L1..D-L4 implemented). Box only.
Test: `crates/gaia/tests/real_gaia_faults.rs` (GALION_REAL=1, 9 scenarios +
teardown). It drives the real release `gaia` binary (or `heal_tick_at`
in-process for the fake clock), real `gaia-dummy` fault services, real
hades-gate keys/tokens/ledgers, a programmable fake nerve (127.0.0.1:20590)
and the real `openmind-nerve serve` (127.0.0.1:20591).

Run:

```
GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve \
  cargo test --release -p gaia --test real_gaia_faults -- --nocapture --test-threads 1
```

`GAIA_FAULT_EVIDENCE=FILE` also appends each evidence line to FILE.

## What changed in code

| Change | Where | Why |
|---|---|---|
| Strict `/health` JSON rule: body must be ONE JSON value, an object, with top-level `status == "ok"` or `ok == true` | `gaia_live::json_ok`, `Expect::JsonOk` | The old rule searched for the substring `"status":"ok"` after deleting all whitespace. A truncated `{"status":"ok"`, a nested `{"status":"down","last":{"status":"ok"}}`, plain text, HTML, `[{"status":"ok"}]`, `{"ok":false,"checks":{"ok":true}}` and `{"status":"o k"}` all read as **healthy** (fail open). Live VPS bodies of 8420, 8642, 8650 (read-only GET 13:24 +02:00, all `Content-Length`, no chunking) still pass. |
| `gaia-dummy` modes `slow` (`--delay-ms`, `--slow-count`), `badjson` (`--bad KIND`, 8 kinds), `wrong` (503 thief) | `crates/gaia/src/bin/gaia-dummy.rs` | Fault services. Same 19000-19999 guard, loopback only, `forbid(unsafe_code)`. Old modes unchanged. |
| `live_heal::heal_tick_at(cfg, Some(now))` | `crates/gaia/src/live_heal.rs` | Fake wall clock for the cap window, the token check and the ledger record, for in-process tests. `heal_tick` = `heal_tick_at(cfg, None)`. The binary takes no clock from args or env, so the window cannot be moved from outside. |

No new dependencies.

## Fault matrix (all PASS, release, 2026-10-21 13:35-13:37 +02:00)

| # | Fault | Expected (invariant) | Measured |
|---|---|---|---|
| F1 | hang (accepts, never answers) | report-only (D-L4), no token, no restart, never killed | loop 3 ticks: `hung` x3, 3 tokens kept, consumed 0, listener_pids = dummy pid, pid alive |
| F2a | slow /health 800 ms (< 1000 ms read timeout) | healthy | exit 0 in 0.80 s |
| F2b | slow 1400 ms every request | hung, report-only | exit 8 in 2.35 s (two timed-out probes) |
| F2c | slow 1400 ms once, then fast | healthy on the confirming probe | exit 0 "slow answer, up on the confirming probe" |
| F3 | bad JSON, 8 bodies | never healthy | all 8: probe `down`, exit 4 (dry-run, no token); 4 say "body is not JSON", 4 "status not ok" |
| F3b | bad JSON + 3 tokens, port still held | budget 3/h, holder never killed | `apply-failed` 3, `capped` 2; global ledger 3; consumed 3; holder alive and sole listener; 3 spawned copies failed to bind (**D-FI2**) |
| F4 | flapping, **two loops** on one service, separate `--state` and token dirs, 22 s | exactly 3 restarts in the hour | a 1 + b 2 = 3; global ledger 3; capped 108 ticks; consumed 3; all 3 restarts inside one second (**D-FI6**) |
| F5a | port stolen by a foreign 503 process | no kill, budget holds | `apply-failed` 3, `capped` 1, thief alive + sole listener |
| F5b | port stolen by a foreign hung process | hung with foreign pid | exit 8, listener_pids [thief], gaia_started_pids [] |
| F6a | real nerve killed mid-loop, then service crashes | no restart without the nerve; back-off; heals when the nerve returns | 3 error ticks ("nerve ... is not healthy"), consecutive 1,2,3, sleeps 300/600/1200 ms, port stayed closed, ledger 0; nerve restarted -> restart 1.2 s later; recorded on the new nerve's log only |
| F6b | nerve stays down | loop stops | exit 7 `max-errors` after 3 ticks, 0 restarts |
| F7a | nerve /health 503 | fail closed | exit 2, consumed 0 |
| F7b | nerve `/events` loses the probe event | fail closed | exit 2 "nerve did not return the probe event", consumed 0 |
| F7c | nerve /state 500 | fail closed before the token | exit 2 (D-L2), consumed 0 |
| F7d | /state ok first, 500 under the budget lock | no spawn | exit 2, no spawn, ledger 0, **token consumed** (D-FI3); replay then refused `replayed` |
| F7e | POST `gaia.heal.approved` 500 | no spawn | exit 2, consumed, no spawn (D-FI3) |
| F7f | POST `gaia.heal.applied` 500 | the restart still counts | exit 2, service up, global ledger 1; next tick's cap step shows `gaia_restarts_last_hour_before: 1` |
| F8 | DST 2026-10-25 03:00 CEST -> 02:00 CET, fake clock | window = 3600 real s; ledger offsets right; expiry on the same clock | 02:30+02:00 ok, 02:50+02:00 ok, 02:10+01:00 ok, 02:20+01:00 capped retry 600 s; 02:30+01:00: expired token (exp 02:15+01:00) refused `expired`, a 3600 s token refused `lifetime` (gate max_ttl 900, **D-FI5**), fresh token -> restart; 02:31+01:00 capped retry 1140 s. Ledger ts: `02:30+02:00, 02:50+02:00, 02:10+01:00, 02:30+01:00` (02:30 twice) |
| F9a | torn global ledger line | fail closed | exit 2 "ledger: ... EOF while parsing", consumed 0 |
| F9b | corrupt `--state` ledger | fail closed | exit 2, consumed 0 |
| F9c | gate DISABLED file | refused | exit 4 `disabled` |
| F9d | missing hades config | refused | exit 4 "hades-gate unavailable, failing closed" |
| F9e | the same token on the intact gate | not burnt by (a)-(d) | exit 0 restarted |
| teardown | | nothing left | 0 dummies, 19890-19899 and 20591 closed, no token text in 335 nerve events |

Invariants held in every scenario: at most 3 restarts per service per
rolling hour (counted in the global ledger, also across two loops and new
`--state` dirs); Gaia never signalled a process; no restart without the
nerve; no restart without a consumed signed token; every setup fault was
exit 2 or 4 with no spawn.

## Findings and owner decisions

- **D-FI1 strict `/health` JSON (implemented here; accept or revert).** The old
  substring rule failed open on 7 of the 8 misleading bodies (all except
  `empty`; pinned by `old_rule_failed_open` in `gaia-live/tests/json_strict.rs`). The new rule is stricter
  only; the three live JSON-ok probes (8420, 8642, 8650) return valid
  top-level JSON with `Content-Length` and still pass. A chunked or
  non-JSON health endpoint would now read as down. Recommend: accept.
- **D-FI2 restarting a service whose port is still held.** Gaia restarts by
  starting a new copy and never kills; if the port still has a listener
  (bad JSON, 503, a foreign thief), the copy cannot bind, so each restart is
  futile but spends a signed token and one of the 3 hourly restarts (F3b,
  F5a: 3 tokens and the hour's budget gone, `apply-failed` x3). Options:
  (a) keep (safe, wasteful); (b) **recommended:** before spawning, if
  `listener_pids(port)` is non-empty, report `gaia.heal.held` (like D-L4
  hung: evidence, no token, no budget, exit 8); (c) (b) only for foreign
  pids, keep restarting Gaia-started ones. No money or compute cost.
- **D-FI3 token burnt when the nerve fails after approval.** If the
  re-read of `/state` under the budget lock fails, or `POST
  gaia.heal.approved` fails, the token is already consumed and nothing
  restarts (F7d, F7e). Fail closed and correct; it only costs a token. Also:
  if `POST gaia.heal.applied` fails after the spawn (F7f), the tick is an
  error, so the loop's own `restarts` / `--max-restarts` do not count that
  restart (the global 3/h cap still does). Options: keep (recommended,
  document) or make the loop count `spawned` from the ledger.
- **D-FI4 test-port band vs. live ports.** `TEST_PORTS` is 19000-19999, and
  the VPS has POSEIDON listening on 127.0.0.1:19120 (read-only `ss -ltn`
  13:39 +02:00), so if Gaia's heal path ever ran on the VPS, the scope guard
  would treat a live port as a test port. Also, wave-4 slot ports are
  20000+, so this test had to place its fault services on 19890-19899
  (outside slot 189's 20590-20599; nerves stayed inside). Options:
  (a) **recommended before any VPS install (D-L1):** refuse a target port
  that is in the live inventory or already listened on by a systemd unit;
  (b) move the band (e.g. 29000-29999) and keep slot ranges out of it;
  (c) keep as is (box only).
- **D-FI5 token TTL vs. gate max.** `gaia tokens request --ttl-s` allows up
  to 3600 s (D-L3), but a gate on its default `max_ttl_secs = 900` refuses
  such a token at use (`lifetime`, F8), and the loop then files it as
  refused. Options: `tokens request --hades-config C` refuses a TTL above
  the gate's max (recommended), or set `max_ttl_secs = 3600` in the gate.
- **D-FI6 two loops on one service.** Two loops (different `--state`) on
  one flapping service spent the whole hourly budget in one second (3
  global-ledger entries at the same second; one copy failed to bind). The
  cap held, but restarts were duplicated. Options: a per-service loop lock
  file next to the global ledger (recommended; a second loop refuses to
  start), or re-probe under the budget lock before spawning.

## DST note

The loop sleeps on the monotonic clock (unchanged, S3.1). The cap window,
token `iat`/`exp` and ledger records use Unix seconds, so the repeated local
02:00-03:00 hour on 2026-10-25 neither frees nor blocks a restart (F8).
Every Zagreb string carries its offset (`+02:00` before 01:00 UTC, `+01:00`
after); sorting ledger `ts` strings would be wrong across the switch, so
always compare `epoch_s`.
