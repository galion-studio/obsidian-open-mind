<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: hermes-core (crates/hermes-core)

Written 2026-10-20T21:38:00+02:00 (Europe/Zagreb). Branch `swarm/hermes-core`.
No code was copied. No new crate entered Cargo.lock: every dependency below was already
locked by the workspace (the only Cargo.lock change is the `hermes-core` package entry itself).

## Direct dependencies

| Crate | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde | Derive (de)serialisation of registry, tasks, ledger events, API bodies. |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | JSONL ledger lines, `--json` CLI output, HTTP status API bodies. |
| toml | 0.8.23 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml | Parse `config/hermes/sectors.toml` (already in the workspace lock at this version). |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes | SHA-256 hash chain of the ledger (`prev`). Feature `force-soft` (no SIMD/asm), same as hades-gate. |
| zagreb-time | path | MIT (in-repo) | crates/zagreb-time | Europe/Zagreb timestamps with offset. |
| hades-gate | path | MIT (in-repo, swarm/hades-approval ccaab42) | crates/hades-gate | Ed25519 single-use approval verification. Its own deps are listed in docs/third-party/hades-approval.md. |
| gaia-core | path | MIT (in-repo, swarm/gaia-land 9d3ec18) | crates/gaia-core | The 52 systems / 9 functions and the ownership registry the sector map is validated against. |

## Transitive dependencies (normal edges, `cargo tree -p hermes-core -e normal`)

All are permissive and on the allowlist:

| Crate | Version | Licence | Source |
|---|---|---|---|
| toml_edit | 0.22.27 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_datetime | 0.6.11 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| winnow | 0.7.15 | MIT | https://github.com/winnow-rs/winnow |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | https://github.com/indexmap-rs/equivalent |
| serde_core / serde_derive | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| itoa | 1.0.18 | MIT OR Apache-2.0 | https://github.com/dtolnay/itoa |
| zmij | 1.0.23 | MIT | https://github.com/dtolnay/zmij |
| memchr | 2.8.3 | Unlicense OR MIT (used under MIT) | https://github.com/BurntSushi/memchr |
| proc-macro2 / quote / syn | 1.0.107 / 1.0.47 / 3.0.9 | MIT OR Apache-2.0 | https://github.com/dtolnay |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/dtolnay/unicode-ident |
| digest / crypto-common / block-buffer / generic-array / typenum / cpufeatures / cfg-if | 0.10.7 / 0.1.7 / 0.10.4 / 0.14.7 / 1.20.1 / 0.2.17 / 1.0.5 | MIT OR Apache-2.0 (generic-array MIT) | RustCrypto, fizyk20, paholg, rust-lang |
| chrono / chrono-tz / phf / phf_shared / siphasher / num-traits / iana-time-zone | 0.4.45 / 0.10.4 / 0.12.1 / 0.12.1 / 1.0.4 / 0.2.19 / 0.1.65 | MIT OR Apache-2.0 (phf MIT) | via zagreb-time |
| ed25519-dalek / curve25519-dalek / subtle | 2.2.0 / 4.1.3 / 2.6.1 | BSD-3-Clause | via hades-gate |
| ed25519 / signature / zeroize | 2.2.3 / 2.2.0 / 1.9.1 | Apache-2.0 OR MIT | via hades-gate |

None is GPL, LGPL, AGPL, MPL, SSPL or BUSL.

## Why no other crates

- **HTTP**: the status API is a std `TcpListener` server (GET/HEAD, loopback only). It is about 280 lines, so there is no hyper, axum or tokio runtime for a read-only status page.
- **Concurrency**: std threads, `Mutex`, `Condvar` and `mpsc`. There is no async runtime and no rayon.
- **Ledger locking**: std `File::try_lock` (stable since Rust 1.89). There is no fs2 or fd-lock.
- **No network client**: the OpenRouter provider refuses with the 402 message and opens no socket.
