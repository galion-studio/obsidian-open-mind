<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: POSEIDON (`crates/openmind-queue`, branch `swarm/poseidon`)

Written 2026-10-20T19:35:00+02:00 (Europe/Zagreb).

**No new third-party crates were added, and no code was copied.** The only
`Cargo.lock` change is the new in-workspace `openmind-queue` entry
(additions only; no new `[[package]]` from crates.io).

## Direct dependencies

| Name | Version | Licence (SPDX) | Source | Used for | Why not write it |
| --- | --- | --- | --- | --- | --- |
| `zagreb-time` | 0.1.0 (workspace) | MIT | `crates/zagreb-time` | Europe/Zagreb RFC 3339 stamps with explicit offset | In-workspace; the stack's single clock helper |
| `openmind-provider` | 0.1.0 (workspace) | MIT | `crates/openmind-provider` | Loading the provider registry (approval + health) that gates VRAM devices | In-workspace; it is the approval source of truth (B34) |
| `serde` | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | Derives for log records and HTTP bodies | Already a workspace dependency (`[workspace.dependencies]`); hand-written parsers would be more code and more bugs |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | Log line and HTTP JSON encoding | Already a workspace dependency |

## Transitive dependencies (from `cargo tree -p openmind-queue -e normal`)

All already in `Cargo.lock` on main 13a296ce, pulled in through `zagreb-time`
and `serde`/`serde_json`:

| Crate | Version | Licence |
| --- | --- | --- |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| chrono-tz | 0.10.4 | MIT OR Apache-2.0 |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT (used under MIT) |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| phf / phf_shared | 0.12.1 | MIT |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| serde_core / serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| zmij | 1.0.23 | MIT |

Everything is on the allowlist. Not used: openssl, TLS, async runtimes, HTTP
frameworks, SIMD or asm crates. HTTP, CRC-32, the append-only log and the
CLI argument parser are written on `std`.
