<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-life test: omnode edge boot path (swarm/edge-boot, slot 20)

| Field | Value |
|---|---|
| Run | 2026-10-20T19:38:57+02:00 to 19:39:07+02:00 (Europe/Zagreb), on the build box |
| Script | `scripts/real/edge-boot.sh` (builds, starts, probes, tears down) |
| Cargo test | `edge/openmind-node/tests/real_edge_boot.rs`, runs only with `GALION_REAL=1` |
| Binaries | release, static musl: x86_64 native, aarch64 under `qemu-aarch64` (qemu-user) |
| Ports | 19200-19206 only (slot 20 range 19200-19209), loopback |
| Init | box PID 1 is `tini`, not systemd: the node runs directly, under a small bash restart loop, and as PID 1 of its own PID namespace. No unit files, no systemctl. |
| VPS | not contacted. There is no live omnode/edge service to compare against (omnode runs on owner devices, never on the VPS), so no VPS probe was made. |
| Result | **84 passed, 0 failed**; no omnode/qemu process and no 192xx listener left afterwards |

Secrets: pairing codes are read from a 600 code file into a shell variable and
never printed; the bearer token is shown only as `<redacted, 64 hex chars>`; the
script asserts that neither appears in the store, stdout or stderr. Device ids
and fingerprints are public identifiers (temporary identities, deleted with the
work dir).

## What is exercised

1. **First boot** (`omnode boot --state DIR --listen 127.0.0.1:PORT --code-file F
   --code-ttl 300 [--require-seccomp]`): identity created, `node.key`,
   `pair-store.json`, code file all mode 600, boot event `first_boot: true`.
2. **Pairing refusal / success**: unpaired `GET /v1/whoami` 401; wrong code
   `403 {"error":"no"}`; malformed body 400; right code (sent lower-case) 200 with a
   token; token works, a one-char-off token 401; the code is single use (403).
3. **Expired code and lockout** (second node, `--code-ttl 1`, 2.2 s wait):
   `403 {"error":"expired"}`; after 5 refusals `429 {"error":"later"}`; `fails: 5` in
   the store.
4. **Seccomp enforcement with real tools** (x86_64): `sandbox selftest` (filter in a
   child: ptrace, fsconfig 431, unshare EPERM; getppid/write allowed);
   `sandbox run -- strace -f true` fails with EPERM while the `--no-seccomp`
   control succeeds; `sandbox run -- unshare -U true` EPERM; normal shell work still
   runs, report says `seccomp: deny-list`. Boot with `--require-seccomp` succeeds.
5. **aarch64 under qemu-user**: qemu-user cannot load a guest seccomp filter, so
   the self-test reports `installed: false` and the node **fails closed**:
   `sandbox selftest` exit 2 and `boot --require-seccomp` refuses (exit 2, no
   pairing code opened). Without `--require-seccomp` the whole pairing/restart flow
   passes on the aarch64 binary. Enforcement on real aarch64 hardware is still to be
   shown (see open issues).
6. **Restart**: `kill -9` of the node; the restart loop brings it back: same
   device id, new pid, `boot_count` 2, first-boot time kept, token still valid, old
   code still refused, no new code opened (already paired). Then SIGTERM: exit 0,
   port closed (exit log `exit=137 exit=0`).
7. **PID 1**: `unshare --user --map-root-user --pid --fork --mount-proc omnode boot`;
   `/v1/node` reports pid 1; SIGTERM from the parent namespace stops it with exit 0
   (the kernel drops default-action signals to PID 1; omnode installs a handler).
8. **Offline `pair verify`** on a payload from `omnode pair --dry-run`: fresh one
   accepted; `--now +3600` refused (expired); another server's URL refused; a
   payload with `"money"` added to capabilities refused (signature).

## Output (2026-10-20T19:38:57+02:00 run, verbatim)

The two `Killed` lines are bash reporting the deliberate `kill -9`.

```
omnode edge-boot real-life test, 2026-10-20T19:38:57+02:00, kernel 6.12.94+, PID 1 on this box: tini
== x86_64: 2026-10-20T19:38:57+02:00
  machine: Advanced Micro Devices X86-64
  NEEDED: 0  INTERP: 0
  size: 1557376  sha256: cdaa8a79acfd3a65
  version: omnode 0.1.0 (x86_64, linux)
  PASS x86_64 sandbox selftest exit (0)
  PASS x86_64 selftest enforced (true)
  PASS x86_64 fsconfig(431) denied (true)
  PASS x86_64 strace under seccomp refused (EPERM)
  PASS x86_64 strace with --no-seccomp (control) (0)
  PASS x86_64 unshare -U under seccomp refused (EPERM)
  PASS x86_64 everyday shell work allowed (0)
  PASS x86_64 sandbox report seccomp=deny-list
  PASS x86_64 first boot listening on :19200
  PASS x86_64 boot event first_boot (true)
  PASS x86_64 boot event seccomp.enforced (true)
  device_id omn-655241d522aa  fingerprint SHA256:6552-41d5-22aa-bf19-dbb6-8adb-daae-ff11  first_boot 2026-10-20T19:38:57+02:00
  PASS x86_64 key file mode (600)
  PASS x86_64 pair store mode (600)
  PASS x86_64 code file mode (600)
  PASS x86_64 store holds only the code hash
  PASS x86_64 whoami unpaired (401)
  PASS x86_64 wrong code (403 no)
  PASS x86_64 malformed body (400)
  PASS x86_64 right code (lower-case) (200 true)
  token: <redacted, 64 hex chars>
  PASS x86_64 whoami with token (200 omn-655241d522aa)
  PASS x86_64 whoami with wrong token (401)
  PASS x86_64 code is single use (403 no)
  PASS x86_64 token not on disk or in logs
scripts/real/edge-boot.sh: line 89: 1180682 Killed                  "$@" >> "$log.out" 2>> "$log.err"
  PASS x86_64 restarted by supervisor after kill -9
  PASS x86_64 same device after restart (omn-655241d522aa)
  PASS x86_64 new pid after restart (yes)
  PASS x86_64 first_boot timestamp kept (2026-10-20T19:38)
  PASS x86_64 boot event on restart (false 2 null)
  PASS x86_64 token survives restart (200)
  PASS x86_64 old code still refused after restart (403 no)
  PASS x86_64 pair verify fresh payload (0 true omn-655241d522aa)
  PASS x86_64 pair verify expired payload (2 expired)
  PASS x86_64 pair verify other server (2 enroll_url)
  PASS x86_64 pair verify tampered caps (2 signature)
  PASS x86_64 exits (kill -9, then SIGTERM) (exit=137 exit=0)
  PASS x86_64 port closed after stop (000)
  PASS x86_64 expired code refused (403 expired)
  PASS x86_64 5th refusal (403 no)
  PASS x86_64 locked after 5 refusals (429 later)
  PASS x86_64 fails counted in store (5)
  PASS x86_64 node 2 SIGTERM exit (0)
== x86_64 as PID 1 (unshare --pid --fork): 2026-10-20T19:39:01+02:00
  PASS PID-1 node up
  PASS PID-1 node sees itself as pid (1)
  PASS PID-1 node exits 0 on SIGTERM (handler installed) (0)
  PASS PID-1 stop message
== aarch64-qemu: 2026-10-20T19:39:01+02:00
  machine: AArch64
  NEEDED: 0  INTERP: 0
  size: 1292656  sha256: 723f0a50a8a0f3dd
  version: omnode 0.1.0 (aarch64, linux)
  PASS aarch64-qemu sandbox selftest refuses (not enforceable here) (2)
  PASS aarch64-qemu selftest enforced (false)
  PASS aarch64-qemu boot --require-seccomp refuses (2)
  PASS aarch64-qemu refusal reason: not enforced
  PASS aarch64-qemu no pairing code opened when refused
  PASS aarch64-qemu first boot listening on :19203
  PASS aarch64-qemu boot event first_boot (true)
  PASS aarch64-qemu boot event seccomp.enforced (false)
  device_id omn-57756f06880b  fingerprint SHA256:5775-6f06-880b-074a-2ae3-a958-1b62-e5db  first_boot 2026-10-20T19:39:01+02:00
  PASS aarch64-qemu key file mode (600)
  PASS aarch64-qemu pair store mode (600)
  PASS aarch64-qemu code file mode (600)
  PASS aarch64-qemu store holds only the code hash
  PASS aarch64-qemu whoami unpaired (401)
  PASS aarch64-qemu wrong code (403 no)
  PASS aarch64-qemu malformed body (400)
  PASS aarch64-qemu right code (lower-case) (200 true)
  token: <redacted, 64 hex chars>
  PASS aarch64-qemu whoami with token (200 omn-57756f06880b)
  PASS aarch64-qemu whoami with wrong token (401)
  PASS aarch64-qemu code is single use (403 no)
  PASS aarch64-qemu token not on disk or in logs
scripts/real/edge-boot.sh: line 89: 1184336 Killed                  "$@" >> "$log.out" 2>> "$log.err"
  PASS aarch64-qemu restarted by supervisor after kill -9
  PASS aarch64-qemu same device after restart (omn-57756f06880b)
  PASS aarch64-qemu new pid after restart (yes)
  PASS aarch64-qemu first_boot timestamp kept (2026-10-20T19:39)
  PASS aarch64-qemu boot event on restart (false 2 null)
  PASS aarch64-qemu token survives restart (200)
  PASS aarch64-qemu old code still refused after restart (403 no)
  PASS aarch64-qemu pair verify fresh payload (0 true omn-57756f06880b)
  PASS aarch64-qemu pair verify expired payload (2 expired)
  PASS aarch64-qemu pair verify other server (2 enroll_url)
  PASS aarch64-qemu pair verify tampered caps (2 signature)
  PASS aarch64-qemu exits (kill -9, then SIGTERM) (exit=137 exit=0)
  PASS aarch64-qemu port closed after stop (000)
  PASS aarch64-qemu expired code refused (403 expired)
  PASS aarch64-qemu 5th refusal (403 no)
  PASS aarch64-qemu locked after 5 refusals (429 later)
  PASS aarch64-qemu fails counted in store (5)
  PASS aarch64-qemu node 2 SIGTERM exit (0)
== cargo real test (GALION_REAL=1, x86_64 release binary, ports 19205-19206): 2026-10-20T19:39:05+02:00
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.51s
  PASS tests/real_edge_boot.rs
== done 2026-10-20T19:39:07+02:00: 84 passed, 0 failed
```

## Unit tests

`cd edge/openmind-node && cargo test`: 71 passed (was 52 on main). Under
`qemu-aarch64` (aarch64 musl test binary): 70 passed, 1 failed:
`sandbox::tests::memory_limit_is_set_and_enforced`, which fails the same way on
main 13a296ce under qemu-user (RLIMIT_AS under qemu-user), so it is not from this
branch. armv7 (merged with swarm/ucpu 3e70ffb in a scratch clone, run under
`qemu-arm`): the 8 seccomp/boot tests pass, including the check that every armv7
deny-list number equals libc's `SYS_*` constant.

## Ported to Rust (2026-10-21, +02:00)
`scripts/real/edge-boot.sh` was replaced on swarm/realscript-port-1 by the gated Rust test `crates/galion-realtest/tests/real_sh_edge_boot.rs` (same checks; run with `GALION_REAL=1`). The evidence above is from the shell version; the Rust run is in `docs/real-tests/realscript-port-1.md`.
