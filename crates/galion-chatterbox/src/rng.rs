// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Small deterministic RNG (xoshiro256** seeded through splitmix64) for token
//! sampling and flow-matching noise. Seedable, so a run can be reproduced.

pub struct Rng {
    s: [u64; 4],
    spare: Option<f32>,
}

fn splitmix(x: &mut u64) -> u64 {
    *x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut x = seed;
        let s = [
            splitmix(&mut x),
            splitmix(&mut x),
            splitmix(&mut x),
            splitmix(&mut x),
        ];
        Rng { s, spare: None }
    }

    /// Seed from the clock and the process id (used when no seed is given).
    pub fn from_entropy() -> Self {
        let t = std::time::SystemTime::now() // repo-check: allow (seed only, never printed)
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Rng::new(t ^ ((std::process::id() as u64) << 32))
    }

    pub fn next_u64(&mut self) -> u64 {
        let r = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        r
    }

    /// Uniform in [0, 1).
    pub fn uniform(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Standard normal (Box-Muller).
    pub fn normal(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let mut u1 = self.uniform();
        while u1 <= f64::MIN_POSITIVE {
            u1 = self.uniform();
        }
        let u2 = self.uniform();
        let r = (-2.0 * u1.ln()).sqrt();
        let th = 2.0 * std::f64::consts::PI * u2;
        self.spare = Some((r * th.sin()) as f32);
        (r * th.cos()) as f32
    }

    pub fn normals(&mut self, n: usize) -> Vec<f32> {
        (0..n).map(|_| self.normal()).collect()
    }

    /// Draw an index from unnormalised non-negative weights.
    pub fn categorical(&mut self, w: &[f32]) -> usize {
        let total: f64 = w.iter().map(|&x| x.max(0.0) as f64).sum();
        if total <= 0.0 || !total.is_finite() {
            return w
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map_or(0, |(i, _)| i);
        }
        let mut r = self.uniform() * total;
        for (i, &x) in w.iter().enumerate() {
            r -= x.max(0.0) as f64;
            if r < 0.0 {
                return i;
            }
        }
        w.iter().rposition(|&x| x > 0.0).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_sane() {
        let (mut a, mut b) = (Rng::new(7), Rng::new(7));
        assert_eq!(a.next_u64(), b.next_u64());
        let v = a.normals(20_000);
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        let var = v.iter().map(|x| (x - mean) * (x - mean)).sum::<f32>() / v.len() as f32;
        assert!(mean.abs() < 0.03, "{mean}");
        assert!((var - 1.0).abs() < 0.05, "{var}");
        let mut hits = [0usize; 3];
        for _ in 0..3000 {
            hits[a.categorical(&[1.0, 0.0, 3.0])] += 1;
        }
        assert_eq!(hits[1], 0);
        assert!(hits[2] > hits[0] * 2);
    }
}
