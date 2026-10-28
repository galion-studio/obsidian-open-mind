<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Coding tools on a kernel without Landlock: staged options for D-320-1 (slot 334)

2026-10-28 CET (+01:00). STAGED, NOT INSTALLED. The choice is Slavik's. Nothing on the VPS reads these files.

## Problem (F-320-2)
The VPS (LXC on Proxmox kernel 6.8.12-43-pve, LSMs `lockdown,capability,yama,apparmor`) has no Landlock.
`hermes_tools::preflight` refuses the coding tools on such a kernel unless told otherwise, and
`deploy/staged/hermes-rs.service.d/coding-tools.conf` did not tell it. Installing that drop-in as staged
would make hermes-gateway exit 2 at start (`coding: refusing to start: the kernel has no Landlock`).
The real hermes-gateway binary did exactly that on the VPS in this slot (see the real-test doc).
`staged_preset.rs` used to build `exec::policy` directly and never called preflight, so its 8/8 missed it.

## What changed in code (swarm/coding-tools-landlock-stage)
- New `HERMES_RS_CODING_NO_LANDLOCK=refuse|allow|layered` (`CodingConfig::no_landlock`, enum `NoLandlock`).
  Default `refuse` = today's behaviour. A typo is a config error, so the gateway exits 2 (fail closed).
- Legacy `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` still means `allow`, but only while the new variable is unset.
  An explicit policy always wins, so `refuse` or `layered` cannot be weakened by a stray legacy variable.
- `preflight` now calls `preflight_with(cfg, spend, features, legacy_env)` with the real kernel probe.
  The pure gate is `landlock_gate`, which tests use with injected kernel features, so the staged preset
  goes through the real preflight code anywhere without starting a sandbox.
- Option c also has a per-run check: `exec::layered_check`, called by `exec::run` after every sandbox run.
  If Landlock was not enforced and any of `userns, mountns, pidns, net, ro_root, hide_home, workdir_quota`
  is not reported as enforced, the tool call fails closed and its output is withheld from the model.
  Limit: the command has already run by then. The preflight probe is what keeps an unsuitable kernel from
  starting at all, and the per-run check catches drift after that (for example, a sysctl turning userns off later).
- With Landlock present, all three modes behave the same (no warning, no per-run check).

## The three staged alternatives
Each one is a single `Environment=` line in `deploy/staged/hermes-rs.service.d/landlock-alternatives/`.
Install at most one, next to coding-tools.conf, as `/etc/systemd/system/hermes-rs.service.d/coding-tools-landlock.conf`.
Systemd does not read the `landlock-alternatives/` subdirectory itself.

| Option | File | VPS start (measured) | Isolation without Landlock | Cost / risk |
|---|---|---|---|---|
| (a) allow | `landlock-a-allow.conf` | tools on, `/health` 200 | user/mount/pid/net namespaces, ro root, hidden /opt/galion, /root, /home, masked /proc, workdir tmpfs: all measured enforced, but nothing checks them per run | Silent weakening if a layer stops working (for example, the userns sysctl). Starts even with `WORKDIR_MB=0` |
| (b) refuse | `landlock-b-refuse.conf` | exit 2, coding off | n/a (no tools) | Coding tools stay off on this VPS until a Landlock host exists (Proxmox host kernel `lsm=` change, host reboot: outside this repo) |
| (c) layered | `landlock-c-layered.conf` | tools on, `/health` 200 | same layers as (a), but start needs full unprivileged namespaces + workdir quota in every policy, and every run must report the 7 layers enforced | Small code path more. Any non-enforced layer fails that call. Recommended by slot 320 |

Slot 320's recommendation was (c), with (a) in the meantime. Now that (c) exists and passes on the VPS,
(a) is no longer needed as a bridge. This slot does not decide.

Before any of these goes live, D-318-1 (non-root 80-nonroot.conf) and the base hermes-rs unit still need their own approvals.
Coding is compiled in but off at run time today (no `HERMES_RS_CODING_WORKTREE` on the live unit), so there is no live impact either way.

## Rollback
`rm /etc/systemd/system/hermes-rs.service.d/coding-tools-landlock.conf && systemctl daemon-reload`, then the next
hermes-rs restart runs with the compiled default (refuse). Code rollback: revert this branch. Without the new
variable, the start decision is the same as before (only the message text is longer) (refuse, or allow with the legacy `=1`).
