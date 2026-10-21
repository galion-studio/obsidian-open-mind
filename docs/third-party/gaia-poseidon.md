<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: `swarm/gaia-poseidon`

Written 2026-10-20T20:30:00+02:00 (Europe/Zagreb).

**No new third-party crates were added, and no code was copied.** The only
`Cargo.lock` change is two new dependency names on the in-workspace
`gaia-poseidon` entry (`openmind-queue`, `serde_json`); no new `[[package]]`.

| Name | Version | Licence (SPDX) | Source | Used for | Why |
| --- | --- | --- | --- | --- | --- |
| `openmind-queue` | 0.1.0 (workspace) | MIT | `crates/openmind-queue` (swarm/poseidon 7d0c478) | its std-only HTTP client (`http::request`) and money-word list | In-workspace; it is the service being adapted, so reusing its client avoids a second HTTP implementation |
| `serde_json` | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parsing the queue's JSON answers, building job bodies | Already a workspace dependency and already used by `gaia` and `openmind-queue` |

Transitive crates pulled in via `openmind-queue` (`openmind-provider`,
`zagreb-time`, `serde`, `chrono`, `chrono-tz`, `iana-time-zone`, ...) were
all already in `Cargo.lock` and recorded in `docs/third-party/poseidon.md`
(all MIT or MIT OR Apache-2.0).

`cargo tree -p gaia-poseidon -e normal` (2026-10-20 20:24 +02:00), every
licence on the allow-list:

| Crate | Version | Licence |
| --- | --- | --- |
| chrono | 0.4.45 | MIT OR Apache-2.0 |
| chrono-tz | 0.10.4 | MIT OR Apache-2.0 |
| iana-time-zone | 0.1.65 | MIT OR Apache-2.0 |
| itoa | 1.0.18 | MIT OR Apache-2.0 |
| memchr | 2.8.3 | Unlicense OR MIT (used under MIT) |
| num-traits | 0.2.19 | MIT OR Apache-2.0 |
| phf, phf_shared | 0.12.1 | MIT |
| proc-macro2 | 1.0.107 | MIT OR Apache-2.0 |
| quote | 1.0.47 | MIT OR Apache-2.0 |
| serde, serde_core, serde_derive | 1.0.229 | MIT OR Apache-2.0 |
| serde_json | 1.0.154 | MIT OR Apache-2.0 |
| siphasher | 1.0.4 | MIT OR Apache-2.0 |
| syn | 3.0.9 | MIT OR Apache-2.0 |
| unicode-ident | 1.0.27 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| zmij | 1.0.23 | MIT |
| gaia-core, openmind-queue, openmind-provider, zagreb-time | 0.1.0 (workspace) | MIT |
