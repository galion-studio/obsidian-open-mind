//! Useful functions for checking features.
use std::str::FromStr;

pub fn get_num_threads() -> usize {
    // Respond to the same environment variable as rayon.
    match std::env::var("RAYON_NUM_THREADS")
        .ok()
        .and_then(|s| usize::from_str(&s).ok())
    {
        Some(x) if x > 0 => x,
        Some(_) | None => num_cpus::get(),
    }
}

pub fn has_accelerate() -> bool {
    cfg!(feature = "accelerate")
}

pub fn has_mkl() -> bool {
    cfg!(feature = "mkl")
}

pub fn cuda_is_available() -> bool {
    cfg!(feature = "cuda")
}

pub fn metal_is_available() -> bool {
    cfg!(feature = "metal")
}

// galion candle-zip-fix (int5-candle-repocheck): the SIMD kernels are removed from this vendored copy
// (UNIVERSAL-CPU.md), so these report `false` on every target. f16c: Galion builds never set
// CPU feature codegen flags (scripts/ucpu-gate.sh, repo-check `portable`), so it is never compiled in.
pub fn with_avx() -> bool {
    false
}

pub fn with_neon() -> bool {
    false
}

pub fn with_simd128() -> bool {
    false
}

pub fn with_f16c() -> bool {
    false
}
