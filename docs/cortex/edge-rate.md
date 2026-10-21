<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Per-hostname edge rate (cortex T2.12)

- Written: 2026-10-21 01:25 CEST (+02:00), branch `swarm/cortex-t212-edgerate`, base `swarm/cortex-sensor` e536446.
- Plan: /workspace/swarm/plans/cortex.md T2.12 ("per-hostname edge request rate as DDoS streams, cloudflared journal `dest=`, access.rs detector reuse").
- Code: `crates/openmind-nerve/src/sensors/edgerate.rs` (parser, feeder, sensor), hook in `src/engine.rs`, signal hand-off in `src/sensors/ddos.rs`.

## Why

cloudflared's metrics give one `total_requests` counter per tunnel (stream `edge-cloudflared`). A flood against one
public hostname hides inside that total, and the counter says nothing about *which* hostname is hit. The cloudflared
journal names the hostname of every failed request, so the nerve can split the edge per hostname with data it already
collects (no new command, no new listener, no host read).

## What cloudflared logs (checked against the live VPS, read-only, 2026-10-21 01:10 CEST)

| line | when | counted as |
|---|---|---|
| `ERR Request failed error="..." connIndex=1 dest=https://<host>/<path> event=0 ip=<edge> type=http` | every failed request, at every log level | 1 request + 1 error for `<host>` |
| `ERR  error="Unable to reach the origin service ..." ... originService=http://localhost:<port>` | the same failure, second line | not counted (no hostname; would double count) |
| `DBG GET https://<host>/<path> HTTP/1.1 ... host=<host> ...` (`proxy/logger.go` `logHTTPRequest`) | every request, only with `--loglevel debug` | 1 request for `<host>`; while debug lines are seen, `Request failed` adds an error only |
| `DBG 200 OK ...` (origin response) | debug only | not counted (no hostname) |

VPS, last hour before 01:10 CEST: 1036 cloudflared lines, 518 of them `Request failed ... dest=` (2 lines per failure),
top hostnames merlinelement.com 267, galion.studio 237, then 1-3 each for galion.app, www.maivirtue.com,
www.galion.studio, www.galion.app, slaviknomad.com, ws3d.app, workspace3d.app, maivirtue.com. No debug lines: the VPS
runs cloudflared at info level.

**Consequence:** on the VPS today the per-host streams measure the *failed*-request rate per hostname (error share
100 %). That is still the right flood signal while 21 ingress rules point at closed ports (tunnel-audit): a flood
against a dead hostname shows up per host. Successful-request rates per host need `--loglevel debug` (owner decision 1).

## Data flow

```
journalctl (collect.rs, already)  ->  Collected.lines
engine.rs cycle_with, detector locked:
    EdgeHosts::feed(lines, now, detector)   # parse, dedup, cap, record_count(t, n, errors)
    detector.evaluate(now)                  # access.rs median/MAD per stream, unchanged
sensors:
    ddos      lists every stream (edge-host:* included), but signals only the non-host streams
    edge-rate edge-host:* streams that are elevated/flood -> 1 finding `edge-host-rate` (kind ddos)
              + 1 Signal, cause `ddos:edge-cloudflared` (same cause as the tunnel counter stream)
```

- Streams: `edge-host:<host>`, component `cloudflared`, counter-only (`record_count`): no client IPs are kept, shape
  is `unknown (counter only)`.
- One cause for the whole edge: a flood seen by the tunnel counter and by a host stream folds into one incident
  (`request flood on cloudflared (stream edge-host:*, per public hostname)`), no double scoring.
- Severity: any host stream `flood` -> critical; only `elevated` -> warn (same as the `ddos` sensor).
- Thresholds: the global `NERVE_DDOS_MIN_RPS` (10) / `NERVE_DDOS_MIN_REQUESTS` (300). At today's VPS rates (about
  0.07 failed req/s per busy host) host streams stay `normal`; 40 req/s on one host floods (real test).

## Safety and cost

- Hostnames are request-controlled under wildcard DNS: at most `NERVE_EDGE_HOSTS_MAX` (default 32, 1..=256) host
  streams; later hostnames share `edge-host:other`. Each stream holds at most 720 10-second buckets (2 h).
- Hostname validation: lower-case DNS names only (`[a-z0-9.-]`, a dot, at least one letter, <= 253 chars); IP
  literals and junk are dropped.
- Titles, incident titles and B33 events carry counts only; hostnames stay in finding evidence and in the loopback
  `ddos.streams` list (B41 rule, tested).
- journalctl `--since` is inclusive: lines in the boundary second arrive twice; the feeder remembers the hashes of
  the newest second and skips repeats. (A truly identical line repeated in that same second is undercounted by one.)
- Cost: 3096 cloudflared lines through a full replay in 30 ms (release, box); the sensor itself ran in < 1 us in serve.
- Read-only: no new command, no network, no listener, no reflex. `live` is not needed (journal lines are cycle input),
  so replays of captures see the same streams.

## Settings

| env | default | effect |
|---|---|---|
| `NERVE_EDGE_HOSTS` | on | `0`/`off`/`false`/`no`: no feeder, no sensor |
| `NERVE_SENSORS_OFF=edge-rate` | - | same as above (the engine feeds only when the sensor is registered) |
| `NERVE_EDGE_HOSTS_MAX` | 32 | cap on distinct host streams (clamped 1..=256) |

`/api/v1/state` gains `edge_hosts: {hosts, max_hosts, lines_counted, folded_into_other}` (null when off).

## Shared edits (for the T2.0 merger)

- `src/engine.rs`: one field (`edge_hosts`), `Sensors::from_env` moved into a local so the feeder can follow it, the
  feed call before `detector.evaluate`, `edge_hosts` in the state JSON, and the incident explain text for the
  per-hostname case.
- `src/sensors/ddos.rs`: `edge-host:*` streams are listed but not signalled (5 lines).
- `src/sensors/mod.rs`: `pub mod edgerate;` + `s.add_opt(edgerate::EdgeRate::from_env(env));`; two registry tests now
  expect `edge-rate` after the built-ins.
- Tests: `tests/cortex_replay.rs` and `tests/real_cortex_sensor.rs` strip the additive `edge-host:*` streams before
  comparing with the pre-trait golden (`support::drop_edge_hosts`, `drop_edge_hosts_md`), so the golden files are
  unchanged; the sensor-name lists include `edge-rate`.

## Owner decisions (not decided here)

1. **cloudflared `--loglevel debug`** to get full per-host request rates (not only failures). Options: (a) keep info
   (today: failed-request rate per host only; zero risk); (b) debug on the named tunnel: full rates, but the journal
   grows a lot (each request logs headers, which include visitor IPs via `Cf-Connecting-Ip`) and journald disk/IO
   rise; needs a cloudflared reconfigure + restart (RULES.md 4: owner only); (c) Cloudflare analytics API (T2.16,
   needs a read-only token: owner). Recommendation: (a) now, (c) later; (b) only briefly during an incident.
2. **Per-host thresholds**: the host streams use the global `NERVE_DDOS_MIN_RPS=10`. A lower floor per host would catch
   smaller floods on small sites but also noise. Calibrate with T3.1 on real 300 s windows; tightening needs Slavik.
