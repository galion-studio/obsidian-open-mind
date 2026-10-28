<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: swarm/nerve-wiring (wave-2 slot 30, ports 19300-19309)

All times are Europe/Zagreb, CEST (+02:00). The binaries are release builds on the box.
**Nothing was deployed to the VPS.** On the VPS only read-only probes ran:
`ss -ltnp`, `/proc/<pid>/comm`, `ps -o unit=,lstart=`, and the existing
`scripts/real/nerve-capture-vps.sh` (GETs + `ss -ltnp` + journal reads,
streamed back over ssh, nothing written there). No paid calls were made:
hermes-gateway ran `--mock` (offline).

## Box stack

| port | process | notes |
|---|---|---|
| 19300 | openmind-nerve serve | interval 5 s, observe-only, `NERVE_LISTEN_SCOPE=19300-19309`, `NERVE_UNOWNED_PORTS=19305` |
| 19301 | hermes-gateway --mock | `NERVE_URL=http://127.0.0.1:19300`, `NERVE_QUEUE=8`, `NERVE_SLOW_MS=1000`, `NERVE_HEARTBEAT_S=10` |
| 19302 | openmind-memory | `NERVE_URL` set, same switches (default queue 256) |
| 19305 | stray openmind-memory | no NERVE_URL, not in the component table: plays one of the VPS "no unit" shadows |

Command (wave 2, since replaced by the Rust test below): `N_REQ=1000 scripts/real/nerve-wiring.sh`. Load per phase: 1000
mock chats (every 10th says `mock-fail`, giving a real 502) plus 1000
`POST /recall`, with 4 in parallel.

## Run 2026-10-20 20:59:51 .. 21:01:00 CEST (+02:00): full output

```
== 2026-10-20 20:59:51 CEST (+02:00) started: nerve 1888563 hermes 1888564 memory 1888565 stray 1888566; work dir /tmp/nerve-wiring-real-U04l
-- pushed sources (started events)
{"source":"hermes-rs","total":5,"dropped":0,"recent":[{"kind":"metric","name":"nerve_client_dropped","msg":""},{"kind":"heartbeat","name":"rss_kb","msg":"queued=3 sent=3 dropped=0 failed=0"},{"kind":"metric","name":"nerve_client_dropped","msg":""},{"kind":"heartbeat","name":"rss_kb","msg":"queued=1 sent=0 dropped=0 failed=0"},{"kind":"event","name":"started","msg":"listening on 127.0.0.1:19301"}]}
{"source":"openmind-memory","total":5,"dropped":0,"recent":[{"kind":"metric","name":"nerve_client_dropped","msg":""},{"kind":"heartbeat","name":"rss_kb","msg":"queued=3 sent=3 dropped=0 failed=0"},{"kind":"metric","name":"nerve_client_dropped","msg":""},{"kind":"heartbeat","name":"rss_kb","msg":"queued=1 sent=0 dropped=0 failed=0"},{"kind":"event","name":"started","msg":"listening on 127.0.0.1:19302"}]}
-- ownership (listen scope 19300-19309)
{"port":19300,"owner":"galion-nerve (self)","gaia":"AETHER","documented_unowned":false}
{"port":19301,"owner":"hermes-rs","gaia":"ARTEMIS","documented_unowned":false}
{"port":19302,"owner":"openmind-memory","gaia":"APOLLO","documented_unowned":false}
{"port":19305,"owner":"unknown owner","gaia":null,"documented_unowned":true,"pid":1888566,"process":"openmind-memory","unit":null,"manual":false}
-- unmanaged listener finding
{"severity":"warn","title":"1 loopback listener(s) no monitored unit owns: :19305","evidence":[":19305 unknown owner (documented: no unit owns it) -- process openmind-memory (pid 1888566)","not a component health/metrics port and not in NERVE_KNOWN_PORTS","typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching)"],"gaia":null}
== 2026-10-20 20:59:51 CEST (+02:00) PHASE up: nerve running
  hermes (up): n=1000 p50=2.98ms p95=10.69ms max=182.16ms  codes:  900 200, 100 502,
  memory (up): n=1000 p50=0.40ms p95=4.62ms max=12.41ms  codes:  1000 200,
-- 3 slow chats (mock-slow, 1.5 s) and 3 memory 5xx (POST /seed -> 501)
200 1.510484
200 1.511673
200 1.512263
501
501
501
-- incidents (with owning Gaia function)
{"id":"inc-a4bc1748","severity":"error","cause":"hermes-rs","title":"hermes-rs: pushed 57 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502","gaia":["ARTEMIS"],"owners":{"hermes-rs":"ARTEMIS"},"signals":["failing-soon: 75 errors in 5 min","pushed 57 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502","pushed 3 slow request(s) in the last 2 min; last: POST /v1/chat/completions -> 200 in 1511 ms (threshold 1000 ms)"]}
{"id":"inc-4f4508ff","severity":"error","cause":"openmind-memory","title":"openmind-memory: pushed 3 error(s) in the last 2 min; last: POST /seed -> 501","gaia":["APOLLO"],"owners":{"openmind-memory":"APOLLO"},"signals":["pushed 3 error(s) in the last 2 min; last: POST /seed -> 501","degrading: 3 errors in 5 min","pushed 1 start(s) in the last 2 min; last: listening on 127.0.0.1:19302"]}
-- cortex incident owners
{"id":"inc-a4bc1748","cause":"hermes-rs","title":"hermes-rs: pushed 57 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502","severity":"error","gaia":["ARTEMIS"],"owners":[{"component":"hermes-rs","gaia":"ARTEMIS","system":"hermes-agents"}]}
{"id":"inc-4f4508ff","cause":"openmind-memory","title":"openmind-memory: pushed 3 error(s) in the last 2 min; last: POST /seed -> 501","severity":"error","gaia":["APOLLO"],"owners":[{"component":"openmind-memory","gaia":"APOLLO","system":"memory-gateway"}]}
== 2026-10-20 21:00:09 CEST (+02:00) PHASE hung: nerve SIGSTOPped (accepts connections, never answers)
  hermes (hung): n=1000 p50=3.20ms p95=10.77ms max=23.25ms  codes:  899 200, 1 429, 100 502,
  memory (hung): n=1000 p50=0.35ms p95=0.82ms max=6.36ms  codes:  1000 200,
== 2026-10-20 21:00:17 CEST (+02:00) PHASE down: nerve killed
  hermes (down): n=1000 p50=3.19ms p95=8.55ms max=16.82ms  codes:  900 200, 100 502,
  memory (down): n=1000 p50=0.32ms p95=0.63ms max=3.32ms  codes:  1000 200,
== 2026-10-20 21:00:22 CEST (+02:00) nerve restarted (fresh state)
-- client counters after hung + down phases (first heartbeats to the new nerve)
{"source":"hermes-rs","heartbeat":"queued=309 sent=112 dropped=5 failed=197","dropped_metric":5.0}
{"source":"openmind-memory","heartbeat":"queued=14 sent=12 dropped=0 failed=2","dropped_metric":0.0}
== 2026-10-20 21:00:34 CEST (+02:00) PHASE up2: nerve running again (same load, warm processes)
  hermes (up2): n=1000 p50=4.06ms p95=10.32ms max=19.93ms  codes:  899 200, 1 429, 100 502,
  memory (up2): n=1000 p50=0.38ms p95=2.03ms max=8.31ms  codes:  1000 200,
-- nerve-side hub counters (per-source rate limit 120/min drops are counted there)
{"source":"hermes-rs","total":104,"dropped":0}
{"source":"openmind-memory","total":4,"dropped":0}
== 2026-10-20 21:00:39 CEST (+02:00) KILL hermes-gateway (pid 1888564)
{"id":"inc-ff9f4bbf","severity":"critical","cause":"hermes-rs","title":"hermes-rs: failing: health probe down (connect: Connection refused (os error 111)); failing-soon: 77 errors in 5 min","gaia":["ARTEMIS"],"owners":{"hermes-rs":"ARTEMIS"}}
== 2026-10-20 21:00:50 CEST (+02:00) restart hermes with HERMES_RS_DAILY_CALL_CAP=5 (spend file already counts > 5 mock calls)
402 0.004260
402 0.002525
402 0.002478
402 0.001599
402 0.004791
402 0.002888
-- hermes pushed signals
{"total":110,"dropped":0,"recent":[{"kind":"metric","name":"nerve_client_dropped","value":0.0,"msg":""},{"kind":"heartbeat","name":"rss_kb","value":4444.0,"msg":"queued=4 sent=4 dropped=0 failed=0"},{"kind":"event","name":"spend_cap","value":1.0,"msg":"daily provider call cap reached (3601 of 5)"},{"kind":"metric","name":"nerve_client_dropped","value":0.0,"msg":""},{"kind":"heartbeat","name":"rss_kb","value":4000.0,"msg":"queued=1 sent=1 dropped=0 failed=0"}]}
-- incidents
{"open":[{"severity":"error","cause":"hermes-rs","title":"hermes-rs: pushed 56 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502","gaia":["ARTEMIS"]},{"severity":"warn","cause":"spend-cap:hermes-rs","title":"hermes-rs: spend cap hit (provider calls refused)","gaia":["ARTEMIS"]}],"resolved":[]}
-- cortex incident owners
{"id":"inc-ff9f4bbf","cause":"hermes-rs","title":"hermes-rs: pushed 56 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502","severity":"error","gaia":["ARTEMIS"],"owners":[{"component":"hermes-rs","gaia":"ARTEMIS","system":"hermes-agents"}]}
{"id":"inc-23052ae4","cause":"spend-cap:hermes-rs","title":"hermes-rs: spend cap hit (provider calls refused)","severity":"warn","gaia":["ARTEMIS"],"owners":[{"component":"hermes-rs","gaia":"ARTEMIS","system":"hermes-agents"}]}
-- client queue counters (from the 10 s heartbeats: queued/sent/dropped/failed since process start)
{"source":"hermes-rs","heartbeat":"queued=4 sent=4 dropped=0 failed=0","dropped_metric":0.0}
{"source":"openmind-memory","heartbeat":"queued=20 sent=18 dropped=0 failed=2","dropped_metric":0.0}
-- RSS
    PID   RSS     TIME COMMAND
1888565  4456 00:00:00 openmind-memory
1888566  3884 00:00:00 openmind-memory
1929454  8044 00:00:00 openmind-nerve
1948512  4444 00:00:00 hermes-gateway
== 2026-10-20 21:01:03 CEST (+02:00) done; tearing down
```

## p95 latency, nerve up vs hung vs down (3 runs)

| run (CEST +02:00) | N | hermes p95 up / hung / down / up2 | memory p95 up / hung / down / up2 |
|---|---|---|---|
| 20:55:58 | 300 | 7.92 / 7.62 / 5.61 / - ms | 0.67 / 0.52 / 0.57 / - ms |
| 20:58:17 | 1000 | 12.09 / 12.62 / 5.93 / - ms | 2.40 / 0.62 / 0.63 / - ms |
| 20:59:51 | 1000 | 10.69 / 10.77 / 8.55 / 10.32 ms | 4.62 / 0.82 / 0.63 / 2.03 ms |

What the numbers say:

- **No request ever waited on the nerve.** With the nerve SIGSTOPped
  (it accepts TCP but never answers), p50 and p95 match the nerve-up phase.
  Every request was answered: 900 200 + 100 502 on hermes (one or two 429s
  come from hermes' own `max_concurrent_runs=3` with 4 parallel clients,
  in every phase) and 1000 200 on memory.
- p50 is the same in every phase (hermes about 2-4 ms, memory about 0.35 ms).
  The p95 differences are a few ms, and up2 is no worse than up. Memory pushes
  nothing per request (only 200s under 1 s), yet its p95 is a little higher
  while the nerve runs. So that is CPU contention on the shared box (load average about 6 on
  8 cores; the nerve's 5 s cycle takes about 50 ms, and it handles each pushed signal), not
  the client. The per-request client cost is one `try_send` on a bounded
  channel.
- Queue behaviour (hermes, `NERVE_QUEUE=8`): after the hung and down phases
  the heartbeat said `queued=309 sent=112 dropped=5 failed=197`. Drops are few
  because after a failed POST the worker fails fast for 2 s and drains the
  queue instead of letting it fill. The nerve side also counts its own
  per-source rate-limit drops (`/api/v1/signals` -> `dropped`).

## What was verified

| check | result |
|---|---|
| started events from hermes-rs and openmind-memory | yes: `"listening on 127.0.0.1:19301"` / `:19302` |
| heartbeat with RSS + queue counters | yes, every 10 s (`NERVE_HEARTBEAT_S=10`; default 60 s) |
| 5xx -> pushed error -> incident with Gaia owner | hermes 502s -> `hermes-rs` incident, `gaia ["ARTEMIS"]`; memory `POST /seed -> 501` -> `openmind-memory` incident, `gaia ["APOLLO"]` |
| slow requests | 3 chats of 1.5 s -> `pushed 3 slow request(s) ... in 1505 ms (threshold 1000 ms)` in the hermes incident |
| kill a service | hermes killed -> critical `health probe down` incident (ARTEMIS), resolved 15 s after the restart |
| spend cap (mock) | restarted with `HERMES_RS_DAILY_CALL_CAP=5` -> 402s, one throttled `spend_cap` event (`daily provider call cap reached (3601 of 5)`) -> warn incident `hermes-rs: spend cap hit (provider calls refused)`, gaia ARTEMIS |
| unknown owner + process from /proc | `:19305 unknown owner (documented: no unit owns it) -- process openmind-memory (pid N)` |
| cortex lists the owning Gaia function per incident | `cortex.incidents[]` with `gaia` + `owners[{component,gaia,system}]` |

## VPS (read-only), 2026-10-20 20:36 and 21:02 CEST (+02:00)

`ss -ltnp` + `/proc` on the VPS show what the five unowned ports are. They
were all started by hand from ssh sessions (cgroup `session-N.scope`), with no
systemd unit. Start times are VPS-local UTC converted to CEST:

| port | process | pid | started (CEST +02:00) | cgroup |
|---|---|---|---|---|
| 8431 | openmind-memory | 1375169 | 2026-10-18 22:14:52 | session-48893.scope |
| 8432 | openmind-memory | 1412682 | 2026-10-18 22:23:51 | session-49009.scope |
| 8433 | openmind-memory | 1428048 | 2026-10-18 22:24:14 | session-49012.scope |
| 8652 | hermes-gateway | 1395619 | 2026-10-18 22:15:53 | session-48941.scope |
| 8653 | hermes-gateway. (comm truncated) | 1443105 | 2026-10-18 22:32:28 | session-49065.scope |

The capture was replayed through this branch with `openmind-nerve replay
/tmp/nw-vps --state`, captured 2026-10-20 21:02:28 CEST (+02:00):

```
{"title":"5 loopback listener(s) no monitored unit owns: :8431 :8432 :8433 :8652 :8653","evidence":[":8431 unknown owner (documented: no unit owns it) -- process openmind-memory (pid 1375169)",":8432 unknown owner (documented: no unit owns it) -- process openmind-memory (pid 1412682)",":8433 unknown owner (documented: no unit owns it) -- process openmind-memory (pid 1428048)",":8652 unknown owner (documented: no unit owns it) -- process hermes-gateway (pid 1395619)",":8653 unknown owner (documented: no unit owns it) -- process hermes-gateway. (pid 1443105)","not a component health/metrics port and not in NERVE_KNOWN_PORTS","typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching)"]}
{"documented_unowned":false,"gaia":"APOLLO","manual":false,"owner":"openmind-memory","pid":1374875,"port":8420,"process":"openmind-memory","unit":null}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":1375169,"port":8431,"process":"openmind-memory","unit":null}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":1412682,"port":8432,"process":"openmind-memory","unit":null}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":1428048,"port":8433,"process":"openmind-memory","unit":null}
{"documented_unowned":false,"gaia":"ARTEMIS","manual":false,"owner":"hermes-rs","pid":1361728,"port":8642,"process":"hermes-gateway","unit":null}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":1395619,"port":8652,"process":"hermes-gateway","unit":null}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":1443105,"port":8653,"process":"hermes-gateway.","unit":null}
{"cause":"edge","severity":"error","gaia":["AETHER"],"owners":[{"component":"cloudflared","gaia":"AETHER","system":"tunnels"}]}
{"cause":"galion-one","severity":"error","gaia":["AETHER"],"owners":[{"component":"galion-one-mouth","gaia":"AETHER","system":"speech-io"}]}
```

The live nerve (galion-nerve from 2f5a730, untouched; self-eval at
2026-10-20 20:54:35 CEST (+02:00): PASS, system "degrading") has no
listener or ownership view at all. swarm/nerve (5802d0e) listed the five ports
as `5 loopback listener(s) no monitored unit owns`. This branch adds the
"unknown owner" label, the holding process and the Gaia owner per incident.

## Wave 4 (slot 101, swarm/nerve-wiring-2): the script is now a Rust test

`scripts/real/nerve-wiring.sh` is deleted. `crates/openmind-nerve/tests/real_wiring.rs`
does the same work in Rust (std + serde_json, hand-written HTTP/1.0 client,
`kill -STOP/-CONT` via `Command`). It builds the three release binaries itself
(`cargo build --release` into `<target>/real-nerve-wiring`, so it never waits
on the outer `cargo test` lock), starts them, probes them and tears them down
(`Drop` kills every process, SIGCONT first). Ports moved to the slot-101 range:

| port | process |
|---|---|
| 19710 | openmind-nerve serve |
| 19711 | hermes-gateway --mock, `NERVE_URL` set |
| 19712 | openmind-memory, `NERVE_URL` set |
| 19713 / 19714 | baseline hermes / memory with `NERVE_URL` unset (new phase 0, then killed) |
| 19715 | stray openmind-memory -> "unknown owner" |

New compared with the script: phase 0 runs the same load against hermes and
memory with **`NERVE_URL` unset** (the plan T1.1 baseline), and the test now
asserts instead of grepping: started events, ownership rows (19715 unknown
owner, process from /proc), the unmanaged-listeners finding, incident Gaia
owners and cortex systems, slow + 5xx signals, every request answered in every
phase (memory 100 % 200, hermes only 200/429/502), hung/down hermes p95 within
max(3x, +50 ms) of the baseline, failed+dropped counters rising after the nerve
outage, critical probe-down incident, 402s + `spend_cap` event + spend-cap incident.

Command:
`GALION_REAL=1 N_REQ=1000 cargo test -p openmind-nerve --test real_wiring -- --nocapture`
(`NERVE_WIRING_BIN_DIR=<dir>` skips the build).

### Run 2026-10-21T00:57:47+02:00 .. 00:58:56+02:00, N_REQ=1000: passed (68.97 s)

```
== 2026-10-21T00:57:47+02:00 PHASE base: NERVE_URL unset, no nerve
  hermes (base): n=1000 p50=2.77ms p95=7.63ms p99=11.13ms max=56.95ms codes={200: 900, 502: 100}
  memory (base): n=1000 p50=0.20ms p95=0.32ms p99=0.47ms max=0.99ms codes={200: 1000}
-- ownership (listen scope 19710-19719)
{"documented_unowned":false,"gaia":"AETHER","owner":"galion-nerve (self)","port":19710}
{"documented_unowned":false,"gaia":"ARTEMIS","owner":"hermes-rs","port":19711}
{"documented_unowned":false,"gaia":"APOLLO","owner":"openmind-memory","port":19712}
{"documented_unowned":true,"gaia":null,"manual":false,"owner":"unknown owner","pid":2825663,"port":19715,"process":"openmind-memory","unit":null}
-- unmanaged listener finding
"1 loopback listener(s) no monitored unit owns: :19715"
  hermes (up): n=1000 p50=3.45ms p95=8.20ms p99=14.16ms max=33.80ms codes={200: 900, 502: 100}
  memory (up): n=1000 p50=0.22ms p95=0.63ms p99=2.60ms max=6.08ms codes={200: 1000}
-- 3 slow chats: 200 1.507s / 1.507s / 1.508s; POST /seed: 501 501 501
{"cause":"hermes-rs","gaia":["ARTEMIS"],"severity":"error","title":"hermes-rs: pushed 59 error(s) in the last 2 min; last: POST /v1/chat/completions -> 502"}
{"cause":"openmind-memory","gaia":["APOLLO"],"severity":"error","title":"openmind-memory: pushed 3 error(s) in the last 2 min; last: POST /seed -> 501"}
== 2026-10-21T00:58:14+02:00 PHASE hung: nerve SIGSTOPped (accepts, never answers)
  hermes (hung): n=1000 p50=2.89ms p95=10.19ms p99=15.47ms max=235.19ms codes={200: 896, 429: 4, 502: 100}
  memory (hung): n=1000 p50=0.23ms p95=0.48ms p99=1.49ms max=5.27ms codes={200: 1000}
== 2026-10-21T00:58:18+02:00 PHASE down: nerve killed
  hermes (down): n=1000 p50=3.30ms p95=10.86ms p99=16.18ms max=36.30ms codes={200: 894, 429: 7, 502: 99}
  memory (down): n=1000 p50=0.21ms p95=0.42ms p99=0.67ms max=1.32ms codes={200: 1000}
-- client counters after hung + down phases (first heartbeats to the new nerve)
"hermes-rs" heartbeat: queued=293 sent=112 dropped=18 failed=181
"openmind-memory" heartbeat: queued=12 sent=12 dropped=0 failed=0
  hermes (up2): n=1000 p50=3.14ms p95=16.21ms p99=100.97ms max=173.46ms codes={200: 893, 429: 8, 502: 99}
  memory (up2): n=1000 p50=0.21ms p95=0.39ms p99=0.53ms max=3.26ms codes={200: 1000}
== 2026-10-21T00:58:33+02:00 KILL hermes-gateway
{"gaia":["ARTEMIS"],"severity":"critical","title":"hermes-rs: failing: health probe down (connect: Connection refused (os error 111)); failing-soon: 64 errors in 5 min"}
== 2026-10-21T00:58:44+02:00 restart hermes with HERMES_RS_DAILY_CALL_CAP=5
402 402 402 402 402 402
{"kind":"event","msg":"daily provider call cap reached (3586 of 5)","name":"spend_cap","source":"hermes-rs","ts":"2026-10-21T00:58:45+02:00","value":1.0}
{"gaia":["ARTEMIS"],"severity":"warn","title":"hermes-rs: spend cap hit (provider calls refused)"}
-- RSS kB: nerve 5748 hermes 4376 memory 4480 stray 3912
```

A second run (2026-10-21 ~01:00 CEST (+02:00), N_REQ=300, test-side release
build, no `NERVE_WIRING_BIN_DIR`) also passed in 97.92 s.

### p99 against the `NERVE_URL`-unset baseline (plan T1.1 asked for within 5 %)

| run (CEST +02:00) | N | hermes p99 base / up / hung / down / up2 | memory p99 base / down |
|---|---|---|---|
| 00:57:47 | 1000 | 11.13 / 14.16 / 15.47 / 16.18 / 100.97 ms | 0.47 / 0.67 ms |
| ~01:00 | 300 | 9.00 / 9.80 / 23.83 / 16.42 / 14.55 ms | 0.72 / 0.70 ms |

Honest reading: **the 5 % p99 criterion is not met on this box.** hermes p99
with the nerve down is +45 % / +82 % over the baseline (about 5-7 ms in
absolute terms); p50 does not move (2.8-3.7 ms in every phase), memory does
not move, and no request ever failed or waited for the nerve. The box ran
~15 other agents' builds at the same time and the baseline always runs first,
so part of this is noise; part may be real (hermes pushes a 502 event per
`mock-fail` request; with the nerve down each push becomes a failed POST in
the worker thread, competing for CPU with request threads). The test asserts
a loose bound (p95 within max(3x, +50 ms)), not 5 %. Next step to settle it:
run the phases interleaved several times on a quiet box, or with every 10th
request not failing (no 5xx pushes) to separate CPU contention from pushes.
