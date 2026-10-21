<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real-test record: docs-thirdparty-register (slot 172, docs T2.4)

Branch `swarm/docs-thirdparty-register` on integration-4 42de672. Docs only; no build of shipped code.
All times Europe/Zagreb (+02:00).

| time | check | command | result |
|---|---|---|---|
| 10:50 | lockfile inventory | `cargo metadata --format-version 1 --offline --all-features` | 234 packages = 63 workspace + 171 crates.io; 17 crates.io packages are direct deps |
| 10:50 | licences/bans/sources | `cargo deny --offline --all-features check licenses bans sources` (0.20.2) | `bans ok, licenses ok, sources ok`, exit 0; 45 wildcard + 2 duplicate warnings |
| 10:50 | advisories | `cargo deny --offline --all-features check advisories` | exit 1: RUSTSEC-2026-0367/0368/0369 (zip), RUSTSEC-2024-0436 (paste) |
| 10:53 | same on swarm/integration-5 0d68f46 (read-only) | `cargo deny --offline --all-features check advisories licenses bans sources` | `advisories ok, bans ok, licenses ok, sources ok`, exit 0 (vendored candle-core + pastey shim) |
| 10:51 | cross-check | every direct crate vs whole-word mentions in docs/third-party/*.md | 17/17 direct crates documented; 127/171 crates.io packages named somewhere; 44 transitive named nowhere (now listed in README §3.1 with their parent chain from `cargo tree -i`) |
| 10:53 | link check | every `](x.md)` in README.md resolves in docs/third-party/ | 0 missing; 40/40 per-branch files indexed |
| 10:53 | repo-check (release build, this branch) | `repo-check` | 1383 files, 0 errors, 47 warnings -> PASS |
| 10:53 | repo-check on integration-5 (read-only) | `repo-check` | 1519 files, 10 errors -> FAIL, all `portable` in vendor/candle-core/src/{cpu,quantized}/{avx,neon,simd128}.rs (owner item O1) |
| 10:53 | no-python | `scripts/no-python.sh` | exit 0 |
| 10:53 | VPS read-only | `ldd` + `grep -c -a "SQLite format 3"` on /opt/galion/rust/bin/{hermes-gateway,openmind-memory,openmind-nerve}-aa77864 and galion-one | all 4 statically linked; SQLite present 1/1/0/1 (nerve has no rusqlite, as the register says) |
| 10:53 | VPS nerve | `curl http://127.0.0.1:8670/state` (GET) | 200, cycle 140, healthy 11 / degrading 1 / failing-soon 1 / absent 2, score 61/D; galion-nerve, openmind-memory, hermes-rs active. Cortex not deployed: the nerve was the observer |

Nothing was started on the box (no ports used from 20420-20429) and nothing on the VPS was changed.
