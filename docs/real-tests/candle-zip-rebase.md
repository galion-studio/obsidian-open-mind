<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-test record: `swarm/candle-zip-rebase` (slot 154)

Branch = integration-4 42de672 + candle-zip-fix a4f49b5 (merge 3bc374e), integration-4 not moved.
All runs on the box, 2026-10-21 (CEST +02:00). No VPS changes, no sandbox runs (sandbox_vps_only).

| Time (+02:00) | Check | Result |
|---|---|---|
| 07:55 | vendored candle-core `cargo test --test pth_tests --test serialization_tests` (after adding `tests/test.pt`, `tests/test_with_key.pt`) | 3/3 + 3/3 passed |
| 07:56 | `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings` | ok; ok |
| 07:56-07:57 | `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` | 1229 passed, 0 failed, 6 ignored (earlier run with hermes-tools: 1241/0) |
| 07:57 | `scripts/no-python.sh` | ok |
| 07:57 | `cargo deny check advisories` (cargo-deny 0.20.2, local advisory DB) | zip RUSTSEC-2026-0367/0368/0369 gone (`zip v9.0.5` under vendored candle-core); only RUSTSEC-2024-0436 (paste, unmaintained) left, which is slot 153's task |
| 07:58-08:01 | `cargo build --release --workspace --target x86_64-unknown-linux-musl` | ok, 37/37 binaries static-pie |
| 08:01-08:04 | same for aarch64-unknown-linux-musl (rust-lld; zig cc 0.15.2 per-shell CC for libsqlite3-sys, docs/toolchain-aarch64.md) | ok, 37/37 binaries AArch64 static |
| 08:04-08:06 | `cargo check --workspace --all-targets` for armv7-unknown-linux-musleabihf and aarch64-linux-android (zig cc, musl headers) | ok, ok |

Not run here: the GALION_REAL voice tests (real_voice_serve / real_voice_rust need the model files; the
candle-zip-fix record covers them on a4f49b5) and the sandbox crates (VPS only).
