<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: galion-admin (wave-4 slot 165)

All times Europe/Zagreb (CEST, +02:00). Rule sandbox_vps_only: **no sandbox was run on the dev
box.** On the box the admin drove `examples/fake_box`, a stand-in that creates no namespaces.
Every run of the real galion-box launcher happened on the VPS as a throwaway user.

## Box (2026-10-21 08:28-08:31)

| Check | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --all-targets -- -D warnings` on x86_64-gnu, x86_64-musl, aarch64-musl, aarch64-linux-android, armv7-musleabihf | 0 warnings |
| `cargo check --all-targets` aarch64-linux-android, armv7-unknown-linux-musleabihf | ok |
| release static builds x86_64-musl, aarch64-musl (rust-lld), armv7-musleabihf (rust-lld) | static, 0.87 / 0.90 MB |
| unit tests x86_64 | 25 passed |
| unit tests aarch64-musl under `qemu-aarch64` (`CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUNNER`) | 25 passed |
| unit tests armv7-musleabihf under `qemu-arm` (extra) | 25 passed |
| `qemu-aarch64 galion-admin version / tunnel status / audit tail / audit verify / sandbox start --rootfs /` | `galion-admin 0.1.0 (aarch64-linux)`; port 20352 `down refused`, exit 4; record `2026-10-21T08:29:34+02:00 #1 ... tunnel.status - down`; `audit ok: 1 records`; `--rootfs /` refused, exit 2 |
| `GALION_REAL=1 cargo test --release` → `tests/real_cli.rs` (fake launcher) | **9 passed** (08:28 first run, 08:31 after the `lost`/`launched` change) |
| `tests/real_vps.rs` on the box | 7 compiled, all self-skip (GALION_ADMIN_ON_VPS unset) |

real_cli details (08:31:30-08:31:37): r01 usage/exit codes; r02 start 505 ms (500 ms settle
window), status `running` with `+02:00` time, second start refused, stop 52 ms → `signal:15`,
stop again → exit 4, audit `[sandbox.start, sandbox.exit, sandbox.stop]` and `audit ok: 3
records`; r03 payload `exit 3` → start fails `box exit:3`, `exit 0` → "finished", missing launcher
→ `spawn-error`, audited `failed`; r04 launcher ignoring SIGTERM → SIGKILL after 1 s grace, stop
1059 ms → `signal:9`; r05 supervisor SIGKILL → launcher gone (PDEATHSIG), `lost` recorded once,
name reusable; r06 `--token=hunter2-SECRET` argument never in audit or state (`cmd=exit argc=3`),
dirs 0700 / files 0600, world-writable state dir refused; r07 tunnel: listener on 20350 `up`,
20351 `down refused`, exit 4, the probe sent 0 bytes, `1.1.1.1:443` and `localhost:22` refused;
r08 tail human/JSON, `--follow` picked up a record written by another process, an edited middle
record → `audit BROKEN at line 3` exit 4, a garbage tail → start refused (fail closed);
r09 4 parallel sandboxes → 12 records, chain valid. Ports used: 20350-20353 (slot 165 range).

## VPS (2026-10-21 08:30:25-08:33:03)

Setup (root, then everything else as the temp user): `useradd --system --no-create-home --shell
/usr/sbin/nologin sbx165` (uid 995), `/var/tmp/sandbox-test-20261021-083025` mode 0700 owned by
sbx165, static x86_64 binaries copied in (sha256 prefixes: galion-box c1156847a899963d =
identical to slot 144's build of 4374605; galion-admin 9da84a8a6df796db for run 1 and
30b1ac07a555a8df for runs 2-4; real_vps 6cf8dc5ca1187dd1 / fd8cc3d0ae97e808). Run:
`runuser -u sbx165 -- env -i PATH=/usr/bin:/bin HOME=$W TZ=Europe/Zagreb GALION_REAL=1
GALION_ADMIN_ON_VPS=1 GALION_WORK=$W GALION_ADMIN_BIN=$W/galion-admin GALION_BOX_BIN=$W/galion-box
GALION_NERVE_PORT=8670 timeout 120 ./real_vps --test-threads=1 --nocapture`.

| Run | Time | Result | Leftover processes of sbx165 |
|---|---|---|---|
| 1 | 08:30:46-08:30:48 | 7 passed / 0 failed | 0 |
| 2 | 08:32:38-08:32:40 | 7 passed / 0 failed | 0 |
| 3 | 08:32:40-08:32:43 | 7 passed / 0 failed | 0 |
| 4 | 08:32:43-08:32:45 | 7 passed / 0 failed | 0 |

Run 1 used the first build; it showed two honesty gaps that were fixed before runs 2-4: the
supervisor wrote `sandbox.start ok` even when the launcher then failed (now `launched`, followed
by `sandbox.exit exit:125`), and a killed supervisor left no audit record (now `sandbox.lost`).

| Test | Evidence (run 2 unless noted) |
|---|---|
| v01 probe via admin | `euid=995 max_user_namespaces=2147483647 unprivileged_userns_clone=1 apparmor_restrict_unprivileged_userns=0 landlock=false threads=1 verdict=userns`; audited `sandbox.probe exit:0` |
| v02 start/status/stop | start 502-511 ms (settle window); 3 processes while running (supervisor, launcher, pid-1 init); status `running box_pid=22992 since 2026-10-21T08:32:38+02:00 cmd=galion-box argc=3`; stop `SIGTERM, box signal:15, 50 ms`; afterwards `exited signal:15`; 0 leftovers (all 4 runs) |
| v03 timeout | `--timeout 1` with `sleep 3600` → state `exit:124`, 0 leftovers |
| v04 isolation report through the admin log | `uid=1000 pid=2 ppid=1 hostname=galion-box cap_eff=0000000000000000 no_new_privs=1 proc_state=hidden proc_pids=0 interfaces=lo env_count=4 setuid0=denied:EINVAL mount_tmpfs=denied:EPERM chroot=denied:EPERM kill_init=ALLOWED write_root=ALLOWED write_tmp=ALLOWED nested_userns=ALLOWED` (no `--ro-root` in this test; see finding 1 in docs/admin/galion-admin.md) |
| v05a fail closed | missing rootfs → `failed to start: box exit:125` + launcher message `No such file or directory`; payload never ran |
| v05b supervisor SIGKILL | 3 processes → 0 in 3 ms (runs 1-3) / 57 ms (run 4); status `lost supervisor died without recording an exit`; audit `sandbox.lost v05 supervisor-gone (... box_alive=false)` |
| v06 tunnel status | `nerve 127.0.0.1:8670 up 0 ms`, `closed 127.0.0.1:20359 down refused`, exit 4 (TCP connect + close only, no request sent) |
| v99 audit | `audit ok: 13 records` (run 1 build: 12), chain valid; every record `+02:00`, `uid=995` |

### Live system observed through the nerve (cortex not deployed yet, so nerve only)

`GET http://127.0.0.1:8670/state` (read-only): 08:30:17 cycle 6951, HTTP 200, 13,027 B (before);
08:33:02 cycle 6956, mode `live`, 17 healthy / 4 degrading / 3 absent. Degrading includes
`galion-one-mouth` (503 while the model loads, degraded before this test in every earlier record)
and `cloudflared`; absent includes `voice-worker` and `gaia` (not deployed). This test touched no
service: the only contact was the TCP connect/close to 8670 in v06. No baseline component count
was taken at 08:30, so the degrading count cannot be compared directly; nothing in the test path
can reach cloudflared.

### Cleanup (08:33:03)

`pkill -u sbx165` → 0 processes; folder removed; `userdel sbx165` (group removed with it);
`id sbx165` → no such user; ports 20350-20359 free; no cgroups created. One other folder,
`/var/tmp/sandbox-test-20261021-063156`, appeared during the run; it is not this slot's
(another agent's, named by the VPS's UTC clock) and was left alone. Nothing on the VPS was
installed, restarted or reconfigured; /opt/galion untouched.
