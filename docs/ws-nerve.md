<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# ws/nerve: NERVE (V1 self-eval + V2 nervous system)

Started 2026-10-18 22:10 CEST (+02:00). Owner: Mate. Branch `ws/nerve` (from
GitHub `main` 317d1d4), worktree `/workspace/oom-ws/nerve`, own target dir
`/workspace/oom-ws/nerve-target`.

## What it is

One Rust service, `galion-nerve` (library + binary), plus `nerve-client`.

| piece | what |
|---|---|
| `crates/openmind-nerve` (lib + `openmind-nerve` binary; installed as `galion-nerve`) | probes, hub, correlation, reflexes, alerts, self-eval, loopback API |
| `crates/nerve-client` | optional fire-and-forget push (`NERVE_URL`, no-op if unset), std only |
| `deploy/nerve/galion-nerve.service` | systemd unit: Nice=10, CPUQuota=15%, MemoryMax=64M, TasksMax=64, IO idle, no capabilities, read-only FS except `/var/lib/galion-nerve` |
| `deploy/nerve/install.sh` | backs up binary+unit to `/var/backups/galion/nerve/`, installs, restarts; `DRY=1` installs in dry-run |

Installed at `/opt/galion/rust/bin/galion-nerve`, unit `galion-nerve.service`,
API on **127.0.0.1:8670** (free per `ss -ltnp`; 8660 is galion-health). Not
exposed: no tunnel route, the binary refuses a non-loopback `NERVE_ADDR`, and
the server drops non-loopback peers.

API: `GET /` (HTML dashboard, refresh 15 s), `GET /state` (JSON system state),
`GET /self-eval` and `/self-eval.md` (latest report), `GET /alerts`,
`GET /audit`, `GET /signals`, `GET /healthz`, `POST /signal`.

Files: `/var/lib/galion-nerve/state.json` (every cycle),
`self-eval/latest.{json,md}` + `self-eval-<stamp>.json` (first cycle, then
every 20 cycles = 10 min; newest 96 kept), `alerts.json` (last 200),
`audit.jsonl` (every reflex decision), `reflex.json` (restart budgets, survive
nerve restarts), `mode` (runtime dry-run switch).

## Self-eval (V1)

Same shape as `repo-check --json`: `tool, version, ts (Europe/Zagreb), pass,
errors, warnings, findings[{check, level, component, msg}]`, plus `system`,
`components`, `gaia` and `reflex_actions`. Checks:

| check | level | what |
|---|---|---|
| probe | error | systemctl / journalctl / df / meminfo could not be read |
| coverage | error | a component got no sample |
| component | error / warn / info | failing critical / degrading, failing-soon, non-critical failing / absent |
| agreement | warn | galion-health's ok flag disagrees with the nerve verdict |
| self-rss | warn | nerve RSS > 32 MiB |
| cycle-time | warn | a cycle took more than half the interval |
| loopback | error | API bound off loopback |
| storage | error | state dir not writable |
| time | error | report timestamp not Europe/Zagreb |
| reflex-budget | warn | a unit used its restart budget this hour |
| push, alerts-channel, mode | info | live push sources, alert channel, reflex mode |

`openmind-nerve once [--md]` runs one observe-only cycle (no reflexes, no files)
and exits 0 on pass, 1 on fail, so it can join CI or a cron self-eval.

## Signals (V2 hub)

Per cycle (30 s) the nerve pulls: one batched `systemctl show` (state, result,
enabled, MainPID, NRestarts, MemoryCurrent/Max, CPUUsageNSec,
ActiveEnterTimestampMonotonic), one `journalctl -o json` over the window for
all units (error/warning counts, restart notices, last error line), HTTP
health probes, cloudflared Prometheus metrics, `df -Pk /`, `/proc/meminfo`,
`/proc/loadavg`, galion-health `/status.json`, and `gaia status` when that CLI
exists. Pushes arrive on `POST /signal`:

```json
{"source":"hermes-rs","kind":"heartbeat|metric|event|error","name":"rss_kb","value":2048,"msg":"text"}
```

Limits: body 8 KiB, 120 signals/min/source, 64 sources, 64 recent per source.

| component | unit | probe | Gaia | link today |
|---|---|---|---|---|
| openmind-memory | openmind-memory.service | :8420 /health | APOLLO memory-gateway | pull (push wired on ws/nerve) |
| hermes-rs | hermes-rs.service | :8642 /health | ARTEMIS hermes-agents | pull (push wired on ws/nerve) |
| link-watch-rs | link-watch-rs.service | unit + journal | APOLLO link-watch | pull |
| galion-desk | galion-desk.service | :8646 /health | HEPHAESTUS galion-desk | pull |
| tuner | tuner.service | :8650 /health | DEMETER tuner | pull |
| galion-one | galion-one.service | :8091 /health | ELEUTHIA galion-one | pull |
| galion-one-mouth | (part of One) | :8002 /health | AETHER speech-io | pull, non-critical |
| console | console.service | :8080 / | HEPHAESTUS console | pull |
| galion-health | galion-health.service | :8660 /status.json | DEMETER guardian-tick | pull; its JSON is also consumed |
| cloudflared | cloudflared.service | metrics :20241 (ha_connections, requests, errors) | AETHER tunnels | pull |
| tuner-quick | tuner-quick.service (transient) | metrics :20242 | AETHER tunnels | pull, never restarted |
| voice-worker | galion-voice.service (`NERVE_VOICE_UNIT`, `NERVE_VOICE_PORT`) | unit + port | AETHER voice-relay | absent until ws/voice deploys |
| gaia | - | `gaia status` (`NERVE_GAIA_BIN` or PATH) | GAIA core | absent until the CLI is installed |
| lxc-host | - | df, meminfo, loadavg | POSEIDON quotas | pull |
| galion-nerve | - | own RSS/CPU/cycle time | AETHER nerve | self |

The state also rolls every component up to the nine Gaia functions (plus GAIA
core), as `gaia-design.md` 4.2 does. The nerve itself is AETHER `nerve`; its
alerts serve MINERVA `alerting`, its restart reflex DEMETER `boot-units`, its
rollback reflex HADES `rollback`.

ws/growth: Hermes and memory `/metrics` and `/status` exist but need the
service API key (401 without it). The nerve never reads keys, so it stays on
`/health` there. If growth adds an unauthenticated loopback `/metrics`, add it
as a probe path.

### Turning on push (growth workstream owns the live deploy)

1. Deploy hermes-gateway and openmind-memory built from a branch that has the
   ws/nerve commit (nerve-client wiring), e.g. through `galion-deploy`.
2. Add `Environment=NERVE_URL=http://127.0.0.1:8670` to `hermes-rs.service`
   and `openmind-memory.service` (back the units up first), `daemon-reload`,
   restart.
3. `/state` then shows `"connection":"push+pull"` and `push_age_s` for them;
   pushed 5xx errors count toward the error-rate verdict alongside journald.

## Correlation

Every component keeps 240 samples (2 h). Verdicts:

- **failing**: unit failed/inactive, health probe down or 5xx (critical
  components), tunnel with 0 edge connections, expected unit missing.
- **failing-soon**: 3+ restarts in the last hour (PID, NRestarts or
  ActiveEnter changes), memory >= 90% of MemoryMax, memory trend projects
  hitting MemoryMax (or host RAM) within 30 min, latency > 2 s for 3 probes,
  30+ log errors in 5 min, disk >= 95% or projected full within 24 h, RAM
  available < 5%.
- **degrading**: 1-2 restarts in the hour, memory >= 75% of MemoryMax, memory
  up > 25% over 30 min while rising, latency of the last 3 probes > 3x the
  median, 3+ log errors in 5 min, tunnel origin errors >= 25% of requests over
  ~10 min, < 2 edge connections on the named tunnel, disk >= 85%, RAM
  available < 15%, unit activating, galion-health says not ok, a
  non-critical probe failing.
- **absent**: not deployed yet (voice worker, gaia).

## Reflexes

| rule | action |
|---|---|
| hard failure for 3 consecutive cycles (90 s) on an allowlisted unit | `systemctl restart --no-block <unit>` |
| memory >= 95% of MemoryMax for 2 cycles | pre-emptive restart (same budget) |
| restart budget used up and still failing, a galion-deploy backup newer than 24 h exists | rollback: `systemd-run galion-deploy <svc> /var/backups/galion/deploy/<svc>-<newest>.bin` (backs up, installs, health-checks, auto-reverts), 1 per unit per 24 h |
| budget used up otherwise | stop acting on that unit, critical alert naming the manual next step (`/root/galion-switch-20261018-2140/rollback-gaps.sh` / `rollback.sh`; those say "NOT run automatically", so the nerve never runs them) |

Allowlist: openmind-memory, hermes-rs, link-watch-rs, galion-desk, tuner,
galion-one, console, galion-health, cloudflared, galion-voice. Never touched
(checked even if the allowlist is edited): ssh, fail2ban, ufw, nftables,
iptables, firewall, galion-control, systemd-*, dbus, networkd, resolved,
tuner-quick (its trycloudflare URL would change), galion-nerve itself, cron,
postfix. The nerve never deletes data and never touches cloudflared config or
DNS.

Limits: 3 restarts per unit per hour, cloudflared 1 per hour; max 2 actions
per cycle; no reflex while `galion-deploy`, `switch.sh` or a rollback script
is running; a cleanly stopped unit (`inactive`, result `success`) or a
disabled+stopped unit is treated as an operator action: alert, no restart;
no reflex when systemd state is unreadable. Every decision (restart,
rollback, hold, rate-limited; live or dry-run) is a line in `audit.jsonl`.
Scaling: single LXC, so remediation is restart or rollback only.

Dry-run switch: `NERVE_DRY_RUN=1` in the unit, or at runtime without a
restart: `echo dry-run > /var/lib/galion-nerve/mode` (`echo live > ...` to go
back). Dry-run decisions use their own budget so they never consume the live
one.

## Alerts

journald (priority via the `<N>` prefix: crit=2, warn=4) and
`/var/lib/galion-nerve/alerts.json`; same key deduplicated for 30 min.
External: the host has no Telegram token or Hermes messaging channel
configured (checked by variable name only in the Hermes env files), so none is
used. If one is added, set `NERVE_TELEGRAM_ENV=<env file with
TELEGRAM_BOT_TOKEN and TELEGRAM_CHAT_ID>`; the token is read in place at send
time, handed to curl on stdin (never in argv or logs), alert text only, max 4
per hour (`NERVE_EXT_ALERTS_PER_HOUR`, hard cap 6).

## Coordination with ws/foundation

galion-health (ws/foundation, deployed at 127.0.0.1:8660) was a single
`main.rs` with no library when this was written. The nerve consumes its
`/status.json` (agreement check, `galion_health_ok` per component) and
monitors its unit. `openmind_nerve::probe` is a library module with pure,
tested parsers so galion-health can depend on it later
(`galion-nerve = { path = "../galion-nerve" }`) instead of keeping its own
probe code. `galion-deploy` (ws/foundation) is the rollback path, and a
running deploy holds every reflex.

## Universal CPU

std + serde_json + zagreb-time (chrono, chrono-tz): no asm, SIMD or
target-cpu flags, no C deps, no openssl. Release profile from the workspace
(opt z, lto, panic abort, strip).

| target | galion-nerve | nerve-client |
|---|---|---|
| x86_64-unknown-linux-musl | build, 816 KB static | build |
| aarch64-unknown-linux-musl | build, 703 KB | build |
| armv7-unknown-linux-musleabihf | build, 697 KB | build |
| aarch64-linux-android | cargo check ok | cargo check ok |

RSS on the LXC: about 0.9 MiB steady (cgroup about 1.1 MiB); a cycle takes
about 0.5 s of mostly waiting on systemctl/journalctl.

## Remaining

- Push stays off live until growth redeploys hermes/memory with this wiring
  and sets `NERVE_URL`.
- galion-health could adopt `openmind_nerve::probe` (ws/foundation's call).
- Voice worker and gaia show as absent until they are deployed. Then set
  `NERVE_VOICE_UNIT`/`NERVE_VOICE_PORT` and install `gaia` on PATH or set
  `NERVE_GAIA_BIN`.
- Probe-level /metrics for hermes/memory needs an unauthenticated loopback
  endpoint (ws/growth).

## swarm/nerve (2026-10-20 CEST +02:00): cortex, incidents, score, DDoS

`crates/galion-nerve` from ws/nerve was merged into `crates/openmind-nerve`.
That crate now builds the `openmind-nerve` binary, and `deploy/nerve/install.sh`
installs it as `galion-nerve`, so the unit file is unchanged. The 13a296c
library API (`Writer`, `Event`, `read_all`, `rotation_path`, `MAX_KEEP`) is
re-exported unchanged. New modules:

- `logscan`: finds cloudflared refused origins (`dial tcp [::1]:PORT ...
  connection refused`, `originService=`, `dest=`), scanner paths, and Vite dev servers.
- `access`: CLF/JSON access-log tailing (rotation-safe) and the DDoS
  detector (median/MAD baseline, flood/elevated levels, source shape).
- `incident`: correlates signals by cause into incidents with stable ids
  and resolve-after-2-quiet-cycles.
- `cortex`: lists vulnerabilities and bleeding points, and holds the DDoS findings.
- `score`: computes the deduction-based score and grade, and decides pass/fail.
- `replay`: reruns a read-only VPS capture (`state*.json`, journal JSONL,
  `ss -ltn`, health bodies) through the engine.

Why the live nerve said "PASS" while the stack was degraded (capture taken
2026-10-20 19:23 to 19:25 CEST):

- cloudflared logs `ERR` lines at journald priority 6, so the old nerve counted 0 errors;
- there was no dead-route analysis;
- an HTTP 503 `loading` body was treated as "still loading" for days;
- stray listeners and scanner traffic were not visible.

The new build's replay of the same capture gives FAIL, 61/D, with three explained
incidents. Evidence: [`docs/real-tests/nerve.md`](real-tests/nerve.md).

### gaia-live (4fbbb7f) compatibility

Both branches add `crates/openmind-nerve/src/main.rs` and edit `lib.rs`.
Expect an add/add conflict there; resolve it in favour of swarm/nerve. Its
API is a superset:

- `serve --listen --log --max-bytes --keep`;
- `GET /health` returns `status: ok, service: openmind-nerve`;
- `POST /event` uses the same validation and the same `{"ok":true,"appended":n}`;
- `GET /events?n=N`.

Differences:

- the server is multi-threaded (16 connections) instead of one connection at a time;
- `--listen` defaults to 127.0.0.1:8670;
- the event log is optional (POST /event returns 404 without `--log`/`NERVE_EVENT_LOG`);
- `/health` has extra fields (system, score, cycle);
- the request body cap is 8 KiB instead of 64 KiB.
