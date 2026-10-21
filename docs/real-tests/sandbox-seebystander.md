<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# sandbox-seebystander: real tests (slot 191)

Branch `swarm/sandbox-seebystander`, base `sandbox-quota-hide-merge` 78c5c49.
Design and root cause: `docs/sandbox/proc-mask.md`. Times Europe/Zagreb
(CEST +02:00). Sandbox runs ONLY on the VPS (sandbox_vps_only), as temp user
`sbx191` (nologin) in throwaway `/var/tmp/sandbox-test-<ts>`, inside
`systemd-run --wait --pipe --collect --slice=sbxtest.slice -p User=sbx191
-p Delegate=yes -p MemoryMax=1G -p TasksMax=256 -p CPUQuota=200% -p Nice=10`.

## Root-cause probes (VPS, 13:51-13:53)
| Probe (as sbx191, `unshare -Urpfm`) | Result |
|---|---|
| `--mount-proc` (fresh proc) | Permission denied |
| `mount -t proc -o subset=pid proc /proc` | refused (kernel 6.8.12-43-pve) |
| tmpfs on `/proc`, bind on `/proc` | refused (AppArmor, LXC) |
| tmpfs on `/proc/1`, `/proc/sys`, `/tmp`, rbind `/proc` elsewhere | allowed |
| tmpfs on `<copy>/proc` after `mount --rbind / <copy>`, then `pivot_root . .` | allowed, `pids=0` afterwards |
| `/proc/self/mountinfo` proc lines | lxcfs FUSE over cpuinfo, diskstats, loadavg, meminfo, slabinfo, stat, swaps, uptime; `/dev/.lxc` absent |
| LSMs | lockdown,capability,yama,apparmor (no Landlock) |

## Final VPS run (14:16:20-14:16:59, `/var/tmp/sandbox-test-20261021-141557`)
Binaries: static x86_64 musl from the branch head's code (sha256 prefixes:
sandbox suite `cac9da695424d298`, lib `9e19f4cdafdc8589`, real_escape
`dd4c64921a401351`, real_workdir_quota `adeea0def25ac325`, escape example
`63e2a9720b50994e`, hermes-tools sandboxed `d8cbc461b0b4811a`, guard
`cf5d9896f7dc987a`, workdir_quota `b92ff47b29745f4a`, srp `27d977ded37f41f8`).

| Test | Result |
|---|---|
| openmind-sandbox lib unit tests | 9 passed, 0 failed |
| sandbox suite (tests/sandbox.rs), 2 threads | **31 passed, 0 failed** (26 old + 5 new r1-r5) |
| same suite again, 4 threads | 31 passed, 0 failed |
| real_escape (GALION_REAL=1, delegated cgroup) | passed: **seebystander CONTAINED**, new procpeek CONTAINED (`pid_dirs=0 self_exe=false readable=[]`); control with `ProcFallback::Host`: procpeek ESCAPED `pid_dirs=41 readable=[cmdline=14B,status=1515B,stat=175B,fd=3]` (environ and maps not readable) |
| real_workdir_quota (GALION_REAL=1) | 9 passed, 0 failed (q7 observes via /proc/self, now pinned to Host) |
| hermes-tools sandboxed (GALION_REAL=1, HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1, 32 MiB small fs) | **10 passed, 0 failed** under the mask |
| hermes-tools guard / workdir_quota | 6/0, 5/0 |
| srp `SRP_ONLY=S3,S4,S5,S6,S7,S10` | **6 scenarios, 0 FAIL** |

New sandbox suite tests on the VPS:
- r1 default policy: `proc_view Masked { static_files: 6, live_files: 0 }`,
  bystander `sleep 3181` seen 0, environ canary 0, `/proc` entries 6
  (cpuinfo, loadavg, meminfo, stat, uptime, version), `/proc/self` absent,
  `head -c 9 /proc/meminfo` = `MemTotal:`.
- r2 control `ProcFallback::Host`: view Host, bystander seen 1, 107 /proc
  entries, environ canary 0 (not readable even without the mask).
- r3 `ProcFallback::Refuse`: Status::Error (fails closed, nothing ran).
- r4 mask + ro_root + hide_paths: work dir write reaches the host, `/usr`
  write refused, hidden dir lists 0, `pwd` = work dir.
- r5 mask + 8 MiB work dir tmpfs: dd 16 MiB -> ENOSPC, `small` copied back,
  host `big` <= 8 MiB, copy back complete.

srp S10 (rewritten for the fix): sandbox (preset, mask) 0 pids, 0 cmdlines, 0
environ; baseline Host 40 pids, 37 other-uid cmdlines, services
identifiable; bare user 39 pids. S4/S5 use a synthetic cloudflared-like
journal generated on the VPS (21,017,482 B, 21,429 failed-request lines,
`ref.txt` j.requests from root's grep): no live data was copied.

## Earlier VPS runs (kept for honesty)
- 14:03 run 1: q7 failed (util-linux umount needs /proc/self/mountinfo under
  the mask) -> q7 pinned to Host. srp not run (wrong argv in my harness).
- 14:05 run 2: srp S4/S5 failed on the missing journal (harness, no data
  prep), S7 failed because the payload read interfaces from /proc/net/dev ->
  payload falls back to if_indextoname.
- 14:08 / 14:10 runs 3-4: S5 needed `ref.txt` (harness); mask kept only 1
  static file: the mask tmpfs was 16 KiB and the 25 KiB cpuinfo did not fit,
  and snapshots read inside the namespaces failed -> 512 KiB tmpfs, snapshot
  read by the caller before the fork.
- 14:12 run 5 and 14:16 run 6: all green; run 6 is the result above.

## Cleanup and live system (every run)
After each run: `procs_left=0 mounts_left=0 dir_left=0 user_left=0
group_left=0 units_left=0 sbxtest_run_cgroups=0`. MainPID and NRestarts of
galion-nerve, galion-nerve-shadow, hermes-rs, openmind-memory, tuner,
cloudflared, galion-one and openmind-queue identical before and after.
Nerve `127.0.0.1:8670/state` 200 before and after; 14:19 read: cycle 547,
mode live, 61/D, 11 healthy / 1 degrading / 2 absent (cortex not live, so
the nerve is the observer). Nothing installed, enabled or restarted;
/opt/galion/openmind not touched.

## Box
- `cargo fmt --check` ok (workspace and tools/sandbox-real-problems).
- `cargo clippy --workspace --all-targets -- -D warnings` ok; srp clippy ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect
  --exclude hermes-tools`: 205 suites, **957 passed, 0 failed, 3 ignored**
  (same as base).
- `cargo test -p openmind-sandbox --lib proc_mask_tests` (pure, no sandbox
  run): 4 passed.
- x86_64 + aarch64 musl release builds (openmind-sandbox, hermes-tools, srp);
  armv7-musleabihf and aarch64-linux-android `cargo check --all-targets` ok.
- `scripts/no-python.sh` ok; `scripts/spdx-scan.sh` 0 missing.
  `scripts/ucpu-gate.sh` could not run (no zig on the box).
- No new dependencies.
