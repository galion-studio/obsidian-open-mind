<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Retiring legacy Python Hermes: inventory, archive and removal runbook, rollback

- Slot: wave-4 163 `hermes-legacy-retire` (non-rust-inventory item #2, section 4)
- Probed: 2026-10-21 08:11 to 08:18 CEST (+02:00). Read-only ssh probes of the production VPS (called "the VPS" here; the address is
  kept out of the repo). Nothing on the VPS was stopped, disabled, changed, moved or deleted. No temp files were written there.
  Secret files (`.env`, `auth.json`, `/root/galion-secrets/*`, `/etc/memory-gateway.env`, `galion-light/secrets`) were never opened;
  only their names and paths are listed.
- VPS times from journald and `ps` are UTC on the box; every time below is converted to Europe/Zagreb (+02:00).

## TL;DR

1. **Legacy Python Hermes is dead and stays dead on its own today.** `hermes.service` is `disabled` and `inactive (dead)` since
   **2026-10-18 21:42:17 +02:00** (SIGTERM from systemd during the Rust switch; it ran 19:36:46 to 21:42:17 that evening). No Python
   Hermes process runs, nothing listens for it, and no unit, timer or cron starts it. `hermes-rs.service` (`/opt/galion/rust/bin/hermes-gateway`,
   pid 1361728 since 2026-10-18 22:00:49 +02:00) owns 127.0.0.1:8642: `GET /health` 200 in 0.4 ms, nerve `:8670/state` shows hermes-rs
   healthy, 0 systemd restarts, RSS about 4.4 MB.
2. **The code is safe to archive and remove:** `/usr/local/bin/hermes` (wrapper), `/opt/galion/venv` (has the editable `hermes_agent-0.21.4`
   install), `/opt/galion/tooling/hermes-agent` (heavily patched NousResearch checkout plus its own venv), `/usr/local/lib/hermes-agent`
   (symlink), `galion-light/shadow/hermes-agent`, and the `hermes.service` unit file. That is about **560 MB, about 19.7k .py files**.
   Nothing that runs today executes any of it (evidence in section 2).
3. **The data is NOT safe to remove.** `/opt/galion/hermes` (417 MB) is live input for hermes-rs (and its legacy `state.db` is opened at
   startup: if it is missing, hermes-gateway **will not start**). The console writes into it as well, and galion-desk and galion-one read
   from it. `/opt/galion/galion-light` is galion-one's working directory and `GALION_HOME`, link-watch-rs's read-write state directory,
   and the console's secrets/keys/guardian store. Both stay. Only their Python *code* subtrees can go later, with the console port.
4. **Two traps** to fix before or during retirement (section 4): starting `hermes.service` by mistake **stops hermes-rs** (the hermes-rs unit
   has `Conflicts=hermes.service`, and systemd applies conflicts both ways), then runs Python Hermes with `Restart=always` on the paid keys.
   And the dormant `galion-light/keep.sh` relaunches `hermes gateway run` with `nohup` if anyone runs it.

## 1. Inventory (state at 2026-10-21 08:11-08:18 +02:00)

| Item | What it is | State | Size | Used by something live? |
|---|---|---|---|---|
| `hermes.service` (`/etc/systemd/system/hermes.service`) | `Type=simple`, `Restart=always`, `EnvironmentFile=/etc/memory-gateway.env`, `HERMES_HOME=/opt/galion/hermes`, `ExecStart=/usr/local/bin/hermes gateway run --accept-hooks`, `WorkingDirectory=/opt/galion`, `WantedBy=multi-user.target` | **disabled, inactive (dead)** since 2026-10-18 21:42:17 +02:00 (exit 1 after SIGTERM). No drop-ins; nothing `Wants=`/`Requires=` it; no timer triggers it. Journal: no entries since the stop | 1 unit file | No. Referenced only by hermes-rs `Conflicts=` |
| `/usr/local/bin/hermes` | 8-line bash wrapper (Oct 8): sets `HERMES_HOME`, sources `/root/galion-secrets/hermes.env` if present, `exec /opt/galion/venv/bin/hermes "$@"` | idle | 193 B | No. Only caller is `hermes.service`; `stack-boot.sh` would regenerate it (dormant) |
| `/opt/galion/venv` | Python venv; `bin/hermes` -> `hermes_cli.main`; `__editable__.hermes_agent-0.21.4.pth` maps every module to `/opt/galion/tooling/hermes-agent/*` | idle | 185 MB, 3,681 .py | Only by the wrapper and by `/opt/galion/bin/pw.py` (Playwright helper, shebang into this venv; no unit, console route, config.yaml or cron job calls it) |
| `/opt/galion/tooling/hermes-agent` | NousResearch/hermes-agent checkout (MIT, `Copyright (c) 2025 Nous Research`), HEAD `fde4997` (2026-09-22). **Local tree is far from HEAD: 5,471 tracked files modified, 6,016 deleted** (`git --no-optional-locks status`). Contains a second venv (`venv/`, 101 MB, its own `bin/hermes`) | idle; newest file 2026-10-18 (last run) | 258 MB, 5,476 .py | No. Users: the editable install above, `/usr/local/lib/hermes-agent` symlink, dormant `keep.sh`/`stack-boot.sh`, manual `galion-light/scripts/flash_contest.py` |
| `/usr/local/lib/hermes-agent` | symlink -> `/opt/galion/tooling/hermes-agent` (Oct 8, made by `stack-boot.sh`) | — | — | No |
| `/root/.hermes` | symlink -> `/workspace/hermes` (= `/opt/galion/hermes`; `/workspace` is a symlink to `/opt/galion` on the VPS) | — | — | Harmless; keep (points at live data) |
| `galion-light/shadow/hermes-agent` | a shadow copy of the agent tree (used by `flash_contest.py`, manual) | idle; newest file 2026-10-05 | 114 MB, 7,094 .py | No |
| `galion-light/hermes-home` | an older Hermes home | idle; newest file 2026-10-08 | 150 MB, 109 .py | **Yes, indirectly:** console `src/lib/bank.ts` lists `galion-light/hermes-home/.env` as a fallback key file. Keep until the console no longer reads it |
| `/opt/galion/hermes` (`HERMES_HOME`) | Hermes home: `.env` (not read), `config.yaml`, `SOUL.md`, `channels/`, `cron/jobs.json`, `state.db` (40.8 MB), `sessions/`, `archive/` (267 MB), `uploads/`, `modes.json`, `loops.json`, `guard.json`, `spend.json`, `gateway_state.json`, ... | **live data** | 417 MB | **Yes, see section 3** |
| `/opt/galion/galion-light` (whole tree) | old Python stack + live state (`state/`, `secrets/`, `keys/`, `guardian/`, `one/`, `docs/build-session`); `tulu/` alone is 4.6 GB | **live data** | about 4.9 GB | **Yes, see section 3** |
| `galion-light/keep.sh`, `stack-boot.sh`, `persist-boot.sh`, `vps-boot.sh`, `start.sh` | the old pod/VPS boot and keep-alive loop | **not running**, not wired to boot (no rc.local, no unit/cron/profile reference) | — | No, but `keep.sh` is a trap (section 4) |

Not Hermes and out of scope here, but seen during the probe: the stray `/root/growth-20261018/bin/*.new` processes on :8432/:8433/:8652/:8653
from the non-Rust inventory are **gone** (nothing listens there now). The live Python on the VPS is only `networkd-dispatcher` and
`fail2ban-server` (both OS packages, separate rulings).

## 2. Is legacy Hermes active? Who calls it?

| Check (read-only) | Result |
|---|---|
| `systemctl is-enabled / is-active hermes.service` | `disabled` / `inactive` |
| `systemctl show hermes.service` | `NRestarts=0`, `WantedBy=`/`RequiredBy=`/`TriggeredBy=` empty, `InactiveEnterTimestamp` 2026-10-18 21:42:17 +02:00 |
| `journalctl -u hermes.service --since` the stop | `-- No entries --` |
| `ps` for python / hermes / venv / galion-light | only `networkd-dispatcher`, `fail2ban-server`, and the Rust `hermes-gateway` |
| `ss -ltnp` 84xx/86xx | 8642 = `hermes-gateway` (Rust) pid 1361728; no Python listener anywhere |
| established connections on :8642 at probe time | none (idle) |
| units/timers/cron referencing the wrapper, the venv or `tooling/hermes-agent` | only `hermes.service` itself. Root crontab empty; `/etc/cron.d` = e2scrub_all, sysstat |
| scripts referencing `/usr/local/bin/hermes`, `venv/bin/hermes`, `hermes_cli` | `/usr/local/bin/hermes` (itself), `galion-light/scripts/flash_contest.py` (manual contest tool, last edited 2026-10-05), dormant `keep.sh` / `stack-boot.sh` |
| console-ide source (`/opt/galion/console-ide/src`, read with grep) | talks to Hermes **only over HTTP 127.0.0.1:8642** (now hermes-rs). No spawn of the `hermes` CLI or the venv |
| `galion-desk` binary strings | no `hermes` CLI; it forks `python3 /opt/galion/bin/desk-hermes.py` (system python3, not the venv) and reads Hermes files (section 3) |

Verdict: **no live caller runs the legacy Python Hermes code.** Port 8642 consumers (console BFF, guardian probe, galion-one, nerve)
all get hermes-rs.

## 3. What still reads or writes the Hermes data (why `/opt/galion/hermes` and `galion-light` stay)

### hermes-rs (`hermes-rs.service`, active, enabled) still depends on `/opt/galion/hermes`. Yes.
From the unit (values of the secret variables not shown), the live process (`/proc/<pid>/environ` names and path values only, `/proc/<pid>/fd`)
and the source on `vps/ws-switch-gaps` 14595cc (the branch the live binary came from):

| Path | How | What breaks without it |
|---|---|---|
| `/opt/galion/hermes/.env` | `EnvironmentFile=` (API_SERVER_KEY, OPENROUTER_API_KEY, RIVER_API_KEY, TDAI_GATEWAY_API_KEY by name) | systemd refuses to start the unit; hermes-gateway also refuses without API_SERVER_KEY |
| `/opt/galion/hermes/config.yaml` | `HERMES_HOME/config.yaml` (crates/hermes-rs/src/config.rs:395-404) | falls back to env defaults (model and provider routing change) |
| `/opt/galion/hermes/SOUL.md`, `channels/<name>.md` | system prompt (persona.rs:58-60) | prompts lose the persona and channel rules |
| `/opt/galion/hermes/cron/jobs.json` | cron.rs:87 (cron is OFF in the unit; read only if enabled) | nothing today |
| `/opt/galion/hermes/state.db` | `HERMES_RS_LEGACY_STATE_DB`, opened read-only **at startup** as the legacy layer of `session::Layered` (hermes-gateway main.rs:45, `SqliteStore::open_ro(p)?`). **fd is open now** | missing file -> open error -> `?` -> **hermes-gateway fails to start** on its next restart. Old sessions vanish from history |
| `ReadOnlyPaths=/opt/galion/hermes` | sandbox | — |
| own state | `/var/lib/hermes-rs/` (rs-state.db, rs-spend.json, rs-cron-*) | — |

Spend caps on the live unit (non-secret values): session 1.00 USD, daily 2.00 USD, daily calls 500, MCP 0. (Not changed; noted because the
retirement must not move the paid-model path back to Python, which has none of these caps.)

### Others
| Reader/writer | Paths | Mode |
|---|---|---|
| **console.service** (node/vite dev, `/opt/galion/console-ide`) | `/workspace/hermes/{config.yaml, modes.json, loops.json, guard.json, uploads/, spend.json}`; `state.db` via `execFileSync("python3", ["-c", ...sqlite3...])` (src/lib/hermes.ts:477, :610-619: insert/update rows in `sessions`); `bank.ts` key and spend files; `galion-light/{secrets/persist, keys, guardian/guardian.db, docs/build-session, state/survival.json, state/machine.json, logs/hermes.log}`; `loops.ts` spawns `python3 galion-light/loop/loop.py stop` | **read + write**. Every write to `/opt/galion/hermes` after the legacy stop came from here: config.yaml, modes, loops, guard, memories/USER.md, uploads, spend, state.db, all 2026-10-18 22:12 to 23:57 +02:00. **0 files written in `/opt/galion/hermes` since 2026-10-19 00:00 +02:00** |
| **galion-desk** (`/opt/galion/bin/galion-desk`, Oct 10 build) | `/opt/galion/hermes/gateway_state.json` (**stale: last written by Python Hermes 2026-10-18 21:42 +02:00**), `galion-light/state/{link,survival,machine}.json`, `desk-focus`, `desk-hold`, `desk-notes.jsonl`, `galion-light/one/session.jsonl`, `one/voice`; forks `python3 /opt/galion/bin/desk-hermes.py`, which reads `state.db`, `activity.jsonl`, `memories/MEMORY.md`, `tasks/one-galion.*`, caches | read (desk-hermes may write its caches) |
| **galion-one** | `WorkingDirectory=/opt/galion/galion-light`, `GALION_HOME=/workspace/galion-light`; reads `/workspace/hermes/modes.json` (docs/legacy-python.md) | read + its own state under galion-light/one |
| **link-watch-rs** | `ReadWritePaths=/opt/galion/galion-light/state` (`link.json` last written 2026-10-21 08:12 +02:00) | read + write |
| hermes-rs cron (off) | would run jobs with `HERMES_HOME` | — |

## 4. Traps found (unsafe states, nothing changed)

1. **`systemctl start hermes.service` takes the gateway down.** `hermes-rs.service` has `Conflicts=hermes.service`. systemd treats it as
   two-way, so starting the legacy unit stops hermes-rs, and then `Restart=always` keeps Python Hermes up on 8642 with the paid keys from
   `/root/galion-secrets/hermes.env` + `/etc/memory-gateway.env`, **without the hermes-rs spend caps**. The unit is only `disabled`, not
   `masked`, so any `start` (by a person, a script or an agent) does this. Fix: phase 1 masks it.
2. **`galion-light/keep.sh` is a dormant relauncher.** If run, it puts `/workspace/tooling/hermes-agent/venv/bin` on PATH, sources
   `/workspace/hermes/.env`, and, when `pgrep -f "hermes gateway run"` finds nothing (always true now), runs
   `nohup hermes gateway run --accept-hooks &`. Python would then fight hermes-rs for 8642. `stack-boot.sh` also rewrites
   `/usr/local/bin/hermes` and the `/usr/local/lib/hermes-agent` and `/root/.hermes` links. Neither is wired to boot today. Fix: archive in phase 2.
3. **galion-desk shows frozen Hermes state.** `gateway_state.json` has not changed since 2026-10-18 21:42 +02:00, so the desk's Hermes
   panel reports the dead Python gateway. Code follow-up (not this slot): have galion-desk read hermes-rs `GET 127.0.0.1:8642/health`.
4. **Python still runs from the console**, outside Hermes: `python3 -c` sqlite writes into the legacy `state.db`, and `loop.py`. These go
   away with the console-static / console port, not with this retirement.

## 5. Runbook: archive, then remove (owner runs it; NOT done by this slot)

Rules for whoever runs it: one phase at a time; keep each phase's archive until the soak window passes; never open or print `.env` /
`auth.json` / `secrets` content (tar copies bytes without reading them into the terminal); never touch `/opt/galion/openmind`.
`TS=$(TZ=Europe/Zagreb date +%Y%m%dT%H%M%S%z)`; archive root `A=/var/backups/hermes-legacy-$TS` (`install -d -m 0700 "$A"`). Disk: 75 GB
free of 100 GB, the archive needs about 0.6 GB (phase 2) and at most 0.8 GB with phase 4.

### Phase 0: preflight (read-only, 2 min)
```
systemctl is-active hermes-rs && curl -fsS -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8642/health   # expect active / 200
systemctl is-active hermes.service; systemctl is-enabled hermes.service                                    # expect inactive / disabled
pgrep -af 'hermes gateway run|keep.sh|stack-boot' || echo none                                             # expect none
ss -ltnp | grep ':8642'                                                                                    # expect hermes-gateway only
```
Abort if any check differs.

### Phase 1: make an accidental start impossible (seconds, fully reversible)
```
systemctl mask hermes.service        # unit file is a regular file in /etc: move it to the archive first (mask cannot overwrite it)
```
Exact order: `install -m 0600 /etc/systemd/system/hermes.service "$A/hermes.service"` -> `rm /etc/systemd/system/hermes.service` ->
`systemctl mask hermes.service` (creates the `/dev/null` link) -> `systemctl daemon-reload`. hermes-rs keeps running (its `Conflicts=` on a
masked unit is a no-op). Check: `systemctl is-enabled hermes.service` = `masked`, hermes-rs `NRestarts` unchanged, 8642/health 200.
Rollback: `systemctl unmask hermes.service && install -m 0644 "$A/hermes.service" /etc/systemd/system/ && systemctl daemon-reload`.

### Phase 2: archive the code and the traps (5 min, no service impact)
```
tar -C / --numeric-owner --xattrs --acls -cpf "$A/hermes-code.tar" \
  usr/local/bin/hermes usr/local/lib/hermes-agent opt/galion/venv opt/galion/tooling/hermes-agent \
  opt/galion/galion-light/shadow/hermes-agent \
  opt/galion/galion-light/keep.sh opt/galion/galion-light/stack-boot.sh
sha256sum "$A/hermes-code.tar" > "$A/SHA256SUMS"; tar -tf "$A/hermes-code.tar" | wc -l
```
A tar of the live tree is required: the checkout's local patches (5,471 modified / 6,016 deleted files vs HEAD `fde4997`) cannot be
recreated from upstream. Optionally copy the tar off-box with the existing offsite path (owner decision D3). Leave `/opt/galion/bin/pw.py`
in place: its venv is going, so it would stop working; it is a manual tool with no caller (decision D1).

### Phase 3: remove the code (after a 7-day soak with phase 1, 1 min)
```
rm /usr/local/bin/hermes /usr/local/lib/hermes-agent
rm -rf /opt/galion/tooling/hermes-agent /opt/galion/venv /opt/galion/galion-light/shadow/hermes-agent
mv /opt/galion/galion-light/keep.sh /opt/galion/galion-light/stack-boot.sh "$A/"   # already inside the tar; mv keeps a loose copy
```
Then check: hermes-rs active, 8642/health 200, nerve `:8670/state` hermes-rs healthy, console `/hermes` panel answers, galion-desk `/desk`
answers, `ss -ltnp` unchanged, `systemctl --failed` unchanged. Frees about 560 MB.
Rollback (about 2 min): `tar -C / -xpf "$A/hermes-code.tar"` (restores files, links and modes), then phase 1 rollback if Python Hermes
must actually run again, then `systemctl start hermes.service` (**this stops hermes-rs, by design of `Conflicts=`**). Full fallback to
Python also exists as `/root/galion-switch-*/rollback.sh` from the 2026-10-18 switch.

### Phase 4: data. NOT now (blocked)
`/opt/galion/hermes` and `/opt/galion/galion-light` stay until all of these hold:
- hermes-rs no longer needs the legacy `state.db` (old sessions migrated into `/var/lib/hermes-rs/rs-state.db`, then
  `HERMES_RS_LEGACY_STATE_DB` dropped from the unit), and its `.env`/`config.yaml`/`SOUL.md`/`channels` move to a hermes-rs-owned path;
- the console no longer writes there (console port; removes the `python3 -c` sqlite writes);
- galion-desk reads hermes-rs health instead of `gateway_state.json`, and desk-hermes runs the Rust `crates/desk-hermes`;
- galion-one and link-watch-rs get a non-legacy home (`GALION_HOME` and `state/` move).
After that, archive-then-remove can split per subtree: `galion-light/hermes-home` (150 MB, once bank.ts stops listing its .env),
`galion-light/tulu` (4.6 GB, no live reader found, needs its own check), `/opt/galion/hermes/archive` (267 MB) etc. Same pattern:
tar with `--numeric-owner`, sha256, soak, remove, rollback = untar.

## 6. Owner decisions (Slavik)

| # | Decision | Options | Recommendation |
|---|---|---|---|
| D1 | Approve phases 1-3 (mask unit, archive code, remove code after soak) | (a) all three; (b) phase 1 only now (closes trap 1), rest later; (c) leave as is | (b) now, then (a) after 7 days. Phase 1 is a one-line reversible change that removes the only path back to uncapped paid Python |
| D2 | `/opt/galion/bin/pw.py` (Playwright helper on the venv) | archive with the venv (breaks it), or port it later | archive; no caller found |
| D3 | Where the archive lives | VPS `/var/backups` only; plus the existing offsite backup | both; the code tar has local patches that exist nowhere else |
| D4 | Order of the data retirement (phase 4) | as listed, or tie it to the console port | tie it to the console port; it removes most blockers at once |
| D5 | Fix galion-desk's stale Hermes panel (trap 3) | small Rust change in galion-desk to probe hermes-rs `/health` | yes, as a separate swarm slot |

## 7. Evidence log (commands were read-only; outputs abbreviated, no secret values)

- 08:11:46 +02:00 `systemctl status hermes.service`: `Loaded: ... disabled; preset: enabled`, `Active: inactive (dead) since Sun 2026-10-18 19:42:17 UTC`
  (= 21:42:17 +02:00), `Duration: 2h 5min 30.823s`, `Main PID: 1217064 (code=exited, status=1/FAILURE)`, last lines "Shutdown context: signal=SIGTERM under_systemd=yes".
- `systemctl list-unit-files | grep -i hermes`: `hermes-rs.service enabled`, `hermes.service disabled`. `systemctl list-timers --all | grep -i hermes`: none.
- `ss -ltnp`: `127.0.0.1:8642 users:(("hermes-gateway",pid=1361728,fd=5))`; also 8420 openmind-memory, 8646 galion-desk, 8650 tuner, 8660 galion-health, 8670 nerve, 8671 tanga-serve, 8672 nerve-shadow.
- `/proc/1361728/fd`: `/opt/galion/hermes/state.db`, `/var/lib/hermes-rs/rs-state.db`.
- `curl http://127.0.0.1:8642/health` -> `200 0.000422s`. Nerve `:8670/state` hermes-rs entry: `http 200, n_restarts_systemd 0, mem_kb 4732, mem_max_kb 65536, port 8642`.
- `find /opt/galion/hermes -newermt "2026-10-19 00:00 +0200" -type f | wc -l` (excluding .env) -> `0`.
- `git --no-optional-locks status --porcelain` in tooling/hermes-agent: `5471 M`, `6016 D`, `2 ??` (no index write).
- `du -sh`: tooling/hermes-agent 258M, its venv 101M, /opt/galion/venv 185M, shadow/hermes-agent 114M, hermes-home 150M, /opt/galion/hermes 417M, galion-light/tulu 4.6G. `df -h /`: 100G, 26G used, 75G free.
