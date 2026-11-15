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

- Desktop swarm owns `LICENSE`, `README.md`, `docs/STATUS.md`, `docs/crate-index.md`, and dependency pull requests. Do not edit those. Do not bump dependency versions. Do not merge pull request 57. Desktop merges its own major bumps once they are green (15:29 decision below). Linux rebases onto that `main`.
- Do not edit, push, or merge draft pull requests 51 or 52, or their branches `cursor/hermes-mcp-client-3083` and `cursor/desktop-ci-artifacts-c14b`.
- Do not edit voice, symphony-the-product, `galion-one-voice`, Emotion V2, billing, servers, nexuslang, downloader, or infrastructure. `crates/galion-one/**` stays out of scope (Galion One).
- Do not edit `SOUL.md`, `config.yaml`, `LOCKS.toml`, `voices/`, `weights/`, `runtimes/`.
- No secrets, tokens, keys, or dotted IPv4 addresses in commits, pull requests, comments, or logs. Say “IPv4 loopback” and the port number.
- No new crates.io dependency (the desktop swarm owns dependency pulls). Time things with `std`. No SIMD, no `target-cpu=native`, no `target-feature` (see `UNIVERSAL-CPU.md`).
- Do not edit `.github/workflows/release.yml`, `.github/workflows/openmind-desktop.yml`, `vendor/`, or `contrib/`.

## Owner decisions (FRXDEL, 2026-11-15 15:29 CET)

1. Older pull requests, opened before Swarm 9: a core lane takes one only when it is blocked or failing **and** the change is inside that lane’s files. Carry the fix on a new branch from current `main`. Do not push the old branch. A pull request that is ready and green stays for the owner. Do not merge it and do not redo it.

   Checked the open pre-Swarm-9 pulls against the lane lists on 2026-11-15. None are both blocked or failing and inside a core lane. Not taken:

   - #14 conflicts on `crates/hermes-gateway/src/main.rs` and `crates/hermes-rs/**`. Desktop plan #67, re-read at `c8439c04`, keeps `crates/hermes-rs/**` until two pull requests have each merged: the #14 recreation, and #70 (getrandom 0.3.4, which edits `crates/hermes-rs/Cargo.toml` and `crates/hermes-rs/src/enroll/mod.rs`). CPU #71 drops the whole `hermes-rs` tree. The overlap with #70 is `lib.rs` only. Linux keeps `crates/hermes-gateway/tests/real_size_2mib.rs` only and does not edit `crates/hermes-gateway/src/**` or `crates/hermes-rs/**`.
   - #16 and #17 stay with the owner. Desktop plan #67 says their rollups were 21 failures and 1 skipped at 15:30, and the owner still keeps them. This orchestrator did not re-query those checks.
   - #15 is recreated by desktop. The conflict is `docs/crate-index.md` (#60). #7 and #28 stay with desktop.
   - #51 and #52 stay untouched.

2. Desktop major bumps are allowed. The desktop swarm merges them once every non-skipped check is green. Named set: #2 (`toml` 1.x), #19 (`actions/upload-artifact` 7), #20 (`actions/checkout` 7), #54 (`cosign-installer` 4), #55 (`codeql-action` 4), plus any later major they take. Core does not merge those and does not bump versions. #54 and #55 were already green apart from skipped `miri full` (desktop #58 cited runs 43684433869 and 43684453433). They still stay with desktop.

   Linux watches `Cargo.lock` and the pins in `.github/workflows/self-eval.yml`. After a desktop major reaches `main`, merge `main` into the Linux branch (no force-push) and re-run the cold `cargo tree` test before merging Linux. If that lockfile move changes a release binary’s features or size, re-measure `hermes-gateway`. Hygiene does not absorb #2. If `toml` 1.x lands and `release_feature_unify` fails on a new mismatch, hygiene fixes that test after rebasing. The red 0.x bumps #21 (`rusqlite`), #22 (`candle-nn`), and #23 (`getrandom`) are not core-lane files. Leave them unless desktop takes them as majors.

   Desktop plan is #67, re-read after its 15:34 update. Owner decision recorded there: `Copyright (c) 3000 Galion / galion-studio` stays. No core lane edits `LICENSE` or a copyright header. #65 stays a draft for the owner. This swarm does not merge it.

   Agreed split after that decision:

   - Linux owns `.github/workflows/self-eval.yml`, including the license job `run:` block. Desktop no longer edits that block. Desktop checkout (#20 / #64) and upload-artifact (#19 / #63) still edit `uses:` pins, and only while that bump pull request is the one in progress. Linux does not edit those pins. Rebase after they merge.
   - Hygiene owns `.github/workflows/miri.yml` except the `actions/checkout` pin lines, which #64 edits.
   - Do not add a `Cargo.lock` edge while a desktop bump that edits `Cargo.lock` is open (#66 and the queue in #67: #2, #21, #22, #23, #24, #26, #30). Hygiene may still add `rust-version` beside the `toml` line.
   - `README.md`, `docs/STATUS.md`, and `docs/crate-index.md` stay with desktop for the docs refresh. #60 is the crate-index pull request. The licence text itself does not change.
   - Desktop does not claim `crates/openmind-desktop/**`. CPU #71 keeps `src/files.rs`, `src/probe.rs`, and `benches/cpu.rs`. Linux #69 does not edit those three. Linux may still edit `src/lib.rs` and `src/http.rs` for a bind default, and it does not edit `files.rs` while #71's diff includes it.
   - `release_feature_unify.rs` stays hygiene. Desktop comments on this plan before editing that test.

   Live clashes seen 2026-11-15T15:32+01:00. Core lanes do not edit these lines. Rebase after desktop merges them:

   - #64 (`actions/checkout` 7.0.1) edits `.github/workflows/self-eval.yml` (Linux) and `.github/workflows/miri.yml` (hygiene). The diff is the checkout pin only.
   - #63 (`actions/upload-artifact` 7.0.2) edits the upload-artifact pin in `.github/workflows/self-eval.yml`.
   - #61 edits `.github/workflows/release.yml` only. #62 edits `.github/workflows/scorecard.yml` only. Neither file is a core lane.
   - #65 (`licence`) is stopped. It stays a draft. Do not merge it and do not build on its `LICENSE` diff. A `**/` lane still does not edit a `LICENSE` file or a copyright header.
   - #66 (`toml` 1.2.1) edits `Cargo.lock`, the `toml` line in the root `Cargo.toml`, and `crates/link-watch/Cargo.toml` (adds `galion-feature-pin`). Hygiene may add `rust-version` in the root `Cargo.toml` and must not change the `toml` version. Linux does not edit `crates/link-watch/Cargo.toml` (only `src/main.rs`). Rebase onto #66 after it merges. The feature-pin dependency it adds is desktop’s fix for the #2 mismatch. Hygiene does not copy it.

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
2. Linux. The hygiene test depends on the cold-cache `cargo tree` fix. If a desktop major lands on `main` while Linux is open, update Linux from `main` and let CI run again before hygiene.
3. Hygiene.
4. CPU. It may merge before Linux only when its diff touches none of the Linux or hygiene paths and Linux is not mid-update from `main`.

Refuse to merge a diff that turns `mcp-http` on in the default build, raises the 2 MiB size cap, or sets `rust-version` above 1.83. Those are owner decisions. The safe defaults are below.

## Lane 1 — CPU (`cpu`)

Real benchmarks and hot paths, in service of a flow a person can feel: local JSON, the mock bench server, and the llama health/model parse. No voice model, no candle, no gemm, no banking or payment path.

### Files (this lane only)

- `crates/galion-bench/**` except `LICENSE`
- `crates/galion-jsonout/**` except `LICENSE`
- `crates/openmind-llama/**` except `LICENSE`
- `docs/swarm-9/2026-11-15/core/cpu.md`

CPU draft #71 also keeps `crates/openmind-desktop/src/files.rs`, `src/probe.rs`, and `benches/cpu.rs`. Drop `crates/hermes-rs/**` and `crates/galion-common/src/http.rs` from that pull request. Hygiene owns `galion-common`.

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
- `scripts/fetch-musl-graphs.sh` (new). Do not add `crates/musl-tree-fetch` or any other `Cargo.lock` edge while a desktop `Cargo.lock` bump is open.
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

While a desktop `Cargo.lock` bump is open, the fix is `scripts/fetch-musl-graphs.sh`: the same empty-`RUSTFLAGS` `cargo fetch` the CI job already runs, for both musl targets. No `Cargo.lock` edit. No new crate. Call that script from the test-job fetch step in `self-eval.yml`. Do not rewrite the license job to change licence text. Leave the existing prefetch in place until a cold run of `cargo test -p galion-feature-pin --locked --test release_feature_unify` has passed after the script. A workspace member that depends on `curve25519-dalek-derive` waits until those bump pull requests have merged. Do not merge #57.

Paste the cold-cache command and its exit status in `linux.md`.

### Binds, musl, aarch64, units

`docs/bind-defaults.md` already lists IPv4 loopback for galion-one (ports 8091 and 8002), galion-desk (8646), tuner (8650), room (4020), and openmind-desktop (`DEFAULT_BIND` port 18181, stack dial port 8091). Console probe port 18080 is not a second desktop bind. Do not bind port 18081. A source read of `deploy/*.service` on this base showed loopback addresses, plus a comment in `deploy/console-prod.service` about an older non-loopback bind. Confirm with a command. If a unit is already loopback, do not rewrite it. If one is not, fix that unit only. Do not install or start units.

Musl and aarch64: the gate is already `scripts/ucpu-gate.sh` and the `ucpu-tier1`, `ucpu-tier2`, and `ucpu-qemu` jobs in `self-eval.yml`. Re-run the hermes-gateway x86_64 musl release build because the size backlog needs it. Run aarch64 the same way when the toolchain is on the machine. If it is not, write “not run” and point at the CI job. Do not rebuild the whole workspace for its own sake when you did not change linker flags.

## Lane 3 — Rust hygiene (`hygiene`)

MSRV, clippy, miri, a bounded unsafe audit, and tests. The user-facing result is that the tree builds on rustc 1.83, which is the toolchain pull request 51 hit, and the feature-pin test is honest.

### Files (this lane only)

- `crates/galion-common/**` except `LICENSE`
- `crates/galion-feature-pin/**` except `LICENSE`
- `crates/hades-gate/**` except `LICENSE`
- `crates/openmind-schema/**` except `LICENSE`
- `crates/openmind-sandbox/**` except `LICENSE`
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
| Swarm 9 core plan | `ed6b8058`, then the clash note | on `ed6b8058` at 15:32 +01:00: 25 queued, 1 skipped (`miri full`); CI restarts on the clash note | n/a | not merged |
| Swarm 9 core/linux | not opened | | | |
| Swarm 9 core/hygiene | not opened | | | |
| Swarm 9 core/cpu | not opened | | | |

Desktop plan pull request: #67, read 2026-11-15T15:34+01:00. The split above is the record. #60 remains `README.md`, `docs/STATUS.md`, `docs/crate-index.md`, and its notes file.

Owner decisions still open, not settled here:

1. Whether `mcp-http` joins the default `hermes-gateway` build. Safe default: no.
2. Whether the 2,097,152 byte cap moves. Safe default: no.
3. Copyright line on `LICENSE` (desktop swarm). Not this plan.
