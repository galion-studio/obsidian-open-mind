<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Nerve T3.5 cross-target builds + qemu smoke (slot 217, 2026-10-21 +02:00)

Base `aa77864` (the openmind-nerve build that went live on the VPS at 09:43:54 +02:00,
`/opt/galion/rust/bin/openmind-nerve-aa77864`). Box only; VPS read-only.
Toolchain: rustc 1.99.1 (58cf8f9c3 2026-10-13), qemu-aarch64 10.0.13 (Debian).
Repeatable test: `crates/openmind-nerve/tests/real_cross_target.rs`
(`GALION_REAL=1 cargo test -p openmind-nerve --test real_cross_target -- --nocapture`,
ports 20870-20874, nested target dir `NERVE_XT_DIR`).

## Binaries (release, opt-level z, lto, strip, panic=abort)

| build | command | size (B) | sha256 | ELF |
|---|---|---|---|---|
| x86_64 musl, solo | `cargo build --release --target x86_64-unknown-linux-musl -p openmind-nerve` | 1,205,024 | `39f1a64db8269c4785a647f07ef9ae8aab433d20ee5cb8fc1a207784d1ae4989` | static-pie x86-64 |
| aarch64 musl, solo | same, `--target aarch64-unknown-linux-musl` (linker rust-lld) | 1,003,968 | `240b1411c7b72e1a58443774b38c4d987240b1c2e515b913e80302af8bfdc047` | static (non-PIE) aarch64 |
| x86_64 musl, live recipe | `cargo build --release --target x86_64-unknown-linux-musl -p openmind-nerve -p openmind-memory` | 1,213,216 | `f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68` | static-pie x86-64 |
| **live VPS** `openmind-nerve-aa77864` | sha256sum/stat read-only 21:02:28 +02:00 | 1,213,216 | `f0994b6db1fd8ed87c16bd72c9583c2894ec4dd5ba89d407016877322aed6a68` | static-pie x86-64 |
| live VPS `openmind-nerve-bb287cc` (running now, PID 772576, NRestarts 0) | read-only | 1,415,176 | `c15be071f8263175949465d9848c7f0d6e2643cb6362ef2ae613c0346eae5673` | static-pie x86-64 (not this base) |

All under the 2 MiB budget. aarch64 is 201,056 B (16.7 %) smaller than x86_64.

### Finding: the live binary is reproducible bit for bit, but only with the joint build
- The live aa77864 binary was built in one cargo invocation together with
  openmind-memory (docs/ops/nerve-eventlog-nerveurl-20261021.md on swarm/nerve-eventlog-deploy).
  openmind-memory enables `serde_json/preserve_order`; Cargo feature unification
  turns it on for the nerve too (pulls indexmap + hashbrown: +8,192 B).
- Rebuilding that recipe here gives **the identical sha256** (f0994b6d…, two clean runs,
  different target dirs). The solo `-p openmind-nerve` build is also stable (39f1a64d… twice)
  but differs from live.
- Observable effect on the wire: the solo build sends `/health` keys sorted
  (`appended, cycle, score, service, status, system, ts, version`); the live-recipe build
  sends source order (`status, service, version, appended, ts, system, score, cycle`).
  Same key sets, same values otherwise. JSON consumers do not care; byte goldens and
  hashes of bodies do (tests/support/mod.rs already normalizes for this).
- The currently running bb287cc sends sorted keys (VPS `/health` read 21:08:48 +02:00),
  i.e. it was built without preserve_order. So the live nerve's key order changed at the
  19:16:57 cutover. Owner decision below.

## qemu / native smoke (each binary started with `serve`, components = live thread :20873 + dead :20874)

| run | port | runner | first cycle | GET /health | POST /signal | bad POST (no kind) | GET /state | signal in /api/v1/signals |
|---|---|---|---|---|---|---|---|---|
| x86_64 solo | 20870 | native | 0.10 s | 200, service openmind-nerve, status ok | 202 | 400 | 200, 23 keys, xt-live healthy, xt-dead failing, system failing | yes |
| aarch64 solo | 20871 | qemu-aarch64 user mode | 0.10-0.21 s | 200, same keys + order as x86_64 | 202 | 400 | 200, 23 keys, same statuses + verdict | yes |
| x86_64 live recipe | 20872 | native | 0.10 s | 200, same keys, source order | 202 | 400 | 200, same 23 keys | yes |

`qemu-aarch64 openmind-nerve --version` → `openmind-nerve 0.1.0`. After each run the
process is killed and the port is checked closed; `ss -ltn` showed 0 listeners on 2087x after.

## cargo check
- `aarch64-linux-android -p openmind-nerve`: ok (6.8 s clean)
- `armv7-unknown-linux-musleabihf -p openmind-nerve`: ok (4.2 s clean)

## Runs
| run | Zagreb time | result |
|---|---|---|
| 1 | 21:06:35-21:07:55 | builds/smoke ok, FAILED on key-order assert (test parsed order with its own serde_json; fixed: order now read from raw body) |
| 2 | 21:08:23-21:08:34 | 2/2 pass (nested builds cached) |
| 3 clean (`NERVE_XT_DIR` deleted first) | 21:08:45-21:10:04 | 2/2 pass, 79.3 s (builds 17.7 / 18.8 / 31.3 s) |
| 4 after clippy fix | 21:14:53-21:14:54 | 2/2 pass (cached) |

Report JSON of run 3: `/workspace/swarm/logs/nerve-cross-target/report-run3.json` (box only).

## VPS read-only probes
- 21:02:28 +02:00: `ls -l /opt/galion/rust/bin`, `sha256sum`/`stat`/`file` of both nerve
  binaries, `readlink`+`sha256sum /proc/<MainPID>/exe`, `systemctl show galion-nerve -p NRestarts -p ActiveEnterTimestamp`
  (19:16:57 +02:00), GET :8670/health 200 (cycle 10, score 61, degraded).
  The aa77864 binary was copied to the box by `ssh cat` (read-only) for the strings diff.
- 21:08:48 +02:00: GET :8670/health 200 (149 B), /state 200 (37,934 B), PID 772576 NRestarts 0,
  gaia-cortex :8674/health ok dry-run, llm_enabled true. Disclosure: the bodies went to
  `/tmp/.x217` on the VPS for one command and were removed in the same batch (checked absent).
- Nothing deployed, restarted, written (besides that temp file) or POSTed on the VPS.

## Owner decisions
- **D-217-1** Pin the build recipe for nerve deploys: (a) always `-p openmind-nerve` alone
  (sorted keys, what bb287cc does now), (b) always joint with openmind-memory (what aa77864 did),
  or (c) make the nerve's own Cargo.toml set `serde_json/preserve_order` explicitly so the
  output no longer depends on what else is in the cargo command (recommended: (c) or (a),
  plus record the exact command in every deploy record).
- **D-217-2** Ship an aarch64 nerve? It builds static and behaves the same under qemu;
  no aarch64 host is deployed today, so nothing to do unless a Pi/phone node is planned.
