# FACTS — claim to source trace

Owner: Hermes (lead). Appended as tickets land. Every claim in `repo/`, `site/`, `brand/`
and `marketing/` must appear here (or in `marketing/FACT-CHECK.md`) with a `file:line`
source, or be marked `TODO(verify)`.

Path roots (read-only outside `release/`):

- `ROOT` = `/opt/galion/openmind`
- `M` = `ROOT/contrib/grokbot/docs/main.md` (master plan)
- `A` = `ROOT/contrib/grokbot/docs/study/A-foundation.md` (foundation study)

Snapshot note: the workspace is being built by another worker during this session. Every
claim below was re-read at the timestamp shown in the row group. Line numbers refer to the
files as of that read. Row groups whose read is older than the row group above them are
labelled with their own timestamp; treat them as older facts.

## Workspace state — re-read 2026-10-18T19:14:44+02:00 (listing) / 19:16:54+02:00 (build)

| Claim | Source |
|---|---|
| `crates/` contains seven entries: `desk-hermes`, `galion-desk`, `galion-one`, `link-watch`, `openmind-schema`, `room`, `zagreb-time` | `ls -1 ROOT/crates/`, read 2026-10-18T19:14:44+02:00 |
| Built artifacts present: `galion-one` 615624 B, `galion-desk` 402720 B, `room` 344304 B, `desk-hermes` 1427472 B, `libopenmind_schema.rlib` 240750 B, `libzagreb_time.rlib` 93654 B | `ls -l ROOT/target/release/`, read 2026-10-18T19:16:54+02:00 |
| `link-watch` has a `Cargo.toml` but an empty `src/` and no binary | `ls -l ROOT/crates/link-watch/`, read 2026-10-18T19:16:54+02:00 |
| The workspace is a Cargo workspace, `resolver = "2"`, members `crates/*`, `contrib` excluded | `ROOT/Cargo.toml:2-3,6` |
| Workspace package: edition 2021, license MIT, version 0.1.0 | `ROOT/Cargo.toml:9-11` |
| Workspace dependencies are serde, serde_json, chrono, chrono-tz | `ROOT/Cargo.toml:14-17` |
| Release profile: `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true` | `ROOT/Cargo.toml:23-28` |
| Release profile is tuned for footprint, not throughput | `ROOT/Cargo.toml:19-22` |
| The workspace root `LICENSE` is the MIT text, holder `Copyright (c) 2026 Slavik / Galion Studio` | `ROOT/LICENSE:1,3` (read 2026-10-18T19:03 CEST) |
| The reference toolchain used in development is rustc 1.99.0 | `A:30`; toolchain probe `~/.cargo/bin/rustc 1.99.0 (b940084d7 2026-09-28)` |
| No Rust toolchain version is pinned in the repo (`rust-version` absent) | `ROOT/Cargo.toml` (no `rust-version` key); no `rust-toolchain.toml` in `ROOT` |
| There is no git remote and nothing has been pushed | `git remote -v` in `ROOT` empty (2026-10-18T19:14 CEST) |
| The repo has one commit, "T1: Cargo workspace + MIT LICENSE + openmind-schema moved in" | `git log --oneline` in `ROOT`: `9ddf211` (read 2026-10-18T19:03 CEST) |

## Python evidence — read 2026-10-18T19:14:44+02:00

This is the evidence that "Rust-only" is not currently true, and it is the source for the
qualified wording used in `repo/README.md`, `site/index.html`, `brand/BRAND.md`,
`marketing/announcement.md` and `marketing/posts.md`.

| Claim | Source |
|---|---|
| Rust code spawns Python at three sites in `galion-one` and one in `galion-desk` | `rg -n -i 'python' ROOT/crates/` -> `crates/galion-one/src/main.rs:1136,1238,1830`; `crates/galion-desk/src/main.rs:236` |
| No `.py` file lives under `crates/` | `find ROOT/crates -name '*.py'` -> nothing (read 2026-10-18T19:14:44+02:00) |
| The Python dependency is a known defect, not a design choice: `one` runs `python3 -c` to edit Hermes `state.db`, calls `models.py` and the TTS venv, and the desk calls `desk-hermes.py` | `A:47,51,64` |
| The fix is planned: port `desk-hermes.py` and link-watch to Rust and replace the `state.db` DELETE | `A:91` (item 2 of the study's gap list) |
| "Rust-only" is the *dictated plan's* phrase, not a verified property of the code | `M:9` ("the lightweight, Rust-only foundation of the Obsidian Open Mind stack") |
| The Hermes agent (upstream, Python) and the Node services are not part of the foundation | `A` section 1 |

## Crate descriptions (source of the README and site tables)

| Claim | Source |
|---|---|
| `openmind-schema` — contract types; re-exports the shared time helper | `ROOT/crates/openmind-schema/Cargo.toml:6`; `ROOT/crates/openmind-schema/src/lib.rs:13` |
| `zagreb-time` — Europe/Zagreb time, explicit offset, tz database compiled in | `ROOT/crates/zagreb-time/Cargo.toml:6` |
| `galion-one` — "the operator's own ear, mind and mouth on one box" | `ROOT/crates/galion-one/Cargo.toml:6` |
| `galion-desk` — "Reads the pod, never writes it" | `ROOT/crates/galion-desk/Cargo.toml:6` |
| `room` — "A read-only view of the operator's files as one page" | `ROOT/crates/room/Cargo.toml:6` |
| `desk-hermes` — one compact JSON object, same keys in the same order, Hermes state DB opened read-only | `ROOT/crates/desk-hermes/Cargo.toml:6`; `ROOT/crates/desk-hermes/src/main.rs:3-8` |
| `link-watch` — link watcher port | `ROOT/crates/link-watch/Cargo.toml:6` |

All seven description reads above were taken at 2026-10-18T19:16:54+02:00.

## The Open Mind stance — re-read 2026-10-18T19:17:58+02:00

The schema file grew during the session; these are the current line numbers. Older traces to
`:82-84`, `:50-58`, `:55-59`, `:33-40`, `:44-51` and `:133-145` are stale and were corrected
in `site/index.html`, `brand/BRAND.md` and `marketing/announcement.md`.

| Claim | Source |
|---|---|
| "The processor runs this. The card only runs the model." | `ROOT/crates/openmind-schema/src/lib.rs:4` |
| The stack stance string is "Open Mind", on stack "Obsidian", studio "Galion Studio" | `ROOT/crates/openmind-schema/src/lib.rs:89-91` |
| The processor owns the loop, the skills, the memory and the key box | `ROOT/crates/openmind-schema/src/lib.rs:3` |
| A provider is an adapter, not a secret | `ROOT/crates/openmind-schema/src/lib.rs:4` |
| `Machine` is declared at `:17` and holds studio, stack, stance, ram_mib, gpu, vram_mib, public_ip | `ROOT/crates/openmind-schema/src/lib.rs:17-25` |
| `Machine::weights()` needs a card with at least 8 GB free and 8 GB of memory | `ROOT/crates/openmind-schema/src/lib.rs:28` |
| `Machine::mouth()` covers the no-card case | `ROOT/crates/openmind-schema/src/lib.rs:34-36` |
| `ProviderKind` is `Remote` or `Local` | `ROOT/crates/openmind-schema/src/lib.rs:42-47` |
| "Adding a site is a new row, not a new program." | `ROOT/crates/openmind-schema/src/lib.rs:49` |
| `Provider` holds id, kind, base_url, key_slot, default_model | `ROOT/crates/openmind-schema/src/lib.rs:51-58` |
| `KeySlot` holds only name, present, hint; the schema never holds the secret | `ROOT/crates/openmind-schema/src/lib.rs:62-66` |
| The four surfaces are console, desk, api, hermes | `ROOT/crates/openmind-schema/src/lib.rs:70-75` |
| The stack is defined as a whole contract: machine, providers, keys, surface | `ROOT/crates/openmind-schema/src/lib.rs:78-83` |
| `Stack::cpu_desk()` declares a CPU box (no card, vram 0, ram 16384 MiB) with a local provider | `ROOT/crates/openmind-schema/src/lib.rs:86-132` |
| A test asserts a CPU box does not turn weights on, has a mouth, and that the JSON contains no `sk-` | `ROOT/crates/openmind-schema/src/lib.rs:139-152` (asserts at `:142`, `:143`, `:151`) |
| `openmind-schema` is version 0.1.0, license MIT (inherited from the workspace) | `ROOT/crates/openmind-schema/Cargo.toml:1-6` |

## Plan facts (`M`) — read 2026-10-18T19:03 CEST

| Claim | Source |
|---|---|
| The stack is released under the MIT license | `M:26` |
| The foundation is lightweight and efficient on CPU, with the option to add unlimited compute instances from NVIDIA, AMD or Apple | `M:30` |
| The Exo project is the named mechanism for distributed inference across heterogeneous devices | `M:30`; `A:80` |
| Milestone: by October 31, 2026 the stack must be published — everything live, fact-checked, timestamped | `M:48` |
| Timestamps use Europe/Zagreb with an explicit offset; never server (UTC) time | `M:52-55` |
| The VPS clock is UTC | `M:54`; `A:3` |
| Connecting Revolut and giving real-money access is NOT approved | `M:44` |

## Foundation study facts (`A`) — read 2026-10-18T19:03 CEST

| Claim | Source | Nature |
|---|---|---|
| The Rust parts of the stack use about 10 MB of RAM and almost no CPU | `A:26` | measurement of the **pre-workspace** binaries, 2026-10-18 snapshot; not a measurement of the crates in `crates/` |
| The non-Rust parts use about 1.5 GB of RAM; Hermes is the only piece with real CPU use | `A:26` | measurement, same snapshot |
| There is no shared Rust foundation yet; three single-file std-only programs exist, built with bare rustc | `A:30` | study of the pre-workspace state |
| The Rust binaries still depend on Python helpers | `A:64` | confirmed by the Python evidence above |
| Accelerators are added as `Provider` rows over an OpenAI-compatible HTTP endpoint | `A:76` | **design rule**, not code. Worded as "is designed so that ... can be added" everywhere in this kit |
| Recommended workspace members toward the milestone: `openmind-schema` (types + a time module), `openmind-http`, `openmind-state` | `A:90` | recommendation, not a directory listing |
| `galion-rs` is the heavy aspirational design and should not be the base | `A:54,73,94` | design judgement |
| Hardening item 2: remove Python from the Rust binaries | `A:91` | recommendation; the open **TODO(verify)** |

## Accelerator check — read 2026-10-18T19:18:14+02:00

| Claim | Source |
|---|---|
| No accelerator `Provider` row exists in the workspace | `ROOT/crates/openmind-schema/src/lib.rs:42-47` (only `Remote`/`Local`); `:86-132` (`cpu_desk()` builds one `Local` row) |
| Hardware probing exists and is not a provider | `ROOT/crates/galion-one/src/main.rs:3219` runs `nvidia-smi`; a mock fixture at `:1447` names `"device":"cuda"` |
| Search terms `nvidia|cuda|rocm|metal` over `crates/` return only those two hits | `rg -n -i 'nvidia|cuda|rocm|metal' ROOT/crates/`, read 2026-10-18T19:18:14+02:00 |

## Corrections applied after teacher review #1

| Correction | Where | Result |
|---|---|---|
| "Rust-only" not proven | `repo/README.md`, `site/index.html`, `marketing/announcement.md`, `marketing/posts.md` | replaced with "Rust foundation" plus the Python evidence and a `TODO(verify)`; `BRAND.md` never contained the phrase (checked) |
| Present-tense accelerator claim traced to a study paragraph | `repo/README.md`, `site/index.html`, `marketing/announcement.md`, `marketing/posts.md` | reworded to "is designed so that ... can be added as provider rows"; marked design, not code |
| "the binaries stay small" with nothing measured | `marketing/announcement.md` | replaced with the measured `target/release` sizes above |
| "The Rust parts ... about 10 MB of RAM" unqualified | `site/index.html`, `brand/BRAND.md` | qualified as a 2026-10-18 snapshot of the pre-workspace binaries, `A:26` |
| Site crate-table descriptions with no source | `site/index.html` | third column added, each row traced to `Cargo.toml:6` (or `desk-hermes/src/main.rs:3-8`); `link-watch` row added |
| README links written for a moved README | `repo/README.md` | `../crates/...` -> `crates/...` |
| README workspace list stale (one crate) | `repo/README.md` | re-listed seven entries from `ls -1 crates/` with the timestamp and the build state |
| Unverified company fact | `brand/BRAND.md` | "by Dynalist Inc." removed; the not-affiliated sentence kept |

## Planned, not present (explicit TODOs)

| Item | Source | Status |
|---|---|---|
| Python-free Rust binaries | `A:91`; Python evidence above | **TODO(verify)** |
| An accelerator provider row | `A:76` is a design rule only | **TODO(verify)** |
| Minimum supported Rust version | not pinned in `ROOT/Cargo.toml` | **TODO(verify)** |
| Public repository URL | no remote exists | **TODO(verify) / placeholder `<TODO-repo-url>`** |
| `link-watch` builds | `Cargo.toml` only, empty `src/` | **TODO(verify)** |
| `openmind-http` and `openmind-state` crates exist | `A:90` is a recommendation only | **TODO(verify)** |

## Corrections applied after teacher review #2 (tone and placement)

Rule now in force: **user-facing prose cites files, not line numbers.** `repo/README.md`,
`site/index.html`, `marketing/*.md` body text and `brand/BRAND.md` name files (no `:line`),
because line numbers go stale while the other worker edits `crates/`. Line numbers live here
and in `marketing/FACT-CHECK.md`; announcement footnotes may carry them. Re-read after the
change: 2026-10-18T19:25:00+02:00 (Europe/Zagreb).

| Change | Where | Result |
|---|---|---|
| Python caveat moved out of the intro/hero | `repo/README.md` (intro), `site/index.html` (hero lede and meta description) | intro/hero now factual only ("lightweight, CPU-light Rust foundation ... License: MIT.") |
| Known-gaps section added | `repo/README.md` "Status / known gaps"; `site/index.html` "Status" (+ nav link) | Python call sites, Hermes/Node not in the foundation, no accelerator row, `link-watch`/planned crates, not published |
| Status paragraph added, body de-numbered | `marketing/announcement.md` | one short "Status" paragraph before the milestone paragraph |
| Line numbers removed from tables and prose | `repo/README.md` crate table, `site/index.html` crate table and "why" list | third column and `A-foundation.md` citations now name files only |

Every "Status / known gaps" item is traced in this file: Python call sites to the Python
evidence group; Hermes and Node to `A` section 1; the missing accelerator row to the
accelerator-check group; `link-watch` to the workspace-state group; the unpublished
repository and the 2026-10-31 milestone to the plan group.

## Open question for the owner

The release `repo/LICENSE` uses the holder line
`Copyright (c) 2026 Slavik / Galion Studio and Obsidian Open Mind contributors`, while the
workspace root `ROOT/LICENSE:3` reads `Copyright (c) 2026 Slavik / Galion Studio`. These two
lines do not match. The release copy is written to the ticket wording; reconciling the
workspace copy is the other worker's file (`ROOT/LICENSE` is outside the write scope of this
release kit). Flagged, not resolved.
