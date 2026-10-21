<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# voice-speed: making the CPU mouth (Chatterbox in Rust) fast enough

Swarm slot 147, branch `swarm/voice-speed`, based on `swarm/voice-rust` 03d21a9. Ports 20170-20179.
Started 2026-10-21 06:53 CEST (+02:00). Rust only, no new crates in `Cargo.lock`, no asm, no intrinsics, no
target-cpu flags. Nothing deployed.

Target from the task: **under 15 s of wall time for 3 s of speech** on CPU (voice-rust measured 25-34 s with the
reference settings and 15.6-19.2 s with the "fast" profile).

## 1. Where the time went (measured, not guessed)
A small opt-in profiler (`CHATTERBOX_PROF=1`, `src/prof.rs`) and a micro-benchmark (`examples/op-bench.rs`) on
the real shapes (flow decoder T = 630 frames, batch 2 for classifier-free guidance) showed, for the built-in voice,
5 flow steps, before any change (06:59, load 1.6):

| Part | Time | Cause |
|---|---|---|
| flow, 280 transformer blocks | 10.5 s | per block: `gelu_erf` 10.5 ms (candle runs it on 1 thread with libm erf), score scaling + softmax 10 ms on a (2,8,T,T) tensor in RAM, head transposes, then the matmuls |
| T3 decode, 65 tokens | 5.5 s (84 ms/token) | 2.1 GB of f32 weights read per token for a batch of 2 rows (bandwidth-bound), plus ~25 small candle ops and 64 one-row gemm calls per layer for attention |
| flow resnets | 1.8 s | transposed layer norm (generic multi-op path), single-threaded Mish, all-ones mask multiplies |
| HiFT | 1.4 s | candle conv1d's im2col buffer (43 MB per conv in the last stage) and single-threaded Snake |

## 2. What changed
| Change | File | Output |
|---|---|---|
| Multi-threaded, branch-free `exp`/`erf` kernels for gelu (erf form), Mish, SiLU, scaled softmax | `fastops.rs` | same function; error < 1e-6 relative (unit tests vs f64) |
| Channel layer norm on (B,C,T) without the two transposes | `fastops.rs`, `flow.rs` | equal to candle (test, 1e-5) |
| Flow decoder: q/k/v stacked into one matmul; **blocked attention** (one 64-query block per task: gemm -> softmax in cache -> gemm straight into the merged-head output; no (B,H,T,T) tensor, no transposes) | `fastops::attention_qkv`, `flow.rs` | equal to heads+sdpa+merge (test, 1e-5) |
| Flow decoder: dropped the multiplications by the padding mask (always all ones at inference: one utterance + its CFG copy) | `flow.rs` | identical |
| conv1d (stride 1) as one strided gemm per kernel tap instead of im2col | `fastops::conv1d`, `nnx.rs` | equal to candle (test with dilation/padding/bias) |
| Snake in one parallel pass (libm sin) | `fastops::Snake` | same formula |
| **T3 int8 weights** (`Q8`: int8 + one f32 scale per 32 weights, 0.6 GB instead of 2.1 GB), q/k/v and gate/up stacked; prefill (> 8 rows) expands to f32 and uses candle matmul | `qlinear.rs`, `t3.rs` | numerics change slightly (weights within half a quantisation step); default, `--t3-weights f32` keeps the checkpoint numerics |
| T3 KV cache + attention in plain Rust (append-only `[pos][batch][head*64]`, RoPE, causal softmax), parallel over (row, head) | `t3.rs` `KvStore` | equal to candle rope+sdpa (test, 1e-4) |
| `--t3-weights q8|f32` / `GALION_VOICE_T3_WEIGHTS` on `galion-voice-serve`; load note says which | `galion-voice-cpu` | default q8 |

Kernels are portable Rust written so the compiler can auto-vectorise them for the baseline target, and run on
rayon's pool. The two direct uses of `gemm` (already candle's matmul engine) are raw-pointer calls with SAFETY
comments; that and the existing mmap are the only `unsafe` in the crate.

## 3. Results (shared 8-vCPU build box; other agents were running, so the load average is given)
`chatterbox-say`, built-in voice, seed 1, "Good evening. All systems are running normally." (~2.6 s of speech):

| Build | Flow steps | Wall | T3 | Flow | HiFT | Load |
|---|---|---|---|---|---|---|
| before (03d21a9) | 10 | 30.9 s | 6.5 s | 22.9 s | 1.4 s | 1.1 |
| before | 5 | 19.4 s | 6.3 s | 11.6 s | 1.5 s | 2.7 |
| after, q8 | 10 | **13.1 s** | 3.1 s | 9.0 s | 0.9 s | 2.0 |
| after, q8 | 5 | **8.1 s** | 3.5 s | 3.8 s | 0.7 s | 2.5 |

Over HTTP (`real_voice_speed`, five lines, 2.2-3.3 s each, see docs/real-tests/voice-speed.md):
- q8, 10 steps (reference flow settings): 12.3-14.2 s per line, **mean 14.1 s per 3 s of speech**, 4 of 5
  lines under the 15 s target (the miss is the shortest line, 2.24 s, 13.5 s); load 2-6.
- f32 T3, 10 steps: 17.6-22.2 s per line (mean 21.2 s per 3 s); the gain over q8 is mostly T3.
- q8, 5 steps: 11.3-13.6 s per line while the box load was 8-9 (another agent's voice test ran at the same
  time; CPU time per line was 31-40 s vs 52-63 s for 10 steps).
- Transcripts through the legacy ear (whisper large-v3-turbo) are identical across the three profiles (98.2 %
  word match; the only "miss" is "ten" written "10"). Before this work "carry" was heard as "Carrie" on line 2;
  now it is heard correctly in all profiles.
- Peak RSS of the mouth: **2.77 GB with q8** vs 4.07 GB with f32 (voice-rust: 4.16 GB).

## 4. Limits and what is left
- The flow decoder is now bound by its matmuls (~126 GMAC per step at T = 630, batch 2). The remaining big
  lever is input size, which changes output: the S3Gen prompt cap (`--prompt-seconds`, default 10 s of reference
  audio = 500 of the 630 frames for the built-in voice) and the number of flow steps. Both stay at the reference
  values by default; see section 6.
- The int8 T3 kernel runs at about 3.5-4 G weights/s per thread; an int16-activation variant measured 1.4x faster
  in a scratch benchmark (not merged; small gain under load).
- Wall times depend heavily on other agents on this box (same line: 12.3 s at load 3, 26.7 s at load 9).
- The VPS is an AMD Ryzen 7 3800X (AVX2/FMA, no AVX-512; 8 cores / 16 threads, 16 GB). gemm picks AVX2 there at
  run time; the int8/element-wise kernels are baseline code on both. The mouth was **not** run on the VPS
  (read-only permission); numbers there are a projection, not a measurement.
- Perth watermark, real samantha/jarvis clips, PyTorch parity: unchanged gaps from docs/voice-rust.md.

## 5. How to reproduce
```
CHATTERBOX_PROF=1 cargo run --release -p galion-chatterbox --example chatterbox-say -- \
    <models>/chatterbox "Good evening. All systems are running normally." out.wav --seed 1 [--steps 10] [--t3 q8|f32]
cargo run --release -p galion-chatterbox --example op-bench -- 630
GALION_REAL=1 cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox \
    --test real_voice_speed -- --nocapture --test-threads 1      # VS_PROFILES=q8:10,f32:10,q8:5
```

## 6. Decisions for Slavik
1. **Default T3 weights = q8** (rec: yes; 1.5x faster end to end, 1.3 GB less RAM, same transcripts in the test).
   Alternative: default f32 and opt-in q8.
2. **Latency profile for production**: (a) reference 10 flow steps, ~14 s per 3 s here (rec. while speed is
   enough); (b) 5 steps (~8-12 s); (c) shorter `--prompt-seconds` (e.g. 6) for a further ~30 % on the flow.
   (b)/(c) change the sound slightly and should be judged by ear with the real samantha clip.
3. **Permission for one timed run on the VPS** (static musl binary, ordinary user, `/var/tmp/voice-speed-<ts>`,
   no service touched, then removed) to replace the projection in 4 with a real-server number.
4. Still open from voice-rust: real voice clips + licence, Perth watermark, the per-package `opt-level = 3`.
