<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: openmind-nerve (swarm/nerve, slot 10)

All times are Europe/Zagreb, CEST (+02:00). The outputs contain no secrets.

## 1. Read-only capture of the live VPS nerve (galion-nerve, 8670)

The capture used `scripts/real/nerve-capture-vps.sh` over ssh. It made only GETs
to 127.0.0.1 (`curl -s 8670/state` ×3, 90 s apart; `8670/self-eval(.md)`;
`8660/status.json`; `8002/health`), plus `ss -ltn` and `journalctl -o json`
(35 min) for the stack units. It started, stopped and wrote nothing on the VPS.

Deviation, noted for honesty: my very first manual probe used
`curl -o /tmp/.x` on the VPS. I deleted that file at once (`rm -f /tmp/.x`).
Every probe after that streamed to stdout.

Captured 2026-10-20 19:23:05 to 19:25:00 CEST. Health probes (`probes.txt`):

```
8091/health 200
8002/health 503
8642/health 200
8420/health 200
8646/health 200
8650/health 200
8080/ 302
8660/status.json 200
8670/ 200
8670/health 404
```

`8002/health` body: `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}`.

The cloudflared metrics read 82302 requests and 48814 errors (59%), with 4 HA connections.

`ss -ltn` showed loopback listeners that no unit owns: 8431, 8432, 8433, 8652, 8653.

The console journal shows Vite answering /.git/config, /.env and /phpinfo.php.

The live nerve's own verdict for the same moment:

```
- time: 2026-10-20T19:24:35+02:00 (Europe/Zagreb)
- cycle: 5380, mode: live
- result: **PASS** (0 error(s), 2 warning(s))
- system: **degrading**
```

In other words, the old nerve passes while public hostnames fail 59% of their requests.

## 2. Replay of the capture through this build

```
$ openmind-nerve replay /workspace/swarm/nerve-capture/replay1 --md   # exit 1 = FAIL
# openmind-nerve self-eval (galion-nerve)

- time: 2026-10-20T19:24:35+02:00 (Europe/Zagreb)
- cycle: 3, mode: observe-only
- result: **FAIL**, score **61/100 (grade D)**, 3 error(s), 5 warning(s)
- system: **degraded** (worst single component: failing-soon)
- verdict: degraded: 3 open incident(s) (0 critical, 2 error), 2 vulnerability finding(s), 3 bleeding point(s). [error] edge: tunnel routes 13 public hostname(s) to 10 closed origin port(s) :3004 :3005 :3010 :3011 :3020 :4000 :4001 :4003 :4020 :4060: 14.0 failed requests/min over 29 min | [error] galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"loaded":false,"model":"chatterbox","ok":false,"provider":"local","status":"loading"}] | [warn] scanner/exploit probes against console

## Score: what cost points

| points | what | why |
|---|---|---|
| -15 | error incident: edge: tunnel routes 13 public hostname(s) to 10 closed origin port(s) :3004 :3005 :3010 :3011 :3020 :4000 :4001 :4003 :4020 :4060: 14.0 failed requests/min over 29 min | 2 signal(s) from cloudflared correlated into one cause `edge`; open since 2026-10-20T19:23:05+02:00. public hostnames answer with Cloudflare error pages; every failed request is a visitor or bot turned away, and the error counters hide real faults |
| -15 | error incident: galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"loaded":false,"model":"chatterbox","ok":false,"provider":"local","status":"loading"}] | 1 signal(s) from galion-one-mouth correlated into one cause `galion-one`; open since 2026-10-20T19:23:05+02:00. 1 signal(s) in group galion-one (galion-one:healthy, galion-one-mouth:degrading): one cause, one incident |
| -5 | warn incident: scanner/exploit probes against console | 1 signal(s) from console correlated into one cause `scan:console`; open since 2026-10-20T19:23:05+02:00. 5 scanner request(s): /.env, /opt/galion/console-ide/.git/config, /phpinfo.php, /public/.env |
| -4 | vulnerability (warn): 5 loopback listener(s) no monitored unit owns: :8431 :8432 :8433 :8652 :8653 | not a component health/metrics port and not in NERVE_KNOWN_PORTS; typical cause: manual shadow/test processes left running (no unit, no restart policy, no patching) |

## Incidents (correlated)

### inc-6ada9e79 [error] edge: tunnel routes 13 public hostname(s) to 10 closed origin port(s) :3004 :3005 :3010 :3011 :3020 :4000 :4001 :4003 :4020 :4060: 14.0 failed requests/min over 29 min

- open since 2026-10-20T19:23:05+02:00 (3 cycle(s)), components: cloudflared
- why: public hostnames answer with Cloudflare error pages; every failed request is a visitor or bot turned away, and the error counters hide real faults
- next: start the services behind the refused ports, or remove their ingress rules/DNS records (cloudflared config + DNS: owner approval)
- signals:
  - error cloudflared: failing-soon: 194 errors in 5 min
  - error cloudflared: tunnel routes 13 public hostname(s) to 10 closed origin port(s) :3004 :3005 :3010 :3011 :3020 :4000 :4001 :4003 :4020 :4060: 14.0 failed requests/min over 29 min

### inc-31c5d0df [error] galion-one: galion-one-mouth degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"loaded":false,"model":"chatterbox","ok":false,"provider":"local","status":"loading"}]

- open since 2026-10-20T19:23:05+02:00 (3 cycle(s)), components: galion-one-mouth
- why: 1 signal(s) in group galion-one (galion-one:healthy, galion-one-mouth:degrading): one cause, one incident
- next: the dependency never finished loading: read its logs, check the model files, then restart that one service (owner approval)
- signals:
  - error galion-one-mouth: degrading: health probe HTTP 503 -- stuck loading for 1.9 d [health body {"loaded":false,"model":"chatterbox","ok":false,"provider":"local","status":"loading"}]

### inc-079c9e72 [warn] scanner/exploit probes against console

- open since 2026-10-20T19:23:05+02:00 (3 cycle(s)), components: console
- why: 5 scanner request(s): /.env, /opt/galion/console-ide/.git/config, /phpinfo.php, /public/.env
```

Why this verdict is correct:

- **Refused cloudflared upstreams.** The tunnel's ingress still routes 13 public
  hostnames to 10 loopback ports that nothing listens on. In the window, the top
  ports were :4001 (galion.studio/www, 180 refusals), :3011 (merlinelement.com,
  180) and :4020 (42). Cloudflared logs these as zerolog `ERR` lines at journald
  priority 6, so the old nerve saw 0 errors. One incident, `edge`, carries the
  evidence and the counter (59% failed).
- **Voice 8002 HTTP 503.** galion-one-mouth reports `status: loading` with
  `loaded: false` and has been 503 for 1.9 days. That is far past the 900 s grace,
  so it is reported as *stuck loading* (error), not as "still loading".
- **Scanner probes** hit the console, which runs a Vite dev server. This is an
  error vulnerability plus a warn incident. The **5 unmanaged listeners** are a warn.
- **DDoS: normal.** The edge-cloudflared stream ran at 0.2 req/s, but 93% of
  responses were 4xx/5xx. That is bleeding, not a flood.

## 3. Box run, `scripts/real/nerve.sh` (2026-10-20 19:27:11 to 19:32:30 CEST)

Setup: release `openmind-nerve serve` on 127.0.0.1:19100, real `tuner` on 19101, real `room` on
19102, and a dead "ghost" on 19103. The nerve also tails a simulated proxy
access log (`proxy=…/access.log@tuner`). Systemd and journald are off (the box has no systemd),
reflexes are off, and the listen scope is 19100-19109. Full output:
`/workspace/swarm/nerve-capture/box/run1.txt` on the box. Excerpt:

```
== 2026-10-20 19:27:11 CEST (+02:00) start: nerve pid 1114054, work dir /tmp/nerve-real-LOwq
-- GET /health
{"appended":0,"cycle":3,"score":70,"service":"openmind-nerve","status":"ok","system":"failing","ts":"2026-10-20T19:27:11+02:00","version":"0.1.0"}
-- system after 2 cycles
{"status":"failing","score":70,"grade":"C","pass":false,"open_incidents":2}
-- incidents
{"id":"inc-4d446ebe","severity":"critical","title":"ghost: failing: health probe down (connect: Connection refused (os error 111))"}
{"id":"inc-6a8068e1","severity":"warn","title":"lxc-host: degrading: disk 92% used"}
-- components
{"name":"tuner","status":"healthy","http":200,"reasons":[]}
{"name":"room","status":"healthy","http":200,"reasons":[]}
{"name":"ghost","status":"failing","http":null,"reasons":["failing: health probe down (connect: Connection refused (os error 111))"]}
{"name":"lxc-host","status":"degrading","http":null,"reasons":["degrading: disk 92% used"]}
{"name":"galion-nerve","status":"healthy","http":null,"reasons":[]}
== 2026-10-20 19:27:11 CEST (+02:00) kill tuner (pid 1114052)
{"id":"inc-4d446ebe","severity":"critical","title":"ghost: failing: health probe down (connect: Connection refused (os error 111))","components":["ghost"]}
{"id":"inc-6a8068e1","severity":"warn","title":"lxc-host: degrading: disk 92% used","components":["lxc-host"]}
== 2026-10-20 19:27:22 CEST (+02:00) restart tuner
{"open":["ghost: failing: health probe down (connect: Connection refused (os error 111))","lxc-host: degrading: disk 92% used"],"resolved":[{"title":"tuner: failing: health probe down (connect: Connection refused (os error 111))","resolved":"2026-10-20T19:27:29+02:00","duration_s":15}]}
== 2026-10-20 19:27:38 CEST (+02:00) scanner lines in the proxy access log
{"level":"normal","requests_60s":4,"scan_hits_60s":3,"top_ips":[{"ip":"203.0.113.9","requests":4}]}
== 2026-10-20 19:27:44 CEST (+02:00) warm-up: light polling for 200s to build a request baseline
{"level":"normal","rps":0.53,"baseline_rps":0.5,"baseline_buckets":11,"threshold_rps":10.0}
== 2026-10-20 19:31:05 CEST (+02:00) FLOOD: 6000 GETs, 8 parallel, against the nerve API
flood took 4109 ms
-- ddos verdict
{
  "level": "flood",
  "rps": 100.47,
  "requests_60s": 6028,
  "baseline_rps": 0.5,
  "threshold_rps": 10.0,
  "factor": 200.93,
  "shape": "single-source",
  "top_ips": [
    {
      "ip": "127.0.0.1",
      "requests": 6028
    }
  ],
  "flood_since": "2026-10-20T19:31:09+02:00",
  "explain": "nerve-api: 100.5 req/s over the last 60 s vs baseline 0.50 req/s (threshold 10.0, x201); single source 127.0.0.1 (6028 of 6028 requests); 0% 4xx/5xx"
}
-- system + incidents
{"status":"failing","score":32,"grade":"F","pass":false,"ddos_firing":true,"verdict":"failing: 3 open incident(s) (2 critical, 1 error), 0 vulnerability finding(s), 3 bleeding point(s). [critical] ghost: failing: health probe down (connect: Connection refused (os error 111)) | [critical] request flood on galion-nerve (stream nerve-api) | [error] lxc-host: degrading: disk 95% used; failing-soon: disk full in ~0.1 h at this rate"}
{"id":"inc-4d446ebe","severity":"critical","title":"ghost: failing: health probe down (connect: Connection refused (os error 111))","next":"read the unit journal; the reflex restarts allowlisted units within budget"}
{"id":"inc-1f744fab","severity":"critical","title":"request flood on galion-nerve (stream nerve-api)","next":"check the top source IPs; block or rate-limit them at the edge (Cloudflare WAF rule / rate limit) -- owner approval; the nerve never changes the edge"}
{"id":"inc-6a8068e1","severity":"error","title":"lxc-host: degrading: disk 95% used; failing-soon: disk full in ~0.1 h at this rate","next":"read the unit journal; the reflex restarts allowlisted units within budget"}
-- nerve CPU/RSS during the run
    PID   RSS     TIME     ELAPSED CMD
1114054  4744 00:00:01       04:16 /workspace/swarm/target/nerve/release/openmind-nerve serve
== 2026-10-20 19:31:15 CEST (+02:00) gaia-live compat
{"appended":1,"ok":true}
[{"data":{"ok":false,"port":19103},"kind":"probe","severity":"error","source":"gaia-live","ts":"2026-10-20T19:31:15+02:00"}]
== 2026-10-20 19:31:15 CEST (+02:00) quiet 75 s: flood ends, verdict returns to normal
{"level":"normal","rps":0.0,"floods_total":1,"peak_rps":100.55}
{"open":["ghost: failing: health probe down (connect: Connection refused (os error 111))","lxc-host: degrading: disk 95% used; failing-soon: disk full in ~0.2 h at this rate","galion-nerve: degrading: memory rising 61% over 5 min (249 KiB/min)"],"resolved":["request flood on galion-nerve (stream nerve-api)","galion-nerve: degrading: memory rising 30% over 4 min (115 KiB/min)","tuner: failing: health probe down (connect: Connection refused (os error 111))"]}
# openmind-nerve self-eval (galion-nerve)

- time: 2026-10-20T19:32:29+02:00 (Europe/Zagreb)
- cycle: 67, mode: observe-only
- result: **FAIL**, score **55/100 (grade D)**, 2 error(s), 4 warning(s)
- system: **failing** (worst single component: failing)
- verdict: failing: 3 open incident(s) (1 critical, 1 error), 0 vulnerability finding(s), 3 bleeding point(s). [critical] ghost: failing: health probe down (connect: Connection refused (os error 111)) | [error] lxc-host: degrading: disk 95% used; failing-soon: disk full in ~0.2 h at this rate | [warn] galion-nerve: degrading: memory rising 61% over 5 min (249 KiB/min)
## Score: what cost points
| points | what | why |
|---|---|---|
-- nerve log
<6>openmind-nerve 0.1.0 listening on 127.0.0.1:19100 interval=5s mode=observe-only data=/tmp/nerve-real-LOwq/data event-log=/tmp/nerve-real-LOwq/events.jsonl components=5
<2>galion-nerve ALERT crit ghost: failing: failing: health probe down (connect: Connection refused (os error 111))
<2>galion-nerve ALERT crit ghost: incident inc-4d446ebe [critical]: ghost: failing: health probe down (connect: Connection refused (os error 111))
<4>galion-nerve ALERT warn lxc-host: incident inc-6a8068e1 [warn]: lxc-host: degrading: disk 92% used
<4>galion-nerve self-eval FAIL: score 70 (C), 1 error(s), 2 warning(s), system "failing"
<6>openmind-nerve cycle 1 system=failing score=70 incidents=2 ddos=false cycle_ms=24
<2>galion-nerve ALERT crit tuner: failing: failing: health probe down (connect: Connection refused (os error 111))
<2>galion-nerve ALERT crit tuner: incident inc-15a0c624 [critical]: tuner: failing: health probe down (connect: Connection refused (os error 111))
<4>galion-nerve ALERT warn lxc-host: failing-soon: degrading: disk 92% used; failing-soon: disk full in ~0.3 h at this rate
<4>galion-nerve ALERT warn lxc-host: incident inc-6a8068e1 [error]: lxc-host: degrading: disk 92% used; failing-soon: disk full in ~0.3 h at this rate
<6>openmind-nerve cycle 11 system=failing score=60 incidents=2 ddos=false cycle_ms=25
<4>galion-nerve ALERT warn galion-nerve: incident inc-1cbcc091 [warn]: galion-nerve: degrading: memory rising 26% over 0 min (715 KiB/min)
<4>galion-nerve self-eval FAIL: score 55 (D), 2 error(s), 4 warning(s), system "failing"
<6>openmind-nerve cycle 21 system=failing score=55 incidents=3 ddos=false cycle_ms=24
<6>openmind-nerve cycle 31 system=failing score=55 incidents=3 ddos=false cycle_ms=23
<6>openmind-nerve cycle 41 system=failing score=55 incidents=3 ddos=false cycle_ms=24
<2>galion-nerve ALERT crit ddos:nerve-api: incident inc-1f744fab [critical]: request flood on galion-nerve (stream nerve-api)
<6>openmind-nerve cycle 51 system=failing score=30 incidents=4 ddos=true cycle_ms=37
<4>galion-nerve self-eval FAIL: score 32 (F), 4 error(s), 3 warning(s), system "failing"
<6>openmind-nerve cycle 61 system=failing score=32 incidents=3 ddos=true cycle_ms=32
<4>galion-nerve ALERT warn galion-nerve: incident inc-b448a90a [warn]: galion-nerve: degrading: memory rising 61% over 5 min (242 KiB/min)
== 2026-10-20 19:32:30 CEST (+02:00) done; tearing down
```

Results:

- Killing tuner opened a critical incident. Restarting it resolved the incident
  15 s later.
- Scanner lines in the proxy log were detected.
- A 200 s warm-up built a 0.5 req/s baseline.
- The **flood** was 6000 GETs with 8 in parallel over 4.1 s. It was detected as
  `flood`, at 100.5 req/s against a 10 req/s threshold (×201), shape
  `single-source` 127.0.0.1. Incident `request flood on galion-nerve (stream
  nerve-api)` was critical, and the score dropped to 32/F.
- Once quiet, the verdict returned to `normal` and the flood incident resolved.
- The nerve process stayed small through the run: RSS 4.7 MiB and 1 s of CPU in
  4 min 16 s, including the flood. A cycle takes about 25 ms.
- The run surfaced a **real** box problem: lxc-host disk at 92%, then 95%,
  "full in ~0.1 h". The swarm's target dirs fill `/` (125G, 6.8G free at
  19:33 CEST).
- After the run, the nerve flagged its own RSS growth (+1.6 MiB) as "memory
  rising". That was noise. It is fixed: a rise now also needs 8 MiB or more of
  absolute growth (test `tiny_rss_growth_is_not_a_leak`).

## 4. Gated Rust test

```
$ GALION_REAL=1 cargo test -p openmind-nerve --test real_box     # 2026-10-20 19:36:21 CEST
test real_nerve_on_the_box ... ok
test result: ok. 1 passed; 0 failed; ... finished in 12.23s
```

This test spawns the real binary on 19105, with a responder on 19106 and a dead
port on 19107. It checks the healthy/failing states and the critical incident.
It then floods 800 requests and asserts `flood`, `firing`, `single-source` and
the `ddos:nerve-api` incident.

## 5. Cross targets (2026-10-20 19:33 CEST)

- `x86_64-unknown-linux-musl` release: static-pie, 1114912 bytes.
- `aarch64-unknown-linux-musl` release: links with `rust-lld`
  (`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=…/rust-lld`; the host `ld`
  rejects `--fix-cortex-a53-843419`). A smoke test under
  `qemu-aarch64-static` on 19108 worked: `GET /health`, `POST /event` →
  `{"appended":1,"ok":true}`, `GET /events?n=1`, `GET /api/v1/state`.
- `cargo check` for `aarch64-linux-android` and `armv7-unknown-linux-musleabihf`: OK.

After every run, nothing is left listening on 19100-19199.

Note: the box run and the qemu smoke test printed `{"appended":1,"ok":true}`
because serde_json sorts keys. Under `cargo test --workspace`, another crate
switches on serde_json `preserve_order`, so the key order changed and the
test failed. The `/event` reply is now a fixed string, `{"ok":true,"appended":n}`,
byte-identical to gaia-live's.
