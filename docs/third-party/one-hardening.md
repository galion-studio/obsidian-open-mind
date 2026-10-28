<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: swarm/one-hardening (galion-one)

Recorded 2026-10-20 20:50 CEST (+02:00).

## New runtime dependencies: none

The galion-one binary still has **zero** crates.io dependencies
(`cargo tree -p galion-one -e normal` is just `galion-one`). The static musl
builds have no `NEEDED` entries.

## New dev-dependency (tests only, not linked into the binary)

| crate | version | licence | source | use |
|---|---|---|---|---|
| zagreb-time (workspace crate) | 0.1.0 | MIT | `crates/zagreb-time` in this repo | cross-checks `src/zagreb.rs` against the tz database, every hour 2020-2040, and gives the real-life test the expected Zagreb timestamp |

Its transitive crates are already in the workspace `Cargo.lock` (no new
entries): chrono 0.4.45 (MIT OR Apache-2.0, https://github.com/chronotope/chrono),
chrono-tz 0.10.4 (MIT OR Apache-2.0, https://github.com/chronotope/chrono-tz),
phf 0.12.1 and phf_shared 0.12.1 (MIT, https://github.com/rust-phf/rust-phf),
siphasher 1.0.4 (MIT OR Apache-2.0, https://github.com/jedisct1/rust-siphash),
num-traits 0.2.19 (MIT OR Apache-2.0, https://github.com/rust-num/num-traits),
iana-time-zone 0.1.65 (MIT OR Apache-2.0, https://github.com/strawlab/iana-time-zone).

Why not use zagreb-time in the binary: it would pull chrono + chrono-tz +
phf into a crate that is deliberately dependency-free and builds for
x86_64/aarch64 musl, aarch64-android and armv7. The Zagreb rule (EU summer
time) is 15 lines; the tz-database crate is used only to prove those lines
right.

## Borrowed algorithm (re-implemented, no code copied)

- `days_from_civil` / `civil_from_days` in `crates/galion-one/src/zagreb.rs`:
  Howard Hinnant, "chrono-Compatible Low-Level Date Algorithms",
  http://howardhinnant.github.io/date_algorithms.html. The page says
  "Consider these donated to the public domain." Re-written in Rust from the
  description (Euclidean division instead of the C++ branches); the source
  is cited in the code.

## Evaluated and rejected (2026-10-20)

| replaces | candidate | licence | why not |
|---|---|---|---|
| `pdftotext` (poppler) | lopdf 0.45.0 (https://github.com/J-F-Liu/lopdf) | MIT | Hard (non-optional) deps on `aes`, `cbc`, `ecb`, `md-5`, `sha2`, `rand`, `getrandom`, `stringprep`, `brotli-decompressor`, `nom`, `flate2`... (~20+ crates). RustCrypto `aes`/`sha2` select AES-NI / SHA-NI intrinsics at run time via `cpufeatures`, which breaks the no-SIMD rule, and the tree breaks the minimal-deps rule. |
| `pdftotext` | pdf-extract 0.12.1 (https://github.com/jrmuizel/pdf-extract) | MIT | Built on lopdf (same problem) plus euclid, adobe-cmap-parser, type1-encoding-parser, cff-parser, postscript, unicode-normalization. |
| `curl` (HTTPS edge probes, OpenRouter model list) | rustls (https://github.com/rustls/rustls) | Apache-2.0 OR ISC OR MIT | Needs a crypto provider: `ring` (contains assembly) or `aws-lc-rs` (C and assembly); the RustCrypto provider (`rustls-rustcrypto`) is experimental and its AES/SHA crates dispatch to SIMD. `ureq`/`attohttpc`/`minreq` with TLS all end at rustls or native-tls (openssl). |
| `nvidia-smi` | nvml-wrapper (https://github.com/rust-nvml/nvml-wrapper) | MIT OR Apache-2.0 | FFI to NVIDIA's closed libnvidia-ml (dlopen); no pure-Rust way to query the GPU. Kept nvidia-smi, now optional and timed. |
| `ps` | (none needed) | - | Replaced by reading `/proc/<pid>/cmdline` with std only. |

Decision: keep `pdftotext`, `curl` and `nvidia-smi` as optional subprocesses
(missing binary = feature off, never a crash or hang); remove `ps`.
