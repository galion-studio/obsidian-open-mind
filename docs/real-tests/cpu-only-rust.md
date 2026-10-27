<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real-tests: CPU-only Rust (swarm/cpu-only-rust)

Europe/Zagreb (CET +01:00). Box ports: 21970–21973.

## Gate

```
scripts/cpu-only-gate.sh
# RESULT ok (candle + lock + tree for nerve/hermes/memory/console-static + cuda feature rejected)
```

Cortex (`swarm/cortex-live-merge-3` 73583ab): `cargo tree -p gaia-cortex` has no cuda/cudarc/opencl/metal/mkl/candle. `cargo test -p gaia-cortex --lib` 29/0.

## Release smoke (2026-10-27 18:22:07 +0100)

Built: `cargo build --release -p openmind-nerve -p hermes-gateway -p openmind-memory -p console-static`.
Binaries: no `libcuda` / OpenCL / NVIDIA in `ldd` or embedded strings.

| Service | Port | Result |
|---|---|---|
| openmind-nerve serve --mode event | 127.0.0.1:21970 | listening; /state HTTP probed |
| openmind-memory | 127.0.0.1:21971 | listening; schema migrate 0→4 |
| hermes-gateway | 127.0.0.1:21972 | listening (local smoke API_SERVER_KEY only) |
| galion-console-static --ui rust | 127.0.0.1:21973 | listening |

Procs killed after probes; ports freed. No VPS unit change. No secrets printed.
Raw HTTP capture: `/workspace/swarm/logs/cpu-only-rust-smoke.http` (box-local).
