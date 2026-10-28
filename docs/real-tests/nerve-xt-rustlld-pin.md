<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Commit the x86_64 musl rust-lld linker line (slot 386, D-376-2 / F-376-4)

- Branch `swarm/nerve-xt-rustlld-pin`, base `swarm/nerve-xt-pin-refresh` 52aedbc7.
- Times Europe/Zagreb, CET +01:00. Slot ports 22560-22569 (used 22560-22562).
- NO deploy, nothing changed on the VPS.

## Change

`.cargo/config.toml` gains (with a comment that cites F-376-4):

```toml
[target.x86_64-unknown-linux-musl]
linker = "rust-lld"
```

Linker choice only: no codegen, CPU or rustflags change; the env var
`CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER` still overrides it. Host
builds/tests (`x86_64-unknown-linux-gnu`) are not affected. All three musl
targets now link the same way (aarch64/armv7 had the line since slot 145).
`scripts/ucpu-gate.sh` and `galion-deploy` `real_build` already forced the
same linker through env, so they are unchanged in effect.

`crates/openmind-nerve/tests/real_cross_target.rs` (test only):

- `NERVE_XT_PIN=live` now rebuilds `git archive 9d1a4f3` with THIS tree's
  committed config (`git show HEAD:.cargo/config.toml`, not the working copy)
  overlaid on the archive, and with `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER`
  removed from the cargo env, so the committed file alone picks the linker.
- `NERVE_XT_LIVE_CONFIG` = `committed` (default) | `env` (slot-376 way:
  archive config + linker env) | `archive` (negative control: archive config,
  no env = what a deploy got without the line).
- New always-on unit tests: `repo_config_pins_rust_lld` (working-copy config
  links x86_64/aarch64/armv7 musl with rust-lld; a local edit that drops a
  line fails before commit), `config_linker_parser`, `live_config_parsing`.
  Unit run: 8/8 pass (incl. the gated test as skip).

## Proof: git archive 9d1a4f3 + committed config == VPS binary

VPS read-only, 2026-10-28 13:10:06 +01:00: `galion-nerve` active, NRestarts 0,
started 2026-10-27 18:15:00 +01:00, ExecStart
`/opt/galion/rust/bin/openmind-nerve-9d1a4f3`; sha256
d0c70b879e5866bb105707d881686757b5732168da2c016f791316ec3f521d45, 1 458 760 B;
nerve `/health` 200, `/state` 200, cortex `:8674/health` 200; no VPS listener
on 22560-22569.

Real runs (`GALION_REAL=1 NERVE_XT_PIN=live NERVE_XT_PORT_BASE=22560`, box,
rustc 1.99.1 58cf8f9c3 2026-10-13):

| time (+01:00) | NERVE_XT_LIVE_CONFIG | outer linker env | live-pin result | verdict |
|---|---|---|---|---|
| 12:58:07-13:00:13 | committed (default) | unset | d0c70b87... 1 458 760 B (20.9 s build) | **== VPS, pass** (1/1, 126 s) |
| 13:00:24 | archive | removed | 64ce3aa0... 1 463 072 B | fails as intended (same bytes slot 376 measured for the default linker) |
| 13:00:46 | env | (test sets rust-lld) | d0c70b87... 1 458 760 B | pass (slot-376 path still works) |
| 13:01:08 | bogus | - | - | fails as intended: `NERVE_XT_LIVE_CONFIG must be committed, env or archive` |
| 13:01:34-13:09:56 (queued) | committed, pin dir cold | `=cc` (hostile) | d0c70b87... 1 458 760 B | **pass**: env removed for the pin, committed config alone wins; tree builds in the same run used `cc` (1 930 048 B ccc551fb = slot 376 number), so the env override still works |

Each live run also passed everything in `budget`: smokes on :22560 (x86_64),
:22561 (aarch64 under qemu), :22562 (recipe, then live-pin), first cycle
0.10/0.21/0.10 s, `/health` wire order equal on the three tree binaries,
live-pin `/health` sorted (same as the real VPS), /state 24 keys tree vs 23
live-pin; aarch64-linux-android and armv7 musl `cargo check` ok. Ports
22560-22569 free after every run.

Tree nerve with the committed line: x86_64 1 923 432 B a58ecbb8...
(rust-lld) vs 1 930 048 B ccc551fb (cc): -6 616 B, headroom to 2 MiB
173 720 B (was 167 104 B; F-376-1 still WARN at 92 %). solo == recipe still
bit-identical (a58ecbb8).

## Other binaries still build and run

`cargo build --release --locked --target x86_64-unknown-linux-musl --workspace --bins`,
twice in separate target dirs, 2026-10-28 13:01-13:08 +01:00: committed
config (rust-lld) and `CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=cc`
(old default). Both exit 0 (2 m 15 s / 2 m 14 s, `-j 2`), including the C
code (bundled SQLite via `x86_64-linux-musl-gcc`).

- 40 binaries each side; 80/80 static (no PT_INTERP, `readelf -lW`).
- Every rust-lld binary is smaller: total 34 299 808 -> 34 082 704 B
  (-217 104 B), per binary -600 B (galion-health) to -9 752 B
  (hermes-mock-model).
- Smoke parity: each binary run with `--version` in a throwaway dir
  (`env -i`, 3 s timeout); output hash identical cc vs lld for 40/40
  (e.g. `openmind-nerve 0.1.0`, `tuner 0.1.0 (commit unknown, built unknown)`,
  hermes-gateway refuses to start without its key on both). No leftover
  processes.
- Note: `hermes-gateway` is 2 187 664 B with rust-lld (2 194 192 B with cc),
  still above the 2 MiB ceiling, a known issue (slot 373 gateway size land
  order), not caused by this change.

## Gates

Box, 2026-10-28 13:10-13:15 +01:00: `cargo fmt --all --check` ok; `cargo clippy
--workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect --
-D warnings` ok; `cargo test --workspace --no-fail-fast` (excl.
openmind-sandbox, cgroup-detect, hermes-tools; sandbox_vps_only): 378 suites,
2014 passed, 0 failed, 18 ignored (+3 new unit tests vs slot 376);
`scripts/no-python.sh` ok; SPDX headers on both new docs. No new deps,
Cargo.lock unchanged.

## Owner decision note (D-376-2 -> D-386-1)

**Recommend: accept the committed line.** Reasons: (1) the live nerve
9d1a4f3 then reproduces bit for bit from git alone (proved above; without
the line it does not: 64ce3aa0 vs d0c70b87); (2) every x86_64 musl binary
stays static, behaves the same and is 0.6-9.8 KB smaller; (3) the gate
script and galion-deploy already link with rust-lld, so deploy and repo
agree; (4) override stays possible by env.

Effect to know: every x86_64 musl binary built after this lands has new
bytes vs a cc-linked build of the same commit. That matters only for pins
that were taken with the cc linker; the nerve live pin was taken with
rust-lld, so it gets **more** stable, not less. No live service changes
until its next deploy, and then it is the same binary the deploy recipe
already produced.

Alternative (not recommended): keep the config as is and require the env
var in every deploy recipe (status quo; easy to forget, as F-376-4 shows).
