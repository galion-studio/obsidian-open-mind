<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: gaia-fault-inject (wave-4 slot 189), 2026-10-21

Box: `real_gaia_faults` (GALION_REAL=1, release), 3 full runs:
13:33-13:34 +02:00 (9/10, F8 assumed a 3600 s token TTL the gate refuses;
test fixed, recorded as D-FI5), 13:35-13:36 +02:00 10/10, 13:41-13:42 +02:00
10/10 (final, below). Plus one F4-only run at 13:36-13:37 +02:00 that kept its
directory to count spawns: loop b's log had 2 spawns, 1 of them failed to
bind; all 3 global-ledger entries were at 13:36:58 +02:00 (D-FI6).

Ports: fake nerve 127.0.0.1:20590 (test thread), real `openmind-nerve serve`
127.0.0.1:20591 (built from this branch), fault services 19890-19899 (heal
guard band, see D-FI4). Teardown: 0 gaia-dummy left, all ports closed, temp
dir removed.

```
GALION_REAL=1 GAIA_NERVE_BIN=$T/release/openmind-nerve GAIA_FAULT_EVIDENCE=ev.txt \
  cargo test --release -p gaia --test real_gaia_faults -- --nocapture --test-threads 1
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 60.94s
```

## Evidence (final run)

```
[2026-10-21T13:42:02+02:00] F1 hang: 3 ticks in 7.380329996s: outcomes {"hung":3} restarts 0, tokens consumed 0 (3 kept), hung events 3 with listener_pids [1373852], probes ["read: operation would block","read: operation would block"], pid alive
[2026-10-21T13:42:03+02:00] F2 slow: ["--mode", "slow", "--delay-ms", "800"] -> exit 0 outcome "healthy" detail String("service is up; nothing to do") in 802.389944ms
[2026-10-21T13:42:05+02:00] F2 slow: ["--mode", "slow", "--delay-ms", "1400"] -> exit 8 outcome "hung" detail String("fi-slow on 127.0.0.1:19891 holds its port but does not answer (read: operation would block); reported to the nerve, not restarted or killed (D-L4)") in 2.355003911s
[2026-10-21T13:42:06+02:00] F2 slow: ["--mode", "slow", "--delay-ms", "1400", "--slow-count", "1"] -> exit 0 outcome "healthy" detail String("slow answer, up on the confirming probe; nothing to do") in 1.307098621s
[2026-10-21T13:42:07+02:00] F3 badjson: --bad truncated: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok (body is not JSON)"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad nested: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad text: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok (body is not JSON)"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad html: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok (body is not JSON)"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad spaced: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad array: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad empty: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok (body is not JSON)"]
[2026-10-21T13:42:07+02:00] F3 badjson: --bad okfalse: exit 4 (dry-run refusal, no token) probe state "down" notes ["GET :19892/health -> HTTP 200, status not ok"]
[2026-10-21T13:42:16+02:00] F3 badjson: tokens 3, loop 5 ticks: outcomes {"apply-failed":3,"capped":2} -> global ledger 3, consumed 3, holder pid 1374306 alive and sole listener, 3 spawned copies failed to bind (finding D-FI2: tokens + budget burnt on a held port)
[2026-10-21T13:42:38+02:00] F4 flap: 2 loops x 22 s, 22.038313643s: restarts a 1 + b 2 = 3; global ledger ["2026-10-21T13:42:16+02:00", "2026-10-21T13:42:16+02:00", "2026-10-21T13:42:17+02:00"]; capped 108 (token burnt in lock race 0); consumed 3; outcomes a {"capped":53,"healthy":3,"restarted":1} b {"capped":55,"healthy":2,"restarted":2}
[2026-10-21T13:42:47+02:00] F5 stolen: thief pid 1375108 (503) on 19894: outcomes {"apply-failed":3,"capped":1} global ledger 3, thief alive + sole listener, failed events 3 (health_verified false)
[2026-10-21T13:42:49+02:00] F5 stolen: hanging thief pid 1375157 on 19895: exit 8 hung, listener_pids [1375157], gaia_started_pids [] (foreign), alive
[2026-10-21T13:42:52+02:00] F6 nerve: real nerve killed; 3 error ticks in 1.200998394s (first: String("nerve at 127.0.0.1:20591 is not healthy"); consecutive [Number(1), Number(2), Number(3)]); port 19896 stayed closed, global ledger 0; nerve back -> restarted after 1.201675248s; loop ticks 6 restarts 1 errors_total 3 sleeps_ms [300,300,300,600,1200,300]
[2026-10-21T13:42:53+02:00] F6 nerve: nerve stays down: loop exit 7 stop max-errors after 302.125649ms, ticks 3, restarts 0, last_error String("nerve at 127.0.0.1:20591 is not healthy")
[2026-10-21T13:42:53+02:00] F7 nerve-fault: (a) /health 503 -> exit 2 "gaia: nerve at 127.0.0.1:20590 is not healthy"; consumed 0, nothing posted for the tick, not started
[2026-10-21T13:42:53+02:00] F7 nerve-fault: (b) /events without the probe -> exit 2 "gaia: nerve did not return the probe event fi-nfault-1792582973-1375275"; consumed 0
[2026-10-21T13:42:53+02:00] F7 nerve-fault: (c) /state 500 -> exit 2 "gaia: nerve restart count (D-L2): nerve GET /state: HTTP 500"; consumed 0 (token kept)
[2026-10-21T13:42:53+02:00] F7 nerve-fault: (d) /state 500 only under the lock -> exit 2, no spawn, global ledger 0, but token consumed 1 (finding D-FI3)
[2026-10-21T13:42:53+02:00] F7 nerve-fault: (e) POST gaia.heal.approved 500 -> exit 2 "gaia: nerve POST /event: HTTP 500: {\"error\":\"injected POST fault\"}"; consumed 2, no spawn
[2026-10-21T13:42:53+02:00] F7 nerve-fault: (f) POST gaia.heal.applied 500 -> exit 2 "gaia: nerve POST /event: HTTP 500: {\"error\":\"injected POST fault\"}" but service up and global ledger 1
[2026-10-21T13:42:53+02:00] F7 nerve-fault: next tick counts the unreported restart: cap step gaia_restarts_last_hour_before 1 -> restart 2/3
[2026-10-21T13:42:55+02:00] F8 dst: fake clock ticks ["2026-10-25T02:30:00+02:00 exit 0", "2026-10-25T02:50:00+02:00 exit 0", "2026-10-25T02:10:00+01:00 exit 0", "2026-10-25T02:20:00+01:00 exit 5", "2026-10-25T02:30:00+01:00 expired token (exp 2026-10-25T02:15:00+01:00) exit 4 code expired", "2026-10-25T02:30:00+01:00 ttl-3600 token exit 4 code lifetime (gate max_ttl_secs 900)", "2026-10-25T02:30:00+01:00 exit 0 (restart, window 00:50Z..01:30Z)", "2026-10-25T02:31:00+01:00 exit 5 capped retry 1140 s"]; global ledger ts ["2026-10-25T02:30:00+02:00", "2026-10-25T02:50:00+02:00", "2026-10-25T02:10:00+01:00", "2026-10-25T02:30:00+01:00"] (02:30 twice, +02:00 and +01:00); hades consumed ts ["2026-10-25T02:30:00+02:00", "2026-10-25T02:50:00+02:00", "2026-10-25T02:10:00+01:00", "2026-10-25T02:30:00+01:00"]
[2026-10-21T13:42:55+02:00] F9 ledger: (a) torn global ledger line -> exit 2 "gaia: ledger: $TMP/gate2/gaia-restarts.jsonl: EOF while parsing an object at line 1 column 34"; not started, consumed 0
[2026-10-21T13:42:55+02:00] F9 ledger: (b) corrupt --state ledger -> exit 2 "gaia: ledger: $TMP/state-ledger-b/restarts.jsonl: expected ident at line 1 column 2"; not started, consumed 0
[2026-10-21T13:42:55+02:00] F9 ledger: (c) gate DISABLED file -> exit 4 code "disabled"; not started
[2026-10-21T13:42:55+02:00] F9 ledger: (d) missing hades config -> exit 4 "hades-gate unavailable, failing closed: $TMP/nope.toml: entity not found"
[2026-10-21T13:42:55+02:00] F9 ledger: (e) same token on the intact gate -> exit 0 restarted (none of (a)-(d) consumed it)
[2026-10-21T13:42:55+02:00] teardown: 0 dummies, 19890-19899 + 20591 closed (20590 is this test's thread); fake nerve 335 events, 160 /state GETs; no token text in events
```

## VPS (read-only, nothing changed)

- 13:24 +02:00: `GET /health` 8420 `{"status":"ok",...}` (openmind-memory),
  8642 `{"status":"ok","platform":"hermes-agent",...}` (hermes-rs), 8650
  `{"ok":true}`; all `200`, `Content-Length` (no chunking), so the strict
  JSON rule (D-FI1) reads all three as up (bodies pinned in
  `gaia-live/tests/json_strict.rs`). The body capture used a temp file
  `/tmp/.gfi-h` on the VPS, removed in the same command (checked absent 13:39).
- 13:24 and 13:38 +02:00: nerve `GET :8670/state`: mode live, cycle 443 / 470,
  score 61/D. Cortex is not live, so the nerve is the observation point.
- 13:38 +02:00: `gaia nerve-restarts --state-file <live /state capture>` for
  hermes-rs, openmind-memory, galion-one, cloudflared: nerve count 0 for each,
  `left_for_gaia` 3 (D-L2 parser works on today's live format).
- 13:38 +02:00: last 500 nerve events have no `gaia.*` kind, and there is no
  `gaia*` systemd unit (D-L1: nothing installed).
- 13:39 +02:00: `ss -ltn` shows 127.0.0.1:19120 (POSEIDON) listening inside
  Gaia's `TEST_PORTS` 19000-19999 (D-FI4).
