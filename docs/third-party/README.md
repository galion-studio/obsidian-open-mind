<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party register

Written 2026-10-21 10:52 (+02:00) Europe/Zagreb, slot 172 docs-thirdparty-register (docs plan T2.4, gap D15), on
`swarm/integration-4` 42de672. One page that answers "what outside code is in this repo, under
which licence, and who documented it". The 40 per-branch files below stay the detailed record;
this page links them and cross-checks them against the lockfile.

## 1. Summary (integration-4 42de672, measured 2026-10-21 10:50 (+02:00))

| item | number | source |
|---|---|---|
| packages in `Cargo.lock` | 234 | `grep -c 'name = ' Cargo.lock` |
| workspace crates (ours, MIT) | 63 | `cargo metadata --offline --all-features`, `source == null` |
| crates.io packages (third party) | 171 | same, `source` = crates.io registry; no git or path sources |
| third-party crates used **directly** by a workspace crate | 17 | `dependencies[]` of workspace crates |
| direct crates documented in a third-party file | 17 of 17 | §3, column "documented in" |
| crates.io packages named in at least one third-party file | 127 of 171 | §3 |
| crates.io packages named in no file (all transitive) | 44 | §3.1 |
| `cargo deny --offline --all-features check licenses bans sources` | **licenses ok, bans ok, sources ok** (exit 0; 45 `wildcard` + 2 `duplicate` warnings, no errors) | §5 |
| `cargo deny --offline --all-features check advisories` | **FAILED** (exit 1): RUSTSEC-2026-0367/0368/0369 (zip via candle-core), RUSTSEC-2024-0436 (paste, unmaintained) | §4, §5 |
| licences outside the RULES.md allow-list that a binary links | **0** | §2 |
| GPL/LGPL programs used as tools (never linked) | 6 entries, **pending owner ruling** | §6 |

The docs plan quoted "130 SBOM components" from an earlier tree (GATES-OCT31.md:15 at that time).
integration-4 has more because later waves added rusqlite, candle, ed25519-dalek, regex and their
dependencies. Re-measure with the command in §5 rather than trusting either number.

## 2. Licences in the lockfile

Every crates.io package declares a licence (none missing). Expressions with `/` are the old
spelling of `OR`. Counts are packages:

| declared licence | packages | taken under |
|---|---|---|
| `MIT OR Apache-2.0` | 98 | MIT or Apache-2.0 |
| `MIT` | 27 | MIT |
| `Apache-2.0 OR MIT` | 12 | MIT or Apache-2.0 |
| `MIT/Apache-2.0` | 6 | MIT or Apache-2.0 |
| `Unlicense OR MIT` | 4 | MIT (Unlicense is not on the allow-list) |
| `Unicode-3.0` | 4 | Unicode-3.0 |
| `BSD-3-Clause` | 4 | BSD-3-Clause |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 3 | MIT or Apache-2.0 |
| `Zlib OR Apache-2.0 OR MIT` | 2 | MIT or Apache-2.0 |
| `Unlicense/MIT` | 2 | MIT (Unlicense is not on the allow-list) |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 2 | MIT or Apache-2.0 |
| `Zlib` | 1 | Zlib |
| `MIT OR Zlib OR Apache-2.0` | 1 | MIT or Apache-2.0 |
| `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | 1 | MIT (the LGPL branch is **not** used) |
| `MIT OR Apache-2.0 OR BSD-1-Clause` | 1 | MIT or Apache-2.0 (BSD-1-Clause is not on the allow-list) |
| `Apache-2.0` | 0 on swarm/safetensors-mit-alt (was 1: safetensors 0.7.0, now replaced by MIT vendor/safetensors-mit) | Apache-2.0 |
| `0BSD OR MIT OR Apache-2.0` | 1 | MIT or Apache-2.0 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 1 | (MIT or Apache-2.0) and Unicode-3.0, both allowed |

Notes:
* `r-efi` 5.3.0 is `MIT OR Apache-2.0 OR LGPL-2.1-or-later`; it is a UEFI-only target dependency of
  `getrandom` (pulled in by candle-core through `rand`) and is taken under MIT. No shipped target (musl x86_64/aarch64, android, armv7) compiles it.
* `libsqlite3-sys` 0.30.1 (via `rusqlite`, `bundled`) compiles the SQLite C amalgamation. SQLite itself
  is public domain; the crate is MIT. This is the only C code linked into our binaries
  (see [rust-ports](rust-ports.md), [memory-real](memory-real.md), [ucpu](ucpu.md)).
* BSD-3-Clause crates (`ed25519-dalek`, `curve25519-dalek`, `subtle`, `opus-pure`) need the copyright
  notice reproduced in binary distributions. Nothing is distributed yet; the launch export must carry
  their notices (see §7, item O3).

## 3. Full crates.io register

"direct users" are workspace crates that name the package in `[dependencies]`
(`dev` = dev-dependency only). "documented in" lists every file in this folder that names the
crate (whole-word match); a direct crate must always have at least one. To see why a transitive crate is
in the graph: `cargo tree -i <crate> --all-features --target all -e normal,build,dev`.

| crate | version | licence (as declared) | direct users | documented in |
|---|---|---|---|---|
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | transitive | [rust-ports](rust-ports.md) |
| ahash | 0.8.12 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| aho-corasick | 1.1.5 | Unlicense OR MIT | transitive | [guard-rust](guard-rust.md) |
| allocator-api2 | 0.2.21 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| android_system_properties | 0.1.6 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| autocfg | 1.5.1 | Apache-2.0 OR MIT | transitive | [gaia-land](gaia-land.md) |
| base64ct | 1.8.3 | Apache-2.0 OR MIT | transitive | [hades-approval](hades-approval.md) |
| bitflags | 2.13.2 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | transitive | [common](common.md), [hades-approval](hades-approval.md) |
| bumpalo | 3.20.3 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| bytemuck | 1.25.2 | Zlib OR Apache-2.0 OR MIT | transitive | [voice-mouth](voice-mouth.md) |
| bytemuck_derive | 1.12.1 | Zlib OR Apache-2.0 OR MIT | transitive | [voice-mouth](voice-mouth.md) |
| byteorder | 1.5.0 | Unlicense OR MIT | transitive | [voice-mouth](voice-mouth.md) |
| candle-core | 0.9.2 | MIT OR Apache-2.0 | `galion-voice-cpu` | [voice-mouth](voice-mouth.md) |
| candle-nn | 0.9.2 | MIT OR Apache-2.0 | `galion-voice-cpu` | [voice-mouth](voice-mouth.md) |
| cc | 1.8.0 | MIT OR Apache-2.0 | transitive | [desk-linkwatch](desk-linkwatch.md), [rust-ports](rust-ports.md), [ucpu](ucpu.md) |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md), [voice-mouth](voice-mouth.md) |
| chrono | 0.4.45 | MIT OR Apache-2.0 | `zagreb-time` | [bench-banking](bench-banking.md), [bench-harness](bench-harness.md), [bench-wallstreet](bench-wallstreet.md), [common](common.md), [deploy-rust](deploy-rust.md), [deploy-sim](deploy-sim.md), [gaia-land](gaia-land.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [guard-rust](guard-rust.md), [health-land](health-land.md), [lan-discovery](lan-discovery.md), [nerve](nerve.md), [poseidon](poseidon.md) |
| chrono-tz | 0.10.4 | MIT OR Apache-2.0 | `zagreb-time` | [bench-banking](bench-banking.md), [bench-harness](bench-harness.md), [bench-wallstreet](bench-wallstreet.md), [common](common.md), [deploy-rust](deploy-rust.md), [gaia-land](gaia-land.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [guard-rust](guard-rust.md), [health-land](health-land.md), [lan-discovery](lan-discovery.md), [nerve](nerve.md), [poseidon](poseidon.md) |
| const-oid | 0.9.6 | Apache-2.0 OR MIT | transitive | [hades-approval](hades-approval.md) |
| core-foundation-sys | 0.8.7 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | transitive | [common](common.md), [deploy-rust](deploy-rust.md), [hades-approval](hades-approval.md), [rust-ports](rust-ports.md) |
| crc32fast | 1.5.3 | MIT OR Apache-2.0 | transitive | [rust-ports](rust-ports.md), [voice-mouth](voice-mouth.md) |
| crossbeam-deque | 0.8.8 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| crossbeam-epoch | 0.9.21 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| crossbeam-utils | 0.8.23 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| crunchy | 0.2.4 | MIT | transitive | none (transitive only) |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | transitive | [common](common.md), [hades-approval](hades-approval.md) |
| curve25519-dalek | 4.1.3 | BSD-3-Clause | transitive | [edge-boot](edge-boot.md), [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md) |
| curve25519-dalek-derive | 0.1.1 | MIT/Apache-2.0 | transitive | [hades-approval](hades-approval.md) |
| der | 0.7.10 | Apache-2.0 OR MIT | transitive | [hades-approval](hades-approval.md) |
| digest | 0.10.7 | MIT OR Apache-2.0 | transitive | [common](common.md), [deploy-rust](deploy-rust.md), [hades-approval](hades-approval.md) |
| dyn-stack | 0.13.2 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| dyn-stack-macros | 0.1.3 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| ed25519 | 2.2.3 | Apache-2.0 OR MIT | transitive | [edge-boot](edge-boot.md), [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md) |
| ed25519-dalek | 2.2.0 | BSD-3-Clause | `hades-gate` | [edge-boot](edge-boot.md), [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md) |
| either | 1.19.0 | MIT OR Apache-2.0 | transitive | [bench-banking](bench-banking.md), [common](common.md), [deploy-rust](deploy-rust.md), [hades-approval](hades-approval.md), [sandbox](sandbox.md), [voice-mouth](voice-mouth.md) |
| enum-as-inner | 0.6.1 | MIT/Apache-2.0 | transitive | none (transitive only) |
| equivalent | 1.0.3 | Apache-2.0 OR MIT | transitive | [bench-harness](bench-harness.md), [voice-mouth](voice-mouth.md) |
| fallible-iterator | 0.3.0 | MIT/Apache-2.0 | transitive | none (transitive only) |
| fallible-streaming-iterator | 0.1.9 | MIT/Apache-2.0 | transitive | none (transitive only) |
| fiat-crypto | 0.2.9 | MIT OR Apache-2.0 OR BSD-1-Clause | transitive | [hades-approval](hades-approval.md) |
| find-msvc-tools | 0.1.14 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| float8 | 0.6.1 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| foldhash | 0.2.0 | Zlib | transitive | [bench-harness](bench-harness.md), [desk-linkwatch](desk-linkwatch.md), [voice-mouth](voice-mouth.md) |
| futures-core | 0.3.34 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| futures-task | 0.3.34 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| gemm | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| gemm-c32 | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| gemm-c64 | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| gemm-common | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| gemm-f16 | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| gemm-f32 | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| gemm-f64 | 0.19.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| generic-array | 0.14.7 | MIT | transitive | [deploy-rust](deploy-rust.md), [hades-approval](hades-approval.md) |
| getrandom | 0.2.17 | MIT OR Apache-2.0 | transitive | [edge-boot](edge-boot.md), [hades-approval](hades-approval.md), [voice-mouth](voice-mouth.md) |
| getrandom | 0.3.4 | MIT OR Apache-2.0 | transitive | [edge-boot](edge-boot.md), [hades-approval](hades-approval.md), [voice-mouth](voice-mouth.md) |
| half | 2.7.1 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| hashbrown | 0.14.5 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [voice-mouth](voice-mouth.md) |
| hashbrown | 0.16.1 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [voice-mouth](voice-mouth.md) |
| hashbrown | 0.17.1 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [voice-mouth](voice-mouth.md) |
| hashlink | 0.9.1 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| heck | 0.5.0 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| hermit-abi | 0.5.3 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [gaia-land](gaia-land.md), [gaia-poseidon](gaia-poseidon.md), [health-land](health-land.md), [nerve](nerve.md), [poseidon](poseidon.md) |
| iana-time-zone-haiku | 0.1.2 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| indexmap | 2.14.2 | Apache-2.0 OR MIT | transitive | [bench-harness](bench-harness.md), [voice-mouth](voice-mouth.md) |
| itoa | 1.0.18 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [bench-vending](bench-vending.md), [common](common.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| js-sys | 0.3.106 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| libc | 0.2.191 | MIT OR Apache-2.0 | `galion-backup`, `galion-guard` (dev), `openmind-sandbox` | [cutover-prep](cutover-prep.md), [deploy-rust](deploy-rust.md), [deploy-sim](deploy-sim.md), [edge-boot](edge-boot.md), [hades-approval](hades-approval.md), [hermes-tools](hermes-tools.md), [lan-discovery](lan-discovery.md), [rust-ports](rust-ports.md), [sandbox](sandbox.md), [voice-mouth](voice-mouth.md) |
| libm | 0.2.16 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| libsqlite3-sys | 0.30.1 | MIT | transitive | [ucpu](ucpu.md) |
| log | 0.4.34 | MIT OR Apache-2.0 | transitive | [common](common.md), [lan-discovery](lan-discovery.md), [poseidon](poseidon.md) |
| memchr | 2.8.3 | Unlicense OR MIT | transitive | [bench-harness](bench-harness.md), [bench-vending](bench-vending.md), [common](common.md), [desk-linkwatch](desk-linkwatch.md), [gaia-hades-tokens](gaia-hades-tokens.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [guard-rust](guard-rust.md), [hades-approval](hades-approval.md), [lan-discovery](lan-discovery.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| memmap2 | 0.9.11 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | `galion-backup` | [cutover-prep](cutover-prep.md), [rust-ports](rust-ports.md) |
| num-complex | 0.4.6 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| num-traits | 0.2.19 | MIT OR Apache-2.0 | `galion-voice-cpu` | [bench-harness](bench-harness.md), [gaia-land](gaia-land.md), [gaia-poseidon](gaia-poseidon.md), [health-land](health-land.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| num_cpus | 1.17.0 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| opus-pure | 0.2.2 | BSD-3-Clause | `persona-plex-relay` | [voice-mouth](voice-mouth.md) |
| paste | 1.0.15 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| phf | 0.12.1 | MIT | transitive | [bench-harness](bench-harness.md), [gaia-land](gaia-land.md), [gaia-poseidon](gaia-poseidon.md), [health-land](health-land.md), [nerve](nerve.md), [poseidon](poseidon.md) |
| phf_shared | 0.12.1 | MIT | transitive | [bench-harness](bench-harness.md), [gaia-land](gaia-land.md), [gaia-poseidon](gaia-poseidon.md), [health-land](health-land.md), [nerve](nerve.md), [poseidon](poseidon.md) |
| pin-project-lite | 0.2.17 | Apache-2.0 OR MIT | transitive | none (transitive only) |
| pkcs8 | 0.10.2 | Apache-2.0 OR MIT | transitive | [hades-approval](hades-approval.md) |
| pkg-config | 0.3.34 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| ppv-lite86 | 0.2.21 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 | transitive | [bench-vending](bench-vending.md), [common](common.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| pulp | 0.22.3 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| pulp-wasm-simd-flag | 0.1.1 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| quote | 1.0.47 | MIT OR Apache-2.0 | transitive | [bench-vending](bench-vending.md), [common](common.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| r-efi | 5.3.0 | MIT OR Apache-2.0 OR LGPL-2.1-or-later | transitive | none (transitive only) |
| rand | 0.9.5 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md), [memory-real](memory-real.md), [voice-mouth](voice-mouth.md) |
| rand_chacha | 0.9.0 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| rand_core | 0.6.4 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md), [voice-mouth](voice-mouth.md) |
| rand_core | 0.9.5 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md), [voice-mouth](voice-mouth.md) |
| rand_distr | 0.5.1 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| raw-cpuid | 11.6.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| rayon | 1.12.0 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| rayon-core | 1.13.0 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| reborrow | 0.5.5 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| regex | 1.13.1 | MIT OR Apache-2.0 | `galion-guard` | [guard-rust](guard-rust.md) |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | transitive | [guard-rust](guard-rust.md) |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | transitive | [guard-rust](guard-rust.md) |
| rusqlite | 0.32.1 | MIT | `desk-hermes`, `galion-backup`, `hermes-rs`, `one-defence`, `openmind-memory` | [cutover-prep](cutover-prep.md), [desk-linkwatch](desk-linkwatch.md), [memory-real](memory-real.md), [rust-ports](rust-ports.md) |
| rustc_version | 0.4.1 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md) |
| rustversion | 1.0.23 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| safetensors (Galion clean-room, `vendor/safetensors-mit`, replaces registry 0.7.0 via `[patch.crates-io]`) | 0.7.0-compatible | MIT | transitive (candle) | [safetensors-mit-alt](safetensors-mit-alt.md) — OWNER-PENDING AP-1 option C; registry safetensors 0.7.0 (Apache-2.0) no longer in Cargo.lock on this branch |
| same-file | 1.0.6 | Unlicense/MIT | transitive | none (transitive only) |
| semver | 1.0.28 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md) |
| seq-macro | 0.3.6 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| serde | 1.0.229 | MIT OR Apache-2.0 | `desk-hermes`, `galion-boot`, `galion-common`, `galion-guard`, `galion-vending`, `galion-voice-cpu`, `galion-wallstreet`, `guardian-tick`, `link-watch`, `one-defence`, `openmind-config`, `openmind-discovery`, `openmind-memory`, `openmind-nerve`, `openmind-provider`, `openmind-queue`, `openmind-schema` | [bench-banking](bench-banking.md), [bench-harness](bench-harness.md), [bench-hermes](bench-hermes.md), [bench-scoreboard](bench-scoreboard.md), [bench-vending](bench-vending.md), [bench-wallstreet](bench-wallstreet.md), [common](common.md), [cortex-sensor](cortex-sensor.md), [desk-linkwatch](desk-linkwatch.md), [edge-boot](edge-boot.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [guard-rust](guard-rust.md), [hades-approval](hades-approval.md), [integration-1](integration-1.md), [lan-routing](lan-routing.md), [memory-real](memory-real.md), [nerve](nerve.md), [poseidon](poseidon.md), [real-harness](real-harness.md), [survival-heal](survival-heal.md), [voice-mouth](voice-mouth.md) |
| serde_core | 1.0.229 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [bench-vending](bench-vending.md), [common](common.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| serde_derive | 1.0.229 | MIT OR Apache-2.0 | transitive | [bench-vending](bench-vending.md), [common](common.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| serde_json | 1.0.154 | MIT OR Apache-2.0 | `bench-banking`, `bench-scoreboard`, `desk-hermes`, `gaia-live`, `gaia-poseidon`, `gaia`, `galion-bench`, `galion-boot`, `galion-common`, `galion-desk` (dev), `galion-guard`, `galion-realtest`, `galion-vending`, `galion-voice-cpu`, `galion-wallstreet`, `guardian-tick`, `hades-gate`, `hermes-bench`, `hermes-gateway`, `hermes-rs`, `hermes-tools`, `link-watch`, `one-defence`, `one-set-model`, `one-set-model` (dev), `one-talk`, `openmind-discovery`, `openmind-exo`, `openmind-llama`, `openmind-mcp-stack`, `openmind-memory`, `openmind-nerve`, `openmind-provider`, `openmind-queue`, `openmind-schema`, `openmind-speech` | [bench-banking](bench-banking.md), [bench-harness](bench-harness.md), [bench-hermes](bench-hermes.md), [bench-scoreboard](bench-scoreboard.md), [bench-vending](bench-vending.md), [bench-wallstreet](bench-wallstreet.md), [common](common.md), [cortex-sensor](cortex-sensor.md), [desk-linkwatch](desk-linkwatch.md), [edge-boot](edge-boot.md), [gaia-land](gaia-land.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [guard-rust](guard-rust.md), [hades-approval](hades-approval.md), [hades-redteam](hades-redteam.md), [hermes-tools](hermes-tools.md), [lan-routing](lan-routing.md), [memory-real](memory-real.md), [nerve](nerve.md), [poseidon](poseidon.md), [real-harness](real-harness.md), [survival-heal](survival-heal.md), [voice-mouth](voice-mouth.md) |
| serde_spanned | 0.6.9 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | `hades-gate` | [common](common.md), [deploy-rust](deploy-rust.md), [edge-boot](edge-boot.md), [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md), [rust-ports](rust-ports.md) |
| shlex | 2.0.1 | MIT OR Apache-2.0 | transitive | [hades-redteam](hades-redteam.md) |
| signature | 2.2.0 | Apache-2.0 OR MIT | transitive | [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md) |
| siphasher | 1.0.4 | MIT OR Apache-2.0 | transitive | [bench-harness](bench-harness.md), [gaia-land](gaia-land.md), [gaia-poseidon](gaia-poseidon.md), [health-land](health-land.md), [nerve](nerve.md), [poseidon](poseidon.md) |
| slab | 0.4.12 | MIT | transitive | none (transitive only) |
| smallvec | 1.16.3 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| socket2 | 0.6.5 | MIT OR Apache-2.0 | `openmind-discovery` | [lan-discovery](lan-discovery.md), [lan-routing](lan-routing.md) |
| spki | 0.7.3 | Apache-2.0 OR MIT | transitive | [hades-approval](hades-approval.md) |
| stable_deref_trait | 1.2.1 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| subtle | 2.6.1 | BSD-3-Clause | transitive | [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md) |
| syn | 2.0.119 | MIT OR Apache-2.0 | transitive | [bench-vending](bench-vending.md), [common](common.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| syn | 3.0.9 | MIT OR Apache-2.0 | transitive | [bench-vending](bench-vending.md), [common](common.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| synstructure | 0.14.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| sysctl | 0.6.0 | MIT | transitive | none (transitive only) |
| thiserror | 1.0.69 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| thiserror-impl | 1.0.69 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| thiserror-impl | 2.0.21 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| toml | 0.8.23 | MIT OR Apache-2.0 | `galion-boot`, `galion-guard`, `guardian-tick`, `openmind-config`, `openmind-schema`, `openmind-tulu` | [cortex-sensor](cortex-sensor.md), [docs-sync](docs-sync.md), [edge-boot](edge-boot.md), [gaia-hades-tokens](gaia-hades-tokens.md), [guard-rust](guard-rust.md), [hades-approval](hades-approval.md), [hades-redteam](hades-redteam.md), [launch-gates](launch-gates.md), [room](room.md), [sandbox](sandbox.md), [ucpu](ucpu.md) |
| toml_datetime | 0.6.11 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| toml_edit | 0.22.27 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| toml_write | 0.1.2 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| typed-path | 0.12.4 | MIT OR Apache-2.0 | transitive | [voice-mouth](voice-mouth.md) |
| typenum | 1.20.1 | MIT OR Apache-2.0 | transitive | [hades-approval](hades-approval.md) |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 | transitive | [bench-vending](bench-vending.md), [common](common.md), [desk-linkwatch](desk-linkwatch.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |
| vcpkg | 0.2.15 | MIT/Apache-2.0 | transitive | none (transitive only) |
| version_check | 0.9.5 | MIT/Apache-2.0 | transitive | [hades-approval](hades-approval.md) |
| walkdir | 2.5.0 | Unlicense/MIT | transitive | none (transitive only) |
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | transitive | [hades-approval](hades-approval.md) |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | transitive | none (transitive only) |
| wasm-bindgen | 0.2.129 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| wasm-bindgen-macro | 0.2.129 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| wasm-bindgen-macro-support | 0.2.129 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| wasm-bindgen-shared | 0.2.129 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| winapi-util | 0.1.11 | Unlicense OR MIT | transitive | none (transitive only) |
| windows-core | 0.62.2 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| windows-implement | 0.60.2 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| windows-interface | 0.59.3 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| windows-link | 0.2.1 | MIT OR Apache-2.0 | transitive | [lan-discovery](lan-discovery.md) |
| windows-result | 0.4.1 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| windows-strings | 0.5.1 | MIT OR Apache-2.0 | transitive | none (transitive only) |
| windows-sys | 0.61.2 | MIT OR Apache-2.0 | transitive | [lan-discovery](lan-discovery.md) |
| winnow | 0.7.15 | MIT | transitive | none (transitive only) |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT | transitive | none (transitive only) |
| yoke | 0.8.3 | Unicode-3.0 | transitive | [voice-mouth](voice-mouth.md) |
| yoke-derive | 0.8.4 | Unicode-3.0 | transitive | [voice-mouth](voice-mouth.md) |
| zerocopy | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | transitive | [desk-linkwatch](desk-linkwatch.md), [voice-mouth](voice-mouth.md) |
| zerocopy-derive | 0.8.63 | BSD-2-Clause OR Apache-2.0 OR MIT | transitive | [voice-mouth](voice-mouth.md) |
| zerofrom | 0.1.8 | Unicode-3.0 | transitive | [voice-mouth](voice-mouth.md) |
| zerofrom-derive | 0.1.8 | Unicode-3.0 | transitive | [voice-mouth](voice-mouth.md) |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | `hades-gate` | [gaia-hades-tokens](gaia-hades-tokens.md), [hades-approval](hades-approval.md) |
| zip | 7.4.0 | MIT | transitive | [voice-mouth](voice-mouth.md) |
| zmij | 1.0.23 | MIT | transitive | [bench-harness](bench-harness.md), [bench-vending](bench-vending.md), [common](common.md), [gaia-live](gaia-live.md), [gaia-poseidon](gaia-poseidon.md), [nerve](nerve.md), [poseidon](poseidon.md), [voice-mouth](voice-mouth.md) |

### 3.1 Packages named in no file (44, all transitive, all allowed licences)

`ahash` 0.8.12, `android_system_properties` 0.1.6, `bumpalo` 3.20.3, `core-foundation-sys` 0.8.7, `crunchy` 0.2.4, `enum-as-inner` 0.6.1, `fallible-iterator` 0.3.0, `fallible-streaming-iterator` 0.1.9, `find-msvc-tools` 0.1.14, `futures-core` 0.3.34, `futures-task` 0.3.34, `futures-util` 0.3.34, `hashlink` 0.9.1, `heck` 0.5.0, `hermit-abi` 0.5.3, `iana-time-zone-haiku` 0.1.2, `js-sys` 0.3.106, `pin-project-lite` 0.2.17, `pkg-config` 0.3.34, `r-efi` 5.3.0, `rustversion` 1.0.23, `same-file` 1.0.6, `serde_spanned` 0.6.9, `slab` 0.4.12, `smallvec` 1.16.3, `sysctl` 0.6.0, `toml_datetime` 0.6.11, `toml_edit` 0.22.27, `toml_write` 0.1.2, `vcpkg` 0.2.15, `walkdir` 2.5.0, `wasip2` 1.0.4+wasi-0.2.12, `wasm-bindgen` 0.2.129, `wasm-bindgen-macro` 0.2.129, `wasm-bindgen-macro-support` 0.2.129, `wasm-bindgen-shared` 0.2.129, `winapi-util` 0.1.11, `windows-core` 0.62.2, `windows-implement` 0.60.2, `windows-interface` 0.59.3, `windows-result` 0.4.1, `windows-strings` 0.5.1, `winnow` 0.7.15, `wit-bindgen` 0.57.1.

They arrive through already documented direct crates (checked with `cargo tree -i`): `chrono`/`chrono-tz`
(iana-time-zone, iana-time-zone-haiku, android_system_properties, core-foundation-sys, windows-*, wasm-bindgen*,
js-sys -> futures-util/-core/-task, slab, pin-project-lite; rustversion as a wasm-bindgen build dep, bumpalo),
`toml` (toml_edit, toml_datetime, toml_write, serde_spanned, winnow), `rusqlite` (hashlink, ahash,
fallible-iterator, fallible-streaming-iterator, smallvec; pkg-config, vcpkg as libsqlite3-sys build deps;
find-msvc-tools via `cc`) and `candle-core` (sysctl -> enum-as-inner -> heck, walkdir -> same-file / winapi-util,
num_cpus -> hermit-abi, half -> crunchy, rand/getrandom -> r-efi, wasip2 -> wit-bindgen). The windows-*, wasm-*, js-sys, futures-*, core-foundation-sys, android_system_properties, iana-time-zone-haiku, hermit-abi, r-efi, wasip2 and wit-bindgen rows are platform-gated and not compiled for our Linux/Android targets; the rest (toml_edit, winnow, hashlink, smallvec and so on) are compiled. The next branch that
touches one of these direct crates should name them in its own file; until then this register covers
them.

## 4. Vendored and copied code

| what | where | licence | upstream | status | record |
|---|---|---|---|---|---|
| Whisper model code (candle 0.9 examples) | `crates/galion-voice-cpu/src/whisper/` | MIT OR Apache-2.0 | https://github.com/huggingface/candle | in integration-4 | [voice-mouth](voice-mouth.md), [integration-1](integration-1.md) (REUSE: `LICENSES/Apache-2.0.txt`) |
| kitten-core model code (tts-web @ 4ac4680a) | `crates/galion-voice-cpu/src/kitten/` | MIT, notice in `crates/galion-voice-cpu/LICENSE-kitten-core.txt` | https://github.com/idle-intelligence/tts-web | in integration-4 | [voice-mouth](voice-mouth.md) |
| CMU Pronouncing Dictionary (cmudict.dict, 3,618,488 B) | data fetched by `scripts/voice/fetch-models.sh`, **not in git**; notice `crates/galion-voice-cpu/NOTICE-cmudict.txt` is in git | BSD-2-Clause style (CMU) | https://github.com/cmusphinx/cmudict @ 7479086 | notice in integration-4 | [voice-mouth](voice-mouth.md) |
| Mel filter bytes (melfilters.bytes, 64,320 B) | fetched with the models, **not in git** | MIT OR Apache-2.0 (candle repo) | huggingface/candle @ a3b16994 | fetched data | [voice-mouth](voice-mouth.md) |
| **candle-core 0.9.2, patched** (`[patch.crates-io] candle-core = { path = "vendor/candle-core" }`) | `vendor/candle-core/` with `GALION-PATCH.md` | MIT OR Apache-2.0 (upstream `LICENSE` kept) | crates.io candle-core 0.9.2, upstream commit 3b39794c | **not in integration-4**; on swarm/candle-zip-fix a4f49b5, swarm/candle-zip-rebase b246f66 and swarm/integration-5 0d68f46 | this page; GALION-PATCH.md in the vendored folder |
| paste -> pastey shim (`[patch.crates-io] paste`) | `vendor/paste-pastey/` re-exporting pastey 0.2.3 | MIT OR Apache-2.0 | https://crates.io/crates/pastey | **not in integration-4**; on swarm/deny-paste-replace 4bfffe8 and integration-5 | this page |
| sha256.rs copied inside the repo | `crates/galion-realtest/src/sha256.rs` from `crates/galion-one/src/sha256.rs` | MIT (ours) | this repo @ 13a296c | in integration-4 | [real-harness](real-harness.md) |
| Apache-2.0 licence text | `LICENSES/Apache-2.0.txt` (from serde 1.0.229 `LICENSE-APACHE`) | Apache-2.0 text | https://github.com/serde-rs/serde | in integration-4 | [integration-1](integration-1.md) |
| MIT licence text | `LICENSES/MIT.txt` | MIT (list data CC0-1.0) | https://github.com/spdx/license-list-data | in integration-4 | [launch-gates](launch-gates.md) |
| Linux syscall numbers / audit constants | omnode seccomp tables | GPL-2.0 WITH Linux-syscall-note (UAPI facts, numbers only) | linux `syscall.tbl`, `linux/audit.h` | in integration-4 | [ucpu](ucpu.md), [edge-boot](edge-boot.md); **pending owner ruling** (§6) |

The candle-core patch (what changed, from GALION-PATCH.md): `zip` requirement 7.2.0 -> 9.0.5
(fixes RUSTSEC-2026-0367/0368/0369); `src/npy.rs` (2 places) and `src/pickle.rs` (1 place) add `?`
for the zip 9 `ZipResult<Cow<str>>` API; upstream Python test generators `tests/npy.py`, `tests/pth.py`
removed (no-Python rule); `tests/test.pt`, `tests/test_with_key.pt` re-added byte-identical. Every source
edit carries a `galion candle-zip-fix` comment. Remove the folder and the `[patch.crates-io]` line once a
candle release needs zip >= 9 (none did on 2026-10-21). Known side effect: `repo-check` reports 10
`portable` errors (upstream `core::arch` SIMD paths) inside `vendor/candle-core` on integration-5
(slot 177), because `vendor/` is not on repo-check's skip list the way `contrib/` is. Owner item O1.

## 5. How the numbers were taken (re-run these)

```sh
cd <worktree at integration-4 42de672>
cargo metadata --format-version 1 --offline --all-features > meta.json
jq -r '.packages[] | select(.source!=null) | [.name,.version,.license] | @tsv' meta.json   # 171 rows
cargo deny --offline --all-features check licenses bans sources   # cargo-deny 0.20.2, deny.toml
cargo deny --offline --all-features check advisories              # local RustSec DB copy
```

Output on 2026-10-21 10:50 (+02:00): `bans ok, licenses ok, sources ok` (exit 0) with
`warning[wildcard]` x45 (intra-workspace path deps without `publish = false`, see deny.toml) and
`warning[duplicate]` x2 (`syn` 2 versions, `hashbrown` 3 versions). Advisories exit 1 with
RUSTSEC-2026-0367, RUSTSEC-2026-0368, RUSTSEC-2026-0369 (`zip` 7.4.0 via candle-core) and RUSTSEC-2024-0436
(`paste` unmaintained, via gemm-common). Both are cleared on swarm/integration-5 0d68f46 by the two vendored
patches in §4 (slot 177: advisories/bans/licenses/sources ok).

### 5.1 Live cross-check on the VPS (read-only, 2026-10-21 10:53 (+02:00))

`ldd` and a byte search on the deployed Rust binaries (no service touched): `hermes-gateway-aa77864`,
`openmind-memory-aa77864`, `openmind-nerve-aa77864` and `galion-one` are all **statically linked**, so no
shared third-party library is loaded at run time. The bundled SQLite (`SQLite format 3` header string) is in
hermes-gateway, openmind-memory and galion-one and absent from openmind-nerve, matching §3 (nerve has no
`rusqlite`). Nerve `GET /state` 200, cycle 140, 11 healthy / 1 degrading / 1 failing-soon / 2 absent, 61/D;
cortex is not deployed yet, so the nerve was the observer. Details: `docs/real-tests/docs-thirdparty-register.md`.

## 6. Tools used, never linked or shipped

These run as separate programs on the box, in CI or on the VPS. They are not in `Cargo.lock` and no code
from them is in our binaries. RULES.md bans GPL/LGPL for **borrowed code**; whether it also bans using such
programs as tools is not yet ruled, so every copyleft row is **pending owner ruling** (gap D15).

| tool | version (where recorded) | licence | used for | record | status |
|---|---|---|---|---|---|
| strace | 6.13+ds-1 (Debian) | LGPL-2.1-or-later | `crates/room/tests/no_bind.rs` traces network syscalls | [room](room.md) | **pending owner ruling** |
| util-linux `unshare`, `nsenter` | box default | GPL-2.0-or-later | network/user namespaces in tests and real scripts | [room](room.md), [deploy-sim](deploy-sim.md) | **pending owner ruling** |
| qemu-user-static (`qemu-aarch64-static` etc.) | Debian 1:10.0.13+ds-0+deb13u1 | GPL-2.0-only | runs aarch64/armv7 test binaries on x86_64 | [ucpu](ucpu.md), [tuner-cutover](tuner-cutover.md) | **pending owner ruling** |
| nftables `nft` (+ libnftables1, libnftnl11, libjansson4 for netns tests) | Debian 1.1.3 | GPL-2.0 | galion-guard firewall backend, run as a process (`nft -f -`) | [guard-rust](guard-rust.md), [rust-ports](rust-ports.md) | **pending owner ruling** (runtime, not only test) |
| journalctl (systemd) | box/VPS default | LGPL-2.1-or-later | journald source for galion-guard and cortex, run as a process | [guard-rust](guard-rust.md) | **pending owner ruling** (runtime) |
| Linux UAPI syscall/audit constants | kernel headers | GPL-2.0 WITH Linux-syscall-note | numbers only, in seccomp tables | [ucpu](ucpu.md), [edge-boot](edge-boot.md) | **pending owner ruling** (the syscall-note exists to allow this) |
| curl | box 8.14.1 | curl (MIT-style) | HTTPS for hermes-rs CurlProvider and link-watch; probes in real tests | [desk-linkwatch](desk-linkwatch.md), [bench-hermes](bench-hermes.md), [room](room.md) | allowed |
| bash, coreutils (`sha256sum`, `timeout`, `od`, `install`), `ss`, `pgrep`, `jq` | box default | GPL-3.0-or-later / GPL-2.0 (bash, coreutils, iproute2, procps); MIT (jq) | legacy scripts/real/*.sh and voice fetch script (being ported to Rust) | [deploy-sim](deploy-sim.md), [lan-routing](lan-routing.md), [voice-mouth](voice-mouth.md) | shell scripts are being retired (Rust-only rule 20:41); listed for completeness |
| zig (`zig cc`, `zig ar`) | 0.15.2 (sha256 in ucpu.md); 0.13 in rust-ports | MIT | C cross compiler for bundled SQLite on musl/aarch64 | [ucpu](ucpu.md), [rust-ports](rust-ports.md), [desk-linkwatch](desk-linkwatch.md) | allowed |
| rust-lld | Rust toolchain | Apache-2.0 WITH LLVM-exception | musl linker | [tuner-cutover](tuner-cutover.md) | allowed |
| gitleaks | 8.30.1 | MIT | secret scan of export tree and history | [launch-gates](launch-gates.md) | allowed |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | licence/ban/source/advisory policy | [launch-gates](launch-gates.md) | allowed |
| cargo-cyclonedx | 0.5.9 | Apache-2.0 | CycloneDX SBOM | [integration-1](integration-1.md) | allowed |
| comply | 0.1.1 | MIT OR Apache-2.0 | REUSE 3.3 lint in Rust (the FSFE `reuse` tool is Python) | [launch-gates](launch-gates.md) | allowed |
| GitHub Actions (checkout, upload-artifact, codeql-action, rust-toolchain, cosign-installer, scorecard-action, sbom-action) | pinned in workflows | MIT / Apache-2.0 | CI | [launch-gates](launch-gates.md) | allowed; CodeQL licence ruling still open (security plan) |

Model weights and data files (Whisper-tiny, KittenTTS nano, rejected Piper/Kokoro options) are listed with
their licences in [voice-mouth](voice-mouth.md) §3 "Model weights"; none is in git.

## 7. Open items for the owner (Slavik)

* **O1 vendor/ in repo-check.** When candle-zip-rebase / integration-5 is merged, `repo-check` fails with 10
  `portable` errors inside `vendor/candle-core` (upstream SIMD code, not ours). Options: (a) skip `vendor/`
  in repo-check like `contrib/` (recommended: vendored code keeps upstream form, and GALION-PATCH.md records
  every edit), (b) add `repo-check: allow` markers to the vendored lines (edits upstream code further),
  (c) keep the failure as a blocker. Main-merge blocker 0 in integration-5.md.
* **O2 GPL/LGPL tools (D15).** Rule whether the six "pending owner ruling" rows in §6 are allowed as tools
  that are never linked or shipped. Recommendation: allow as tools (they run as separate processes, which is
  outside the licence's linking terms); nft and journalctl are also runtime dependencies on the VPS host.
* **O3 Notices at publication.** BSD-3-Clause (4 crates), Unicode-3.0 (4), Zlib (foldhash) and the CMU and
  kitten-core notices must ship with any binary release. Recommendation: generate a NOTICE file from
  `cargo metadata` in the release job before the first public binary.
* **O4 Upstream ask (C2 from slot 109).** Asking candle upstream for zip 9 is a public post; it needs a yes/no.

## 8. Index of the 40 per-branch files

"crates named" = how many of the 171 crates.io packages the file names (0 is normal for std-only branches).

| file | title | crates named |
|---|---|---|
| [bench-banking.md](bench-banking.md) | Third-party code: bench-banking (swarm slot 51) | 5 |
| [bench-harness.md](bench-harness.md) | Third-party: bench-harness (`crates/galion-bench`) | 19 |
| [bench-hermes.md](bench-hermes.md) | Third-party: bench-hermes (crate `hermes-bench`) | 2 |
| [bench-scoreboard.md](bench-scoreboard.md) | Third-party: bench-scoreboard | 2 |
| [bench-vending.md](bench-vending.md) | Third-party code: bench-vending | 12 |
| [bench-wallstreet.md](bench-wallstreet.md) | Third-party: bench-wallstreet (`crates/galion-wallstreet`) | 4 |
| [common-migrate-2.md](common-migrate-2.md) | common-migrate-2: third-party review (wave 2 slot 28, branch `swarm/common-migrate-2`) | 0 |
| [common.md](common.md) | galion-common: third-party review (slot 16, branch `swarm/common`) | 21 |
| [cortex-sensor.md](cortex-sensor.md) | Third-party: swarm/cortex-sensor | 3 |
| [cutover-prep.md](cutover-prep.md) | Third-party code: swarm/cutover-prep (slot 38) | 3 |
| [deploy-rust.md](deploy-rust.md) | Third-party code: swarm/deploy-rust (slot 39) | 8 |
| [deploy-sim.md](deploy-sim.md) | Third-party code and dependencies: swarm/deploy-sim (slot 33) | 2 |
| [desk-linkwatch.md](desk-linkwatch.md) | Third-party code and dependencies: desk-linkwatch (slot 18) | 8 |
| [docs-sync.md](docs-sync.md) | Third-party code: swarm/docs-sync | 1 |
| [edge-boot.md](edge-boot.md) | Third-party code: swarm/edge-boot | 10 |
| [gaia-hades-tokens.md](gaia-hades-tokens.md) | Third-party code: gaia-hades-tokens | 9 |
| [gaia-land.md](gaia-land.md) | Third-party code: swarm/gaia-land | 9 |
| [gaia-live.md](gaia-live.md) | Third-party code and dependencies: swarm/gaia-live | 8 |
| [gaia-poseidon.md](gaia-poseidon.md) | Third-party code: `swarm/gaia-poseidon` | 19 |
| [guard-rust.md](guard-rust.md) | Third-party code: galion-guard (swarm/guard-rust) | 10 |
| [hades-approval.md](hades-approval.md) | Third-party code: hades-approval (crates/hades-gate) | 36 |
| [hades-redteam.md](hades-redteam.md) | Third-party code: hades-redteam (S3.1) | 3 |
| [health-land.md](health-land.md) | Third-party: swarm/health-land | 7 |
| [hermes-tools.md](hermes-tools.md) | Third-party code: hermes-tools (slot 17) | 2 |
| [integration-1.md](integration-1.md) | Third-party: swarm/integration-1 | 1 |
| [lan-discovery.md](lan-discovery.md) | Third-party code: lan-discovery (swarm slot 11) | 8 |
| [lan-routing.md](lan-routing.md) | Third-party code: lan-routing (swarm slot 31) | 3 |
| [launch-gates.md](launch-gates.md) | Third-party material: swarm/launch-gates | 1 |
| [memory-real.md](memory-real.md) | Third-party code: swarm/memory-real | 4 |
| [nerve.md](nerve.md) | Third-party code: openmind-nerve, nerve-client (swarm/nerve) | 19 |
| [one-probe.md](one-probe.md) | Third-party code: swarm/one-probe | 0 |
| [poseidon.md](poseidon.md) | Third-party code: POSEIDON (`crates/openmind-queue`, branch `swarm/poseidon`) | 20 |
| [real-harness.md](real-harness.md) | Third-party code: galion-realtest (swarm/real-harness) | 2 |
| [room.md](room.md) | Third-party code: room (branch swarm/room) | 1 |
| [rust-ports.md](rust-ports.md) | Third-party: swarm/rust-ports (slot 47) | 8 |
| [sandbox.md](sandbox.md) | Third-party code and dependencies — `sandbox` slot (swarm/sandbox) | 3 |
| [survival-heal.md](survival-heal.md) | Third-party code and dependencies: survival-heal | 2 |
| [tuner-cutover.md](tuner-cutover.md) | Third-party code and dependencies: tuner-cutover | 0 |
| [ucpu.md](ucpu.md) | Third-party record: swarm/ucpu (UCPU-1) | 3 |
| [voice-mouth.md](voice-mouth.md) | Third-party code, crates and model weights: `swarm/voice-mouth` | 81 |

## 9. Keeping this register current

A branch that adds a crate, vendors code or starts using a new tool adds a row to its own
`docs/third-party/<slug>.md` (RULES.md "Open-source borrowing") **and** to §3, §4 or §6 here, then
re-runs §5. Re-check: every crate in `[dependencies]` of a workspace crate must appear in §3 with a
"documented in" file.
