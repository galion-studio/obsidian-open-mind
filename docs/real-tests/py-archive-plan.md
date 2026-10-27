<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->

# Real-server probes for slot 308 py-archive-plan (VPS, read-only)

All runs 2026-10-27, from the box over `ssh slavik-vps` (VPS clock is UTC; times shown CET +01:00).
No secrets read or printed; secrets dirs pruned from `find`.

| Time (CET) | Probe | Result |
|---|---|---|
| 21:52:50 | `find / -xdev … -name '*.py' -o -name '*.mjs'` (prunes /proc /sys /usr /snap docker secrets) | 45 528 files; 28 582 in venv/site-packages/node_modules |
| 21:53 | same, grep named files | `/opt/galion/bin/{desk-hermes.py,pw.py,replicate-picture.py}`, `hermes/archive/grok-files/artifacts/replicate-picture.py`; `/opt/galion/archive/py-units-20261027/{link-watch.service,memory-gateway.service}` |
| 21:54 | `ps -eo args \| grep python\|node\|.py\|.mjs` | only `/usr/bin/python3 /usr/bin/fail2ban-server`, `/usr/bin/python3 /usr/bin/networkd-dispatcher` (OS) |
| 21:54 | `grep -rhE '^Exec.*(python\|\.py\|node\|\.mjs\|venv)' /etc/systemd/system` | 0 lines |
| 21:54 | cron (`crontab -l`, `/etc/cron.d/*`) grep python/node | 0 lines |
| 21:54 | units whose text matches | `galion-one.service`, `galion-desk.service.d/rust-desk-hermes.conf`, `link-watch-rs.service` — comment/Description lines only |
| 21:55 | `systemctl is-active` | galion-one active, galion-desk active, link-watch-rs active, link-watch inactive (masked), memory-gateway inactive (masked); hermes.service masked |
| 21:55 | `systemctl show galion-desk -p ExecStart -p Environment` | ExecStart `/opt/galion/rust/bin/galion-desk`; `DESK_HERMES_BIN=/opt/galion/rust/bin/desk-hermes`; ActiveEnter 19:56:50 CET |
| 21:55 | `strings` old `/opt/galion/bin/galion-desk` | contains `python3` + `/opt/galion/bin/desk-hermes.py` (old caller, not ExecStart) |
| 21:55 | `strings /opt/galion/rust/bin/galion-desk` | no `.py`/`python`; default desk-hermes `/opt/galion/openmind/dist/bin/desk-hermes` (overridden by env) |
| 21:56 | `ss -ltnp` + `readlink /proc/<pid>/exe` | 16 listeners: Rust bins under /opt/galion/rust/bin (cortex, console-serve ×2 :8080/:18080, desk, health, one, hermes-gateway, memory, nerve), tanga-serve, tuner, openmind-queue, cloudflared, sshd, postfix, resolved |
| 21:56 | `strings` on 9 live binaries for venv/.py/.mjs/python/node | none, except `node_modules` in galion-one (source: dir-walk skip filter, main.rs:2436) |
| 21:55 | `file /opt/galion/bin/{pw,replicate-picture}` | symlinks → `/opt/galion/rust/bin/pw-check`, `/opt/galion/rust/bin/replicate-picture` (created 20:02:33 CET) |
| 21:55 | grep refs to pw.py / replicate-picture in hermes skills/home/scripts, /root/.hermes | only 10 old `/root/.hermes/sessions/request_dump_one-warm_*` (Oct 12–18 history) |
| 21:55 | `systemctl cat guardian-tick.service`, journal | ExecStart `/opt/galion/rust/bin/guardian-tick` (Rust), last run 21:54:54 CET exit 0 (`probes=11 failed=5`, report-only) |
| 21:55 | `systemctl list-timers` | no timer runs Python/Node |
| 21:55 | `du -sh` trees | see plan §4 (venvs 787 M, Hermes V1 ≈ 372 M, box-archive 11 G, tulu 4.6 G); `/` 30 G / 100 G used |
| 21:55 | nerve `GET 127.0.0.1:8670/state` | up; recent alerts: galion-one mouth stuck loading 3.2 h (inc-865945dd, 21:30 CET), edge tunnel incident — neither Python-related. Cortex live on :8674 |
| 21:56 | console `GET 127.0.0.1:8080` `/server/index.mjs`, `/node_modules/`, `/static/`, `/` | 404, 404, 404, 302 — Node SSR bundle not served |
| 21:56 | desk `GET 127.0.0.1:8646/` | 404 (root has no route; service active) |

Local checks on branch: `scripts/no-python.sh` (see commit), docs-only diff.
