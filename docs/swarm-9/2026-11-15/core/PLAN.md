<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Maciej Grajczyk -->

# Swarm 9 core plan

Orchestrator plan for Swarm 9 **core** (Rust, CPU, and Linux in obsidian-open-mind).
Written 2026-11-15T15:27:37+01:00 (Europe/Zagreb). Base commit `de7480e71ce98191d2ee11f018fa15be6c5a1707`.

This file is the lane contract. Workers read it on the draft pull request titled **Swarm 9 core plan** before they start. Each worker stays inside one lane. Crates and files do not overlap. A path that is not listed for a lane is not yours.

## Design direction (FRXDEL, 2026-11-15 15:25 CET)

Not perfect, but **working**. A genuinely great user experience. A very minimal but beautiful UI. Everything connected like a symphony: the operating system, the desktop, the apps, and the browsers on Linux should feel like one product.

For core, that means CPU, Rust, and Linux work serves a working, connected user flow first. Skip perfection for its own sake. Do not open a second UI, a second licence, or a second desktop. The desktop swarm owns the visible console. Core keeps the local Linux services, the builds, and the hot paths coherent so that console, apps, and a browser on the same machine are one product.

## Standing permission (FRXDEL, 2026-11-15 15:21 +01:00)

This replaces “draft only / never merge” for Swarm 9 pull requests in this swarm. It does not apply to pull requests 51, 52, or 57, and it does not apply to another swarm’s pull requests.

- A worker marks their pull request ready for review when every non-skipped CI check on the current head is green. Workers do not merge.
- The core orchestrator merges it when those checks are green **and** the review has nothing open. One pull request at a time. Normal merge or squash. No force-push, no admin merge, no skipped or disabled checks.
- Before the next merge, update that branch from `main` and wait until CI is green again on the new head.
- After a merge, existing GitHub workflows may run. Agents have no server credentials and must not seek any. Do not touch DNS, Cloudflare, HADES, firewall, ports, or production hosts.
- Do not settle an owner decision. Record it, and merge only the safe default named in this plan.

## Keep going (FRXDEL, 2026-11-15 15:26 CET)

Keep developing until the three lanes are finished and the tests you were required to run are green. Do not pause for another approval on work this plan already allows. The safety rules above do not change.

## What this swarm does not touch

- Desktop swarm owns `LICENSE`, `README.md`, `docs/STATUS.md`, `docs/crate-index.md`, and dependency pull requests. Do not edit those. Do not bump dependency versions. Do not merge pull request 57.
- Do not edit, push, or merge draft pull requests 51 or 52, or their branches `cursor/hermes-mcp-client-3083` and `cursor/desktop-ci-artifacts-c14b`.
- Do not edit voice, symphony-the-product, `galion-one-voice`, Emotion V2, billing, servers, nexuslang, downloader, or infrastructure. `crates/galion-one/**` stays out of scope (Galion One).
- Do not edit `SOUL.md`, `config.yaml`, `LOCKS.toml`, `voices/`, `weights/`, `runtimes/`.
- No secrets, tokens, keys, or dotted IPv4 addresses in commits, pull requests, comments, or logs. Say “IPv4 loopback” and the port number.
- No new crates.io dependency (the desktop swarm owns dependency pulls). Time things with `std`. No SIMD, no `target-cpu=native`, no `target-feature` (see `UNIVERSAL-CPU.md`).
- Do not edit `.github/workflows/release.yml`, `.github/workflows/openmind-desktop.yml`, `vendor/`, or `contrib/`.

## How to open your pull request

Base `main`. Draft until CI is green, then mark it ready. Title, exactly:

- `Swarm 9 core/cpu`
- `Swarm 9 core/linux`
- `Swarm 9 core/hygiene`

Notes file, in that same pull request:

- `docs/swarm-9/2026-11-15/core/cpu.md`
- `docs/swarm-9/2026-11-15/core/linux.md`
- `docs/swarm-9/2026-11-15/core/hygiene.md`

The notes file says what you ran, the exit status, and any number you measured. Unmeasured numbers stay “not measured”. Timestamps are Europe/Zagreb with an explicit `+01:00` offset. Commits carry `Signed-off-by`.

## Merge order

Coordinate with the open pull request titled **Swarm 9 desktop plan** in this repository. Before each merge, read that plan. If it lists a file your diff also edits, do not merge. Comment on both pull requests and wait for the file lists to diverge. This plan only adds `docs/swarm-9/2026-11-15/core/PLAN.md`, which the desktop plan does not own.

Preferred order, one at a time, each updated from `main` with CI re-run:

1. This plan (docs only).
2. Linux. The hygiene test depends on the cold-cache `cargo tree` fix.
3. Hygiene.
4. CPU. It may merge before Linux only when its diff touches none of the Linux or hygiene paths and Linux is not mid-update from `main`.

Refuse to merge a diff that turns `mcp-http` on in the default build, raises the 2 MiB size cap, or sets `rust-version` above 1.83. Those are owner decisions. The safe defaults are below.

## Lane 1 — CPU (`cpu`)

Real benchmarks and hot paths, in service of a flow a person can feel: local JSON, the mock bench server, and the llama health/model parse. No voice model, no candle, no gemm, no banking or payment path.

### Files (this lane only)

- `crates/galion-bench/**`
- `crates/galion-jsonout/**`
- `crates/openmind-llama/**`
- `docs/swarm-9/2026-11-15/core/cpu.md`

### Work

There is no `[[bench]]` target under `crates/` on this base (workspace search, 2026-11-15). Add a real timed run, not a guessed number.

- `galion-jsonout`: time `PyFormatter` on a mixed ASCII and non-ASCII payload. This writer is on the desk and link-watch path.
- `galion-bench`: time the loopback mock server hit path. Do not add a live bank, broker, or supplier call. The mock already refuses outbound forwarding.
- `openmind-llama`: time parsing of the health body and the model list. This crate does not multiply matrices and does not dial. Do not add a GPU or a network call.
- SHA-256 lives in `galion-common`, which hygiene owns. You may call the public function from a bench under `crates/galion-bench` without editing `galion-common`. If the public function is not enough, stop and say so in `cpu.md`. Do not edit the other lane’s files.

Use `std::time`. Do not add `criterion` or any other new dependency. Portable Rust only.

### Done

`cpu.md` quotes the command, the exit status, the iteration count, and the timing. “Not measured” if you did not run it. `cargo test` for the three crates is green. No new UI.

## Lane 2 — Linux (`linux`)

Musl and aarch64 builds, the binary size cap, systemd units, and loopback bind defaults. The user-facing result is one set of static Linux binaries the desktop and a local browser can run, listening on loopback, under the existing size cap.

### Files (this lane only)

- `scripts/ucpu-gate.sh`
- `scripts/size-budget.sh`
- `scripts/install-zig.sh`
- `scripts/target-check.sh`
- `scripts/fetch-musl-graphs.sh` (new, this lane)
- `config/release/size-budget.tsv`
- `.cargo/config.toml`
- `.github/workflows/self-eval.yml`
- `docs/ucpu.md`
- `docs/bind-defaults.md`
- `docs/toolchain-aarch64.md`
- `docs/real-tests/hermes-gateway-2mib.md`
- `docs/real-tests/size-budget-gateway-fix.md`
- `crates/hermes-gateway/tests/real_size_2mib.rs`
- `crates/galion-realtest/tests/size_budget_gate.rs`
- `crates/galion-realtest/tests/real_sh_ucpu.rs`
- `crates/galion-realtest/tests/real_toolchain_aarch64.rs`
- `deploy/**/*.service` and `deploy/**/*.timer`, except any unit that invokes `cloudflared` (Cloudflare stays untouched; record those units, do not edit them)
- Bind-default sources, and only if the desktop plan does not list the same path:
  - `crates/galion-desk/src/main.rs`
  - `crates/link-watch/src/main.rs`
  - `crates/tuner/src/config.rs`
  - `crates/tuner/src/lib.rs`
  - `crates/room/src/config.rs`
  - `crates/room/tests/no_bind.rs`
  - `crates/openmind-desktop/src/lib.rs`
  - `crates/openmind-desktop/src/files.rs`
  - `crates/openmind-desktop/src/http.rs`
- A new non-release member such as `crates/musl-tree-fetch/**` if you use that fix
- `docs/swarm-9/2026-11-15/core/linux.md`

Do not edit `crates/galion-one/**`. Do not edit `crates/galion-feature-pin/**`. Do not edit `crates/hermes-rs/**` (pull request 51). Do not edit `crates/galion-realtest/Cargo.toml`.

### Backlog A — hermes-gateway size, from pull request 51

Do not edit pull request 51. Re-measure. Leave `mcp-http` in the default build as an owner decision.

What is already on record, read from pull request 51 at `ff9d400387f598c1c2091f84ad19c3544f9dc962` (not a new measurement):

- `docs/hermes-mcp.md` and `docs/swarm-8/2026-11-15/a1-mcp-desktop.md` record an x86_64 musl `hermes-gateway` of 2,091,904 bytes on commit `08b8318e` (size-budget log 2026-11-15T01:31:46+01:00), cap 2,097,152, `mcp-http` off. The same docs record 2,092,640 bytes on `05adf328` (job 132836192211, 2026-11-15T15:05:37+01:00), still under the cap, still `mcp-http` off.
- This orchestrator did not find the literal string `TODO(verify)` in those two files. The backlog still treats 2,091,904 as the pre-merge figure that must be measured again. Do not copy either number forward as the current size.

Required measurements, written only in `linux.md`:

1. Current `main`, x86_64 musl, default features. `mcp-http` is not on `main`. Command starts from `scripts/size-budget.sh` or an equivalent `cargo build --release --locked -p hermes-gateway --target x86_64-unknown-linux-musl`. Quote bytes, stripped or not, and the cap.
2. With and without `mcp-http`: use a local worktree of pull request 51’s head. Build default and `--features mcp-http`. Do not push that branch. Do not open a pull request from it. Do not change `default` in any `Cargo.toml`.

Safe default you may merge: `mcp-http` stays out of the default build. If a build is over 2,097,152 bytes, record it and stop. Do not raise `config/release/size-budget.tsv`.

### Backlog B — `cargo tree --offline` and `curve25519-dalek-derive`

`crates/galion-feature-pin/tests/release_feature_unify.rs` runs `cargo tree --offline --locked --target` for `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. `.cargo/config.toml` sets `curve25519_dalek_backend="serial"`, which drops `curve25519-dalek-derive`. `cargo fetch` honors that cfg. `cargo tree` does not, so a cold cache fails. The crate is already in `Cargo.lock` (`name = "curve25519-dalek-derive"`). CI prefetches in `.github/workflows/self-eval.yml` with an empty `RUSTFLAGS` before `cargo test`. Pull request 51 saw the workspace test fail on those two tests until an online `cargo tree` had downloaded the crate; the file then passed 5 tests.

Make a cold `cargo test -p galion-feature-pin --locked --test release_feature_unify` pass **without editing** `crates/galion-feature-pin/**`.

Keep the serial cfg. Do not clear `[build] rustflags` in `.cargo/config.toml`. `crates/hades-gate/tests/simd_free.rs` fails if `RUSTFLAGS` replaces that list, and that test file is hygiene’s.

A working fix: a new workspace member this lane owns, not a release binary and not linked into `hermes-gateway`, whose only dependency is the already-locked `curve25519-dalek-derive`. A normal fetch then downloads it before the offline tree. Do not change any existing dependency version. The only `Cargo.lock` edit is that new edge. If that collides with pull request 57, stop and comment. Do not merge 57. Leave the CI prefetch in place until the cold-cache test has passed once.

Paste the cold-cache command and its exit status in `linux.md`.

### Binds, musl, aarch64, units

`docs/bind-defaults.md` already lists IPv4 loopback for galion-one (ports 8091 and 8002), galion-desk (8646), tuner (8650), room (4020), and openmind-desktop (`DEFAULT_BIND` port 18181, stack dial port 8091). Console probe port 18080 is not a second desktop bind. Do not bind port 18081. A source read of `deploy/*.service` on this base showed loopback addresses, plus a comment in `deploy/console-prod.service` about an older non-loopback bind. Confirm with a command. If a unit is already loopback, do not rewrite it. If one is not, fix that unit only. Do not install or start units.

Musl and aarch64: the gate is already `scripts/ucpu-gate.sh` and the `ucpu-tier1`, `ucpu-tier2`, and `ucpu-qemu` jobs in `self-eval.yml`. Re-run the hermes-gateway x86_64 musl release build because the size backlog needs it. Run aarch64 the same way when the toolchain is on the machine. If it is not, write “not run” and point at the CI job. Do not rebuild the whole workspace for its own sake when you did not change linker flags.

## Lane 3 — Rust hygiene (`hygiene`)

MSRV, clippy, miri, a bounded unsafe audit, and tests. The user-facing result is that the tree builds on rustc 1.83, which is the toolchain pull request 51 hit, and the feature-pin test is honest.

### Files (this lane only)

- `crates/galion-common/**`
- `crates/galion-feature-pin/**`
- `crates/hades-gate/**`
- `crates/openmind-schema/**`
- `crates/openmind-sandbox/**`
- `crates/galion-realtest/tests/miri_workflow.rs`
- `.github/workflows/miri.yml`
- `.github/workflows/msrv.yml` (new, this lane only)
- `docs/security/sec-miri-pure.md`
- `Cargo.toml` at the repo root, and only to set `[workspace.package] rust-version` after the 1.83 build works. Do not change dependencies, profiles, or patches.
- `docs/swarm-9/2026-11-15/core/hygiene.md`

Do not edit `.github/workflows/self-eval.yml` or `.cargo/config.toml` (Linux). Do not add a second fetch workaround inside `release_feature_unify.rs`. Clippy for your crates is `cargo clippy -p <crate> --all-targets --locked -- -D warnings`. The workspace clippy job already lives in the Linux-owned workflow. Leave that job alone.

### Backlog C — rustc 1.83 and `galion-common` (E0658)

Pull request 51’s note `docs/swarm-8/2026-11-15/a1-mcp-desktop.md` says rustc 1.83.0 cannot compile current `galion-common` (`E0658` on `is_multiple_of` and `as_chunks`). This plan did not re-run that compile (not run).

Sites read on this base:

- `crates/galion-common/src/hex.rs` uses `is_multiple_of` and `as_chunks`
- `crates/galion-common/src/base64.rs` uses `is_multiple_of`
- `crates/galion-common/src/sha256.rs` uses `as_chunks` and `as_chunks_mut`

Reproduce with rustc 1.83 before editing. Quote the error. Then make `galion-common` compile on 1.83 with the same behaviour (length checks and chunking that 1.83 already has). Do not raise MSRV above 1.83 to avoid the edit. That would hide the mismatch. After the 1.83 compile exits 0, set `rust-version = "1.83"` in the root workspace package. If a further E0658 remains inside `galion-common`, fix it there. If it is outside this lane’s files, record it and stop.

There is no `rust-toolchain.toml` and no `rust-version` on this base (`Cargo.toml` workspace package, `RELEASE-CHECKLIST.md` item 1.8 still open).

### Backlog D — `release_feature_unify`

You own `crates/galion-feature-pin/tests/release_feature_unify.rs`. Linux owns the cold-cache fix. Do not reimplement that fix in the test. Re-run the test after Linux has landed on `main`, and paste the result. If your branch is still based on a `main` without the fix, say that the cold-cache failure is Linux’s and do not paper over it. The assertions (solo feature set equals the full release set, both musl targets) stay.

### Clippy, miri, unsafe, tests

- Clippy `-D warnings` on the crates you own. Fix warnings in those crates only.
- Miri: keep `.github/workflows/miri.yml` green for `galion-common`, `hades-gate`, and `openmind-schema`. Do not add crates for completeness. `miri_workflow.rs` is yours if the skip list changes.
- Unsafe: `galion-common` is `#![forbid(unsafe_code)]`. The largest `unsafe` surface you own is `crates/openmind-sandbox`. Audit the crates you own. Fix a small, tested hole. Do not rewrite the sandbox for style. For `unsafe` in crates you do not own, write the file and the count in `hygiene.md` and do not patch them.
- Tests: `cargo test --locked` for the crates you own. Paste the summary line.

## Orchestrator

Review comments only, until the standing-permission merge check passes. No approvals via the review button as a substitute for that check. The merge itself is a normal merge once the pull request is ready and the head is green.

Status (updated by the orchestrator):

| Pull request | Head | CI | Review | Merged SHA |
| --- | --- | --- | --- | --- |
| Swarm 9 core plan | this file | not run yet | n/a | not merged |
| Swarm 9 core/linux | not opened | | | |
| Swarm 9 core/hygiene | not opened | | | |
| Swarm 9 core/cpu | not opened | | | |

Desktop plan pull request: not seen at 2026-11-15T15:27:37+01:00 (open pulls checked; none titled Swarm 9 desktop plan).

Owner decisions still open, not settled here:

1. Whether `mcp-http` joins the default `hermes-gateway` build. Safe default: no.
2. Whether the 2,097,152 byte cap moves. Safe default: no.
3. Copyright line on `LICENSE` (desktop swarm). Not this plan.
