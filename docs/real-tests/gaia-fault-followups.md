<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: gaia-fault-followups (wave-4 slot 199), 2026-10-21

The box ran release builds of `gaia`, `gaia-dummy` and `openmind-nerve` from this branch. The VPS was
only read: `ss -ltn`, plus GET requests to nerve `/state` and `/cortex` and POSEIDON `/healthz`.
Nothing was installed, written or restarted on the VPS (the only file written
was a temporary copy of `/state` in `/tmp`, removed right after).
Ports: every Gaia test ran in its own slot band (see
`docs/gaia-fault-followups.md`). This slot itself used 20690-20692 and 20695.
All of them were free before the runs and closed afterwards.

## GALION_REAL runs (release)

```
GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve cargo test --release -p gaia \
  --test <name> -- --nocapture --test-threads 1
```

| Test | Window (+02:00) | Result | Ports |
|---|---|---|---|
| real_gaia_faults | 18:22:03-18:25:38 | 10/10 in 68.55 s | 20590-20599, 20690-20692 |
| real_gaia_loop | 18:25:49-18:26:28 | 1/1 in 12.37 s | 20300-20303 |
| real_gaia_decisions | 18:26:28-18:27:16 | 1/1 in 24.91 s | 20450-20456 |
| real_gaia_live | 18:27:16-18:27:26 | 1/1 in 1.78 s | 19230-19239 (unchanged) |

Key evidence lines:

```
[18:25:32+02:00] F7 (f) POST gaia.heal.applied 500 -> exit 2 (Ok report, outcome restarted, nerve_error set) "gaia heal-tick: restart result not recorded on the nerve: nerve POST /event: HTTP 500: {\"error\":\"injected POST fault\"}"; service up, global ledger 1
[18:25:32+02:00] F7 next tick counts the unreported restart: cap step gaia_restarts_last_hour_before 1 -> restart 2/3
[18:25:33+02:00] F7 (g) --loop --max-restarts 1 with POST gaia.heal.applied 500: stop "max-restarts" after 1 tick, restarts 1, errors_total 1, tokens_spent 1, tick 1 exit 2 spawned true token_moved "spent"; global ledger 1, consumed 1
[18:25:15+02:00] F4 flap: 2 loops: restarts a 2 + b 1 = 3; global ledger 3 (cap held)
[18:25:25+02:00] F5 stolen: thief (503) on 20596: apply-failed x3, capped 1, thief alive and sole listener
[18:25:38+02:00] teardown: 0 dummies, 20592-20599 + 20690-20692 + 20591 closed; fake nerve 292 events, 138 /state GETs; no token text in events
[18:26:15+02:00] real_gaia_loop: openmind-nerve on 127.0.0.1:20300, dummies hang 20301 flap 20302 crash 20303
  dummy-hang: ticks 4 restarts 0 outcomes {"hung":4} tokens_spent 0
  dummy-flap: ticks 23 restarts 3 outcomes {"capped":10,"healthy":10,"restarted":3} tokens_spent 3
  dummy-crash: ticks 23 restarts 1 outcomes {"healthy":22,"restarted":1} tokens_spent 1
[18:26:27+02:00] teardown: 0 dummies, ports 20300 + 20301-20303 closed
[18:26:56+02:00] D-L2: nerve 2 -> 1 Gaia restart, then capped (combined 1+2=3)
[18:27:12+02:00] D-L4: 4 ticks -> 4 x gaia.heal.hung, 0 restarts, 0 tokens, dummy alive
[18:27:16+02:00] teardown: 0 dummies, 20451 + 20452-20456 closed
```

## Release CLI checks (18:27:45 +02:00)

```
gaia-dummy --port 19120 / 20241 / 20242 / 21000      -> exit 2 (usage: P in 19000-20999, not a live VPS port)
gaia-dummy --port 20695                              -> serves {"status":"ok",...,"started":"2026-10-21T18:27:45+02:00"}; killed after
gaia heal-plan --port 19120 --allow-ports 19120-19120 -> exit 2 "allowed range 19120-19120 contains live VPS port 19120"
gaia heal-plan --port 19232 --allow-ports 19000-19999 -> exit 2 "allowed range 19000-19999 contains live VPS port 19120"
gaia heal-plan --port 20241 --allow-ports 20240-20249 -> exit 2 "... contains live VPS port 20241"
gaia heal-plan --port 20695 --allow-ports 20690-20699 -> exit 0, plan_sha256 e59ed4ba...
```

On base e970622, the same `19000-19999` range was accepted (the code checked
only `TEST_PORTS.contains(start/end)`), and so was the POSEIDON port 19120.

## Negative controls (18:28 +02:00)

* `port_bands`: after adding `"127.0.0.1:19890"` to `gaia/tests/live.rs` and `19120`
  to `real_gaia_loop.rs`, the run FAILED with "2 port problems" naming both
  lines. Both edits were reverted.
* `heal_loop`: with the old loop code (from e970622) the run FAILED 12/14:
  `a_restart_whose_applied_post_failed_still_counts` and
  `unrecorded_restarts_count_toward_max_errors` failed (`(MaxTicks, 0)` vs
  `(Errors, 7)`). With the new code: 14/14.

## Box suites

* `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect
  --exclude hermes-tools` (18:29:50-18:32:59 +02:00): 269 suites, **1278
  passed / 0 failed / 6 ignored** (base 1268/0/6). After the last LIVE_PORTS
  edit (3333), gaia + gaia-live passed again: 111/0.
* `cargo clippy --workspace --all-targets` (excluding sandbox and cgroup-detect) with `-D warnings`: clean.
  `cargo fmt --check`: clean.
* `scripts/no-python.sh` ok. `spdx-scan`: 516 files, 0 missing. `repo-check`: 1 error,
  the existing `heal_decisions.rs` 8.8.8.8 one; nothing new.
* Release builds for x86_64 and aarch64 musl (static) of `gaia` and `gaia-live` succeeded.
  `cargo check` passed for aarch64-linux-android and armv7-unknown-linux-musleabihf.

## VPS read-only (17:57-18:30 +02:00)

* `ss -ltn` at 17:57 and 18:30 showed the same listeners: 22, 25, 53, 8002, 8080, 8091,
  8420, 8642, 8646, 8650, 8660, 8670, 8671, 8672, 18800 (sshd forward),
  19120, 20241, 20242. All of them are in `LIVE_PORTS`.
* nerve `/state` 18:29:28: 200 (32 334 B), started 18:14:58, cycle 30,
  **mode dry-run**, score 76/C, 11 healthy / 1 degrading / 1 failing-soon /
  2 absent. The edge finding lists closed tunnel origin ports :3004 :3010 :3011 :3020
  **:3333** :4001 :4020 :4060 :4444, so 3333 was added to `LIVE_PORTS`.
* POSEIDON `127.0.0.1:19120/healthz` 200. This is the live port the old band contained.
* nerve `/cortex` 200 (a vulnerabilities JSON, including "3 loopback listener(s) no
  monitored unit owns: :8672 :18800 ..."). No `gaia*` unit and no `gaia` binary
  in /usr/local/bin. Gaia is still not installed on the VPS (D-L1).

## Resume re-run at HEAD (retry 2, 18:48-18:53 +02:00)

The 18:22-18:27 runs above came before the `:3333` edit (0e8bb15), so they were
all run again on the final code:

| Test | Window (+02:00) | Result |
|---|---|---|
| real_gaia_faults | 18:49:17-18:50:29 | 10/10 in 61.02 s |
| real_gaia_loop | 18:50:29-18:50:47 | 1/1 in 11.76 s |
| real_gaia_decisions | 18:50:47-18:51:18 | 1/1 in 23.60 s |
| real_gaia_live | 18:51:18-18:51:25 | 1/1 in 1.25 s |

After the runs, no `gaia-dummy` or test nerve was left running, and 0 listeners were open on 20300-20309,
20450-20459, 20590-20599 or 20690-20699.

VPS read-only at 18:51 +02:00: `ss -ltn` showed the same listeners as at 18:30, plus three new
loopback ones. They are 8673 (openmind-nerve, `/health` cycle 5, score 76), 8674 (gaia-cortex,
`/health` mode dry-run, llm_enabled false, 1 incident open) and 8675 (openmind-nerve, score 61).
All three were added to `LIVE_PORTS` and to the guard test's baseline list. They lie outside the
19000-20999 test band, so no test port changed. Nerve `:8670/state` returned 200 (36 907 B):
cycle 74, mode dry-run, score 66/D. POSEIDON `:19120/healthz` returned 200. The VPS still has no `gaia*` unit and
no gaia binary. A cortex now answers on the VPS (8674, dry-run).

Box suites after that edit: gaia + gaia-live 111/0. The workspace run (excl.
openmind-sandbox, cgroup-detect, hermes-tools) from 18:52:31 to 18:53:19 had 269 suites,
**1278 passed / 0 failed / 6 ignored**. clippy `-D warnings` (workspace, excl. sandbox and
cgroup-detect) and `cargo fmt --check` were clean, and no-python was ok.
