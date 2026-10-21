<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve event log (D2), NERVE_URL (D3), first-cycle observation (D1) — 2026-10-21

Decision: Mate t184 (owner Slavik t175 permission), slot 180 follow-up to
swarm/nerve-t36-consumer 334beaa (docs/nerve-consumers.md). Times Europe/Zagreb (+02:00).
No secrets, no host addresses in this file.

## What actually runs (09:25 +02:00)

| port | unit | binary | NERVE_EVENT_LOG honored? | NERVE_URL honored? |
|---|---|---|---|---|
| 8670 | galion-nerve.service | /opt/galion/rust/bin/galion-nerve (816456 B, 2026-10-18, 2f5a730 line; sha256 3b150dea...) | **no** (string absent; no /event route) | accepts `POST /signal` (proven) |
| 8672 | galion-nerve-shadow.service | galion-nerve-shadow = openmind-nerve 4800770 | yes | n/a |
| 8642 | hermes-rs.service | hermes-gateway (2026-10-18) | n/a | **no** (no nerve-client / NERVE_ strings) |
| 8420 | openmind-memory.service | openmind-memory (2026-10-18) | n/a | **no** |

So D2 and D3 cannot take effect with today's binaries. Per the rule "don't swap
binaries", the drop-ins were installed as **staged config** (inert now, active
when the binaries are replaced by builds containing openmind-nerve / nerve-client).

## Sandbox (VPS, temp user, /var/tmp/sandbox-test-20261021072842, removed after)

Both nerve binaries copied, run as a non-root temp user, dry-run, reflex/rollback off,
no external alerts, `NERVE_EVENT_LOG` set; client = small Rust probe built on the
real `nerve-client` crate with `NERVE_URL` (sources hermes-rs, openmind-memory) plus
gaia-live style `POST /event`.

| | live galion-nerve binary | openmind-nerve (shadow build) |
|---|---|---|
| nerve-client signals | sent 5/5 per source, both sources in /signals | sent 5/5 per source, both in /signals |
| POST /event | 404 | 200 (`appended` 1..6) |
| GET /events | 404 | 200, 6 records |
| events.jsonl | not created | 6 lines, 690 B |
| t+4 ms | /healthz 200, / 200 1014 B (title galion-nerve), /state 200 `null` | /state `null`, /incidents /cortex /ddos 200 `null`, /events 200 `[]`, /health 200 |
| cycle 1 done | before t+1.1 s (cycle_ms 350) | before t+1.1 s (cycle_ms 374) |
| cycle 2 | t+30.5 s | t+30.5 s |

## D1: live restart of galion-nerve (09:31:08 +02:00), polled every second

```
09:31:06.851 cycle 7073, 15 comps, degrading 3 (galion-one, galion-one-mouth, cloudflared), / 3988 B, self-eval FAIL
09:31:08.053 systemctl restart issued
09:31:08.058 Stopping ; 08.061 Deactivated (Consumed 3min 32s CPU, 7.9M peak) ; 08.078 Started (new pid)
09:31:08.186 journal: listening on 127.0.0.1:8670 interval=30s mode=live
09:31:08.512 journal: self-eval PASS: 1 warning(s) ; cycle 1 system=degrading cycle_ms=326
09:31:08.998 first poll after restart: cycle 1, 15 comps, degrading 1 (galion-one-mouth only), / 3814 B
09:31:39.344 cycle 2 (cycle_ms 710) ; 09:32:47 cycle 4
throughout: /healthz 200, / 200 title galion-nerve, /health 404, /incidents /cortex /ddos /events 404, /signals 200
```

Observations:
- Listener gap ~125 ms (old process gone 08.061, new listening 08.186); `/state` is
  `null` only for ~330 ms (listen -> cycle 1); 1 s polling did not hit that window
  live (sandbox did: `null` at t+4 ms).
- The cycle counter restarts at 1 (`started` = restart time); it is not persisted.
- In-memory history is lost: restarts_1h of galion-one and cloudflared went 1 -> 0,
  both flipped degrading -> healthy and self-eval FAIL -> PASS; reflex live_1h counts reset.
  A restart therefore "forgets" the last hour (relevant for gaia nerve-restarts, which reads restarts_1h).
- Components 15 before and after; no unit restarts, NRestarts 0.

## Live changes

| unit | drop-in | backup of unit file | restarted? |
|---|---|---|---|
| galion-nerve | /etc/systemd/system/galion-nerve.service.d/60-nerve-eventlog.conf: `Environment=NERVE_EVENT_LOG=/var/lib/galion-nerve/events.jsonl` (default 8 MiB x 3 kept = max 32 MiB; path inside ReadWritePaths) | /var/backups/galion-nerve.service-20261021-093106 | yes (D1), env now in process, ignored by binary |
| hermes-rs | /etc/systemd/system/hermes-rs.service.d/60-nerve-url.conf: `Environment=NERVE_URL=http://127.0.0.1:8670` | /var/backups/hermes-rs.service-20261021-093233 | no (binary has no nerve-client; pid unchanged) |
| openmind-memory | /etc/systemd/system/openmind-memory.service.d/60-nerve-url.conf: same | /var/backups/openmind-memory.service-20261021-093233 | no (same) |

Verified 09:32:47: nerve cycle advancing, 15 comps; hermes-rs /health 200; memory /health 200;
galion.app, www, one.galion.app 200 (from the box); /events still 404 (expected, old binary).

## What is still needed

- D2 effective: replace galion-nerve binary with openmind-nerve (cutover). Then /event,
  /events, /health work; events.jsonl created at start (if it cannot be opened the
  nerve exits 1, so keep the path inside ReadWritePaths).
- D3 effective: deploy hermes-gateway / openmind-memory builds that include nerve-client
  (nerve-wiring line). A restart then picks up NERVE_URL; signals land in the live nerve
  (sandbox proved the live binary accepts them).

## Rollback

```
rm /etc/systemd/system/galion-nerve.service.d/60-nerve-eventlog.conf && systemctl daemon-reload && systemctl restart galion-nerve
rm /etc/systemd/system/hermes-rs.service.d/60-nerve-url.conf && systemctl daemon-reload   # + restart hermes-rs only if restarted since
rm /etc/systemd/system/openmind-memory.service.d/60-nerve-url.conf && systemctl daemon-reload   # same
# unit files themselves were not edited; backups in /var/backups/*-20261021-0931*/0932*
```
