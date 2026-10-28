<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: hermes-coding-dir-stage (wave-4 slot 397)

All times Europe/Zagreb CET (+01:00). No secrets. Port 22670 only passed as config
(`--check-config` never binds). Scratch in `$TMPDIR`, removed by the tests.

## What is tested

The staged `deploy/staged/tmpfiles.d/hermes-coding.conf`, applied with the real
`systemd-tmpfiles --create --root=<sim root>` (sim root has its own `etc/passwd`/`group`;
as a non-root user `root root` is swapped for the test user, everything else
byte-identical), then the real `CodingConfig` / release `hermes-gateway --check-config`
on the staged coding drop-in env remapped under the sim root.

| test | what |
|---|---|
| `hermes-tools --test staged_tmpfiles` (4 static, always) | only `d`, 0700, root root, age `-`, no dupes; covers state + stage/tmp/home/cargo-home/target + audit parent + the optional RW root + worktree parent; nothing in the worktree, all in ReadWritePaths, none in DENY_ROOTS; unit + drop-in have no ExecStartPre / DefaultDependencies=no |
| `staged_tmpfiles::sim_root_*` (GALION_REAL=1) | create x2 rc 0 (idempotent), all 0700; 0755 -> 0700 fixed; `--clean` + `--remove` keep a file in stage; `git init` worktree; `from_env` ok, `write_problems` empty, `prepare_dirs` creates **nothing new**; bare sim root -> `No such file` |
| `hermes-gateway --test real_coding_dir_stage` (GALION_REAL=1, release musl bin) | A-F below |

Mutation check (box, 16:44 CET): removing the `target` line, `d`->`D`, 0700->0755, or
removing the `/srv/hermes-coding` line each make `staged_tmpfiles` fail (2/2/2/1 failures).

## Box, 2026-10-28 16:45 CET (uid 1000, systemd-tmpfiles 257)

```
A missing dir: exit=2 coding=error | coding: config error: HERMES_RS_CODING_WORKTREE <sim>/srv/hermes-coding/worktree: No such file or directory (os error 2)
B dirs, no clone: exit=2 coding=error | ... worktree: No such file or directory
C dirs + clone: exit=0 coding=on worktree=<sim>/srv/hermes-coding/worktree branch=hermes/ticket state=<sim>/srv/hermes-coding/state target=.../state/target audit=.../state/audit.jsonl on_error=disable
D staged toolchain missing: exit=2 coding=error | HERMES_RS_CODING_TOOLCHAIN <sim>/opt/rust/toolchain: No such file or directory
E toolchain present: exit=0 coding=on ... on_error=disable
F modes all 0700, no audit.jsonl created by --check-config
```
staged_dropin 4/0, staged_tmpfiles 5/0, real_coding_dir_stage 1/0.

## VPS throwaway as nobody (setpriv 65534), 2026-10-28 16:46:06-16:46:10 CET

`/var/tmp/sandbox-test-397-20261028164606` (nobody 0700), static x86_64 musl binaries
piped in over ssh (hermes-gateway sha256 d409d8030588d3d4..., tests 8f0078e3.../64649fb0.../4fabd951...),
VPS `systemd 255 (255.4-1ubuntu8)`.

| suite | result |
|---|---|
| staged_dropin | 4 passed / 0 failed |
| staged_tmpfiles (incl. sim root on systemd-tmpfiles 255) | 5 / 0 |
| real_coding_dir_stage | 1 / 0, A exit 2, B exit 2, C exit 0 `coding=on`, D exit 2 (toolchain), E exit 0 |

After: folder removed, 0 nobody processes, `/srv/hermes-coding` still absent (real /srv
untouched), hermes-rs MainPID 255539 NRestarts 0 active, :8642 /health 200, nerve :8670
`/state` 200 with hermes-rs `healthy`, restarts_1h 0, status_since 2026-10-27 21:00:01 +01:00.
Cortex not consulted (observed through the nerve per standing rule).

## Not tested

- Real ownership `root:root` under an unprivileged run (chown to root impossible as
  uid 1000/65534; owner asserted statically and matches the live unit's root user).
  No user namespace used to fake it (kept clear of sandbox_vps_only).
- Inside the real unit namespace (`systemd-run -p ProtectSystem=strict ...`): needs
  root on the VPS = a live change; that is runbook step 2 at install time.
