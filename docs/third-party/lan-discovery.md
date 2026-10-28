<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: lan-discovery (swarm slot 11)

- Written: 2026-10-20T19:30:00+02:00 (Europe/Zagreb)
- Branch: `swarm/lan-discovery`, crate `crates/openmind-discovery`

## New direct dependency

| Crate | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| socket2 | 0.6.5 (`features = ["all"]`) | MIT OR Apache-2.0 | https://github.com/rust-lang/socket2 (crates.io: https://crates.io/crates/socket2) | `SO_REUSEADDR` + `SO_REUSEPORT` before `bind` so the announcer can share UDP 5353 with avahi; selecting the outgoing multicast interface (`IP_MULTICAST_IF`), multicast TTL/loop, and joining 224.0.0.251. |

Why not std only: `std::net::UdpSocket` has `join_multicast_v4`,
`set_multicast_loop_v4` and `set_multicast_ttl_v4`, but no way to set
`SO_REUSEADDR` / `SO_REUSEPORT` before `bind` and no `IP_MULTICAST_IF`.
Without reuse the announcer cannot bind 5353 next to avahi; without
`IP_MULTICAST_IF` the query may leave on the wrong interface. Doing it by
hand would need `unsafe` `libc` calls per OS. socket2 is the rust-lang
maintained, widely used wrapper (it is what tokio/mio use), it is small and
has no build script that needs a C toolchain, and it builds for every
UNIVERSAL-CPU target (checked below).

Everything else in this work is our own std-only code: the DNS wire codec
(`src/dns.rs`, RFC 1035 / 6762 / 6763), the mDNS querier and responder
(`src/mdns.rs`), the probe (`src/probe.rs`) and the admit/log code
(`src/admit.rs`). No code was copied from other projects.

## Transitive dependencies added to Cargo.lock

| Crate | Version | Licence | Source | Note |
|---|---|---|---|---|
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs | socket2 on Windows targets only; not compiled for Linux/Android. |

Already in the lock before this branch and reused: `libc` 0.2.191
(MIT OR Apache-2.0, https://github.com/rust-lang/libc, socket2 on unix),
`windows-link` 0.2.1 (MIT OR Apache-2.0). Internal crates used:
`openmind-provider` (registry) and `zagreb-time` (timestamps; it brings the
existing workspace deps `chrono` and `chrono-tz`).

Licence check (2026-10-20T19:26+02:00, `cargo tree -p openmind-discovery
--target all` joined with `cargo metadata` licences): 47 crates, all of
MIT, MIT OR Apache-2.0, Apache-2.0 OR MIT, (MIT OR Apache-2.0) AND
Unicode-3.0, or Unlicense OR MIT (memchr, taken under MIT). Nothing GPL,
LGPL, AGPL, MPL-only, SSPL, BUSL or unlicensed.

## Targets

| Target | Command | Result |
|---|---|---|
| x86_64-unknown-linux-musl | `cargo build --release -p openmind-discovery --target x86_64-unknown-linux-musl` | ok, 680656 bytes |
| aarch64-unknown-linux-musl | same, `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld` | ok, 588496 bytes |
| armv7-unknown-linux-musleabihf | `cargo check --all-targets` | ok |
| aarch64-linux-android | `cargo check --all-targets` | ok |

No asm, SIMD, `target-cpu` flags or OpenSSL.
