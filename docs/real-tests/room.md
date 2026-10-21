<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-life tests: room (swarm slot 08, ports 19080-19089)

- Written: 2026-10-20T19:04:51+02:00 (Europe/Zagreb)
- Branch: `swarm/room` on 13a296c. Box only; nothing was built or written on the VPS.
- Script: `scripts/real/room.sh` (builds release, runs every test including the gated ones, starts three real sessions, tears them down)
- Gated Rust tests: `GALION_REAL=1 cargo test --release -p room --test real_room` (ports 19081 and 19082)

## What a "room session" is

room is a read-only document page and HTTP reader, **not a sandbox**. It cannot
run commands, and this branch did not add a way to. A real session is: start
`room --listen`, make real HTTP requests, check the isolation rules (secret
filter, read-root confinement, symlink escape, no socket outside `--listen`,
network-namespace containment), and tear down.

## Tool availability on the box

| tool | before | after | notes |
|---|---|---|---|
| strace | missing | 6.13 (`sudo apt-get install -y strace`) | ptrace works (no seccomp, uid 1000) |
| `unshare --net` | refused | refused | needs CAP_SYS_ADMIN, which uid 1000 does not have |
| `unshare --user --map-root-user --net` | works | works | unprivileged user namespaces are enabled |
| nsenter into that namespace | works | works | the namespace owner can join it |

`tests/no_bind.rs` now tries `unshare --net` first and falls back to the
user-namespace form. **Both of its tests run for real on the box. They do not
skip.** No room test genuinely needs a privileged host. They skip only where
neither form of namespace works or strace is missing. That includes GitHub's
ubuntu runners, which have no strace installed, run as non-root, and on Ubuntu
24.04 restrict unprivileged user namespaces through AppArmor.

## Evidence (scripts/real/room.sh, release build)

```
[2026-10-20T19:04:13+02:00] tools: unshare=/usr/bin/unshare strace=/usr/bin/strace nsenter=/usr/bin/nsenter
[2026-10-20T19:04:13+02:00] unshare --net: refused (needs CAP_SYS_ADMIN)
[2026-10-20T19:04:13+02:00] unshare --user --map-root-user --net: ok
[2026-10-20T19:04:13+02:00] build: nice -n 10 cargo build --release -p room -j 4
    Finished `release` profile [optimized] target(s) in 3.23s
[2026-10-20T19:04:16+02:00] binary: 375032 bytes, ELF interpreter: interpreter: /lib64/ld-linux-x86-64.so.2
[2026-10-20T19:04:16+02:00] cargo tests incl. namespace/strace tests and gated real tests (release, GALION_REAL=1)
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test listen_bind_failure_is_a_clean_error ... ok
test page_mode_reads_the_configured_root ... ok
test fixture_mode_single_file_dir_and_missing ... ok
test listen_full_session_on_a_free_port ... ok
test help_version_and_usage_errors ... ok
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
room no_bind: netns via ["unshare", "--user", "--map-root-user", "--net", "--"]
room listen_control: netns via ["unshare", "--user", "--map-root-user", "--net", "--"]
test binary_no_flags_and_fixture_do_not_bind_or_open_a_socket ... ok
test positive_control_listen_is_seen_by_strace_and_stays_in_the_namespace ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.05s
REAL room host session on 127.0.0.1:19081: room: 24 objects
REAL room host session torn down: pid 788046 gone, 127.0.0.1:19081 closed
test real_session_on_the_host_port ... ok
REAL room netns: /health answered ok inside the namespace (nsenter)
REAL room netns session: listening on 127.0.0.1:19082 inside the namespace; host connect refused
test real_session_inside_a_network_namespace_is_invisible_to_the_host ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
[2026-10-20T19:04:30+02:00] session 1: host loopback 127.0.0.1:19083
[2026-10-20T19:04:30+02:00]   stderr: room: 21 objects room: listening on 127.0.0.1:19083 
[2026-10-20T19:04:30+02:00]   ss: 127.0.0.1:19083
[2026-10-20T19:04:31+02:00]   GET /health -> 200 ok
[2026-10-20T19:04:31+02:00]   GET /api/objects -> 200 2158 bytes, 21 objects, secret listed: 0
[2026-10-20T19:04:31+02:00]   GET /api/read guide -> 200 # Operator guide
[2026-10-20T19:04:31+02:00]   GET /api/read secret -> 404 held
[2026-10-20T19:04:31+02:00]   GET /api/read symlink escape -> 404 held
[2026-10-20T19:04:31+02:00]   GET /api/read /etc/passwd -> 404 held
[2026-10-20T19:04:31+02:00]   GET /api/read bad escape -> 404 (old decoder panicked = abort)
[2026-10-20T19:04:31+02:00]   POST /api/objects -> 405
[2026-10-20T19:04:31+02:00]   GET /health after hostile requests -> 200
[2026-10-20T19:04:31+02:00]   teardown: pid 788138 alive=no, listener: 0
[2026-10-20T19:04:31+02:00] session 2: inside a private network namespace on 127.0.0.1:19084
[2026-10-20T19:04:31+02:00]   stderr: room: 21 objects room: listening on 127.0.0.1:19084 
[2026-10-20T19:04:31+02:00]   host ss for :19084: 0 listeners
[2026-10-20T19:04:31+02:00]   host curl :19084 -> 000 (000 = unreachable)
[2026-10-20T19:04:31+02:00]   curl inside the namespace (nsenter) -> ok
[2026-10-20T19:04:31+02:00]   links inside the namespace: lo 
[2026-10-20T19:04:31+02:00]   teardown: pid 788216 alive=no
[2026-10-20T19:04:31+02:00] session 3: no-flag page under strace in an empty netns (must show zero network syscalls)
[2026-10-20T19:04:31+02:00]   page bytes: 2159, network syscalls: 0
[2026-10-20T19:04:32+02:00]   control (--listen, 1 s): socket( bind( listen( 
[2026-10-20T19:04:32+02:00] leftover room processes: none
[2026-10-20T19:04:32+02:00] done
```

Release binary sha256 (glibc, box): `5394603bff731062a0dc98497b878a6cff1590bb23828ab4f898c6c2e33bae6a`, 375032 bytes.
Static musl x86_64: `ac3bbdcce3aaf3785843ecbce811c40e1f6a37c6af74a1ee7d7bf47c443f26a8`, 500512 bytes, no ELF interpreter.

## Cross targets

| target | result |
|---|---|
| x86_64-unknown-linux-musl | release build ok, static |
| aarch64-unknown-linux-musl | release build ok with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld`. The box's `cc` has no aarch64 linker; the same failure happens on unmodified 13a296c. Under qemu-aarch64-static: `--version` gives `room 0.1.0`, `--fixture` gives the expected JSON, and `--listen` on 127.0.0.1:19086 answers `/health` with `ok`. Killed afterwards. |
| aarch64-linux-android | `cargo check --all-targets` ok |
| armv7-unknown-linux-musleabihf | `cargo check --all-targets` ok |

## VPS comparison (read-only, 2026-10-20T18:52:20+02:00)

`ss -ltnp` showed nothing on :4020. There is no room systemd unit and no room
process, and `GET 127.0.0.1:4020/health` was refused. The old
`dist/bin/room` (344304 bytes, Oct 18) is on disk but not running, so there
is no live room to compare responses against. The data roots room reads
(`/workspace/galion-light/docs`, `/workspace/galion-src`,
`/workspace/galion-light/room/index.html`) exist on the VPS. A second
read-only look at the VPS (galion-deploy's header) was refused by the box's
safety review and was not retried. The cutover doc lists checking the
galion-deploy install path as a go/no-go item instead.

## Processes

All started processes were killed. The script ends with `leftover room
processes: none`, and each test's `Room` guard kills and reaps its child.
