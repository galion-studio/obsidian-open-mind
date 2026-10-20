# CLAIMS — every factual claim on the R2 site, with its source

Written 2026-10-18T19:09:10+02:00 (Europe/Zagreb, CEST). Owner: subagent A "site-brand".
Corrected by Hermes (lead) 2026-10-18T19:20+02:00 after teacher review #1.
Purpose: the trace table check K1 needs. `release/TEACHER/CHECKS.md` K1 asks that every
factual claim in `site/`, `marketing/` and `brand/` exist in `release/FACTS.md` with a
`file:line` source. This file lists each claim on `site/index.html` with the row it maps to
and the primary source. Rows marked **NEW ROW** are not in `FACTS.md` yet and must be added
by whoever owns `FACTS.md` (it is outside this agent's write scope).

Path roots: `ROOT` = `/opt/galion/openmind`. Line numbers read at
2026-10-18T19:20:00+02:00; re-read before reusing one, the workspace is being edited live.

## Claims on site/index.html

| # | Claim on the page | FACTS.md row | Primary source |
|---|---|---|---|
| 1 | "the stance and the code of the Obsidian stack by Galion Studio: a lightweight, CPU-light Rust foundation" (the phrase "Rust-only" was removed) | FACTS "Stance / studio / stack", FACTS "Python evidence" | `ROOT/crates/openmind-schema/src/lib.rs:89-91`; `release/repo/README.md:3-11`; Python: `crates/galion-one/src/main.rs:1136,1238,1830`, `crates/galion-desk/src/main.rs:236`; `M:9` is the plan's wording, not a code fact |
| 2 | "where the CPU box owns the loop, the memory and the keys" | FACTS "CPU owns loop" | `ROOT/crates/openmind-schema/src/lib.rs:3` |
| 3 | "A GPU is not a special case in this design; it is one more `Provider` row" | FACTS "GPU is a Provider row" | `ROOT/contrib/grokbot/docs/study/A-foundation.md:76`; `ROOT/contrib/grokbot/docs/main.md:30` |
| 4 | "License: MIT" / "The workspace is a plain Cargo workspace under the MIT license" | FACTS "License MIT" | `ROOT/contrib/grokbot/docs/main.md:26`; `ROOT/Cargo.toml:10`; `ROOT/README.md:3`; `ROOT/LICENSE:1` |
| 5 | "The processor owns the loop, the skills, the memory and the key box." | FACTS "CPU owns loop" | `ROOT/crates/openmind-schema/src/lib.rs:3` |
| 6 | "Video memory is only for a model. A provider is an adapter, not a secret." | FACTS "Card only runs model" | `ROOT/crates/openmind-schema/src/lib.rs:4` |
| 7 | Contract types `Machine`, `Provider`, `ProviderKind`, `KeySlot`, `Surface`, `Stack` exist | FACTS "Contract types" | `ROOT/crates/openmind-schema/src/lib.rs:17, 42, 51, 62, 70, 78` |
| 8 | `Stack::cpu_desk()` exists as the reference CPU-only shape | FACTS "cpu_desk" | `ROOT/crates/openmind-schema/src/lib.rs:86-97` |
| 9 | "The schema re-exports the shared time helper, so a consumer gets it without a second dependency line" | **NEW ROW** | `ROOT/crates/openmind-schema/src/lib.rs:13` (`pub use zagreb_time as time;`); `ROOT/crates/openmind-schema/Cargo.toml:11` |
| 10 | `Machine::weights()` is true only when the card has at least 8192 MiB free and the box has at least 8192 MiB of memory | FACTS "weights gate 8 GiB" | `ROOT/crates/openmind-schema/src/lib.rs:28-31` |
| 11 | `Machine::mouth()` needs ordinary memory only, never a card; a CPU-only box is first-class | FACTS "mouth memory only" | `ROOT/crates/openmind-schema/src/lib.rs:33-36` |
| 12 | `Provider { id, kind, base_url, key_slot, default_model }`; `ProviderKind` is only `Remote` or `Local`; no per-backend code path | FACTS "Provider shape" | `ROOT/crates/openmind-schema/src/lib.rs:42-58`; `release/repo/README.md:18-20` |
| 13 | `KeySlot` records only `name`, `present`, `hint`; no key material in the contract types | FACTS "KeySlot shape" | `ROOT/crates/openmind-schema/src/lib.rs:60-66` |
| 14 | "a test asserts the serialized stack never contains the string `sk-`" | FACTS "sk- test" (**NEW ROW** for the line) | `ROOT/crates/openmind-schema/src/lib.rs:139-151`, test `a_cpu_box_does_not_turn_weights_on`, assertion at :151 |
| 15 | Release profile: `opt-level = "z"`, `lto = true`, `codegen-units = 1`, `panic = "abort"`, `strip = true`, tuned for footprint | FACTS "release profile" | `ROOT/Cargo.toml:19-28` |
| 16 | "A 2026-10-18 snapshot of the pre-workspace binaries put the Rust parts at about 10 MB of RAM and almost no CPU." (qualified; it is not a measurement of the crates listed on the page) | FACTS "Rust parts 10 MB" | `ROOT/contrib/grokbot/docs/study/A-foundation.md:26` |
| 17 | "No accelerator is required to run the foundation; Open Mind is designed so that accelerators from NVIDIA, AMD or Apple can be added as provider rows exposing an OpenAI-compatible HTTP endpoint." (reworded from the present tense; design, not code) | FACTS "No accelerator required" | `ROOT/contrib/grokbot/docs/study/A-foundation.md:76`; `ROOT/contrib/grokbot/docs/main.md:30`; `release/repo/README.md` (release kit, design paragraph) |
| 18 | `crates/` held seven crates at the page's snapshot timestamp: `openmind-schema`, `zagreb-time`, `galion-one`, `galion-desk`, `room`, `desk-hermes`, `link-watch` (the last has an empty `src/` and no binary) | FACTS "Workspace state" | `ls -1 ROOT/crates/` at 2026-10-18T19:14:44+02:00; `ls -l ROOT/crates/link-watch/` at 19:16:54 |
| 19 | One-line role of each crate in that table | **NEW ROW** each | `ROOT/crates/<name>/Cargo.toml:6` (`description =`) |
| 20 | `desk-hermes` "prints one compact JSON object with the same keys in the same order, and opens the Hermes state database read-only" | **NEW ROW** | `ROOT/crates/desk-hermes/src/main.rs:1-9` |
| 21 | `zagreb-time`: "every timestamp is RFC 3339 with an explicit offset, never `Z`. The timezone database is compiled in, so it works offline." | **NEW ROW** (FACTS still calls the helper planned) | `ROOT/crates/zagreb-time/src/lib.rs:1-8`; `ROOT/crates/zagreb-time/Cargo.toml:6` |
| 22 | Workspace members are `crates/*`; `contrib/` is excluded because it is a vendored read-only material dump with its own package | FACTS "exclude contrib" | `ROOT/Cargo.toml:3-6` |
| 23 | Quick-start commands (`git clone <TODO-repo-url>`, `cargo build -j 2 --release --workspace`, `cargo test -j 2 --workspace`) and the sentence about `nice`/`-j 2` on a shared box | FACTS "quick start" | `release/repo/README.md:50-60` (identical block) |
| 24 | "`<TODO-repo-url>` is a placeholder: the repository does not exist yet and nothing has been pushed." | FACTS "no remote / not published" | `release/repo/README.md:59-60`; `FACTS.md` (git remote -v empty, 2026-10-18T19:03 CEST) |
| 25 | "The workspace does not pin a toolchain version: `rust-version` is not set and there is no `rust-toolchain.toml`." | FACTS "no toolchain pin" | re-verified 2026-10-18T19:09:10+02:00: `NO_RUST_TOOLCHAIN_FILE`; `NO_RUST_VERSION_KEY` |
| 26 | "The reference toolchain used to develop this workspace is rustc 1.99.0." | FACTS "reference toolchain" | `release/repo/README.md:67` |
| 27 | Workspace dependencies are Rust crates only: `serde`, `serde_json`, `chrono`, `chrono-tz` | FACTS "workspace deps" | `ROOT/Cargo.toml:13-17` |
| 28 | "The workspace package declares `license = "MIT"`" | FACTS "License MIT" | `ROOT/Cargo.toml:10` |
| 29 | Milestone 2026-10-31, "published, everything live, fact-checked and timestamped" | FACTS "milestone" | `ROOT/contrib/grokbot/docs/main.md:48` |
| 30 | "The repository is not published: there is no remote, no push and no site deployment. Those steps wait for the owner's approval." | FACTS "not published" | `release/repo/README.md:73-74`; `ROOT/contrib/grokbot/docs/main.md` (not-approved section) |
| 31 | "Obsidian Open Mind is not affiliated with the Obsidian note-taking application" | FACTS "not the Obsidian app" | `release/brand/BRAND.md` (this directory, section 1) plus `ROOT/crates/openmind-schema/src/lib.rs:89-91` for what "Obsidian" names here |
| 32 | "Timestamps in this project are Europe/Zagreb with an explicit UTC offset, never `Z`." | FACTS "timezone rule" | `ROOT/contrib/grokbot/docs/main.md:52-55` |
| 33 | Hero tagline "The CPU owns the loop and the keys." | **NEW ROW** (brand line) | `release/brand/BRAND.md` tagline 1; `ROOT/crates/openmind-schema/src/lib.rs:3` |
| 34 | The three timestamps on the page: snapshot/table `2026-10-18T19:14:44+02:00`, build state `19:16:54+02:00`, footer "last updated" `19:16:54+02:00` | timestamp rule, not a fact row | `TZ=Europe/Zagreb date -Iseconds` |

## Rows in FACTS.md that are now stale

These are not site problems; they are `FACTS.md` rows that lost their source while the
workspace was being edited. The site does not repeat them.

| FACTS.md says | Now actually | Source |
|---|---|---|
| `crates/` holds exactly one crate, `openmind-schema`; `zagreb-time` and the program crates are "planned" | seven entries present: `openmind-schema`, `zagreb-time`, `galion-one`, `galion-desk`, `room`, `desk-hermes`, `link-watch` | `ls -1 ROOT/crates/` 2026-10-18T19:14:44+02:00 |
| `zagreb-time` planned / TODO(verify) | present, with its own `Cargo.toml` and `src/lib.rs` | `ROOT/crates/zagreb-time/` |
| contract types at `lib.rs` lines 17-24, `Stack` at 82-84 | types now at 17, 42, 51, 62, 70, 78; `Stack::cpu_desk()` at 86-132; stance strings at 89-91 | re-read 2026-10-18T19:17:58+02:00 |
| site crate-table descriptions had no source of their own | every row now carries a "Source of the description" cell: each `crates/<crate>/Cargo.toml:6`, plus `crates/desk-hermes/src/main.rs:3-8` | read 2026-10-18T19:16:54+02:00 |
| "accelerators ... are added as provider rows" was present tense | reworded to "is designed so that ... can be added", marked design not code | `A:76`; `ROOT/crates/openmind-schema/src/lib.rs:42-47`, `:86-132` re-read 19:17:58 |

`release/repo/README.md` had the same staleness (its "What is in the workspace today" table
listed one crate and called `zagreb-time` planned); corrected 2026-10-18T19:20+02:00 to the
seven-entry listing with the build state per crate. The site's table is the dated snapshot;
where the two differ, the dated `ls` is the fact. The README also carried "Rust-only" in its
lede and `../crates/...` links written for a README sitting one level deeper; both corrected.
