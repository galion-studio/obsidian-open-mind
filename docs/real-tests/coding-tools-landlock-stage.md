<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: coding tools vs no Landlock, staged D-320-1 options (slot 334)

All times are CET (+01:00) on 2026-10-28. No sandbox ran on the box. The box only ran the pure tests
(injected kernel features, no fork/unshare probe). Every sandbox and gateway run was on the VPS as
uid/gid 65534 (`setpriv --reuid=65534 --regid=65534 --clear-groups`, `env -i`, `umask 077`) in
`/var/tmp/sandbox-test-20261028-011958` (root 0755, chown 65534), which was removed afterwards.
There was no temp user and no cgroup. Ports 22040-22043 were used.

Binaries: static x86_64-musl release builds from the branch, copied with the 5 staged deploy files.
`staged_preset` sha256 139f39a3… (final; the first build 57b0de01… differed only in the /opt/galion probe),
`sandboxed` 49225019…, `hermes-gateway` 1bcbe4b6… (2 202 384 B), plus `real_coding_landlock`, `guard` and `workdir_quota`.

## VPS kernel (real `openmind_sandbox::features()`)
`Features { landlock_abi: 0, userns: true, userns_full: true, mount_setattr: true, caps: true, android: false }`.
LSMs `lockdown,capability,yama,apparmor`, kernel 6.8.12-43-pve.

Baseline (01:20): hermes-rs MainPID 255539 NRestarts 0, galion-desk 264677/0, galion-nerve 184014/0, nerve `:8670/state` 200.

## 1. Real hermes-gateway start per staged alternative (`real_coding_landlock`, 3 runs at 01:25:37-01:25:38, 3/3 pass)
| Case | Exit | /health | Time | Gateway log |
|---|---|---|---|---|
| coding-tools.conf alone (F-320-2) | **2** | - | 51 ms | `coding: refusing to start: the kernel has no Landlock; ... owner decision D-320-1` |
| + (a) allow | running | 200 | 52-54 ms | `WARNING: no Landlock; relying on the mount namespace only (... option a)` ... `tools on: ["fs_read","fs_write","git","cargo"]` |
| + (b) refuse | **2** | - | 50-51 ms | same refusal as the base alone |
| + (c) layered | running | 200 | 53-54 ms | `WARNING: no Landlock; layered mode (... option c): every run must report userns+mountns+pidns+net+ro_root+hide_home+workdir_quota enforced or it fails closed` ... `tools on` |
All cases logged `worktree cap 512 MiB / 32768 entries`. Ports 22040-22043 were closed after every case (0 listeners afterwards).
Re-run after the clippy fix (the child is now always reaped), in a new folder `/var/tmp/sandbox-test-20261028-013317` at 01:33:43 CET: 3/3 pass. a: 200 in 53-61 ms; b: exit 2 in 51-52 ms; c: 200 in 54-59 ms (base asserted exit 2). The folder was removed. Afterwards: 0 folders, 0 processes, 0 listeners; hermes-rs, desk and nerve unchanged, nerve 200.

## 2. `staged_preset` with GALION_REAL=1 GALION_SANDBOX_ON_VPS=1 (3 runs at 01:23:49-01:23:50: 15/0 each; first build 3x at 01:20:28: 15/0)
- `real_staged_preflight_alternatives_on_this_kernel`: BASE Err, a Ok (`no_landlock=Allow`), b Err, c Ok (`no_landlock=Layered`).
  Real option c runs of Fs, Git and Cargo (preset) each returned `Exited(0)`, `OPTN=0` (/opt/galion is an empty tmpfs inside),
  and passed `layered_check`. Enforced: `rlimits,env,workdir,workdir_quota,no_new_privs,userns,net,pidns,mountns,ro_root,hide_home,proc,caps,fds`.
  Not enforced: `landlock (kernel has no landlock)`, `cgroup (no cgroup_parent)` (plain user, no delegation).
- Slot 305/318 checks still pass: link.env HIDDEN (host mode 666), canary hidden and readable in the control, /proc Masked
  (6 static files, PIDDIRS=0), 16 MiB WDQ fill WROTE=64 then ENOSPC, host bytes 16 777 216.
- Pure tests (also on the box): alternatives are one key each; the base refuses on VPS features;
  a/b/c through `preflight_with` on VPS and Landlock features; c refuses without `userns_full` and with `WORKDIR_MB=0`;
  legacy env = allow, explicit policy wins; typo `alow` is a config error; `layered_check` fails closed for each of the 7 layers.

## 3. Regression `sandboxed.rs` (01:20:41-01:20:45)
- Plain: 2 passed, 8 failed (fail closed in preflight, new message). Same as slot 320 (2/8).
- `HERMES_RS_CODING_ALLOW_NO_LANDLOCK=1` (legacy): **10/0** x3 (1.67 / 1.33 / 1.61 s). Same as slot 320.
- `guard` 6/0, `workdir_quota` 5/0.

## Cleanup and after (01:25:47)
The folder was removed. Afterwards: 0 `/var/tmp/sandbox-test-*`, 0 such mounts, 0 uid-65534 test or gateway processes,
0 `hermes-tools-sbx` leftovers, 0 listeners on 22040-22049. hermes-rs 255539/0, galion-desk 264677/0,
galion-nerve 184014/0 (unchanged), nerve `/state` 200, hermes `:8642/health` 200. The nerve was used for observation.
The live gaia-cortex was not queried. No live file, unit or permission was changed.

## Box gates
- `cargo fmt --check` ok. `cargo clippy --all-targets -D warnings` ok for the workspace minus sandbox/cgroup-detect, and for hermes-tools + hermes-gateway + openmind-sandbox.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools --no-fail-fast`: 379 suites, 2010 passed, 1 failed, 18 ignored. The one failure was `room` `default_and_fixture_do_not_open_a_socket`: it saw a socket fd from a parallel test in the same process, and it passes on re-run (room 38/0). An earlier run hit `galion-deploy` `systemd_backend` bind 19394 AddrInUse (another agent's fixed port), which passes on re-run (1/0). Neither crate is touched here. A first run also aborted on box disk full (100%, shared box).
- hermes-gateway 11/0 on the box (the real test SKIPs). staged_preset 15/0 (pure). no-python ok, spdx-scan 0 missing.
- `cargo check -p hermes-tools` passes on aarch64-unknown-linux-musl, armv7-unknown-linux-musleabihf and aarch64-linux-android.

## Not tested
- The delegated cgroup path (`HERMES_RS_CODING_CGROUP=delegate`) needs a systemd unit with `Delegate=`, which is a live-unit step. It was dropped in every run.
- A real model-driven tool call through the gateway (no model call by design: no spend). Tool runs were exercised via `exec::run` in `staged_preset`.
- aarch64 on a real kernel.
