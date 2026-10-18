# Switch gaps (2026-10-18 evening, Europe/Zagreb)

Follow-up to the 21:42 switch of the dev VPS to hermes-rs + openmind-memory.
VPS log: `/root/galion-switch-20261018-2140/SWITCH-LOG-gaps.md`; rollback: `rollback-gaps.sh [all|memory|link-watch]`.

## 1. Memory wiring (done, live)
hermes-rs `memory.rs` (see ws-hermes.md "Memory provider"). VPS: `/opt/galion/hermes/.env` got
`MEMORY_TENCENTDB_GATEWAY_HOST`, `MEMORY_TENCENTDB_GATEWAY_PORT` and the `TDAI_GATEWAY_API_KEY` line copied in place
from `/etc/openmind-memory.env` (the Python unit loaded `/etc/memory-gateway.env` instead). Gate: config.yaml
`memory.provider: memory_tencentdb` (already there). Verified: a river-flash chat produced a `/capture` (L0 rows for the
session 0 -> 1 via authenticated `/search/conversations`), recall context injected.

## 2. link-watch (done, live as link-watch-rs.service)
`crates/link-watch` already covered link-watch.py; finished with urllib parity (follow http redirects, only 2xx is up,
stop at Content-Length). Same files (`state/link.json`, `state/link-log.json`, reads `state/laptop.json`), one extra key
`at_local`. Unit: `deploy/link-watch-rs.service`, sets `LINK_WATCH_FRANCE_URL` to the host the Python hard-codes so
`france` stays a bool probed every 30 s (unset it for `"unconfigured"` and no remote probe).

## 3. Hermes cron (assessed, left OFF)
All four jobs in `/opt/galion/hermes/cron/jobs.json` are `no_agent` bash wrappers that run Python. hermes-rs now refuses
such wrappers (`cron::invokes_python`), so even `HERMES_RS_CRON=1` would not start Python; they would just fail every run.

| job | every | what it does | Rust on rust-v1? |
|---|---|---|---|
| daily-digest (`digest.sh`) | 1440 m | POSTs `digest <utc> gateway=<8642 /health> ide=<8080 code> disk=<df /workspace>` to console `/api/hermes/activity` (python3 only to JSON-escape), then runs `survival.py` | Partial: `galion-guardian` `digest_text` appends one static `digest` row per Zagreb day to `activity.jsonl` (no health/disk numbers, no survival). Not a hermes-cron job: a daemon (`deploy/staged/galion-guardian.service`). |
| survival-cash (`survival-cash.sh`) | 15 m | `survival.py`: reads the OpenRouter key from hermes/.env to measure cash, GPU/CPU/volume, writes `state/survival.json` (galion-desk reads `volume_used_pct`) and `state/eval/survival.txt` | No. ws-survival: "not ported (secret read; money is out of scope)". survival.json now goes stale. |
| warn-sink (`warn-sink.sh`) | 15 m | appends at most one new `WARN id=<sha12> code=<kind> when=<utc>` to `state/eval/tool-warn.txt` from tool-loop lines in Python Hermes `logs/{agent,gateway,errors}.log` | Partial: `galion-guardian` `tool_warn` writes the same file but from `[Tool loop warning:` rows in the legacy state.db. Both sources are frozen since Python Hermes stopped; hermes-rs logs its guardrail only in the turn error. |
| shadow-status (`shadow-status.sh`) | 30 m | `shadow.py status` (git state of the Python hermes-agent shadow checkout -> `state/shadow/status.json`) then `boss.py` (probes 7 ports into `state/boss/boss.db`) | No. Shadow tree is the Python Hermes being retired; the port probes are covered by `galion-guardian` probes (health rows), not the boss.db schema. |

`galion-healer` and `galion-boot fire-loops` replace none of the four (healer = desk fault cases/repairs; fire-loops =
loop-daemon.sh POST /api/hermes/loops).

Proposal (not applied): keep `HERMES_RS_CRON` unset. If a hermes-rs cron digest is wanted, a no-Python wrapper in
`HERMES_HOME/scripts` plus a jobs.json entry would be:
```
# /opt/galion/hermes/scripts/digest-rs.sh
#!/bin/bash
set -euo pipefail
stamp=$(date -u +%Y-%m-%dT%H:%M:%SZ)
health=$(curl -sf --max-time 2 http://127.0.0.1:8642/health || echo down)
ide=$(curl -sf --max-time 2 -o /dev/null -w '%{http_code}' http://127.0.0.1:8080/api/hermes/health || echo 000)
disk=$(df -h /workspace | awk 'NR==2{print $3"/"$2" "$5}')
text="digest ${stamp} gateway=${health//\"/} ide=${ide} disk=${disk}"
curl -sf --max-time 4 -X POST http://127.0.0.1:8080/api/hermes/activity -H 'content-type: application/json' \
  --data-binary "$(jq -cn --arg t "$text" '{kind:"digest",text:$t}')" >/dev/null || true
echo "$text"
```
jobs.json: `{"id":"4567cd958b29","name":"daily-digest","script":"digest-rs.sh","no_agent":true,"deliver":"local","schedule":{"kind":"interval","minutes":1440,"display":"every 1440m"},"enabled":true}`
and `"enabled": false` on survival-cash, warn-sink, shadow-status. For the rest, the Rust path is
`galion-guardian.service` (staged, its own approval), not hermes cron.
