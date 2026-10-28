<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-rs coding folders: staged tmpfiles.d fix (wave-4 slot 397)

Written 2026-10-28 16:50 CET (+01:00). Branch `swarm/hermes-coding-dir-stage` off
`swarm/crashloop-r3-rebase` 657a9988. **STAGED, NOT INSTALLED.** VPS read-only for this
work (plus a throwaway `/var/tmp/sandbox-test-397-*` as nobody, removed).

## Problem (slot 381 note)

The staged coding drop-in `deploy/staged/hermes-rs.service.d/coding-tools.conf` uses
`/srv/hermes-coding/{worktree,state}` with `ReadWritePaths=-/srv/hermes-coding`.
On the VPS `/srv/hermes-coding` does not exist (checked again 2026-10-28 16:40 CET; only
the 19:45 test leftovers `/srv/hermes-coding-worktree`, `-state`, `-audit.jsonl`,
`-worktree.tmpmove` exist, root 0755/0644). So:

- `hermes-gateway --check-config` with the drop-in env exits 2 (`No such file or directory`).
- Worse, the `-` prefix makes systemd silently skip a missing RW path: if the folder were
  created *after* the unit started, it would still be read-only inside the unit
  (ProtectSystem=strict) and coding would fail with `Read-only file system`, the
  second 19:46 CET failure mode.

## Fix (staged)

`deploy/staged/tmpfiles.d/hermes-coding.conf`: seven `d` lines, `0700 root root -`:
`/srv/hermes-coding`, `state`, `state/{stage,tmp,home,cargo-home,target}` (exactly what
`CodingConfig::prepare_dirs` would create; audit = `state/audit.jsonl`, its parent is `state`).

- **No ExecStartPre, no unit change.** `systemd-tmpfiles-setup.service` runs at boot
  before `sysinit.target`; every normal service (hermes-rs has default dependencies) is
  ordered after it, so the folders exist before the unit's mount namespace is built.
  On install, one manual `systemd-tmpfiles --create <file>` does the same at once.
- Owner `root:root`: live hermes-rs runs as root (`User=` unset, `DynamicUser=no`;
  VPS `systemctl show`, 16:40 CET). Mode 0700 = `StateDirectoryMode=0700`, `UMask=0077`.
- Type `d` only: creates, and **fixes mode/owner** if the folder already exists
  (tested: 0755 -> 0700); never empties. Age `-`: `systemd-tmpfiles-clean` never ages
  anything out. `--clean` and `--remove` keep the work (tested).
- The worktree is **not** created by tmpfiles: `worktree_ok` needs a real `.git`
  directory, so it stays a clone step (an empty folder would only change the error text).

## Runbook (replaces step 1 of docs/ops/hermes-rs-crashloop-r3.md; owner-approved only)

1. a. `install -m 0644 deploy/staged/tmpfiles.d/hermes-coding.conf /etc/tmpfiles.d/`
      then `systemd-tmpfiles --create /etc/tmpfiles.d/hermes-coding.conf`
      (expect rc 0; `ls -ld /srv/hermes-coding /srv/hermes-coding/state` = `drwx------ root root`).
   b. Clone the dedicated worktree: `git clone <approved source> /srv/hermes-coding/worktree`
      and `git -C /srv/hermes-coding/worktree switch -c hermes/ticket`
      (never `/opt/galion/openmind`; source repo is an owner call, see D-397-3).
   c. Toolchain: the staged drop-in says `HERMES_RS_CODING_TOOLCHAIN=/opt/rust/toolchain`,
      which does **not** exist on the VPS (F-397-1). Decide D-397-2 first.
2. `--check-config` with the exact env inside the unit's sandbox (step 2 of the R3 runbook,
   fixed binary needed; live b7fbbc78 is blind). Must print `coding=on ...` exit 0.
3. Then the R3 runbook steps 3-4 (drop-ins, daemon-reload, approved restart, watch
   journal + nerve `/state` hermes-rs healthy, restarts_1h 0).

Rollback: `rm /etc/tmpfiles.d/hermes-coding.conf` (folders stay; `rm -rf /srv/hermes-coding`
only if the owner says so). The tmpfiles file alone changes nothing for the live gateway:
coding stays off until the coding drop-in is installed.

## Findings

- **F-397-1** `/opt/rust/toolchain` is missing on the VPS (16:40 CET). With the staged
  drop-in as is, `--check-config` still exits 2 after the folder fix
  (`HERMES_RS_CODING_TOOLCHAIN ...: No such file or directory`; case D in the real test).
  The only toolchain found is `/root/.rustup/toolchains/stable-x86_64-unknown-linux-gnu`
  (readable under ProtectHome=read-only; no `cargo` on root's PATH).
- **F-397-2** The `-` in `ReadWritePaths=-/srv/hermes-coding` hides a missing folder; with
  tmpfiles in place it could be dropped so a missing folder fails loudly, but then the
  unit fails with 226/NAMESPACE (not exit 2, so 40-config-no-loop does not stop the
  restart loop) and chat goes down too. Kept as is: with `-` plus `ON_ERROR=disable`
  chat stays up and coding is off. Noted, no change.
- Live during the work: hermes-rs MainPID 255539, NRestarts 0, :8642 /health 200;
  nerve `/state` hermes-rs healthy, restarts_1h 0. `/srv/hermes-coding` still absent
  (nothing created on the real `/srv`).

## Owner decisions

- **D-397-1** Install `/etc/tmpfiles.d/hermes-coding.conf` + run `systemd-tmpfiles --create`
  (runbook 1a) before any coding drop-in. Recommended: yes (no live effect alone).
- **D-397-2** Toolchain for coding: (a) set `HERMES_RS_CODING_TOOLCHAIN=/root/.rustup/toolchains/stable-x86_64-unknown-linux-gnu`
  in the staged drop-in; (b) install a pinned toolchain at `/opt/rust/toolchain`;
  (c) leave coding off. Note RULES hard rule 4 (never build on the VPS) conflicts with
  Hermes running cargo on the VPS at all; owner call. Recommended: (c) until decided, then (b).
- **D-397-3** Clone source for `/srv/hermes-coding/worktree` (the 19:45 leftover
  `/srv/hermes-coding-worktree` is a README clone; D-355-4 still open on leftovers).
