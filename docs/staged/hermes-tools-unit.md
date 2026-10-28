<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# STAGED, awaiting Slavik's approval: unit changes for hermes-rs coding tools

Written 2026-10-20 (Europe/Zagreb, +02:00) on branch `swarm/hermes-tools`. **Nothing here is
installed, enabled or running.** No VPS file, unit or Hermes config was touched.

File: `deploy/staged/hermes-rs.service.d/coding-tools.conf`, a drop-in for the already-staged
`deploy/staged/hermes-rs.service`, which is itself still awaiting approval.

## What changes, and why
| setting | today (staged unit) | proposed | why |
|---|---|---|---|
| `MemoryMax` | 64M | 2304M (unit total) | rustc and the linker need about 1-2 GiB. The gateway keeps 64M through its own `gateway` leaf cgroup (`HERMES_RS_CODING_GATEWAY_MEM_MB=64`) |
| `Delegate` | none | `memory pids cpu` | lets hermes-rs create one cgroup per tool run with `memory.max`, `pids.max` and `cpu.max`, then kill the whole tree with `cgroup.kill` |
| `TasksMax` | 64 | 512 | cargo `-j 2` plus rustc threads; each run is still capped by `pids.max` (cargo 256, git 32, fs 16) |
| `CPUQuota` | none | 200% | hard ceiling for the whole unit on the VPS; each cargo run gets `cpu.max` = 1 CPU, `nice 19`, `-j 2` |
| `Nice`, `IOSchedulingClass` | none | 10, idle | production services win every contention |
| `MemorySwapMax` | none | 0 | no swap escape |
| `HERMES_RS_CODING_*` | none | worktree `/var/lib/hermes-rs/work/openmind`, branch `hermes/ticket`, state `/var/lib/hermes-rs/coding`, toolchain `/opt/rust/toolchain` | tools are off without these |
| `ProtectSystem=strict`, `ProtectHome=read-only`, `NoNewPrivileges`, `PrivateTmp`, `ReadWritePaths=/var/lib/hermes-rs` | as is | **unchanged** | the worktree and coding state live under the existing StateDirectory |

The unit must NOT gain `RestrictNamespaces=`, `PrivateUsers=` or `ProtectControlGroups=`. The
sandbox needs unprivileged user, net, pid and mount namespaces, and write access to its own
delegated cgroup. `ProtectSystem=strict` leaves `/sys` alone, so that part is fine.

## One-off preparation (only after approval, by the operator, not by hermes)
1. `git clone` GitHub `galion-studio/openmind` into `/var/lib/hermes-rs/work/openmind` and
   `git checkout -b hermes/ticket`. This is a **separate clone**. `/opt/galion/openmind` (live,
   2f5a730) is never touched, and the gateway refuses it as a worktree in code.
2. Copy a Rust toolchain to `/opt/rust/toolchain` (read-only, so not under `/root/.rustup`) and
   prefetch crates into `/var/lib/hermes-rs/coding/cargo-home` (`cargo fetch` as the operator).
   The tool builds `--offline` only.
3. Pushes stay with the owner: hermes-rs has no push tool. A finished ticket is a commit on
   `hermes/ticket` that the owner reviews and pushes with `push-branch.sh`.

## Still needs an owner decision
- **Paid model use.** The staged unit runs `--mock`. A real ticket needs a real model, which
  means OpenRouter spend under the existing $1.00 per session and $2.00 per day caps. Not approved, not proposed here.
- **User.** The staged unit runs as root (no `User=`). Recommended: a dedicated `hermes-rs` user
  owning `/var/lib/hermes-rs`. The sandbox works either way, but a non-root owner shrinks the
  blast radius of a sandbox bug.
- **`delegate` mode is not exercised on the dev box**, whose cgroups are root-owned. The
  first run on a delegated unit should be a `--mock` smoke test: check `systemd-cgls` for
  `hermes-rs.service/gateway` and an `om-sbx-*` child during a cargo call.

## Rollback
`rm /etc/systemd/system/hermes-rs.service.d/coding-tools.conf && systemctl daemon-reload`.
Without `HERMES_RS_CODING_WORKTREE` the gateway behaves exactly as before: the same tools
(clock, delegate_task, MCP) and no cgroup changes.
