// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Linear-interpolation resampler (portable, scalar). Good enough for speech
//! into STT (24 kHz -> 16 kHz) and TTS out (16/22.05 kHz -> 24 kHz); a
//! windowed-sinc filter is a later quality step.

pub fn linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() {
        return input.to_vec();
    }
    let out_len = (input.len() as u64 * to_hz as u64 / from_hz as u64) as usize;
    let step = from_hz as f64 / to_hz as f64;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let j = pos as usize;
            let frac = (pos - j as f64) as f32;
            let a = input[j.min(input.len() - 1)];
            let b = input[(j + 1).min(input.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

pub fn i16_to_f32(s: &[i16]) -> Vec<f32> {
    s.iter().map(|&v| v as f32 / 32768.0).collect()
}

pub fn f32_to_i16(s: &[f32]) -> Vec<i16> {
    s.iter()
        .map(|&v| (v.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_and_identity() {
        let x: Vec<f32> = (0..2400).map(|i| (i as f32 * 0.01).sin()).collect();
        assert_eq!(linear(&x, 24_000, 16_000).len(), 1600);
        assert_eq!(linear(&x, 16_000, 24_000).len(), 3600);
        assert_eq!(linear(&x, 8, 8), x);
        let y = linear(&x, 24_000, 16_000);
        // sample 3 at 16 kHz = sample 4.5 at 24 kHz
        assert!((y[3] - (x[4] + x[5]) / 2.0).abs() < 1e-6);
        assert_eq!(f32_to_i16(&i16_to_f32(&[1000, -1000])), vec![999, -999]);
    }
}
