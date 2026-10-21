<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-life escape suite: galion-box (wave-4 slot 164)

All times Europe/Zagreb (CEST, +02:00). Rule sandbox_vps_only: **no sandbox was run on the
dev box.** The box only built, ran unit tests (no namespaces) and compiled the escape harness.
Every sandbox escape probe ran on the VPS as a throwaway system user.

## Box (2026-10-21 08:19 – 08:28)

| Check | Result |
|---|---|
| `cargo fmt --check` | ok |
| `cargo clippy --all-targets -D warnings` | 0 warnings |
| unit tests x86_64 | 14 passed (was 12; +2 escape) |
| unit tests aarch64 under `qemu-aarch64-static` | 14 passed |
| release x86_64-unknown-linux-musl / aarch64-unknown-linux-musl | static, ok |
| `cargo check` aarch64-linux-android + armv7-musleabihf | ok |
| real_escape on the box (env unset) | 8/8 self-skip |

## VPS (2026-10-21 08:28:16 – 08:28:28)

Setup: `useradd --system sbx164` (uid 995), `/var/tmp/sandbox-test-20261021-082816` (0700,
owned by sbx164). Static binaries copied in (sha256 prefixes: galion-box
`344a040dc7a03ce3`, real_escape `6d24e72af083ef3e`). Run:
`runuser -u sbx164 -- env -i GALION_REAL=1 GALION_BOX_ON_VPS=1 GALION_BOX_BIN=...
GALION_BOX_WORK=... ./real_escape --test-threads=1 --nocapture`.
Ports: abstract AF_UNIX name `galion-box-escape-164` (no TCP port needed for the suite).
Host: Ubuntu 24.04 LXC on 6.8.12-pve; userns on; AppArmor userns restriction 0;
yama_ptrace_scope=1; landlock=false; keyctl → ENOSYS in this LXC.

### Results: 8 passed / 0 failed (0.81 s)

| Test / family | Verdict | Evidence |
|---|---|---|
| escape_01 ptrace | CONTAINED | `seize_hostlike=denied:ESRCH`, `attach_hostlike=denied:ESRCH`, `kill_hostlike=denied:ESRCH`; host bystander survived; MEASURED `seize_init=denied:EPERM` |
| escape_02 mount / pivot_root | CONTAINED | `tmpfs/proc/bind_root/pivot_root=denied:EPERM`, `remount_root_rw=denied:EACCES`, `oldroot_path=hidden:ENOENT` |
| escape_03 userns nesting | CONTAINED | `unshare=ALLOWED` (by design); `map_host_root=denied:ENOENT` (hidden /proc); `host_proc1_ns=hidden:ENOENT` |
| escape_04 bpf / perf | CONTAINED | `map_create=denied:EPERM`, `prog_load=denied:EPERM`, `perf_event_open=denied:EACCES` |
| escape_05 io_uring | CONTAINED | MEASURED `setup=ALLOWED`; `host_etc_passwd=hidden:ENOENT`, `host_opt_galion=hidden:ENOENT` |
| escape_06 keyctl | CONTAINED | all keyctl ops `denied:ENOSYS` on this LXC (incl. `read_id1`) |
| escape_07 socket families | CONTAINED | `af_packet=denied:EPERM`, `abstract_unix=blocked:ECONNREFUSED` (host listener hits=0), `tcp_internet=blocked:ENETUNREACH`; MEASURED `af_netlink=ALLOWED`, `af_vsock=ALLOWED`, `af_bluetooth=denied:EAFNOSUPPORT`, `af_xdp=denied:EPERM` |
| escape_08 all families smoke | CONTAINED | combined run; every hard host-escape key denied/blocked/hidden |

### Nerve (read-only, cortex not deployed)

`GET http://127.0.0.1:8670/state` before (08:28:05) and after (08:28:48): live, no new alerts
from this run; openmind HEAD stayed at `2f5a730`.

### Cleanup (08:28:48)

`pgrep -u sbx164` → 0; folder removed; `userdel sbx164`; no `/var/tmp/sandbox-test-*` left;
no cgroups created; no live service or permission change; `/opt/galion/openmind` untouched.

## Owner decisions (not decided here)

| # | Decision | Evidence from this run |
|---|---|---|
| D-S3 (from slot 144) | Add seccomp deny list? | `io_uring_setup=ALLOWED`, `AF_VSOCK socket()=ALLOWED` inside the sandbox. Nested userns still ALLOWED. Recommend deny: `ptrace*`, `keyctl`, `bpf`, `perf_event_open`, `mount`/`pivot_root`/`umount*`, `io_uring_*`, `socket(AF_PACKET\|AF_VSOCK\|AF_XDP)`. |
| D-E1 | Is AF_VSOCK an escape on LXC/phone? | socket() succeeded; no payload was sent. On LXC this can reach the host/hypervisor. Decide whether Galion OS forbids VSOCK entirely. |
| D-E2 | Keep nested userns allowed? | Still allowed; host-root map impossible with hidden /proc. Seccomp can also deny `unshare`/`clone` with NEWUSER if wanted. |
