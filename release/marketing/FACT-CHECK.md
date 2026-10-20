# FACT-CHECK — marketing drafts (R4)

Owner: subagent B "marketing"; corrections applied by Hermes (lead) after teacher review #1.
Drafted 2026-10-18T19:05:52+02:00; corrected 2026-10-18T19:20+02:00 (Europe/Zagreb).
Scope: every factual claim in `marketing/announcement.md` and `marketing/posts.md`.
Rule: a claim with no `file:line` source is removed or marked `TODO(verify)`.
Financial-gain claims are out of scope and were never written; real-money features are not
covered here. Repository paths are under
`/opt/galion/openmind` (`ROOT`); `M` = `contrib/grokbot/docs/main.md`;
`A` = `contrib/grokbot/docs/study/A-foundation.md`. Schema line numbers below are the
re-read of 2026-10-18T19:17:58+02:00; older traces to `:82-84`, `:50-58` and `:55-59` were
stale and are corrected.

| claim | where used (file) | source | status |
|---|---|---|---|
| "Obsidian Open Mind is the stance and the code of the Obsidian stack by Galion Studio" | announcement.md [^1]; posts.md P1; thread 1/5 | `ROOT/crates/openmind-schema/src/lib.rs:89-91` (stack `Obsidian`, stance `Open Mind`, studio `Galion Studio`) | verified |
| "The CPU box owns the loop, the memory and the keys" | announcement.md [^2]; posts.md P1; thread 1/5 | `ROOT/crates/openmind-schema/src/lib.rs:3` | verified |
| "The processor runs this; the card only runs the model" | announcement.md [^3]; posts.md P1; thread 2/5 | `ROOT/crates/openmind-schema/src/lib.rs:4` | verified |
| "A GPU is not a special case; it is one more `Provider` row" | announcement.md [^4]; posts.md P2; thread 2/5 | `ROOT/crates/openmind-schema/src/lib.rs:42`; `:51-58` | verified |
| "Adding a provider is a new row, not a new program" | announcement.md [^5]; posts.md P2; thread 3/5 | `ROOT/crates/openmind-schema/src/lib.rs:49` | verified |
| "A provider is an adapter, not a secret" | announcement.md [^6] | `ROOT/crates/openmind-schema/src/lib.rs:4` | verified |
| "Providers are either local or remote" (`ProviderKind`) | announcement.md [^7] | `ROOT/crates/openmind-schema/src/lib.rs:42-47` | verified |
| "Secrets never live in the contract; a KeySlot records name, present, hint; no key material" | announcement.md [^8]; posts.md P3; thread 4/5 | `ROOT/crates/openmind-schema/src/lib.rs:62-66`; test at `:151` asserts no `sk-` in the serialised stack | verified |
| "Rust foundation" (replaces the earlier "Rust-only foundation") | announcement.md [^2]; posts.md P1, thread 1/5 | `M:9` uses the phrase "Rust-only" as the plan's wording, but the code does not: `rg -n -i 'python' ROOT/crates/` hits `crates/galion-one/src/main.rs:1136,1238,1830` and `crates/galion-desk/src/main.rs:236`; `find ROOT/crates -name '*.py'` is empty; `A:47,51,64`; fix planned at `A:91` (read 2026-10-18T19:14:44+02:00) | verified as "Rust foundation"; the Python-free version is **TODO(verify)** |
| "The Hermes agent (upstream, Python) and the Node services are not part of this foundation" | announcement.md [^18] | `A` section 1 | verified |
| "small on purpose; release profile tuned for footprint, not throughput" | announcement.md [^9]; posts.md P4 | `ROOT/Cargo.toml:19-28` | verified |
| "Release binaries measured 2026-10-18T19:16:54+02:00: galion-one 615624 B, galion-desk 402720 B, room 344304 B, desk-hermes 1427472 B" | announcement.md [^16] | `ls -l ROOT/target/release/` at that timestamp | verified (dated snapshot) |
| "Open Mind is designed so that accelerators from NVIDIA, AMD or Apple can be added as provider rows over an OpenAI-compatible HTTP endpoint" | announcement.md [^10]; posts.md P5 | `A:76` — a design rule, not code | verified as **design**; no such row in the workspace — **TODO(verify)** |
| "No accelerator provider row exists in the workspace yet" | announcement.md [^17] | `ROOT/crates/openmind-schema/src/lib.rs:42-47` (`Remote`/`Local` only); `:86-132`; the only `nvidia|cuda|rocm|metal` hits are hardware probing (`galion-one/src/main.rs:3219`) and a mock fixture (`:1447`) | verified |
| "No accelerator is required to run the foundation" | posts.md P5 | `A:76`; `ROOT/crates/openmind-schema/src/lib.rs:86-132` (`Stack::cpu_desk()` runs with no card) | verified |
| "MIT license" | announcement.md [^11]; posts.md P1-P5; thread 3/5,4/5 | `ROOT/Cargo.toml:10`; `M:26`; `ROOT/LICENSE:1` | verified |
| "Every timestamp is Europe/Zagreb time with an explicit offset" | announcement.md [^12] | `M:52-55`; `ROOT/crates/zagreb-time/src/lib.rs:29-37` | verified |
| "Milestone 2026-10-31 — published and fact-checked" | announcement.md [^13]; posts.md P1-P5; thread 5/5 | `M:48` | verified |
| "The repository is not published yet; there is no remote and nothing has been pushed" | announcement.md [^14]; thread 5/5 | `git remote -v` in `ROOT` empty (read 2026-10-18T19:14 CEST); `release/repo/README.md:73-74` | verified |
| "This announcement / thread is a draft and has not been posted" | announcement.md [^15]; thread 5/5; posts.md header+footer | self-referential status, not a project claim | verified |

Post character counts are measured, not estimated: `Post 1` 261, `Post 2` 166, `Post 3` 163,
`Post 4` 144, `Post 5` 209, thread parts 158 / 162 / 139 / 157 / 184. All are under 280.
Counted 2026-10-18T19:18:14+02:00 (`wc -m` per line). Post 1, Post 5 and thread part 1 were
re-counted after the review #1 rewording.

## Removed or not used (reason)

| claim (not written anywhere in marketing/) | reason | disposition |
|---|---|---|
| "the binaries stay small on ordinary CPUs" with no measurement | nothing had been measured | removed; replaced by the measured `target/release` sizes above |
| "the Rust parts improve RAM/CPU" as a current claim | `A:26` is a snapshot of the pre-workspace binaries | not asserted; the site states it with that qualification |
| "The workspace contains exactly one crate, `openmind-schema`" | the workspace grew during the session (other worker): `crates/` held seven entries at 2026-10-18T19:14:44+02:00 | removed — no crate-count claim made |
| "`openmind-http` / `openmind-state` crates" | `A:90` is a recommendation; the crates do not exist | removed |
| "minimum supported Rust version" | not pinned in `ROOT/Cargo.toml`; no `rust-toolchain.toml` | removed |
| "public repository URL / clone URL" | no remote exists | removed (README uses `<TODO-repo-url>`) |
| Any star count, user or adoption number | no primary source in `ROOT` or the contrib docs | removed — not used |
| Anything calling the project released, published, downloadable or on GitHub | not true before 2026-10-31 | removed |
| Real-money / financial-return content | not approved and not present | removed — never written |

Unverified (`TODO(verify)`) items left in marketing text: **three**.
1. Python-free Rust binaries — the announcement and Post 1 say the helpers are being removed.
2. An accelerator provider row — the announcement says none exists yet.
3. The milestone itself (2026-10-31) is a plan, not a result.
Every other factual sentence in `announcement.md` and `posts.md` is traced above.
