<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: hermes-tools sandboxed.rs re-run on the VPS (slot 320, hermes-tools-vps-retest)

Why: slot 294 (release-size-trim, b2d15657) ran `cargo test --workspace` without
`--exclude hermes-tools`, so the 10 `crates/hermes-tools/tests/sandboxed.rs` tests
(they start openmind-sandbox) ran on the box. That broke `sandbox_vps_only`.
This slot re-runs them where the rule says: on the VPS, as uid/gid 65534, in a
throwaway `/var/tmp/sandbox-test-<ts>`. No sandbox was started on the box. NO deploy.

## Binary (built on the box, never run there)
- `cargo-slot test -j 2 --release --locked --target x86_64-unknown-linux-musl -p hermes-tools --test sandboxed --no-run`
  on b2d156570297c2431fe7fe468c89033b40066388 (head) and on its base 98466ab9 (control).
- Both test binaries are **byte-identical**: sha256 `173c2683202f132c516d452e73ec42ba75e036e10ebc05653fc90e71dd171e43`, 1 319 840 B.
  The only b2d15657 change that reaches hermes-tools/openmind-sandbox/hermes-rs is `.cargo/config.toml`
  (SQLite flags + C `CFLAGS` unwind-off). This test binary links no C code from those paths,
  so release-size-trim cannot change sandboxed behaviour here. The VPS runs below therefore
  cover head and base at once.

## VPS (slavik-vps, kernel 6.8.12-43-pve LXC, LSMs `lockdown,capability,yama,apparmor`: no Landlock)
Folder `/var/tmp/sandbox-test-20261027-225458` (root 0755, chown 65534). Every run:
`setpriv --reuid=65534 --regid=65534 --clear-groups env -i PATH=/usr/bin:/bin HOME=<dir> TMPDIR=<dir> ... ./sandboxed --test-threads=1 --nocapture`.
No temp user, no cgroup, no systemd-run.

Baseline 22:55:01 CET (+01:00): hermes-rs MainPID 255539 NRestarts 0, galion-desk 264677/0,
galion-nerve 184014/0, nerve `127.0.0.1:8670/state` 200.

| Run | Time (CET +01:00) | Env | Result |
|---|---|---|---|
| 1-3 | 22:55:16-22:55:17 | plain (as the rule's minimal form) | **2 passed, 8 failed** each (0.21-0.24 s). All 8 fail in `hermes_tools::preflight`: `the kernel has no Landlock; refusing coding tools (HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1 overrides)`. The 2 "passes" are the GALION_REAL/small-fs SKIPs. Fail-closed by design, see F-320-1. |
| A1-A3 | 22:55:52, 22:55:54, 22:55:55 | + `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` (same as slots 176/185 on 2026-10-21) | **10 passed, 0 failed** each (1.62 / 1.37 / 1.44 s); the 2 heavy tests SKIP |
| B1-B2 | 22:56:07, 22:56:11 | + `GALION_REAL=1` + `HERMES_TOOLS_SMALL_FS=<dir>/small` (root-mounted 32 MiB tmpfs, uid 65534, removed after) | **10 passed, 0 failed, 0 skipped** each (3.83 / 3.99 s) |

Numbers from run B1 (B2 the same within a few bytes / ms):
| Case | Result |
|---|---|
| read/write/git commit on branch, escapes refused+audited, wrong branch refused, spend/call caps, layers reported | ok |
| default 512 MiB cap on, 5 runs | each `limit_mib 512`, first run in 27 473 B, tmpfs used 143 360 B, 9 B back, `out_complete true` |
| 16 MiB cap, 13 658 961 B worktree, fs_write 4 MiB | `/usr/bin/install: ... No space left on device`; host 16 665 425 B, partial over.bin 3 006 464 B |
| preflight 20 MiB worktree | cap 16 refused, cap 32 accepted |
| **real default 512 MiB cap**, 509 MiB worktree | fs_read ok 671 ms; fs_write 4 MiB -> ENOSPC in 738 ms; used 536 870 912 B (= cap); host 536 759 119 B; copy back complete |
| D-WDQ3 32 MiB host fs, cap 64, 26 MiB worktree | tool error "worktree copy back is INCOMPLETE: the host file system is full; 1 entries / 1933312 bytes reached the host"; `out_complete false`, errno 28; part.bin 1 953 792 B kept |

These match the 2026-10-21 VPS numbers (docs/real-tests/hermes-tools-quota.md, sandbox-quota-hide-merge.md: 10/10).

Cleanup 22:56:24 CET: tmpfs unmounted, folder removed; 0 `/var/tmp/sandbox-test-*`, 0 such mounts,
0 uid-65534 `sandboxed` processes, 0 `hermes-tools-sbx-*` leftovers in /tmp or /var/tmp.
After: hermes-rs 255539/0, galion-desk 264677/0, galion-nerve 184014/0 (unchanged), nerve /state 200,
hermes-rs :8642/health 200. Cortex: live gaia-cortex not queried here (nerve used per rule).

## Findings
- **F-320-1 (box result was not representative).** Slot 294's 10/10 on the box passed because the box
  kernel has Landlock. On the VPS kernel (no Landlock LSM) the same binary refuses the coding tools
  unless `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`. That is correct fail-closed behaviour, but:
- **F-320-2 (staged preset gap).** `deploy/staged/hermes-rs.service.d/coding-tools.conf`
  (swarm/sandbox-nonroot-stage a1255fc0) does not set `HERMES_RS_CODING_ALLOW_NO_LANDLOCK`.
  If coding tools are switched on with that preset on this VPS, `preflight` refuses them at start
  (no tools offered). staged_preset.rs builds `exec::policy` directly and never calls `preflight`,
  so its 8/8 does not catch this. Live today: coding compiled in but runtime off (slot 294 VPS RO), so no live impact.
- No regression from release-size-trim b2d15657 (identical test binary, 10/10 on the VPS).

## Owner decisions
- **D-320-1** For coding tools on this LXC kernel: (a) add `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` to the
  staged preset (the mount namespace, userns and the other Enforced layers carry the isolation, as tested here), or
  (b) keep refusing until a host with Landlock (kernel LSM list needs `landlock`; Proxmox host change), or
  (c) let preflight accept no-Landlock only when userns+mountns+workdir_quota are all Enforced. Recommendation: (c), then (a) meanwhile.
- **D-320-2** Make the workspace test recipe exclude hermes-tools by default (e.g. `cargo xtask test`
  or a `[workspace] default-members` without sandbox crates) so the slot-294 slip cannot recur.

## Not tested
- euid 0, delegated cgroup path, aarch64 (no aarch64 VPS). real_ticket.rs (needs a cargo toolchain on the VPS; no builds on production).
