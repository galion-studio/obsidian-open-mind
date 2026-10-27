---
name: survival
description: "Keep Hermes able to resume after the VPS is off. Host-gone continuity. Separate from on-host probes and from the cash lane."
---

# Survival

Repo copy of `/opt/galion/hermes/skills/galion/survival/SKILL.md`. That VPS path was not mounted in the checkout that added this file. Cash and CPU rules below are the ones the desk and the cash lane already depend on. Off-host lines are new. Do not drop the cash rules when copying this file onto the VPS.

## What this is

Hermes (Rust gateway on loopback `:8642`, crate `hermes-rs`) must still have its sessions, cron book, and spend ledger after the current VPS is powered off. The caller note of 2026-10-27 CET is the reason: Galion servers may later run on new Croatian government-linked infrastructure at Teglenice (Split-Dalmatia waste-to-energy), and that is about 1.5 years out. This skill covers the gap. It does not deploy that infrastructure.

The mechanism is `hermes-rs` `offhost`:

- a snapshot of the files Hermes already uses
- a periodic heartbeat export to a secondary path or URI
- a boot resume when local files are missing or stale
- an optional loopback standby that only serves health

## What this is not

- **On-host probes.** `openmind-survival` and `guardian-tick` read HTTP, disk, memory, and unit state on the machine that is up. They suggest. They do not restart anything. Leave them there.
- **Cash lane.** Cron job `survival-cash` owns `cash_usd`, `lane`, and `when` in `survival.json`. This skill does not read a provider balance and does not write those fields.
- **Desk disk line.** `galion-desk` reads `volume_used_pct` from `survival.json` for the board. Off-host writes must leave that number alone.
- **Nerve and cortex.** They do not gain a model route here. Off-host export is a JSON document PUT to an operator store. It is not a chat call. A URI on `openrouter.ai` is refused.
- **Python `state.db`.** The legacy history file stays read-only and is not part of the bundle.

## Files in the snapshot

| role | path | resume |
| --- | --- | --- |
| gateway_state | `$HERMES_HOME/gateway_state.json` | if missing or stale |
| cron_state | `$HERMES_RS_CRON_STATE` (default `$HERMES_HOME/rs-cron-state.json`) | if missing or stale |
| spend | `$HERMES_RS_SPEND_FILE` (default `$HERMES_HOME/rs-spend.json`) | if missing or stale, unless the local ledger is higher the same day |
| cron_jobs | `$HERMES_HOME/cron/jobs.json` | only if the local file is missing |
| config | `$HERMES_HOME/config.yaml` | never (sha256 only) |
| sessions | `$HERMES_RS_STATE_DB` or the `.sessions` JSONL directory | if missing or stale, and only when the body fits the cap |

Same-day spend: the higher `day_usd`, then the higher `day_calls`, stays. A stale file must not be replaced by a smaller number and reopen the daily cap.

## Enable

Default is off. Nothing is exported until `HERMES_OFFHOST=1`.

```
HERMES_OFFHOST=1
HERMES_OFFHOST_PATH=/mnt/hermes-offhost
HERMES_OFFHOST_URI=https://store.example/hermes/continuity.json
HERMES_OFFHOST_TOKEN=...
HERMES_OFFHOST_SURVIVAL=/opt/galion/galion-light/state/survival.json
HERMES_OFFHOST_INTERVAL_S=300
HERMES_OFFHOST_STALE_S=21600
HERMES_OFFHOST_RESUME=if_local_missing_or_stale
HERMES_OFFHOST_MAX_BYTES=2097152
HERMES_OFFHOST_LOCAL=/var/lib/hermes-rs/offhost/continuity.json
```

The token is an environment variable. It is not written into the snapshot, into `survival.json`, or into logs. Do not put it in the URI. Query strings are stripped from `external_state_uri`.

`hermes-gateway --offhost-once` writes one snapshot and exits. `hermes-gateway --offhost-standby` resumes, then listens on loopback for `/health` only. Standby does not start cron, so `survival-cash` cannot double-fire, and it does not start a model provider.

When the flag is on, defaults are: heartbeat 300 s (minimum 30), stale after 6 h, resume policy `if_local_missing_or_stale`, snapshot cap 2 MiB, local file next to the spend file at `offhost/continuity.json`.

## survival.json

Off-host adds keys and does not remove keys:

- `offhost_enabled` (bool)
- `last_heartbeat` (Europe/Zagreb RFC 3339 with a numeric offset, or null)
- `external_state_uri` (public path or URI, or null)
- `resume_policy`
- `offhost_role` (`primary` or `standby`)
- `continuity_schema` (1)
- `last_export_ok` (bool or null)

`cash_usd`, `lane`, `when`, and `volume_used_pct` stay as they were. A missing off-host key means off-host is off. A file from about 2026-10-18 that only has the cash and disk fields is still valid. See `deploy/survival/SURVIVAL.md`. Do not copy `survival.example.json` over the live file. The example uses null for cash because this repo does not contain the live balance.

## Blockers before this is real

- No secondary host is configured in code. Set `HERMES_OFFHOST_PATH` or `HERMES_OFFHOST_URI` to storage that is not this VPS disk. The local continuity file dies with the disk.
- The token, if the store wants one, has to be installed out of band.
- The staged `hermes-rs` unit writes only `/var/lib/hermes-rs` and does not start at boot. Point `HERMES_OFFHOST_LOCAL` there. Do not install the unit from this change.
- Session bytes over the cap, or a session db that looks like it holds a token, are hash-only. Raise `HERMES_OFFHOST_MAX_BYTES` only on a store you trust.
- This change does not deploy, restart, or spend.
