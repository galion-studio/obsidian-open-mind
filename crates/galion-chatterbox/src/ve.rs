// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Voice encoder: 3-layer LSTM (40 mel -> 256) over 160-frame windows of a
//! 16 kHz clip, final hidden state -> linear -> ReLU -> L2 norm, averaged
//! over windows. Gives T3's 256-dim speaker embedding.

use candle_core::{Device, Module, Result, Tensor};
use candle_nn::{Linear, VarBuilder};

use crate::dsp;

const N_MELS: usize = 40;
const HIDDEN: usize = 256;
const PARTIAL: usize = 160;

struct LstmLayer {
    w_ih: Tensor, // (4H, in)
    w_hh: Tensor, // (4H, H)
    b: Tensor,    // (4H)
}

pub struct VoiceEncoder {
    layers: Vec<LstmLayer>,
    proj: Linear,
    mel: Vec<f32>,
    dev: Device,
}

impl VoiceEncoder {
    pub fn load(vb: VarBuilder) -> Result<Self> {
        let dev = vb.device().clone();
        let l = vb.pp("lstm");
        let mut layers = Vec::new();
        for i in 0..3 {
            let b = (l.get_unchecked(&format!("bias_ih_l{i}"))?
                + l.get_unchecked(&format!("bias_hh_l{i}"))?)?;
            layers.push(LstmLayer {
                w_ih: l.get_unchecked(&format!("weight_ih_l{i}"))?,
                w_hh: l.get_unchecked(&format!("weight_hh_l{i}"))?,
                b,
            });
        }
        Ok(VoiceEncoder {
            layers,
            proj: crate::nnx::linear(&vb.pp("proj"), HIDDEN, 256, true)?,
            mel: dsp::mel_filters_slaney(16000.0, 400, N_MELS, 0.0, 8000.0),
            dev,
        })
    }

    /// Run the LSTM over (B, T, 40) and return the last layer's final h (B, H).
    fn lstm(&self, x: &Tensor) -> Result<Tensor> {
        let (b, t, _) = x.dims3()?;
        let mut seq = x.clone();
        let mut last = None;
        for layer in &self.layers {
            // Input projection for all steps at once: (B, T, 4H).
            let xi = seq
                .broadcast_matmul(&layer.w_ih.t()?)?
                .broadcast_add(&layer.b)?;
            let whh_t = layer.w_hh.t()?.contiguous()?;
            let mut h = Tensor::zeros((b, HIDDEN), candle_core::DType::F32, &self.dev)?;
            let mut c = h.clone();
            let mut outs = Vec::with_capacity(t);
            for s in 0..t {
                let g = (xi.narrow(1, s, 1)?.squeeze(1)? + h.matmul(&whh_t)?)?;
                let i = candle_nn::ops::sigmoid(&g.narrow(1, 0, HIDDEN)?)?;
                let f = candle_nn::ops::sigmoid(&g.narrow(1, HIDDEN, HIDDEN)?)?;
                let gg = g.narrow(1, 2 * HIDDEN, HIDDEN)?.tanh()?;
                let o = candle_nn::ops::sigmoid(&g.narrow(1, 3 * HIDDEN, HIDDEN)?)?;
                c = ((f * &c)? + (i * gg)?)?;
                h = (o * c.tanh()?)?;
                outs.push(h.unsqueeze(1)?);
            }
            seq = Tensor::cat(&outs, 1)?;
            last = Some(h);
        }
        last.ok_or_else(|| candle_core::Error::Msg("empty LSTM".into()))
    }

    /// 256-dim speaker embedding of a 16 kHz clip (trimmed at 20 dB).
    pub fn embed(&self, wav16: &[f32]) -> Result<Vec<f32>> {
        let (a, b) = dsp::trim_range(wav16, 20.0);
        let wav = if b > a { &wav16[a..b] } else { wav16 };
        let spec = dsp::stft(wav, 400, 160, &dsp::hann(400), true, true);
        let mels: Vec<Vec<f32>> = spec
            .iter()
            .map(|p| dsp::apply_mel(&self.mel, N_MELS, p))
            .collect();
        let n = mels.len().max(1);
        // rate 1.3 -> frame step round((16000 / 1.3) / 160) = 77; coverage 0.8.
        let step = 77usize;
        let (mut wins, rem) = (
            (n + step).saturating_sub(PARTIAL) / step,
            (n + step).saturating_sub(PARTIAL) % step,
        );
        if wins == 0 || (rem + PARTIAL - step) as f64 / PARTIAL as f64 >= 0.8 {
            wins += 1;
        }
        let target = PARTIAL + step * (wins - 1);
        let mut flat = vec![0f32; target * N_MELS];
        for (f, m) in mels.iter().enumerate().take(target) {
            flat[f * N_MELS..(f + 1) * N_MELS].copy_from_slice(m);
        }
        let mut parts = Vec::with_capacity(wins * PARTIAL * N_MELS);
        for w in 0..wins {
            parts.extend_from_slice(&flat[w * step * N_MELS..(w * step + PARTIAL) * N_MELS]);
        }
        let x = Tensor::from_vec(parts, (wins, PARTIAL, N_MELS), &self.dev)?;
        let h = self.lstm(&x)?;
        let e = crate::nnx::l2norm(&self.proj.forward(&h)?.relu()?)?;
        let mean = e.mean_keepdim(0)?;
        crate::nnx::l2norm(&mean)?.flatten_all()?.to_vec1::<f32>()
    }
}
