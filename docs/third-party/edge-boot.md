<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: swarm/edge-boot

Written 2026-10-20T19:41:00+02:00 (Europe/Zagreb).

**No new dependencies and no copied code.** `edge/openmind-node/Cargo.toml` and
`edge/openmind-node/Cargo.lock` are unchanged; the root `Cargo.toml`/`Cargo.lock`
are untouched. The new boot path (`src/boot.rs`: HTTP/1.1 parser, pairing store,
signal handling, atomic writes) is std + the crate's existing `libc`, `sha2`,
`serde`/`serde_json` and `getrandom`. `pair::verify` uses the existing
`ed25519-dalek` (`verify_strict`, `is_weak`).

Existing crates the new code calls (already in the lock file before this branch):

| Crate | Version | Licence (SPDX) | Source | Used here for |
|---|---|---|---|---|
| libc | 0.2.191 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc | fork/prctl/ptrace/unshare/syscall probes, `SYS_*` numbers the deny-list is tested against, signal handler |
| ed25519-dalek | 2.x | BSD-3-Clause | https://github.com/dalek-cryptography/curve25519-dalek | `verify_strict` in `pair::verify` |
| sha2 | 0.10 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | SHA-256 of pairing codes and tokens |
| serde / serde_json | 1 | MIT OR Apache-2.0 | https://github.com/serde-rs | store/boot records, `deny_unknown_fields` payload parsing |
| getrandom | 0.2 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom | pairing code, token, nonce randomness |

Transitive licence check (`cargo metadata --offline --filter-platform
x86_64-unknown-linux-musl`, 64 packages): MIT OR Apache-2.0 (and the `/` spelling),
MIT, BSD-3-Clause, MIT OR BSD-3-Clause, (MIT OR Apache-2.0) AND Unicode-3.0, and
one Unlicense OR MIT (taken under MIT). All on the allowlist; no GPL/LGPL/MPL/SSPL/BUSL.

Behaviour ported (own project code, MIT, not third party): the fail-closed pairing
rules of galion-one (swarm/one-probe b44cd4f, from ws/ui 15cdb53). Syscall numbers
are Linux UAPI facts (`arch/x86/entry/syscalls/syscall_64.tbl`,
`include/uapi/asm-generic/unistd.h`), checked in a unit test against libc's
`SYS_*` constants, not copied code.
