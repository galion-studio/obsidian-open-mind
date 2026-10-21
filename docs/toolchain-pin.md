<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Pinned Rust toolchain (integration S4-T6, RELEASE-CHECKLIST 1.8)

Slot 196 `ci-toolchain-pin`, 2026-10-21 CEST (+02:00). Base integration-4 42de672
plus toolchain-aarch64 4d0c036 (cherry-picked, rust-lld cross linkers kept).

## What is pinned, and where

| Place | Value | Who reads it |
|---|---|---|
| `rust-toolchain.toml` `[toolchain] channel` | `1.99.1` (+ `rustfmt`, `clippy`, profile `minimal`) | rustup, for every `cargo`/`rustc` call under the repo root: local builds, `scripts/ucpu-gate.sh`, `cargo xtask`, CI |
| `Cargo.toml` `[workspace.package] rust-version` | `1.99` | cargo (refuses an older compiler with a clear error), clippy MSRV lints; inherited by all 63 `crates/*` via `rust-version.workspace = true` |
| `edge/openmind-node/Cargo.toml` `rust-version` | `1.99` | omnode is its own workspace; the root `rust-toolchain.toml` applies to it too |
| `.github/workflows/*.yml` `dtolnay/rust-toolchain` `toolchain:` | `"1.99.1"` (12 steps, every job that runs cargo) | CI installs the pinned compiler with the job's `targets:`/`components:` |

1.99.1 is rustc `58cf8f9c3 2026-10-13`: the stable the swarm has built and tested with,
and the compiler stamped into every live Rust binary in `/opt/galion/rust/bin` on the VPS
(read-only probe 2026-10-21 15:54 +02:00, docs/real-tests/ci-toolchain-pin.md).

Why the workflow input repeats the version: `dtolnay/rust-toolchain` does not read
`rust-toolchain.toml`, and with `toolchain: stable` it would install a floating stable that the
file then overrides (rustup would auto-install 1.99.1 a second time, without the job's targets).
Passing the same channel makes the action install exactly the compiler the file selects, keeps
every `run:` line unchanged (compatible with ci-workflow-rust's "every run: is `cargo xtask`"
rule) and needs no new action.

Targets are not in the file on purpose: each job adds only the target it needs. Locally:

```
rustup toolchain install 1.99.1 --profile minimal -c rustfmt -c clippy \
  -t x86_64-unknown-linux-musl -t aarch64-unknown-linux-musl \
  -t armv7-unknown-linux-musleabihf -t aarch64-linux-android
```

`cargo +nightly miri ...` (sec-miri-pure) still works: an explicit `+toolchain` beats the file.

## Guard

`crates/galion-realtest/tests/toolchain_pin.rs` (std only, no new dependencies):

* `toolchain_pin_consistent`: channel is an exact `X.Y.Z`; rustfmt + clippy listed;
  workspace `rust-version` = `X.Y`; every crate inherits it; omnode matches; every
  `toolchain:` in the workflows equals the channel; every job that runs cargo (directly, or a
  `scripts/*.sh` that calls cargo) has a `dtolnay/rust-toolchain` step with that channel;
  no `+stable`/`+beta`/`+nightly`/`RUSTUP_TOOLCHAIN`/`rustup default|override` in workflows,
  `scripts/*.sh` or `crates/xtask/**/*.rs`; `CARGO_PKG_RUST_VERSION` of the test crate = `X.Y`;
  `$CARGO` (what `cargo xtask` spawns) is the pinned toolchain's cargo; the slot-145 rust-lld
  linkers for aarch64/armv7 musl are still in `.cargo/config.toml`.
* six `rejects_*` negative controls mutate a scratch copy (floating channel, workflow
  `toolchain: stable`, job without toolchain step, crate without rust-version, mismatched
  rust-version, `+nightly` in a script and `RUSTUP_TOOLCHAIN` in xtask) and require the
  checker to fail.
* `real_toolchain_pin` (`GALION_REAL=1`): rustup resolution with the env cleaned, a missing
  pinned channel fails loudly (no silent fallback), tuner built for x86_64/aarch64/armv7 musl,
  rustc stamp read from each binary, binaries run (aarch64/armv7 under qemu-user) on
  127.0.0.1:20660-20662 with real GETs.

## Bumping the compiler (one commit)

1. `channel` in `rust-toolchain.toml`.
2. `rust-version` in `Cargo.toml` and `edge/openmind-node/Cargo.toml` (only if the minor changes).
3. `sed -i 's/toolchain: "OLD"/toolchain: "NEW"/' .github/workflows/*.yml`.
4. `cargo test -p galion-realtest --test toolchain_pin` (fails until 1-3 agree), then the full gates.

## xtask (ci-workflow-rust 38a26cb, not on integration-4 yet)

`crates/xtask` is not on this base. Checked on a scratch copy of 38a26cb with this pin applied
(same `rust-toolchain.toml`, rust-version lines and workflow sed): the guard flagged all 17
unpinned `toolchain: stable` lines and 17 jobs before the sed, and after it every check passed
except the expected one (38a26cb predates the 4d0c036 linkers); xtask's own tests 11/11,
including its "every `run:` is `cargo xtask`" rule. xtask spawns `$CARGO`, which is the pinned
cargo. When 38a26cb merges, its 17 `toolchain: stable` lines need the same sed (the guard fails
until they do). The xtask in cortex-t53-replay-corpus 55f07ff is std-only and runs under the
same file.
