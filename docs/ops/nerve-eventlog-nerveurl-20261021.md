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

## Follow-up 09:35-09:47 +02:00: D2 and D3 made effective (binary deploy)

Approval: Mate 09:35 +02:00 under Slavik t175 (sandbox first, keep only if it works).

### Build

- Source: this branch at **aa77864** = 334beaa (slot 180) + 364d266 (this doc) + cherry-picks of the
  slot-157 DST fix (swarm/nerve-dst-clock dc0a504 -> f529b55: offset-named self-eval files, so the
  02:xx fold hour on 2026-10-25 no longer overwrites 6 reports; 23dcf4c -> aa77864: repo-check markers).
  334beaa did **not** contain it (157 is on the cortex line, cortex-t20-merge ee75f75); the two
  commits cherry-picked cleanly and only touch openmind-nerve + docs. The rest of the cortex line was not merged.
- `cargo-slot -j 2`, `CARGO_TARGET_DIR=/workspace/swarm/target/nerve-eventlog-deploy` (deleted after),
  `--release --target x86_64-unknown-linux-musl` (static PIE), `GALION_BUILD_COMMIT=aa77864`.
  hermes-gateway built with `--no-default-features --features sqlite` (the live 2f7d48f build had no
  `coding` feature; keeps the hermes-tools code out of the binary).
- Tests on aa77864: openmind-nerve, nerve-client, openmind-memory, hermes-rs, hermes-gateway: 204 passed, 0 failed (26 suites,
  incl. dst_clock_replay 5/5 and consumer_compat).

| binary on VPS (/opt/galion/rust/bin) | sha256 | replaces (file left untouched) | old sha256 |
|---|---|---|---|
| openmind-nerve-aa77864 | f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68 | galion-nerve | 3b150dea692ffac098945e9413c4fe1caf95ee3a50d3f6c937e09df3598d6d3d |
| openmind-memory-aa77864 | 18e35ab49a76702b47742b8c5d4f95f29c3956839979e7df7a6ede671f1d2bb6 | openmind-memory | 473ae4a3b13ceabba007a89f7e3389df1c91fb6e7986bf2efd5890304ff12e37 |
| hermes-gateway-aa77864 | eb691b2967eb03758dcc6b2b6f46fe0a9d45d779b4d4bf937a28f37b19633e2f | hermes-gateway | cd344b167dc65425b63bcb3765f439ee70a0bf47f355126cac8a75f3192ac0f6 |

Box sha == VPS sha for all three.

### Nerve behaviour vs the old galion-nerve (checked in source)

Same env (NERVE_DRY_RUN=0 live, interval 30, FAIL_CYCLES 3, REPORT_EVERY 20; reflex + rollback default on,
as the old one). Same reflex model: `systemctl restart --no-block` on the same 10 allowlisted units, same
deny list, budgets 3/h (cloudflared 1/h), max 2 actions/cycle, rollback via galion-deploy once per unit
per 24 h; restarts only on hard-down (unit failed/inactive, critical probe down, tunnel 0 conns) for 3
cycles, or memory >= 95% MemoryMax for 2 cycles. New findings (cortex incidents, DDoS, tunnel origin
error rate, listener sensors) only raise status/alerts, never an action. External alerts: none
configured, as before (NERVE_TELEGRAM_ENV unset). So no new automatic actions; nothing had to be turned off.
Reflex budgets are now persisted (reflex.json) across nerve restarts.

### Sandbox (09:42 +02:00, temp user, /var/tmp/sandbox-test-20261021074203, removed)

All three new binaries together on loopback ports 20503/20505/20506. The nerve ran in dry-run with reflex and rollback off. Memory ran on a copy of the live data dir
(real instance DB opened, file unchanged, /health body identical to live). hermes ran on a copy of config/SOUL/
state DBs, with fresh random sandbox keys, `HERMES_RS_PROVIDER_LOOPBACK_ONLY=1` and no provider keys.
- nerve cycle 1 at +0.2 s, 15 components; /health 200.
- memory /health 200, hermes /health 200 (same keys as live).
- /signals: hermes-rs (started, heartbeat, metric, error) and openmind-memory (started, heartbeat, metric);
  a forced chat -> 502 (loopback-only refusal, no paid call possible) became a nerve incident.
- POST /event x2 -> 200, GET /events 200 with both, events.jsonl 2 lines.

### Live (one at a time, ExecStart drop-ins `50-rust-bin.conf`, old binaries not overwritten)

| time +02:00 | unit | result |
|---|---|---|
| 09:43:54 | galion-nerve | listening +0.14 s (event-log on, components=15), /state null at +0.57 s, cycle 1 at 09:43:55.708 (896 ms), mode live; /health 200; POST /event marker -> /events 200, events.jsonl 1 line; cycle 2 normal, 0 reflex actions |
| 09:45:09 | openmind-memory | /health 200 at +0.57 s; signals started/heartbeat/metric in nerve; 09:45:26 2nd restart to add `SyslogIdentifier=openmind-memory` (journal ident had become the binary file name) |
| 09:46:22 | hermes-rs | /health 200 at +0.56 s, same body (hermes-agent 0.21.4); signals started/heartbeat/metric in nerve; drop-in also sets `SyslogIdentifier=hermes-gateway`; env names unchanged + NERVE_URL (no coding worktree, same config.yaml, caps, MCP 0: paid-model setup untouched) |

Verified 09:47:28: nerve cycle 7, 15 comps, mode live, no reflex budget used, audit empty; /signals: hermes-rs,
openmind-memory, galion-deploy; rejected 0; /events 200; hermes 8642 + memory 8420 /health 200; galion.app, www,
one.galion.app 200 (from the box). Expected for ~1 h: openmind-memory "degrading: 2 restart(s)", hermes-rs
"degrading: 1 restart(s)" (the deploy restarts; clear by ~10:46). cloudflared shows "tunnel origin errors"
plus an edge incident (4 public hostnames routed to closed origin ports). These are new findings about something that was already there, not caused by this deploy.

Backups: /var/backups/galion-nerve.service-20261021-094354 (+ `.d` copy of drop-ins, + galion-nerve.bin-sha-20261021-094354),
/var/backups/openmind-memory.service-20261021-094509 (+ `.d`, + .bin-sha), /var/backups/hermes-rs.service-20261021-094621 (+ `.d`, + .bin-sha).

Rollback (each independent; old binaries are still in place):
```
rm /etc/systemd/system/galion-nerve.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart galion-nerve
rm /etc/systemd/system/openmind-memory.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart openmind-memory
rm /etc/systemd/system/hermes-rs.service.d/50-rust-bin.conf && systemctl daemon-reload && systemctl restart hermes-rs
```
