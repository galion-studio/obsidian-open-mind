<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# STAGED, awaiting the owner's OK: hermes-rs as a dedicated non-root user (D-SB4)

Written 2026-10-27 22:40 CET (+01:00), slot 318, branch `swarm/sandbox-nonroot-stage`, based on
`swarm/sandbox-dsb-pack` 3289511d (slot 305). **Nothing is installed.** No live unit, user,
file mode or service on the VPS was changed. The evidence is in `docs/real-tests/sandbox-nonroot-stage.md`.

## Files
| file | what it is |
|---|---|
| `deploy/staged/hermes-rs.service.d/80-nonroot.conf` | drop-in for the **live** `hermes-rs.service` (written against its 50/60/70/90 drop-ins as they are tonight) and the staged base unit |
| `deploy/staged/sysusers.d/hermes-rs.conf` | creates the system user `hermes-rs` (home `/var/lib/hermes-rs`, shell nologin) |
| `deploy/staged/hermes-rs.service.d/coding-tools.conf` | now pins `HERMES_RS_CODING_WORKDIR_MB=512` (D-SB pack item 2) |
| `crates/hermes-gateway/tests/nonroot_stage.rs` | 6 unit tests on the staged files and config loading, plus 1 VPS-only real test |

## Why a bare `User=hermes-rs` would break production
Live layout on 2026-10-27: `/opt/galion/hermes` is `drwx------ root:root`. The live gateway
(MainPID 255539, Uid 0, all capabilities) reads two things from it:
1. **`config.yaml`** (mode 666, 111389 B). A non-root process cannot even stat it, and
   `Config::load` skips a file it cannot stat **without an error**. On the VPS as uid 65534,
   `--check-config` was byte-identical to pure defaults: `routes=[]` and `aliases=[]`, against
   1 route and 4 aliases as root. So the gateway would come up healthy while routing models wrong.
2. **The legacy Python `state.db`** (mode 600, 40779776 B, last written 2026-10-18 22:12 CET;
   `hermes.service` is masked). `open_ro` fails, and the gateway exits with status 2 after
   22 ms: `session store error: /opt/galion/hermes/state.db: unable to open database file`. Under
   `Restart=on-failure` this becomes a crash loop, and the Hermes API on :8642 goes down.

`EnvironmentFile=/opt/galion/hermes/.env` is fine: systemd reads it as root before it drops privileges.
Port 8642 is above 1024. `/opt/galion/rust/bin` is 755.

## What the drop-in does (host permissions stay as they are)
- `User=hermes-rs`, `Group=hermes-rs`. This is a static user, like the existing `gaia-cortex`
  and `openmind-queue` users. `DynamicUser=` was not used, because it moves state to
  `/var/lib/private` and gives the coding worktree an unstable uid.
- `LoadCredential=hermes-config.yaml:/opt/galion/hermes/config.yaml` plus
  `Environment=HERMES_RS_CONFIG=%d/hermes-config.yaml`. systemd copies the file as root into the
  service's private credentials directory. The gateway reads its config only at start, so
  nothing changes. Limit: 1 MiB per service in systemd 255 (`CREDENTIALS_TOTAL_SIZE_MAX`).
- `ExecStartPre=+/usr/bin/install -C -m 0400 -o hermes-rs -g hermes-rs /opt/galion/hermes/state.db /var/lib/hermes-rs/legacy-state.db`
  plus `HERMES_RS_LEGACY_STATE_DB` pointing at that copy. The file is too big for a credential.
  The `+` runs this one step as root without sandboxing, and `-C` skips the copy when nothing changed.
  The DB is frozen, so a copy taken at start is the same as the live read.
- `StateDirectory=hermes-rs` (already in the unit) re-owns `/var/lib/hermes-rs` (root 700 today:
  `rs-state.db`, `rs-spend.json`, `coding-state/`, `coding-audit.jsonl`) to `hermes-rs` on the
  first start. `UMask=0077` stays. The real test shows the new files come out as 600 and owned by the service user.
- No `RestrictNamespaces=`, `PrivateUsers=` or `ProtectControlGroups=`. The hermes-tools sandbox
  was proven as non-root on the VPS (staged_preset 8/0 tonight as uid 65534). With
  `coding-tools.conf`, `Delegate=` hands the unit cgroup to `hermes-rs`, which is what
  `HERMES_RS_CODING_CGROUP=delegate` needs. That part is still unexercised (see below).

## Install (owner OK needed; this is a live unit change)
```
install -m 0644 deploy/staged/sysusers.d/hermes-rs.conf /etc/sysusers.d/hermes-rs.conf
systemd-sysusers /etc/sysusers.d/hermes-rs.conf
install -m 0644 deploy/staged/hermes-rs.service.d/80-nonroot.conf /etc/systemd/system/hermes-rs.service.d/
systemctl daemon-reload && systemctl restart hermes-rs
# checks: systemctl show hermes-rs -p MainPID,NRestarts,User ; grep Uid /proc/<pid>/status
#         curl -s 127.0.0.1:8642/health ; journalctl -u hermes-rs -n 20 (no "session store error")
#         ls -ln /var/lib/hermes-rs ; nerve /state hermes-rs healthy
```
Rollback (about 5 s of :8642 downtime): `rm /etc/systemd/system/hermes-rs.service.d/80-nonroot.conf && systemctl daemon-reload && systemctl restart hermes-rs`.
Root reads the hermes-rs-owned files, and StateDirectory= gives them back to root. You can keep the user or `userdel hermes-rs` it.

## Related, staged as a command only (not run): `link.env` mode 666
`/opt/galion/galion-light/publish/obsidian/link.env` (root:root **666**) holds the
`GALION_EAR/MOUTH/MIND/EYE` host:port links. It is written by `install.sh` (`cat > link.env`, no
umask or chmod) and **sourced** by `play.sh` (`. ./link.env`). Because it is world-*writable*, any
local user or process can put shell code in it, and that code runs as whoever next runs `play.sh`
(root on this host). That is a local privilege escalation, not just a read leak. The
hermes-tools hide layer already keeps it away from sandboxed tools (proven again tonight:
`LINKENV=HIDDEN` for Fs, Git and Cargo). Proposed fix (owner OK):
```
chmod 600 /opt/galion/galion-light/publish/obsidian/link.env   # root-only readers today
# and in install.sh: (umask 077; cat > link.env << EOF ... EOF)
```
Use 644 instead of 600 if a non-root reader appears. The fix is the write bit, not the read bit.
Last read (atime) 2026-10-27 19:16 CET, last change 2026-09-26 18:21 CEST.

## Not tested yet (honest gaps)
- systemd actually starting the unit with `User=hermes-rs`. That needs the live restart, so it is
  the owner's call. `systemd-analyze verify` on the VPS loaded the live unit, all 4 live drop-ins and
  80-nonroot.conf with rc 0 and no warnings. It is sensitive: a `User=nobody` variant gave the "Special user nobody" warning.
- The `%d` expansion in `Environment=` (systemd ≥ 248; documented) and the credentials dir at run time.
- The delegated cgroup under a non-root `User=` (coding tools are still OFF live).
- The euid-0 path of the sandbox, which this change makes unnecessary.
