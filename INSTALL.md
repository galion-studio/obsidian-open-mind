<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Install and build (pre-release)

Written 2026-10-27 (Europe/Zagreb, +01:00) on `swarm/docs-publish-readme`, from
main-staging `98466ab9`. **Status: pre-release.** Nothing here is a production
claim; for what is live see [docs/STATUS.md](docs/STATUS.md) and the launch
gates in [docs/launch/GATES-OCT31.md](docs/launch/GATES-OCT31.md).

## Requirements

- Rust stable via [rustup](https://rustup.rs/) (no `rust-toolchain.toml` pinned).
- Linux. No Python, no openssl, no Node needed to build or test the Rust workspace.
- Cross targets (see [UNIVERSAL-CPU.md](UNIVERSAL-CPU.md)):
  - Tier 1, must build: `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`
    (linker: `rust-lld`, bundled with rustup).
  - Tier 2, `cargo check` only: `aarch64-linux-android` (Termux, no NDK),
    `armv7-unknown-linux-musleabihf`.

## Build and test

```sh
git clone https://github.com/galion-studio/obsidian-open-mind
cd obsidian-open-mind

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect
cargo build --release
scripts/no-python.sh
```

The sandbox crates (`openmind-sandbox`, `cgroup-detect`) are excluded locally:
their tests run on a dedicated Linux host only. On a shared box, build gently:
`nice -n 10 cargo build -j 4 --release`.

Static musl builds:

```sh
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld \
  cargo build --release --target aarch64-unknown-linux-musl
```

The edge node `edge/openmind-node` (`omnode`) is its own workspace: `cd
edge/openmind-node && cargo build --release`.

## Services and default ports

All services bind loopback (`127.0.0.1`) by default. Ports are the defaults
recorded in [docs/bind-defaults.md](docs/bind-defaults.md) and
[docs/cutover/README.md](docs/cutover/README.md).

| Service | Crate | Default port | Live state (per STATUS / PUBLISH-BLOCKERS, 2026-10-27) |
|---|---|---|---|
| Hermes (agent gateway) | `hermes-rs`, `hermes-gateway` | 8642 | Rust `hermes-rs.service` live since 2026-10-18 |
| Memory | `openmind-memory` | 8420 | Rust `openmind-memory.service` live since 2026-10-18 |
| Nerve (state bus, `/state`) | `openmind-nerve` | 8670 | runs on the VPS; newer fixes (recovery events) not deployed |
| Cortex | `gaia-*` / cortex | 8674 | live build is an older commit (bd6fbe6); merge-2 fixes **not deployed** |
| Console | `console-static`, `console-ui` | 8080 | **live console is still Node-served** (console-dist via srvx); the Rust console is not live |
| Galion One / mouth | `galion-one` | 8091 / 8002 | mouth/voice returns 503 on live |
| Desk | `galion-desk` | 8646 | built |
| Tuner | `tuner` | 8650 | built |
| Room | `room` | 4020 | built |

## Release artifacts

There is no `docs/release/`. Release material lives under `release/`:

- `release/omnode-0.1.0/`: `omnode-0.1.0-linux-{x86_64,aarch64}.tar.gz`, per-file
  `.sha256`, `SHA256SUMS`, `README.md`.
- `release/repo/`: the public-facing README, LICENSE, SECURITY, CONTRIBUTING,
  CODE_OF_CONDUCT, RELEASE-CHECKLIST.
- `release/FACTS.md`, `release/site/`, `release/brand/`, `release/marketing/`,
  `release/obsidian-open-mind-kit/`.

Signed (cosign) releases and SBOMs are produced by CI only after the self-eval
gate passes and the owner approves; none has been cut yet.

## Publish blockers (2026-10-27)

- PR #12 (staging to main) CI unstable; GitHub main still on `13a296c`.
- Live console still Node; live cortex lags merge-2.
- Mouth/voice 503; Hermes notify channel not configured.
- Owner decisions open (see docs/launch/GATES-OCT31.md and docs/STATUS.md).
