<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# hermes-gateway release recipe: gcc vs zig (D-416-1) and a recorded recipe for every artefact (D-416-3)

Wave 4 slot 434, branch `swarm/hermes-gateway-recipe`, off `swarm/hermes-gateway-candidate` 20936d52.
All times Europe/Zagreb, CET (+01:00), 2026-10-28. **Nothing was deployed.** The gateway deploy stays its
own window (D-373-3 / D-416-2). The rollback text in `docs/release/hermes-gateway-candidate-20261028.md`
is unchanged (one step back to live b7fbbc78 via `90-deploy-t291u.conf`).

## The tool: `cargo xtask release-recipe`

`crates/xtask/src/release_recipe.rs` (Rust, std + the std-only `galion-common`; no new dependency, no
script). It replaces the hand-copied shell recipe that could not reproduce live b7fbbc78 (F-416-1).

```sh
# build: from `git archive <rev>` (never the working tree), fresh target dir, cleared env
cargo xtask release-recipe build --commit 989958be --cc gcc --out DIR      # or --cc zig
#   -> DIR/hermes-gateway-989958be  +  .sha256 (sha256sum -c format)  +  .recipe
# verify: artefact sha == recipe, toolchain == recorded, rebuild in another path, sha must match
cargo xtask release-recipe verify DIR/hermes-gateway-989958be
# check: no build, artefact sha/size == recipe and within config/release/size-budget.tsv
cargo xtask release-recipe check DIR/hermes-gateway-989958be
```

What the build pins and records in `<artefact>.recipe` (key=value, SPDX header):

| key | meaning |
|---|---|
| `commit`, `package`, `bin`, `target`, `features`, `cc` | the inputs that decide the bytes |
| `cargo_argv` | the exact argv: `cargo build --release --locked --offline -p hermes-gateway --bin hermes-gateway --target x86_64-unknown-linux-musl` (`-j` is not recorded: it does not change the bytes) |
| `env` | environment cleared except PATH, HOME, CARGO_HOME, RUSTUP_HOME, RUSTUP_TOOLCHAIN, TMPDIR, ZIG_GLOBAL_CACHE_DIR; then `LC_ALL=C`, `GALION_BUILD_COMMIT`, `SOURCE_DATE_EPOCH` (commit time), `CARGO_INCREMENTAL=0`, `CC_<triple>`, `AR_<triple>`. So a stray `CFLAGS`, `CRATE_CC_NO_DEFAULTS`, `RUSTFLAGS` or `CC` (the F-416-1 cause) cannot reach the build |
| `rustc`, `rustc_commit`, `llvm`, `cargo`, `cc_version`, `lld` | toolchain; `verify` refuses to rebuild if any differs (a rebuild with another toolchain proves nothing) |
| `cc_path`, `ar` | gcc: `/usr/bin/x86_64-linux-musl-gcc` + `ar`; zig: the xtask `zig-cc` shim (drops cc-rs `--target=`, runs `zig cc -target x86_64-linux-musl`, the same command line as the `scripts/ucpu-gate.sh` wrappers, no shell script) + `zig ar` |
| `size`, `budget`, `sha256`, `built_at` | output; `build` fails when size > budget |

`check` also fails when `cargo_argv` was edited by hand (it must equal what the tool would run).
Committed recipes for the candidate: `config/release/recipes/hermes-gateway-989958be{,-zig}.recipe`; a unit
test pins the gcc one to the `# sha256` line and the binary name of the staged `95-gateway-candidate.conf`.

## D-416-1 evidence: C compiler gcc vs zig on 989958be

Same commit 989958bed31f18f92dafc99059cd0eb384898c5e, same rustc 1.99.1 (58cf8f9c) / LLVM 23.1.1 / cargo
1.99.1 / rust-lld (LLD 23.1.1), same argv, `SOURCE_DATE_EPOCH=1793206972`. Only `cc` differs.

| | gcc (Debian musl-tools 1.2.5-3.1~deb13u1, gcc 14.2.0-19) | zig 0.15.2 cc (pinned, sha256+minisign verified, slot 338) |
|---|---|---|
| size | **1,952,592 B** | 2,088,128 B (+135,536 B) |
| 2 MiB headroom | **144,560 B** | 9,024 B |
| sha256 | `0f7bb47123628ff6b76231844991cd3bc577d5cd4eb541a0b8e29df158e38ac8` | `47d3897ad0301a865a6bbd6162ae6408c160a6826d95e864eb0568551090bc49` |
| = slot 416 numbers | yes (= staged drop-in sha) | yes (416 "47d3897a…", 2,088,128 B) |
| repro path A (23:12 CET) | `artifacts/t434/a-gcc/.work-*/src-989958be` | `artifacts/t434/a-zig/.work-*/src-989958be` |
| repro path B (`verify`, 23:13–23:17) | `target/hermes-gateway-recipe-pathB-gcc` → same sha, verify OK | `target/hermes-gateway-recipe-pathB-zig` → same sha, verify OK |
| repro path C | real test, `git archive HEAD` (5a5eb14b, other `GALION_BUILD_COMMIT`/`SOURCE_DATE_EPOCH`) → same sha 0f7bb471 | – |
| box parity P1–P4 vs live b7fbbc78 bytes (c5432a88) | **pass** 23:17:28 (incl. sha == staged drop-in) | **pass** 23:17:40 |
| P2 `/var/lib` worktree `--check-config` | live 0, candidate 2 (fold works) | live 0, candidate 2 |
| P3 9 HTTP routes + chat | identical status + scrubbed body | identical |
| P4 tools (coding off) | `clock`, `delegate_task` both | same |
| build time (warm registry, -j 2) | ~29–31 s | ~20–30 s |
| cross targets | needs a cross gcc per target (box has `aarch64-linux-musl-gcc` / `arm-linux-musleabihf-gcc` wrappers) | one compiler for x86_64/aarch64/armv7 |
| supply | Debian package; drift caught by `verify` (`cc_version`) | pinned tarball under /workspace/swarm/tools |

Both builds behave the same on every parity probe; the difference is size (C code of bundled SQLite: zig
clang at the cc-rs defaults is ~136 KB larger) and supply. P5 (sandboxed preflight) was not re-run: it is
VPS-only (sandbox_vps_only) and this slot stayed VPS read-only; slot 416 already passed P5 for the gcc bytes.

**Recommendation D-416-1: gcc** for the x86_64 hermes-gateway release (the staged candidate bytes), with the
recipe recorded; zig only if the box gcc is ever unavailable, and then only after a fresh size check
(9 KB headroom is one small feature away from the 2 MiB gate).

## D-416-3: record the exact build for every deploy

Proposal (recommended yes): every artefact copied to the VPS travels with its `.recipe` and `.sha256`, and the
deploy runbook gets a step 0 on the box before step 1:

```sh
cargo xtask release-recipe verify <dir>/hermes-gateway-989958be   # must print "verify OK"
```

Slot 416 runbook steps 1–6 and its rollback stay as written. The test that fails when a recorded recipe does
not reproduce its sha is `crates/xtask/tests/real_release_recipe.rs` (R7: recipe commit pointed at 83e573e2 →
rebuild 1,948,240 B sha 8f8ac925… ≠ recorded 0f7bb471… → verify exits 1 "does NOT reproduce").

## Owner decisions

- **D-416-1** C compiler: gcc (rec) vs zig. Evidence above.
- **D-416-3** recipe per deploy: adopt `release-recipe` + step 0 verify (rec yes).
- **D-434-1** land `swarm/hermes-gateway-recipe` (tool + committed recipes + parity port env) with the
  gateway candidate (rec yes; touches only xtask, config/release/recipes, docs and a test).
- **D-434-2** extend recipes to the other release bins (nerve, cortex, console) in a follow-up slot (rec yes;
  the tool already takes `-p/--bin/--features/--target`).
- **D-434-3** Debian gcc/musl-tools drift: record only and let `verify` refuse (rec), vs `apt-mark hold` on
  the box (box-only, not the VPS).

Caveats: zig builds share the zig global cache (`ZIG_GLOBAL_CACHE_DIR`, content-addressed); both zig paths
ran with a warm cache. `verify` needs the same toolchain as recorded; after a rustc update, re-record.
