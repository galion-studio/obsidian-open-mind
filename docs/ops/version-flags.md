<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Non-binding `--version`, `--help`, `--check` on every deployable binary (ops S2.2)

Slot 194, branch `swarm/vps-version-flags`, base integration-4 `42de672`.

## Why

`galion-deploy` runs `<new-binary> --version` before it installs anything
(F1, bounded to 5 s). A binary that ignores its arguments starts serving at
that point: it tries to bind the live port, runs its startup checks, opens
its stores. The tuner already had the fix (tuner pattern, `tuner::cli`);
this branch gives the same contract to every deployable binary.

## The contract (`galion_common::flags`)

Call `galion_common::flags::early(name, version, usage, check)` first thing in
`main`, before config, files, sockets or threads.

| flag | when | output | exit |
|---|---|---|---|
| `--version`, `-V` | first argument | `galion_common::build_info::version_line`: `<name> 0.1.0 (commit <GALION_BUILD_COMMIT>, built <GALION_BUILD_TIME>)` | 0 |
| `--help`, `-h` | only argument | the binary's usage + the common-flags block | 0 |
| `--check` | only argument | settings resolved from env/config files, each named path checked (`ok`/`missing`), last line `<name> check ok` or `<name> check failed: <reason>` | 0 ok, 1 not ok |

`--help`/`--check` followed by more arguments fall through to the binary's
own parser, so no existing subcommand changes meaning. `--check` only reads:
no bind, connect, spawn, thread, DB open or write. Secrets (API keys) are
reported as `set` / `missing`, never printed.

`build_info` was already in the base (slot 48 `build-info` landed via
integration-4), so `swarm/build-info` did not need merging.

## Coverage (34 binaries)

| binary | before | `--check` reports |
|---|---|---|
| hermes-gateway | `--version` ran Config::load (could exit 2 on a bad config), then served | config summary (same as `--check-config`), home dir, state DB present/absent, API key set/missing (never printed), bind. **No store, MCP discovery, coding preflight or provider on any early flag** |
| galion-deploy | own `--version` (unstamped) | manager, version timeout, health tries, backup dir, data-backup script |
| galion-backup | `--version` -> usage error exit 1 | dest, keep, minfree |
| galion-health | `--version` **bound :8660 and served** | bind, every parsed check; malformed entries fail |
| guardian-tick | flag parse error exit 2 | GUARDIAN_PROBES file parsed, probe count |
| openmind-nerve | unstamped `--version` | bind, interval, data dir, dry-run, rollback/deploy bin |
| openmind-memory | stamped `--version`; `--help` exit 64 | bind, data dir, DB present, read-only, loopback, API key set/not set |
| galion-one | stamped `--version` | bind, mouth bind, keep-warm on/off |
| galion-desk, galion-origin, room, link-watch, desk-hermes | stamped `--version` | binds, roots / dirs / output dirs |
| galion-guard, openmind-queue | unstamped `--version` | guard config (file or built-in VPS defaults); queue client address |
| galion-voice-serve | `--version` **bound :8003 after loading models** | bind, models dir and model subdirs, features |
| tuner | already had the pattern | unchanged (`tuner::cli`) |
| bench-banking, bench-scoreboard, gaia, galion-boot, galion-realtest, galion-vending, galleon-arena, hades-gate, hermes-bench, one-defence, one-set-model, one-talk, openmind-discovery, openmind-mcp-stack, persona-plex-relay, repo-check | mostly usage errors | "config none" or the one path they read (one-defence DB, persona-plex-relay config, discovery registry) |
| galion-wallstreet | `--version` -> usage error | serve bind. Implemented locally (same output): its paper-only test pins its deps to serde/serde_json/zagreb-time, and galion-common carries an HTTP client |

Not covered on purpose: the test-only mocks `gaia-dummy`, `galion-bench-mock`,
`hermes-mock-model`.

Dependency change: 25 crates gain the in-workspace, std-only `galion-common`
path dependency. No crates.io dependency was added (see
docs/third-party/vps-version-flags.md).

## Proof

`GALION_REAL=1 cargo test -p galion-common --test real_flags -- --nocapture`
(Rust, builds all 34 release binaries with a stamp; `GALION_REAL_BIN_DIR=...`
runs prebuilt ones). See docs/real-tests/vps-version-flags.md.

## Owner decisions (Slavik)

* **D-194-1 version line change.** galion-deploy, galion-guard, openmind-nerve and openmind-queue printed `<name> 0.1.0`; they now print the stamped `<name> 0.1.0 (commit ..., built ...)` like the other binaries (the prefix is unchanged). openmind-memory `--help` now exits 0 (was 64). Accept (recommended), or keep the old lines for those four.
* **D-194-2 VPS `--check` run.** This row was box only. Next step: copy the static x86_64 binaries to a throwaway `/var/tmp/flags-test-<ts>` on the VPS and run `--version`/`--check` as an ordinary user against the real env (read-only, nothing bound, folder removed after). Needs your OK.
* **D-194-3 galion-wallstreet.** Keep the local copy of the flags (recommended, keeps the paper-only pinned dependency list), or allow `galion-common` in its deps and drop the ~40 duplicated lines.
* **D-194-4 galion-deploy dependency.** galion-deploy had no dependencies at all; it now uses the std-only in-workspace `galion-common`. Accept (recommended), or keep it dependency-free with a local copy.
* **D-194-5 deploy preflight (later).** galion-deploy could also run `<new> --check` (bounded like `--version`) and refuse on exit 1 before it installs anything. Not done here; it changes deploy behaviour.
