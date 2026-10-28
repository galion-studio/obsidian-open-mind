<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# survival-refresh-writer (wave 4 slot 389, restart r2)

Branch `swarm/survival-refresh-writer` off `console-bank-asof` 99ac55b5. Times Europe/Zagreb
CET (+01:00). NO live change, NO deploy, nothing installed on the VPS.

## Who wrote survival.json and why it stopped (VPS read-only, 2026-10-28 14:05–14:07)

| When (Zagreb) | Writer | Evidence |
|---|---|---|
| until 2026-10-18 21:35:50 +02:00 | Python Hermes cron job `survival-cash` (id 55b04c7078b8, every 15 min, 1299 runs) → `/opt/galion/hermes/scripts/survival-cash.sh` → `python3 /workspace/galion-light/scripts/survival.py` | `/opt/galion/hermes/cron/jobs.json`: `last_run_at` 2026-10-18 21:35:50 +02:00, `last_status` ok, `next_run_at` 21:50:50 never happened; `survival.json.bak-20261027-181948` mtime 2026-10-18 21:35:50 +02:00 |
| 2026-10-18 21:42:17 +02:00 | — | `hermes.service: Failed with result 'exit-code'`, stopped; unit now **masked**, so the cron scheduler inside it is gone |
| 2026-10-27 18:19:50 +01:00 | Rust `/opt/galion/rust/bin/galion-survival` (sha256 8f1cb532…), **one manual run** during the S4.7 install (slot t276u) | `survival.json` + `state/eval/survival.txt` mtime 18:19:50, SCOPE `when=2026-10-27T18:19:50+01:00`; no unit, timer or cron runs `galion-survival` (`systemctl list-timers` only shows `guardian-tick.timer`; no `/etc/systemd/system` or cron reference) |
| since | nobody | `survival.json` 1385 B, root 0644, still 2026-10-27 18:19:50 +01:00 at 14:07 (~19.8 h) |

Root cause: the Python writer died with `hermes.service` (Oct 18); the Rust replacement was
installed as a binary only and run once by hand; no scheduler was ever attached to it.
Second blocker for an in-console fix: `console.service` / `console-rust-primary.service` run with
`ProtectSystem=strict` and no `ReadWritePaths`, so anything they spawn cannot write
`/opt/galion/galion-light/state` (staged drop-in adds `ReadWritePaths=-/opt/galion/galion-light/state`).

## What this branch adds (staged)

`galion-console-static` flags (all off by default; live behaviour unchanged without them):

- `--survival-file FILE` — gated `GET /api/v1/survival`: mtime-based `asOf` (Zagreb ISO), `ageS`,
  `stale` (> 3 × refresh), writer state (`running`, `lastStart`, `lastExit`, `runs`, `failures`).
  Never the file content (cash) — POST → 405, foreign host / remote peer on loopback host → 401.
- `--survival-writer BIN` (absolute) — runs the real `galion-survival` when the file is older than
  `--survival-refresh-s` (60..3600, default 900 = old cron cadence). No shell, no args,
  `env_clear` + fixed PATH + `--survival-env` allowlist (GALION_LIGHT_ROOT, GALION_HERMES_ROOT,
  GALION_SURVIVAL_DISK, OPENROUTER_KEY_FILE), stdout/stderr discarded (SCOPE line has cash),
  killed after 90 s. The console never reads the OpenRouter key.
- `--survival-mode on-request|interval` — on-request: GET/HEAD `/bank`, `/api/v1/bank`,
  `/api/v1/survival` start a due run on a side thread (request is not delayed); interval: an
  in-process thread checks every min(N, 60) s. No systemd timer in either mode.
- Single-flight; after success the next run waits for the file to age again (and ≥ 30 s); after a
  failure it backs off a full interval, so request floods start at most one run per interval.
- `crates/galion-survival` imported from `swarm/survival-s47-install` b7fbbc7 (the live binary's
  source) so the real test runs the real writer.
- `deploy/staged/survival-refresh/console-rust-primary-survival.conf` — NOT installed.

## Real tests

### Box: `GALION_REAL=1 CONSOLE_SURVIVAL_LIVE=1 cargo test --release -p console-static --test real_survival_refresh -- --nocapture --test-threads=1` (14:12, 1/1 pass, 45.8 s)

Real release `galion-console-static` + real release `galion-survival`, temp roots, empty key file,
decoy `OPENROUTER_API_KEY` in the console env, ports 22590-22595:

1. on-request, stale file (mtime 2026-10-27 18:19:50 +01:00): `GET /bank` 200 → refreshed in
   100 ms; status `ok:true, stale:false, lastExit:0, runs:1, asOf 2026-10-28T14:12:05+01:00`;
   file `when=2026-10-28T14:12:05+01:00`, cpu/disk/gpu/lane/rules present, openrouter
   `measured:false, "no OPENROUTER_API_KEY"` (decoy key did not reach the writer), lane from modes.json.
2. 60 trigger GETs (/bank, /api/v1/bank, /api/v1/survival) while fresh: 41 ms, runs stays 1;
   non-trigger paths (/console, /api/v1/health, asset) never start a run even when due.
3. aged to 1 h → next trigger refreshes again (runs 2, exit 0).
4. failing writer (GALION_LIGHT_ROOT=/proc/…): `lastExit:1, failures:1, stale:true`; 20 more GETs
   inside back-off → runs 1; file mtime untouched.
5. missing writer binary: `lastExit:-1`; console keeps serving (`/api/v1/health` 200).
6. interval mode, zero requests: refreshed at start (0 ms), runs 1.
7. status-only (no writer): `asOf 2026-10-27T18:19:50+01:00, stale:true, staleAfterS:2700`, no write.
8. live leg (RO `ssh stat`): live mtime 1793121590 = 2026-10-27 18:19:50 +01:00, 19 h → status
   `stale:true, ageS 71570`.

Unit: `cargo test -p console-static --lib` 66/0 (+9: decision table, Zagreb ISO both seasons,
env allowlist, triggers, mode names, missing-file status, spawn-error back-off, config flags
(11 rejected combos), gated status route).

### VPS (real server, throwaway, as `nobody`), 2026-10-28 14:13:20–14:14

Box-built x86_64 musl `galion-console-static` (sha256 79e784f9…) copied to
`/var/tmp/survival-test-20261028141320`, run as `nobody` on 127.0.0.1:22596 with the **live
installed** writer `/opt/galion/rust/bin/galion-survival` (8f1cb532) against a temp root (copy of
live `hardware.json`, temp modes.json, empty key file, decoy env key):

```
before   survival.json mtime 2026-10-27 18:19:50 +01:00
GET /bank 200 0.003324s
/api/v1/survival {"ok":true,...,"asOf":"2026-10-28T14:13:44+01:00","ageS":2,"stale":false,"refreshS":60,
                  "mode":"on-request","writer":"/opt/galion/rust/bin/galion-survival","lastExit":0,"runs":1,"failures":0}
after    mtime 2026-10-28 14:13:44 +01:00 owner nobody
content  {"when":"2026-10-28T14:13:44+01:00","keys":[cpu,disk,gpu,lane,openrouter,revolut,rules,runpod,when],
          "or_measured":false,"or_reason":"no OPENROUTER_API_KEY","lane":"zai-org/GLM-5.3-Flash","disk":30.0,"load":18.02}
decoy key in file: 0
30× GET /api/v1/bank → runs 1, lastExit 0
POST /api/v1/survival 405; Host: evil.example 401
live /opt/galion/galion-light/state/survival.json still 2026-10-27 18:19:50 +01:00 (untouched)
```

Folder, process and port removed afterwards (port count 22590-22599 = 0). Nerve `/state` 200
cycle 91 mode live (ts 14:00:01 +01:00), cortex `:8674/health` 200, `console.service`,
`console-rust-primary.service`, `guardian-tick.timer` active — nothing changed.

## Owner decisions

- **D-389-1:** adopt the in-console refresh (install staged drop-in on `console-rust-primary`
  first with a binary from this branch, `--survival-mode interval`, 900 s; it needs
  `ReadWritePaths=-/opt/galion/galion-light/state`) — or instead a plain `galion-survival.timer`
  (simpler, but a new timer). Either way the writer calls OpenRouter `GET /api/v1/key` (free, no
  spend) every 15 min like the old cron.
- **D-389-2:** on-request vs interval. On-request only refreshes when someone opens /bank, so
  Hermes/skill readers of survival.json still see stale data in quiet hours → interval recommended.
- **D-389-3:** `galion-survival`'s `lane()` may rewrite `/opt/galion/hermes/modes.json` when the
  action is the HOLD park; under the console's `ProtectSystem=strict` that write fails silently
  (intended: the drop-in does not open `/opt/galion/hermes`).
