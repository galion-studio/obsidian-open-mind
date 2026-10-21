<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# hermes-tools-quota: real-life VPS proof (slot 151 follow-up)

Branch `swarm/hermes-tools-quota`, based on `swarm/sandbox-workdir-quota`
28f6c1d. Owner decisions D-WDQ1-3 accepted 2026-10-21 08:23 +02:00 (Slavik
via Mate). Design: `docs/sandbox/workdir-quota.md` ("hermes-tools").
Times Europe/Zagreb, CEST +02:00. Every hermes-tools test (they start the
sandbox) ran on the VPS only (sandbox_vps_only); none ran on the box.

## What changed
- hermes-tools coding tools (fs_read, fs_write, git, cargo) run with the
  private work dir tmpfs on: 512 MiB by default (`HERMES_RS_CODING_WORKDIR_MB`,
  0/off, 16-4096; `_INODES`), the same number for all kinds (one worktree);
  the per-run cgroup `memory.max` of each kind grows by the cap.
- Incomplete copy back (D-WDQ3, keep partial files) = failed tool call with
  an "INCOMPLETE" note; summary/audit carry `workdir_quota`.
- preflight: refuses when the cap is on without user namespaces, or when the
  worktree is already over the cap.
- git: `core.checkStat=minimal`, `core.trustctime=false`, `gc.auto=0`.

## Box (no sandbox runs)
- `cargo fmt --all --check` OK; `cargo clippy --workspace --all-targets -- -D warnings` OK.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  957 passed, 0 failed, 3 ignored (205 test binaries). `scripts/no-python.sh` OK.
- hermes-tools: clippy clean, test binaries built as static x86_64 musl
  release (`cargo test -p hermes-tools --release --target x86_64-unknown-linux-musl --no-run`);
  not run on the box.

## VPS run (2026-10-21 08:33:01 - 08:33:46 +02:00)
- Throwaway dir `/var/tmp/sandbox-test-20261021-083301` (`bin/` root 0755,
  `tmp/` owned by the temp user), temp system user `sbxhtq` (nologin, uid
  995), a 32 MiB tmpfs `small/` owned by it (for the D-WDQ3 case). Every
  suite: `systemd-run --wait --pipe --collect --slice=sbxtest.slice -p User=sbxhtq -p Group=sbxhtq -p Delegate=yes -p MemoryMax=1G -p TasksMax=256 -p CPUQuota=200% -p Nice=10`
  with `TMPDIR=HOME=<dir>/tmp`, `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1`
  (this kernel has Landlock ABI 0; the mount namespace carries the policy).
- Binaries (sha256 prefix): sandboxed `285942254934c171`, workdir_quota
  `0a1052ed0c03047a`, guard `cf5d9896f7dc987a`, lib `b605a2edfc6e8379`,
  real_ticket `d3fc1c0e9a579083`.

| Suite | Result |
|---|---|
| lib unit tests | 0 tests (ok) |
| tests/guard.rs | 6 passed, 0 failed |
| tests/workdir_quota.rs (new, pure) | 5 passed, 0 failed |
| tests/real_ticket.rs | 1 "passed" = SKIP (needs GALION_REAL=1 + a toolchain = cargo build on the VPS; not done, no builds on production) |
| tests/sandboxed.rs, GALION_REAL=1, small fs | **10 passed, 0 failed** (2.43 s): 5 existing + 5 new |

New sandboxed cases (real hermes-tools tools through openmind-sandbox):
| Case | Result |
|---|---|
| default cap on, normal run (fs_write + git commit) | 5 runs, each `workdir_quota` Enforced, limit 512 MiB, out_complete; commit SHA on the host == tool's SHA, `git status` clean, `git fsck` OK. First run: in 27,474 B, tmpfs used 143,360 B, 9 B back |
| 16 MiB cap, 13 MiB worktree, fs_write 4 MiB | `install: error writing './over.bin': No space left on device`, tool error; host worktree 16,665,426 B (<= 16 MiB), partial over.bin 3,006,464 B; afterwards a small fs_write and git status pass |
| **default 512 MiB cap**, 509 MiB worktree, fs_write 4 MiB | fs_read OK in 567 ms (copy in 509 MiB); fs_write -> **ENOSPC** in 596 ms; tmpfs used 536,870,912 B (= cap); host worktree 536,759,122 B (<= 512 MiB); copy back complete (3,006,464 B partial file) |
| preflight, 20 MiB worktree | cap 16 -> refused "over the worktree cap 16 MiB"; cap 32 -> accepted, note "worktree cap 32 MiB" |
| D-WDQ3: 32 MiB host fs, 26 MiB worktree, cap 64, fs_write 4 MiB | fits the cap, host fs full on copy back: tool error "worktree copy back is INCOMPLETE: the host file system is full; 1 entries / 1933312 bytes reached the host, partial files may be left there"; `out_complete=false errno=28`; partial part.bin 1,953,792 B kept on the host |

### Live system (read-only)
- Nerve `127.0.0.1:8670/state` 08:33: HTTP 200, 13,122 B; 11 healthy,
  2 degrading, 2 absent. hermes-rs and galion-nerve active. Cortex not
  deployed; the nerve is the observer.
- `/opt/galion/openmind` at 2f5a730, not touched. Nothing installed or
  deployed; no service, unit, DNS, HADES or Hermes model change. The live
  hermes-rs does not have this code (its coding tools are not deployed).

### Cleanup (08:33:46)
sbxhtq processes 0; small tmpfs unmounted (0 mounts left); dir removed; user
and group sbxhtq removed; 0 `sbxhtq*` units; 0 sbxhtq cgroups under
sbxtest.slice. Two other agents' `/var/tmp/sandbox-test-*` dirs (06:31:56,
08:30:25) were present and left alone.
