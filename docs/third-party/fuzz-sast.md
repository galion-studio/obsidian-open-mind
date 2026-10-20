<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party tools and code: fuzz-sast (swarm slot 25)

- Written: 2026-10-20T21:00:00+02:00 (Europe/Zagreb)
- Branch: `swarm/fuzz-sast`
- No code was copied from other projects. No new dependency was added to any
  crate in the main workspace. Everything below is fuzz-only or CI-only and
  is never linked into a release binary.

## Fuzz-only crates (`fuzz/Cargo.toml`, own workspace, nightly only)

| Crate | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| libfuzzer-sys | 0.4.13 | (MIT OR Apache-2.0) AND NCSA | https://github.com/rust-fuzz/libfuzzer | `fuzz_target!` and the bundled LLVM libFuzzer engine (C++) |
| arbitrary | 1.5.0 | MIT OR Apache-2.0 | https://github.com/rust-fuzz/arbitrary | pulled in by libfuzzer-sys (not used directly) |
| cc | 1.9.0 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs | builds libFuzzer's C++ (build dependency) |
| jobserver | 0.1.35 | MIT OR Apache-2.0 | https://github.com/rust-lang/jobserver-rs | cc dependency |
| find-msvc-tools | 0.1.15 | MIT OR Apache-2.0 | https://github.com/rust-lang/cc-rs | cc dependency (Windows only, unused here) |
| getrandom | 0.4.3 | MIT OR Apache-2.0 | https://github.com/rust-random/getrandom | jobserver dependency |
| r-efi | 6.0.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later (we take MIT OR Apache-2.0) | https://github.com/r-efi/r-efi | getrandom dependency on UEFI targets only; not compiled on Linux |
| serde_json | 1 (feature `float_roundtrip`) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | already a workspace dependency; the fuzz crate only turns on `float_roundtrip` so the JSON round-trip check compares exact floats |

All other crates in `fuzz/Cargo.lock` are the workspace's own crates and
their existing dependencies (listed in the other third-party notes).
Checked with `cargo metadata --locked` in `fuzz/` on 2026-10-20.

**Needs owner approval: NCSA.** libfuzzer-sys bundles LLVM's libFuzzer
sources, so its licence expression includes NCSA (University of
Illinois/NCSA Open Source License, a permissive BSD-style licence). NCSA is
not on the RULES.md allow-list. It is only ever compiled into the nightly
fuzz binaries in `fuzz/` (own workspace, `publish = false`) and never into
anything we ship. There is no libFuzzer binding without it (cargo-fuzz
requires libfuzzer-sys). The task itself named cargo-fuzz.

Why not write it ourselves: a coverage-guided fuzzer (sanitizer coverage
counters, corpus management, mutation, minimisation, crash reproduction) is
a large, subtle piece of software; libFuzzer is the standard engine that
cargo-fuzz, OSS-Fuzz and ClusterFuzzLite all build on.

## Tools (not dependencies of any crate)

| Tool | Version | Licence (SPDX) | Source | Used for |
|---|---|---|---|---|
| cargo-fuzz | 0.13.1 | MIT OR Apache-2.0 | https://github.com/rust-fuzz/cargo-fuzz | `cargo fuzz build/run`; installed into `/workspace/swarm/target/fuzz-sast/tools` on the box and with `cargo install --locked` in CI |
| Rust nightly | nightly-2026-10-20 (rustc 1.101.0-nightly 07e526db9) | MIT OR Apache-2.0 | https://github.com/rust-lang/rust | `-Zsanitizer=address` and sancov flags cargo-fuzz needs. Fuzzing only; release builds stay on stable |
| ClusterFuzzLite | actions `v1` @ 884713a6c30a92e5e8544c39945cd7cb630abcd1 | Apache-2.0 | https://github.com/google/clusterfuzzlite | `.github/workflows/cflite-pr.yml` + `.clusterfuzzlite/` (also what OpenSSF Scorecard's Fuzzing check detects for Rust) |
| OSS-Fuzz base-builder-rust image | sha256:3428d0a90b1e7efbe2ba947bd1d0c9a70594986b4b6553804547f1fb24cfc92f | Apache-2.0 | https://github.com/google/oss-fuzz | ClusterFuzzLite build image (`.clusterfuzzlite/Dockerfile`) |
| github/codeql-action | v4.39.0 @ 6f2ac81f5cd81fe351a197ba0b00d91d03cd2aee | MIT (the action) | https://github.com/github/codeql-action | `.github/workflows/codeql.yml` |
| CodeQL CLI + query packs | downloaded by the action | GitHub CodeQL Terms and Conditions (not an OSI licence) | https://github.com/github/codeql-cli-binaries/blob/main/LICENSE.md | the SAST engine. Free for public open-source repositories; a private repository needs GitHub Advanced Security (paid), so the workflow only runs while the repo is public. **Owner decision** |
| actions/checkout | v4.4.0 @ 11d5960a326750d5838078e36cf38b85af677262 | MIT | https://github.com/actions/checkout | same pin as swarm/launch-gates |
| actions/upload-artifact | v4.6.2 @ ea165f8d65b6e75b540449e92b4886f43607fa02 | MIT | https://github.com/actions/upload-artifact | crash inputs on fuzz failure; same pin as swarm/launch-gates |
| g++ 14.2.0 (Debian `g++-14-x86-64-linux-gnu`, `libstdc++-14-dev`) | 14.2.0-19 | GPL-3.0-or-later WITH GCC-exception-3.1 | https://packages.debian.org/trixie/g++-14 | box only: libfuzzer-sys's build script needs a C++ compiler and the box had none. Unpacked with `dpkg -x` into `/workspace/swarm/target/fuzz-sast/debs` (no root, no system change). A build tool, not linked code: libFuzzer objects link only against the system `libstdc++.so.6`, which the runtime library exception covers. CI runners have g++ already |

SHAs resolved on 2026-10-20 with `git ls-remote` (tags dereferenced to the
commit) and the gcr.io registry API (image manifest list digest).
