<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# integration-3: integration-2 + eight wave-3 branches

Built 2026-10-21 01:31-02:05 CEST (+02:00) by wave-4 slot 110
(`swarm/integration-3`). A candidate for Slavik to review; agents do not
merge main. Order for main stays: integration-1, integration-2, then this.

Base: `swarm/integration-2` df42e0f. Eight normal `--no-ff` merges in the
order of the wave-4 task row, then three fixup commits. Every branch tip was
checked against the SHA in the task row before merging (all 9 matched).
`cortex-s2-a..d` are deliberately NOT included (checked: none of their tips
is an ancestor of the head).

| # | Merge commit | Branch (tip) | Result | Conflicts |
|---|---|---|---|---|
| 1 | a167ed6 | hades-redteam (e6ceca7) | clean | - |
| 2 | 7609944 | health-land (f4a66ea) | clean | - |
| 3 | fcd8efe | rust-ports (26a2a8e) | conflict | `Cargo.lock` only (L) |
| 4 | c3b596c | deploy-rust (787a54d) | conflict | `Cargo.lock` only (L) |
| 5 | f1ac67b | cutover-prep (a5ac49b) | clean | (contains deploy-rust 787a54d, already merged) |
| 6 | 9c80b8a | build-info (db9924d) | conflict | `main.rs` of desk-hermes, galion-desk, link-watch, room, tuner (B) |
| 7 | 483df29 | bench-hermes (457ef74) | conflict | `Cargo.lock` only (L) |
| 8 | 50cf3eb | cortex-sensor (e536446) | clean | - |
| F1 | 2430edc | gaia heal plan v2 | - | semantic conflict hades-redteam x gaia heal-tick (H) |
| F2 | 50d3665 | ownership rows | - | 10 unmapped crates (O); galion-health moved to guardian-watchdog in the docs commit, to match its unit in `UNITS` |
| F3 | 75daf4a | repo-check allow markers | - | galion-guard test fixture IPs (R) |

## Resolutions

**L `Cargo.lock`.** Took ours, let `cargo metadata --offline` add the other
side's packages, then checked that the set of (name, version) pairs is
exactly the union of both sides (no extra, no missing, no version drift):
`UNION-EXACT` for all three. New third-party packages vs integration-2
(6, all from rust-ports/guard-rust and already documented in
`docs/third-party/{rust-ports,guard-rust,cutover-prep}.md`): adler2 2.0.1,
aho-corasick 1.1.5, miniz_oxide 0.8.9, regex 1.13.1, regex-automata 0.4.18,
regex-syntax 0.8.11 (all MIT/Apache-2.0/Zlib/0BSD/Unlicense dual options).

**B build-info x the integration-1 rewrites.** build-info (db9924d) sits on
swarm/common 26d98b0 plus common-migrate-2 (cf40c1b, 123eab2), i.e. before
integration-1. Its change to these five files is one line,
`galion_common::build_info::exit_if_version("<bin>", env!("CARGO_PKG_VERSION"))`,
at the top of `main`. On integration-2 the five `main.rs` are the
integration-1 rewrites (desk-hermes, galion-desk, room into `lib.rs`;
link-watch `http.rs`/`urls.rs`; tuner `cli.rs`). Took ours and added that one
line to each `main`. The old `--version` paths of room (`Mode::Version`) and
tuner (`Action::Version`) are now shadowed by the stamped line, which keeps
their tests (`room 0.`, `tuner `) true.
*Not carried:* common-migrate-2's move of galion-desk, room and link-watch
onto the galion-common strict HTTP server/client (it edited the old
single-file `main.rs`, which integration-1 replaced). Those three still use
their own servers/clients from integration-1. hermes-rs, openmind-memory,
one-talk, openmind-survival and edge `util.rs` auto-merged and are on
galion-common. Consequence: `galion-common --test real_migrate` (GALION_REAL,
ignored) is expected to differ for desk/room/link-watch; not run here.
Follow-up: re-port the strict limits into `galion-desk/src/net.rs`,
`room/src/http.rs`, `link-watch/src/http.rs`.

**H heal plan v2 (F1).** After the merges, 6 of 10 `gaia --test heal_tick`
failed: hades-redteam's S3.1 allow-list (every statement starts with a
`PLAN_VERBS` verb; punctuation only `/ . - _ : , ( ) ;`) refuses gaia's v1
heal plan (lines started `gaia`, `service`, `probe`, `start`, `at`, and the
argv was JSON-quoted with `" [ ]`). Fixed on the gaia side, the allow-list is
unchanged (red-team stays 0 of 82): `PLAN_HEADER` is now
`record gaia heal-apply plan v2 (box test service)`, lines start with
`restart`/`verify`/`keep`/`record`, and argument boundaries stay signed as
`restart with argument count N, ...` plus one `verify argument i is <arg>`
line per argument (the existing test that `["--port","19231"]` and
`["--port 19231"]` hash differently still passes). An argument with a shell
metacharacter now makes the plan unapprovable (fail closed). Tokens signed
for v1 plans no longer match (box test tokens only; nothing live uses them).
`docs/real-tests/gaia-hades-tokens.md` keeps the v1 text as the historical
record.

**O ownership (F2).** `gaia-core --test workspace_ownership` failed listing
exactly the 10 new crates. One row each, all `pending Slavik`; see
`docs/gaia-ownership.md` "integration-3 rows". Counts 53/41 -> 63/51,
systems with a crate 28 -> 29 (`ownership_counts`, `real_cli`, system-crate
count and `pending_rows_are_flagged` updated).

**R repo-check (F3).** 6 `hosts` errors: public IPv4 literals in
`#[cfg(test)]` fixtures of galion-guard (`ipnet.rs` 118/123, `nft.rs`
385/390/391). Same `// repo-check: allow (test fixture address, never
dialled)` marker integration-1/2 used.

## Gates on the final head

| Gate | Command | Result |
|---|---|---|
| fmt | `cargo fmt --all --check` | ok |
| clippy | `cargo-slot clippy --workspace --all-targets --locked -j 2 -- -D warnings` | ok, 0 warnings |
| tests | `cargo-slot test --workspace --locked --no-fail-fast -j 2` | **1262 passed, 0 failed, 7 ignored (275 suites)**; before F1/F2: 1255 / 7 failed |
| ucpu | `UCPU_JOBS=2 scripts/ucpu-gate.sh all` (zig 0.15.2) | RESULT ok: grep ok; x86_64 + aarch64 musl static; size-budget ok (37 + 1 binaries); armv7 + aarch64-android `cargo check` ok |
| repo-check | `cargo run -p repo-check -- --copyright "3000 Galion / galion-studio"` | 1380 files, 0 errors, 47 warnings (`utc`) -> PASS (before F3: 6 errors) |
| no-python | `scripts/no-python.sh` | ok |
| real tests | see `docs/real-tests/integration-3.md` | 11 suites, all pass |

Not run here (verifier fan-out, slot 111 does it for integration-2): the
older `scripts/real/*.sh`, REUSE lint, `cargo deny`, `real_migrate`.

## Diffstat

- vs integration-2 df42e0f: 311 files changed, +52401 / -979 (before docs)
- vs main 13a296c: 782 files changed, +127754 / -4375

## Open items for Slavik

1. The 10 new `pending Slavik` ownership rows (plus the 5 from
   integration-2). Main question: do benchmarks get their own Gaia system?
2. Heal plan v2 wording (F1) is a security-relevant text change that HADES
   signs; it is the smallest change that fits the S3.1 allow-list. If Slavik
   picks HADES option B (typed signed plan steps, slot 124), this text
   format is replaced anyway.
3. galion-desk, room, link-watch are not on the galion-common strict HTTP
   limits yet (B above).
