<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: `swarm/voice-speed` (slot 147)

No new crate enters `Cargo.lock`. Two crates that were already in the tree (pulled in by `candle-core` 0.9.2 at
the same versions) became direct dependencies of `crates/galion-chatterbox`:

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it ourselves |
|---|---|---|---|---|---|
| `rayon` | 1.12.0 | MIT OR Apache-2.0 | https://github.com/rayon-rs/rayon | splitting the new element-wise kernels, the int8 matrix-vector product, the T3 attention and the layer norm across threads (`fastops.rs`, `qlinear.rs`, `t3.rs`) | candle already runs its matmuls on rayon's pool; using the same pool avoids a second set of threads fighting for the 8 vCPUs. |
| `gemm` | 0.19.0 | MIT | https://github.com/sarah-quinones/gemm | called directly (raw-pointer API, strided operands) for the blocked flow-decoder attention (`fastops::attention_qkv`) and the tap-wise conv1d (`fastops::conv1d`) | it is the matmul engine candle itself uses; calling it with strides is what removes the head transposes and the im2col buffer. Pure Rust; its SIMD kernels are selected at run time (no target-cpu flags), as they already were for every candle matmul. |

Transitive licences: unchanged from `swarm/voice-rust` 03d21a9 (no new packages; `cargo metadata` lists the same
package set).

## Algorithms and constants (no code copied)
- `fastops::exp_f`: range reduction by ln 2 and the degree-6 polynomial coefficients of Cephes `expf`
  (Stephen L. Moshier, https://www.netlib.org/cephes/). Only the published numeric coefficients are used; the code
  is our own.
- `fastops::erf_f`: Abramowitz & Stegun, *Handbook of Mathematical Functions* (NBS 1964), formula 7.1.26
  (US government work, public domain).
- `qlinear`: the int8-with-one-f32-scale-per-32-weights layout is the well-known `Q8_0` idea from llama.cpp /
  GGML (MIT, https://github.com/ggml-org/llama.cpp). No code or file format is taken from it; our weights are
  quantised in memory at load from the original safetensors.
- Blocked attention (one query block's scores kept in cache) follows the general idea of FlashAttention
  (Dao et al., 2022); here it is plain blocked softmax with two gemm calls, not their kernel.
