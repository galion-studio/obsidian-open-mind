<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# CPU-only Rust services (nerve, cortex, hermes, memory, console)

Date: 2026-10-27 Europe/Zagreb (CET +01:00). Branch: `swarm/cpu-only-rust` (Mate t276u job1).

## Findings (research)

| Area | Result |
|---|---|
| Live VPS binaries (`openmind-nerve-*`, `gaia-cortex-*`, `hermes-gateway-*`, `openmind-memory-*`) | Static musl PIE; `ldd` shows no `libcuda` / OpenCL / NVIDIA libs. Units set no `CUDA_*` env. No `/dev/nvidia*` on the VPS. |
| `openmind-nerve`, `hermes-gateway`, `openmind-memory`, `console-static` / `console-ui` | No candle / CUDA / OpenCL / Metal / MKL deps in `Cargo.toml`. |
| `gaia-cortex` (on `swarm/cortex-live-merge-*`, not yet on `main-staging`) | Depends on `openmind-nerve` + serde; no GPU crates. |
| `galion-voice-cpu` / `galion-chatterbox` | Use vendored `candle-core` / crates.io `candle-nn` with `default-features = false`. Voice only; not part of the five services. |
| Vendored `candle-core` 0.9.2 (before this branch) | Still declared optional `cuda` / `cudnn` / `nccl` / `metal` / `mkl` / `accelerate` features and deps (`cudarc`, `candle-kernels`, …). Defaults were off, but `--all-features` / accidental enablement could pull GPU stacks. |
| OpenCL | No OpenCL crate in the workspace or lockfile. |
| Poseidon / desk "gpu" fields | Schema / routing labels (`openmind-schema.gpu`, desk board GPU string, LAN "gpu" queue). Not CUDA libraries. Left unchanged. |

## Changes on this branch

1. **`vendor/candle-core`**: remove GPU feature arms and optional deps (GALION-PATCH.md item 7). `dummy_cuda_backend` remains the CPU stand-in.
2. **`deny.toml`**: ban `cudarc`, `candle-kernels`, `candle-metal-kernels`, `accelerate-src`, `intel-mkl-src`, `opencl3`, `ocl`, `cl-sys`, `objc2-metal`.
3. **`repo-check`**: those crates fail the portable check if they appear in `Cargo.lock`; workspace `Cargo.toml` files may not enable `"cuda"` / `"metal"` / … features (`vendor/` skipped).
4. **`scripts/cpu-only-gate.sh`**: candle + lock + `cargo tree` for the five services + prove `--features cuda` fails.

## VPS unit changes

**None required.** Live services already run CPU-only static binaries. No drop-in or `Environment=` change is needed for CPU-only compliance. Future deploys should keep shipping musl static builds without CUDA toolchains.

## How to verify

```bash
export CARGO_TARGET_DIR=/workspace/swarm/target/cpu-only-rust
scripts/cpu-only-gate.sh
nice -n 10 cargo test -p repo-check -j 4
nice -n 10 cargo test -p openmind-nerve -p hermes-gateway -p openmind-memory -p console-static -j 4
```

Cortex: run `cargo tree -p gaia-cortex` on a cortex worktree (e.g. `swarm/cortex-live-merge-3`).
