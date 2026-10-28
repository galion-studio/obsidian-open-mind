<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve T3.2: reflex safety (slot 186), real-test record

Branch `swarm/nerve-t32-reflex-safety`, base aa77864 (= 334beaa + the slot-157
DST fix, the build live on the VPS as `/opt/galion/rust/bin/openmind-nerve-aa77864`
since 2026-10-21 09:43 +02:00). Box + read-only VPS GETs only; ports 20560-20569.
Nothing on the VPS was written, restarted or reconfigured.

## What was added

- `openmind-nerve replay DIR --reflex [--stretch N] [--worst-case] [--log]`:
  the existing capture replay with the reflex layer switched back on but
  **forced dry-run** (`Reflex::force_dry`: beats `NERVE_DRY_RUN` and the
  `mode` file; a throwaway data dir replaces `NERVE_DATA_DIR`, so a live
  `reflex.json` or `mode` file is never read; `deploy_in_progress` is pinned
  to false, the worst case). Prints a verdict; exit 1 on any cloudflared
  restart or any rollback.
  - `--stretch N`: N more cycles (30 s apart) of the last captured window:
    journal lines shifted forward, cumulative tunnel counters advanced at the
    capture's own rate. 130 = 65 min, enough to exhaust every hourly budget.
  - `--worst-case`: rollback armed as if `galion-deploy` existed and every
    allowlisted unit had a deploy backup from the last 24 h.
- `openmind-nerve reflex-audit`: allowlist / never-touch audit of the
  config's own component table (`NERVE_COMPONENTS`, `NERVE_VOICE_UNIT`
  respected). Exit 1 on a violation.
- Fixes found by the audit (`src/reflex.rs`):
  1. **Cross-unit rollback**: `newest_backup("tuner")` matched any
     `tuner-*.bin`, so a `tuner-quick-<stamp>.bin` backup would have been
     chosen as tuner's rollback binary. Now only `<svc>-YYYYMMDD-HHMMSS+HHMM.bin`
     regular files (the exact galion-deploy name, same rule as galion-deploy's
     own prune), and one unreadable entry no longer aborts the whole scan.
  2. **Dry-run rollback limit**: dry-run never recorded its rollbacks, so a dry
     run "rolled back" the same unit every cycle (positive control showed 18 in
     20 cycles) while live would do 1 per 24 h. Dry rollbacks are now kept in
     memory like dry restarts: dry-run predicts live.
  3. **Budget clock**: `execute` stamped restarts with the wall clock while
     `decide` used the cycle clock; the engine now calls `execute_at(.., now)`
     (identical live; a replay keeps budgets on the capture clock).

## Results (2026-10-21, Zagreb +02:00)

`GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release -p openmind-nerve --test real_reflex_safety -- --nocapture --test-threads 1`
at 12:45-12:46 +02:00: **4/4 pass** (71.8 s).

| capture | mode | cycles | restarts | cloudflared | rollbacks | holds | limited | score |
|---|---|---|---|---|---|---|---|---|
| fixture cortex-replay | plain | 3 | 0 | 0 | 0 | 0 | 0 | 21/F |
| fixture cortex-replay | worst+130 | 133 | 0 | 0 | 0 | 0 | 0 | 62/D |
| replay1 (live, 2026-10-20 19:24) | plain | 3 | 0 | 0 | 0 | 0 | 0 | 61/D |
| replay1 | worst+130 | 133 | 0 | 0 | 0 | 0 | 0 | 62/D |
| replay158 (live, 2026-10-21 08:2x) | plain | 2 | 0 | 0 | 0 | 0 | 0 | 66/D |
| replay158 | worst+130 | 132 | 0 | 0 | 0 | 0 | 0 | 66/D |
| replay179 (live, 2026-10-21 09:1x) | plain | 3 | 0 | 0 | 0 | 0 | 0 | 66/D |
| replay179 | worst+130 | 133 | 0 | 0 | 0 | 0 | 0 | 66/D |
| replay186 (live, 12:36:54-12:38:24) | plain | 3 | 0 | 0 | 0 | 0 | 0 | 66/D |
| replay186 | worst+130 | 133 | 0 | 0 | 0 | 0 | 0 | 66/D |
| vps-now (in-test capture 12:45-12:46) | plain | 3 | 0 | 0 | 0 | 0 | 0 | 66/D |
| vps-now | worst+130 | 133 | 0 | 0 | 0 | 0 | 0 | 66/D |

In every capture cloudflared is `failing-soon` / `degrading` (tunnel origin
errors 123/183 in 10 min, ~100 journal errors in 5 min: routes to 8-10 closed
origin ports) with `consecutive_down 0` and `hard_down false`, and an `edge`
incident is raised: **edge dead routes are alert-only** (only `unit failed`,
`unit inactive` or `0 edge connections` make cloudflared hard-down).

Positive controls (same binary, mutated fixture), so the zeros are not vacuous:

- hermes-rs `failed` for the whole capture, `--worst-case --stretch 20`:
  exit 1, restart x3 (budget 3/h), then rollback x1 ("would run: ..."), then
  rate-limited x1; every log entry `mode: dry-run`.
- cloudflared 0 edge connections, `--worst-case --stretch 30`: exit 1,
  cloudflared restart x1 (budget 1/h), rollback 0 (cloudflared is never
  rolled back).

`reflex-audit` on the built-in table: exit 0, no violations, no notes. A
hostile table (`tuner-quick` restarts 1/h, `sshd.service` 3/h): exit 1
(violation: tuner-quick), sshd is hold-only (never-touch list).

Box serve on 127.0.0.1:20560 (`NERVE_DRY_RUN=1`, 5 s cycles): mode
`dry-run`, 10 allowlist budgets, `/audit` `[]`, recent `[]`; killed after.

## Live VPS (read-only GETs on 127.0.0.1:8670 over ssh)

- 12:46:36 +02:00: live nerve cycle 366, **mode `live`** (unit env
  `NERVE_DRY_RUN=0`, no `mode` file), score 61, system degraded. Reflex
  `recent` 0, `/audit` `[]`, every budget 0 (no reflex action since the
  aa77864 start at 09:43:54 +02:00). Live allowlist = this build's ALLOW (10
  units); every component's live `reflex_restarts_per_hour` equals the
  built-in table (3 for the 8 services + voice-worker, cloudflared 1,
  tuner-quick / mouth / gaia / host / nerve 0).
- Rollback readiness (read-only `ls`, 12:46 +02:00): `/opt/galion/rust/bin/galion-deploy`
  exists; `/var/backups/galion/deploy` holds `tuner-20261021-0432{00,10,32}+0200.bin`
  (newest mtime 2026-10-21 04:32:10 +02:00) and two galion-health backups
  from 2026-10-18. So **the live tuner rollback path is armed until about
  2026-10-22 04:32 +02:00**: if tuner went hard-down for 3 cycles and
  used its 3 restarts in an hour, the live nerve would run
  `galion-deploy tuner <backup>` by itself. No tuner-quick backup exists, so
  the cross-unit bug (fix 1) cannot fire today.
- `rollback.sh` / `rollback-gaps.sh` exist in the switch dir; the reflex only
  names them in alerts.

## Allowlist audit (built-in table)

| component | unit | reflex | rollback |
|---|---|---|---|
| openmind-memory, hermes-rs, link-watch-rs, galion-desk, tuner, galion-one, console, galion-health | `<name>.service` | restart <= 3/h | yes |
| voice-worker | galion-voice.service (not installed; dormant) | restart <= 3/h | yes |
| cloudflared | cloudflared.service | restart <= 1/h | never |
| tuner-quick | tuner-quick.service | never (0/h) | never |
| galion-one-mouth, gaia, lxc-host, galion-nerve | none | none | none |

Never-touch (substring): ssh, fail2ban, ufw, nftables, iptables, firewall,
galion-control, systemd-, dbus, networkd, resolved, tuner-quick,
galion-nerve, cron, postfix. No allowlist entry matches it; every entry is a
plain `*.service` name (no leading `-`, no spaces/shell characters), so it
can only ever be one `systemctl restart --no-block <unit>` argument.
`tuner` is a `<x>-` prefix of `tuner-quick`, which fix 1 keeps apart.

## Re-run on the final code and gates

- 12:51-12:53 +02:00: same command, **4/4 pass** (71.9 s); VPS 12:53:09
  +02:00 cycle 379 mode live, score 61, reflex recent 0, audit 0, budgets
  used 0; vps-now plain and worst+130: 0 restarts, 0 cloudflared, 0 rollbacks.
- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D
  warnings` ok; `cargo test --workspace --exclude openmind-sandbox --exclude
  cgroup-detect`: 272 suites, 1275 passed, 0 failed, 6 ignored (openmind-nerve
  lib 81/0, incl. 9 new); no-python ok; spdx-scan 0 missing; repo-check PASS
  (0 errors).
- openmind-nerve release x86_64-musl and aarch64-musl (rust-lld) build, static;
  armv7-musleabihf and aarch64-linux-android `cargo check` ok. The musl
  x86_64 binary's `replay replay186 --reflex --worst-case --stretch 130`:
  pass, 0 cloudflared restarts, 0 rollbacks.
