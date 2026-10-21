// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! op-bench [T]: times the candle CPU operations the flow decoder and T3 use,
//! at their real shapes, to find where the mouth spends its time.

use std::time::Instant;

use candle_core::{DType, Device, Module, Tensor};

fn time<F: FnMut() -> candle_core::Result<Tensor>>(name: &str, reps: usize, mut f: F) {
    let _ = f().unwrap();
    let t = Instant::now();
    for _ in 0..reps {
        let _ = f().unwrap();
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / reps as f64;
    println!("{name:<48} {ms:>9.3} ms");
}

fn main() {
    let t: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(630);
    let dev = Device::Cpu;
    let r = |s: &[usize]| Tensor::randn(0f32, 1.0, s, &dev).unwrap();
    let x256 = r(&[2, t, 256]);
    let x1024 = r(&[2, t, 1024]);
    let c256 = r(&[2, 256, t]);
    let w = r(&[512, 256]);
    let lin = candle_nn::Linear::new(w, None);
    let ln = candle_nn::LayerNorm::new(r(&[256]), r(&[256]), 1e-5);
    let s = r(&[2, 8, t, t]);
    let q = r(&[2, 8, t, 64]);
    let cw = r(&[256, 256, 3]);
    let mask = Tensor::ones((2, 1, t), DType::F32, &dev).unwrap();
    println!("T = {t}");
    time("linear (2,T,256)->512", 20, || lin.forward(&x256));
    time("gelu_erf (2,T,1024)", 20, || x1024.gelu_erf());
    time("gelu tanh (2,T,1024)", 20, || x1024.gelu());
    time("silu (2,T,1024)", 20, || x1024.silu());
    time("affine scale (2,8,T,T)", 10, || s.affine(0.125, 0.0));
    time("softmax_last_dim (2,8,T,T)", 10, || {
        candle_nn::ops::softmax_last_dim(&s)
    });
    time("matmul q k^T (2,8,T,64)x(64,T)", 10, || {
        q.matmul(&q.t()?.contiguous()?)
    });
    time("matmul p v (2,8,T,T)x(T,64)", 10, || s.matmul(&q));
    time("fast softmax_scaled (2,8,T,T)", 10, || {
        galion_chatterbox::fastops::softmax_scaled(&s, 0.125)
    });
    time("fast gelu_erf (2,T,1024)", 20, || {
        galion_chatterbox::fastops::gelu_erf(&x1024)
    });
    let kc = q.t().unwrap().contiguous().unwrap();
    time("matmul q x kT(contig, prepared)", 10, || q.matmul(&kc));
    time("matmul q x k.t() strided", 10, || q.matmul(&q.t()?));
    time("k.t().contiguous()", 10, || q.t()?.contiguous());
    let q16 = q.reshape((16, t, 64)).unwrap();
    let k16 = kc.reshape((16, 64, t)).unwrap();
    time("matmul 3d (16,T,64)x(16,64,T)", 10, || q16.matmul(&k16));
    time("heads (2,T,512)->(2,8,T,64)", 20, || {
        galion_chatterbox::nnx::heads(&r(&[2, t, 512]), 8)
    });
    time("layer_norm contiguous (2,T,256)", 20, || ln.forward(&x256));
    time("layer_norm transposed (2,256,T)^T", 20, || {
        ln.forward(&c256.transpose(1, 2)?)
    });
    time("mish (2,256,T)", 20, || galion_chatterbox::nnx::mish(&c256));
    time("mask mul (2,256,T)", 20, || c256.broadcast_mul(&mask));
    time("conv1d k3 256->256 pad2 (2,256,T)", 20, || {
        c256.pad_with_zeros(2, 2, 0)?.conv1d(&cw, 0, 1, 1, 1)
    });
    time("transpose+contiguous (2,256,T)", 20, || {
        c256.transpose(1, 2)?.contiguous()
    });
    // T3 decode shapes
    let wt3 = r(&[4096, 1024]);
    let l3 = candle_nn::Linear::new(wt3, None);
    let x2 = r(&[2, 1, 1024]);
    let x1 = r(&[1, 1, 1024]);
    time("T3 linear (2,1,1024)->4096", 50, || l3.forward(&x2));
    time("T3 linear (1,1,1024)->4096", 50, || l3.forward(&x1));
    let qn = galion_chatterbox::qlinear::QLinear::from_linear(&l3).unwrap();
    time("T3 q8 linear (2,1,1024)->4096", 50, || {
        candle_core::Module::forward(&qn, &x2)
    });
    let big = candle_nn::Linear::new(r(&[8192, 1024]), None);
    let qbig = galion_chatterbox::qlinear::QLinear::from_linear(&big).unwrap();
    time("T3 f32 linear (2,1,1024)->8192", 50, || big.forward(&x2));
    time("T3 q8 linear (2,1,1024)->8192", 50, || {
        candle_core::Module::forward(&qbig, &x2)
    });
    // 30 layers of distinct weights so the caches cannot hold them (decode-like).
    let many: Vec<_> = (0..30)
        .map(|_| candle_nn::Linear::new(r(&[8192, 1024]), None))
        .collect();
    let qmany: Vec<_> = many
        .iter()
        .map(|l| galion_chatterbox::qlinear::QLinear::from_linear(l).unwrap())
        .collect();
    let mut i = 0;
    time("f32 8192x1024, 30 distinct weights", 60, || {
        i = (i + 1) % 30;
        many[i].forward(&x2)
    });
    time("q8 8192x1024, 30 distinct weights", 60, || {
        i = (i + 1) % 30;
        candle_core::Module::forward(&qmany[i], &x2)
    });
}
