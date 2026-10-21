<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: galion-box (wave-4 slot 144)

All times Europe/Zagreb (CEST, +02:00). Rule sandbox_vps_only: **no sandbox was run on the dev
box.** The box only built, ran unit tests (no namespaces) and ran the read-only `probe`/`plan`
under qemu-aarch64. Every sandbox run happened on the VPS as a throwaway system user.

## Box (2026-10-21 ~06:52)

| Check | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --all-targets -D warnings` on x86_64-gnu, x86_64-musl, aarch64-musl, aarch64-linux-android, armv7-musleabihf | 0 warnings |
| release builds x86_64-unknown-linux-musl, aarch64-unknown-linux-musl (rust-lld) | static, ok |
| unit tests x86_64 | 12 passed |
| unit tests aarch64 under `qemu-aarch64` (CARGO_TARGET_..._RUNNER) | 12 passed |
| real tests on the box | 11 compiled, all self-skip (GALION_REAL/GALION_BOX_ON_VPS unset) |
| `qemu-aarch64 galion-box probe` (read-only) | `arch=aarch64 threads=2 verdict=emulated` |
| `qemu-aarch64 galion-box plan ...` | 16 steps printed, nothing created |
| `qemu-aarch64 galion-box run --rootfs / ...` | refused, exit 2 |

## VPS (final run 2026-10-21 06:50:50 – 06:51:21)

Setup: `useradd --system sbx144` (uid 996), `/var/tmp/sandbox-test-20261021-065050` (0700, owned by
sbx144), static binaries copied in (sha256 prefixes: galion-box-x86_64 c1156847a899963d,
galion-box-aarch64 e68581596d08e28f, real_vps 365cdf2e8d58569e, qemu-aarch64-static
73cc2584f119f9e7). Run: `runuser -u sbx144 -- env -i GALION_REAL=1 GALION_BOX_ON_VPS=1
GALION_BOX_BIN=... ./real_vps --test-threads=1 --nocapture`. Ports used: 20141, 20142 (loopback).
Host: Ubuntu 24.04 LXC on 6.8.12-pve, unprivileged userns on, AppArmor userns restriction 0,
no Landlock.

### x86_64 build, native: 10 passed + 1 not applicable (06:51:05 – 06:51:13)

| Test | Evidence |
|---|---|
| real_01 probe | `euid=996 max_user_namespaces=2147483647 unprivileged_userns_clone=1 apparmor_restrict_unprivileged_userns=0 landlock=false threads=1 verdict=userns` |
| real_02 default isolation | `uid=1000 euid=1000 pid=2 ppid=1 hostname=galion-box cap_eff=0000000000000000 no_new_privs=1 proc_state=hidden proc_pids=0 interfaces=lo env_count=4 setuid0=denied:EINVAL mount_tmpfs=denied:EPERM chroot=denied:EPERM`; `connect 1.1.1.1:443 / 1.0.0.1:80 / 127.0.0.1:20141 = blocked:ENETUNREACH`; host listener on 20141 got 0 connections; `/opt/galion`, `/opt/galion/.../link.env`, `/etc/passwd`, `/root`, `/home`, `/var/tmp`, `/usr` all `hidden:ENOENT`; no leftover processes |
| real_03 ro root + ro bind | `write_root=denied:EROFS`, `/data/marker VISIBLE`, `write /data/new = EROFS`, `write /bin/evil = EROFS`, `/tmp` writable; nothing appeared on the host |
| real_04 `--net lo` | `interfaces=lo`, `127.0.0.1:20142 = blocked:ECONNREFUSED` (sandbox's own lo, host listener got 0), internet ENETUNREACH |
| real_05 timeout | `sleep 3600` with `--timeout 1.5` → exit 124 in ~1.5 s, 0 leftover processes |
| real_06 fork bomb | `--nproc 16`, 200 forks requested → `spawn_ok=14 spawn_first_err=EAGAIN`, 0 leftovers |
| real_07 memory | `--as-mb 256`: 512 MiB → `denied:ENOMEM`, 32 MiB → `ALLOWED` |
| real_08 exit codes | payload `exit 7` → 7; missing payload → 127 |
| real_08b refusals | `--rootfs /` → 2; `--uid 0` → 2 |
| real_09 start-up | n=20 `exit 0`: min 4 ms, **median 6 ms**, p90 41 ms, max 48 ms |
| real_10 | n/a (qemu only) |

### aarch64 build under qemu-aarch64-static: 3 passed + 8 not possible (06:51:13 – 06:51:16)

| Test | Evidence |
|---|---|
| real_01 probe | `arch=aarch64 threads=2 verdict=emulated` |
| real_08b refusals | `--rootfs /` → 2; `--uid 0` → 2 |
| real_10 fail closed | `galion-box: clone(namespaces): Invalid argument (os error 22)`, exit 125, payload never ran (host marker file absent, no stdout) |
| real_02..09 | skipped: qemu-user cannot create namespaces for its guest (see docs/sandbox/galion-box.md finding 1) |

### Bugs the real runs found (fixed before the final run)

| Time | Symptom | Fix |
|---|---|---|
| 06:43 | every run exit 125, no message | init's PDEATHSIG check expected the outer parent pid; inside a new pid namespace getppid() is 0 |
| 06:44 | `--bind-ro`/`--ro-root` → EACCES | LXC refuses MS_REMOUNT\|MS_BIND\|MS_RDONLY; switched to mount_setattr(MOUNT_ATTR_RDONLY) with remount fallback |
| 06:46 | aarch64 under qemu: unshare EINVAL | qemu helper thread; switched to clone() with namespace flags (also right for multi-threaded Android callers); qemu rejects that too → documented + fail-closed test |
| 06:47 | start-up median 20 ms | supervisor polled every 20 ms even without a timeout; now blocking wait → 6 ms |

### Live system observed through the nerve (cortex not deployed yet, so nerve only)

`GET http://127.0.0.1:8670/state` (read-only) at 06:50:35 (cycle 6752) and 06:51:05 (cycle 6753):
mode live, 0 recent alerts, 0 probe errors, 6 components HTTP 200, galion-one-mouth HTTP 503
("degrading: health probe HTTP 503") in both — unchanged by the test, already degraded before it.

### Cleanup (06:51:21)

`pgrep -u sbx144` → 0 processes; folder removed; `userdel sbx144` (group removed with it);
`id sbx144` → no such user; no `/var/tmp/sandbox-test-*` left; ports 20140-20149 free. No cgroups
were created. Nothing on the VPS was installed, restarted or reconfigured; /opt/galion untouched.
