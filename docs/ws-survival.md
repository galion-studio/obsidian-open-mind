---
tags: [survival, guardian, healer, boot, rust]
project: galion
status: staged
---
<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Workstream ws/survival: guardian, healer, boot/keep and origin in Rust

Written 2026-10-18T21:30+02:00 (Europe/Zagreb). Branch `ws/survival`, worktree `/workspace/oom-ws/survival`.
Product: Obsidian Open Mind, made by Galion. Everything here is **staged**. Nothing was installed, enabled,
restarted or switched on the VPS. The VPS was only read (sources, unit files, listeners, file names).
No Python was run. No secret value was read, printed or copied.

## 1. What changed

| component | Python/shell it replaces | Rust | status |
|---|---|---|---|
| `openmind-survival` (lib) | probe helpers; heal-dry.py patterns | + `decide` (B23), `degrade` (B24), `nerve` (B25), `patterns` (heal-dry), `http_get` + `verdict` (desk_heal 503-loading rule) | done |
| `galion-guardian` (bin) | `scripts/guardian-tick.py` 138, alerting half of `scripts/health-tick.py` 119, keep.sh digest heredoc | 60 s beat; health rows + incidents (JSONL store, or `--features sqlite` writing guardian.db with the original schema, which Galion One reads); beat file; streaks + suggestions; tool-loop warning (sqlite build); `health fail ...` activity rows (900 s gate); daily digest row; nerve lines on change | done |
| `galion-healer` (bin) | `guardian/desk_heal.py` 229, `scripts/heal-dry.py` 79, `scripts/heal-apply.py` 105, safe part of `revive.py` 109 | 30 s cycle, cases/inbox/desk.jsonl/nerve/open.json/pulse.json; `plan`; `repair-json`; allow-listed actions (dry-run default, logged to actions.jsonl) | done |
| `galion-boot` (bin) | `keep.sh` 210, `stack-boot.sh` 180, `persist-boot.sh` 65, `vps-boot.sh` 2, `start.sh` 3, `hardware.sh` 59, `scripts/loop-daemon.sh` 8, the `guard/lock-secrets.sh` 66 step, ~190 lines of inline Python heredocs | `hardware` (B54), `secrets-restore` (age bundles), `tunnel-env`, `render-units` (B55), `supervise` (non-systemd fallback), `status`, `fire-loops` | done |
| `galion-origin` (bin) | `origin.py` 684 | slim port of the routes that were actually reachable (see 4) | done, optional to deploy |
| `guardian-tick` (bin) | (Rust already) | `--nerve <path>` (B25), off by default | done |
| upstream guardian library | `guardian/upstream/*.py` 3,734 (Docker-era, never started by anything) | not ported; thresholds reused (85/95 %), fix patterns reused via heal-dry | retired |
| `scripts/survival.py` 205 | reads the OpenRouter key from hermes/.env to measure cash | not ported (secret read; money is out of scope) | left as is |

Python lines replaced: **1,463** live lines (guardian-tick 138, desk_heal 229, heal-dry 79, heal-apply 105,
health-tick 119, revive 109, origin 684) + ~190 inline heredoc lines in keep.sh/stack-boot.sh/hardware.sh;
**3,734** upstream library lines retired without a port. Shell replaced: 593 lines (keep, stack-boot,
persist-boot, vps-boot, start, hardware, loop-daemon, lock-secrets step).

Commits (author Grok Bot): `50da469` UNIVERSAL-CPU.md, `205bd81` survival lib B23/B24/B25, `ec28d09` guardian,
`94d3260` healer, `de1edf9` boot + staged units, `95183c5` origin (B56), `7101702` guardian-tick --nerve, plus this doc.

## 2. Dangerous behaviours removed

| was | where | now |
|---|---|---|
| seal secrets: openssl-encrypt volume copies and write a `SEALED` stub over the plaintext (hermes/.env, tokens, github key) | keep.sh:25 -> lock-secrets.sh | gone. No lock-secrets call; nothing writes `SEALED` anywhere. A live file that is real is never overwritten; a live `SEALED` stub or empty file is replaced from the age bundle; a bundle member that is a stub is refused (tested) |
| plaintext secret copies `secrets/persist/*` -> /root/galion-secrets | keep.sh:10-24 | `galion-boot secrets-restore` decrypts only `*.tar.age` bundles with an identity file (`age -d -i ... \| tar -x` into a 0700 dir inside the live dir, 0400 temp+rename, dir removed); dry-run lists names only (`tar -t`) |
| copy github key to `/root/.ssh/id_ed25519` and write /root/.ssh/config | persist-boot.sh:44-58 | gone; /root/.ssh is never a target |
| tunnel token on the cloudflared command line (`--token "$TOK"`; leaked into gateway-deaths.log via `ps`) | keep.sh:131 | `galion-boot tunnel-env` writes `TUNNEL_TOKEN=` to a 0600 env file used as `EnvironmentFile=`; the config parser refuses any `--token` argv and any lock-secrets argv (tested). The VPS's own cloudflared.service already uses config.yml, so nothing changes there |
| `pkill` of one, tulu, keep.sh when RAM < 8 GiB | stack-boot.sh:148-154 | gone. The supervisor only signals its own children, and only on a healer restart request |
| restart console/one/tulu/hermes/vision/stt/tts/loop from a nohup loop | keep.sh:87-209 | systemd `Restart=always` units (existing ones for core services; new ones for guardian/healer/origin) or `galion-boot supervise` |
| rewrite the hermes-agent package file (SSE patch via python) | keep.sh:58-80 | retired; vendor patches belong to ws/hermes |
| `apt-get install` at boot; `git checkout` of hermes-agent; replace the hermes launcher | persist-boot.sh:28-32; revive.py | retired |
| upstream auto_healer: pip installs, file edits, chmod 755, `kill`/`pkill`/`systemctl` allow-list | upstream/auto_healer.py | not ported |
| desk_heal reads API keys from `/root/.hermes/.env` and `/workspace/hermes/.env` | desk_heal.py:71-102 | not ported; the healer reads no secrets, the model call says `not sent. model call not ported (no secrets read)` |
| origin.py spawns persist-boot.sh, binds 0.0.0.0 on 8 ports, proxies everything to 8080 | origin.py:668-680, :378-400 | gone; galion-origin binds 127.0.0.1:8655 by default |

Healer allow-list (the only things it can change; all dry-run unless `apply = true`, all logged):
1. `clear-stale-gateway-lock`: remove gateway.pid/.lock/.sock in the Hermes home only when the gateway probe is down and the pid is dead; files/symlinks only.
2. `restart-unit`: only units listed in `[[restart]]`; protected names refused at config load and at run time (hermes/gateway, whisper, vision, keep.sh, cloudflared, ssh, memory); one restart per unit per 600 s; backend `systemd`, `supervisor` (request file) or `none`. The staged config has **no** restart rules.
3. `json-brace-repair`: heal-apply.py semantics, confined to the fixture root, backup + rollback.

## 3. Parity checklist

| Python behaviour | Rust | test |
|---|---|---|
| guardian-tick probes (console 200/401, hermes, whisper, vision, voice, origin, tunnel by `pgrep -x`) | `[[probe]]` http/process kinds; tunnel via /proc/*/comm | `probe_statuses` |
| statuses healthy / unhealthy (bad code) / critical (no answer, tunnel missing) | same | `probe_statuses` |
| health_checks row per probe, prune to newest 4000 | JSONL or guardian.db | `prune_rows`, `sqlite_store_parity` |
| incident open (high if critical else low), one per service, resolve `beat healthy` | same SQL in sqlite store | `beat_incidents_streaks_sinks`, `sqlite_store_parity` |
| Galion One's `SELECT ... MAX(id) GROUP BY service` keeps working | same schema | `sqlite_store_parity` |
| beat file, `last-guardian.ts` | `<state>/beat` (+02:00) and `beat =` (unix secs, as keep.sh) | `beat_...` |
| stdout `stamp watch` / `stamp down a,b` | same | `beat_...` |
| tool-loop warning, once per message id | read-only open of state.db (sqlite build) | `ids_and_tool_warn` |
| health-tick `health fail a,b` row in activity.jsonl every 900 s | `activity`, `activity_every_s` | `beat_...` |
| keep.sh daily digest row | `digest_text` (off in staged config) | `beat_...` |
| desk_heal: 503 + `loading` counts as up | `verdict(.., loading_ok)` | `http_get_and_verdict` |
| desk_heal: one case per signature, inbox + desk.jsonl, `heal sent`, follow-up after 60 s or on clear, `heal held/returned`, open.json, pulse.json | same | `cases::lifecycle`, `cycle_writes_files_and_logs_actions` |
| desk_heal: hermes down -> `not sent. the gateway itself is down...` | same text | `gateway_not_sent` |
| heal-dry: 11 patterns in order, protected words, JSON output | `patterns::dry_plan`, `galion-healer plan` | `patterns::table`, `order_and_json` |
| heal-apply: fixture root, protected, backup, brace repair, rollback, exit codes 0/1/2 | `galion-healer repair-json` (+ dry-run) | `json_repair_fixture_only` |
| revive dead_lock | `clear-stale-gateway-lock` | `gateway_lock_repair` |
| hardware.sh/stack-boot hardware.json, machine.txt, machine.json, weights/mouth/line texts | `galion-boot hardware` via `degrade::profile` | `hardware_profiles`, `degrade::table` |
| loop-daemon.sh POST /api/hermes/loops every 60 s | `fire-loops` + 60 s timer | built; manual |
| keep.sh restart loop | systemd units / `supervise` (restart, backoff, start limit, healer requests) | `supervisor_restarts_and_requests` |
| origin.py reachable routes | galion-origin | `routes`, `serves_over_tcp` |

Not kept on purpose: Flash/OpenRouter call (secret), health-tick's Hermes `optional_read` (reads hermes.key),
machine.ts write into console-ide and `/root/galion-link.ps1` edit (other trees), public IP lookup
(`GALION_PUBLIC_HOST` / `--public-host` instead), `/workspace` symlink juggling (RunPod era; the VPS uses /opt/galion).
Timestamps written by the Rust code are Europe/Zagreb with offset; old guardian.db rows keep their `Z` stamps.

## 4. origin.py decision (B56)

Galion's own code, but mostly dead: every GET/POST outside `/desk*` and `/grinder*` was proxied to the console on 8080,
so the JSON catalog (health, domains, cloudflare, protocol) could not be reached. Nothing listens on 80 now, and no tunnel
ingress points at 80 or 8655. Ported (419 lines, std only): `/grinder` static page with the STL download,
`/desk` desk-admin page, `/desk?n=` build-session notes viewer, `/desk/live` link.json, `/soon` and host splash, new `/health`.
The proxy is replaced by an optional 302 to `ORIGIN_CONSOLE_URL`. Deploying it is optional (see 7); B66 decides the
legacy hosts (galion.app -> :4000 and api.galion.app -> :8000 were served by origin.py when it ran on those ports).

## 5. Tests, lint, sizes, RSS

`fmt` clean, `clippy --all-targets -D warnings` clean for all six crates (and galion-guardian with `--features sqlite`).
Tests: **41** with default features (openmind-survival 14, galion-guardian 5, galion-healer 7, galion-boot 7,
galion-origin 2, guardian-tick 6) + 1 sqlite-only test = **42**, all pass on x86_64-gnu and natively as
x86_64-unknown-linux-musl. `systemd-analyze verify` (systemd 257 on the box) passes on all six staged unit files.
Tests use ephemeral loopback ports and target/test-out only (guardian-tick/survival keep their old 18080 test).

Peak RSS (GNU time, x86_64 release, one run): guardian 2.6 MB (jsonl) / 3.8 MB (sqlite, gnu) / 2.0 MB (sqlite, musl static),
healer 2.5 MB, boot hardware 2.4 MB, render-units 2.6 MB, status 2.5 MB, supervise 2.4 MB, guardian-tick 2.4 MB,
origin 2.1 MB after 50 requests.

## Universal CPU

Plan changes after UNIVERSAL-CPU.md (2026-10-18 21:02 +02:00):
- All new crates are std + serde/serde_json/toml only; no C, no TLS, no async, no arch code. The only C dependency
  (bundled SQLite for guardian.db parity) moved behind the opt-in `sqlite` feature; the default store is JSONL.
- systemd is optional: guardian and healer are plain loops; `unit` probes report `skipped: no systemd` where
  `/run/systemd/system` is absent; the healer restart backend can be `systemd`, `supervisor` or `none`;
  `galion-boot supervise` runs the same boot.toml without systemd (Termux, minimal Linux, containers);
  `deploy/staged/boot-device.toml` is the Termux example.
- Hardware probe also detects non-NVIDIA GPUs via /sys/class/drm and reports `arch`/`os`.

Release sizes in bytes (`opt-level="z"`, lto, strip, panic=abort; linker rust-lld; `-j 1`):

| binary | x86_64-gnu | x86_64-musl (T1) | aarch64-musl (T1) | armv7-musleabihf (T2) |
|---|---|---|---|---|
| galion-guardian | 749,016 | 863,800 | 747,448 | 781,012 |
| galion-guardian `--features sqlite` | 1,782,560 | 1,911,240 (static, CC=musl-gcc) | not built (no aarch64 C cross compiler) | not built |
| galion-healer | 749,776 | 863,736 | 747,080 | 777,524 |
| galion-boot | 736,096 | 850,552 | 734,984 | 703,756 |
| galion-origin | 356,832 | 475,528 | 428,544 | 397,740 |
| guardian-tick | 633,920 | 727,448 | 632,672 | 604,932 |

Tier 1: x86_64 and aarch64 musl **build** (release). Tier 2: armv7-musleabihf **builds**; aarch64-linux-android
**`cargo check` passes** (no NDK, so no link). The sqlite feature needs a C cross compiler per target
(x86_64-musl works with musl-gcc, installed on the box; others not available here).
Cross commands: `CARGO_TARGET_<T>_LINKER=rust-lld cargo build -j 1 --release --target <t> -p <crate>`.

## 6. Staged files (deploy/staged/, nothing installed)

`guardian.toml` (B20 probe list as data, read-only probes), `healer.toml` (apply=false, no restart rules),
`boot.toml` (VPS service list), `boot-device.toml` (non-systemd example), and the rendered units:
`galion-guardian.service`, `galion-healer.service`, `galion-hardware.service` (oneshot), `galion-fire-loops.service`
+ `.timer` (60 s), `galion-origin.service`. Units have no `[Install]`; hardening: NoNewPrivileges, ProtectSystem=strict
with explicit ReadWritePaths, PrivateTmp, MemoryMax. Regenerate with
`galion-boot render-units --config deploy/staged/boot.toml --out deploy/staged` (a test fails if they drift).

## 7. Switch steps for Slavik's approval (not done)

Guardian/healer services are on the risky list: each step below needs a yes.

1. Build on the box (never on the VPS):
   `CC_x86_64_unknown_linux_musl=musl-gcc CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld cargo build -j 1 --release --target x86_64-unknown-linux-musl -p galion-guardian --features sqlite`
   and `... -p galion-healer -p galion-boot -p galion-origin` (static musl: the box has glibc 2.41, the VPS 2.39).
2. Copy the four binaries to `/opt/galion/bin/` and `guardian.toml`, `healer.toml` to `/opt/galion/etc/`.
3. Back up the dormant DB: `cp -a /opt/galion/galion-light/guardian/guardian.db /opt/galion/galion-light/guardian/guardian.db.bak-$(date +%Y%m%d-%H%M%S%z)`.
4. One manual dry pass: `/opt/galion/bin/galion-guardian --config /opt/galion/etc/guardian.toml --once` and
   `/opt/galion/bin/galion-healer run --config /opt/galion/etc/healer.toml --once`; read `state/guardian/report.json`, `state/heal/pulse.json`.
5. Install units: `cp deploy/staged/galion-{guardian,healer,hardware}.service /etc/systemd/system/`,
   `systemd-analyze verify /etc/systemd/system/galion-*.service`, `systemctl daemon-reload`,
   `systemctl add-wants multi-user.target galion-guardian.service galion-healer.service galion-hardware.service`
   (boot survival without an [Install] section), `systemctl start galion-hardware galion-guardian galion-healer`.
6. Optional, separate yes each: (a) `galion-fire-loops.timer` (`add-wants timers.target`): revives Hermes loop firing,
   dead since 2026-10-10; (b) `galion-origin.service`: only useful if a tunnel ingress is pointed at 127.0.0.1:8655 (B66);
   (c) `apply = true` in healer.toml (enables the stale gateway lock repair); (d) any `[[restart]]` rule.
7. Verify: `systemctl is-active galion-guardian galion-healer`, Galion One's guard view shows fresh rows, `journalctl -u galion-guardian -n 20`.
8. Rollback: `systemctl stop galion-guardian galion-healer galion-hardware`, remove the
   `/etc/systemd/system/{multi-user.target,timers.target}.wants/galion-*` links and `/etc/systemd/system/galion-*`, `systemctl daemon-reload`,
   restore the guardian.db backup if wanted.
9. After a week green: archive (do not delete) keep.sh, stack-boot.sh, persist-boot.sh, vps-boot.sh, start.sh, hardware.sh,
   loop-daemon.sh, lock-secrets.sh, guardian-tick.py, desk_heal.py, heal-*.py, health-tick.py, revive.py, origin.py.

Secrets on the VPS: no action is needed now (live files are real). If a box ever needs a restore:
`galion-boot secrets-restore --bundle /root/galion-secrets/persist-backup-<stamp>.tar.age --identity /root/galion-secrets/persist-backup.agekey --live /root/galion-secrets --name hermes.env --name hermes.key ...`
(dry-run first; `--apply` to install). Note: `hermes.env.sealed` (openssl, from lock-secrets) is never used.
Owner item from the brief: the tunnel token that leaked into gateway-deaths.log via `ps` should be rotated and the log scrubbed (not read here).

## Open items
- aarch64/armv7 builds of the sqlite feature need a C cross toolchain (or keep JSONL there).
- The supervisor has no signal handler (std only); stop it with its process group.
- `voice` (8002) answers 503 (voice worker missing torch), so the guardian will hold an open incident for it after the switch; expected.
- TEACHER/STAGED.md and BOSS-PROGRESS.md entries are left to integration (shared files).
