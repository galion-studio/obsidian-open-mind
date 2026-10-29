// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Pinned model and native-library files, and their verification.
//!
//! Every file the service opens is checked against a sha256 recorded here
//! before it is used. The model list is exactly what libmoonshine v0.1.5's
//! own `moonshine_get_stt_dependencies("en", model_arch=2)` manifest names for
//! TINY_STREAMING (CDN folder `tiny-streaming-en/quantized_26_08_21`, i.e. the
//! re-quantised 8-bit files published 2026-08-21, after the 2026-07-30 fix).
//! The manifest's own size and CRC32C are kept too, so a fetch can be
//! checked against the vendor's values and ours.

use sha2::{Digest, Sha256};
use std::path::Path;

/// One pinned file.
#[derive(Debug, Clone, Copy)]
pub struct Pinned {
    pub name: &'static str,
    pub size: u64,
    /// Base64 CRC32C (big-endian) from the vendor manifest; empty if none.
    pub crc32c_b64: &'static str,
    pub sha256: &'static str,
}

/// Moonshine Tiny Streaming English, `quantized_26_08_21`.
pub const MODEL_FILES: &[Pinned] = &[
    Pinned {
        name: "adapter.ort",
        size: 1_319_664,
        crc32c_b64: "kwQ+Bw==",
        sha256: "22ecc949e146c49667fda28d102d4e30749a107dc88a396292aa8f277ef1347c",
    },
    Pinned {
        name: "cross_kv.ort",
        size: 1_287_544,
        crc32c_b64: "76wzFQ==",
        sha256: "143a36667b8d05fd9d04e8c337b7ee121f37ef299aea6b3d82bdb3d3401950b4",
    },
    Pinned {
        name: "decoder_kv.ort",
        size: 32_583_720,
        crc32c_b64: "KJjeNw==",
        sha256: "8852553f312adb6c9aa4d17418015049b30f412209ee569d336548c0044627de",
    },
    Pinned {
        name: "encoder.ort",
        size: 7_675_440,
        crc32c_b64: "UjAIpQ==",
        sha256: "a8414e1a5dedf9f2093d7680601dd8a9b0433e7020260eafe0e370ead91134ca",
    },
    Pinned {
        name: "frontend.model.ort",
        size: 23_344,
        crc32c_b64: "aM/+wQ==",
        sha256: "5121b561417b638afce0c6c31b760e37c93cf97f80d9b0031aad1fe7b6f25d61",
    },
    Pinned {
        name: "frontend.weights.ort",
        size: 2_093_464,
        crc32c_b64: "WYZ7EQ==",
        sha256: "217da24ac6f522ebf02da8ef288e77d1ac68d50d4a6821433182e4fbf4204bbd",
    },
    Pinned {
        name: "streaming_config.json",
        size: 509,
        crc32c_b64: "HGL0Ug==",
        sha256: "74fe5ddebd63b17caf59e8a3b18c17547ff7bce1642050edbb1c3962674f8950",
    },
    Pinned {
        name: "tokenizer.bin",
        size: 249_974,
        crc32c_b64: "B7s10Q==",
        sha256: "6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d",
    },
];

/// Native libraries from the moonshine-ai/moonshine v0.1.5 release asset
/// `moonshine-voice-linux-x86_64.tar.gz` (sha256 9c3a87fe...51df).
/// `libonnxruntime.so.1` is byte-identical to Microsoft's own
/// onnxruntime-linux-x64-1.23.2 `libonnxruntime.so.1.23.2`.
pub const LIB_FILES: &[Pinned] = &[
    Pinned {
        name: "libmoonshine.so",
        size: 12_157_928,
        crc32c_b64: "",
        sha256: "9d516b03b178f01b6f986ae29a6d1b9adc8f49bb6e4d202672b22c0181cc9e00",
    },
    Pinned {
        name: "libonnxruntime.so.1",
        size: 22_326_072,
        crc32c_b64: "",
        sha256: "13ab8084954fa4a47c777880180b90810d6020f021441395712b48a75b74c68b",
    },
];

/// Result of checking one file.
#[derive(Debug, Clone)]
pub struct Checked {
    pub name: &'static str,
    pub sha256: String,
    pub ok: bool,
    pub problem: Option<String>,
}

/// Lower-case hex sha256 of a byte slice.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// CRC32C (Castagnoli, reflected polynomial 0x82F63B78), bitwise.
pub fn crc32c(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82F6_3B78 & mask);
        }
    }
    !crc
}

/// Standard base64 (with padding) of a big-endian u32, as GCS-style
/// manifests print CRC32C values.
pub fn crc32c_b64(crc: u32) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let b = crc.to_be_bytes();
    let n1 = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
    let n2 = u32::from(b[3]) << 16;
    let mut s = String::with_capacity(8);
    for sh in [18, 12, 6, 0] {
        s.push(T[((n1 >> sh) & 63) as usize] as char);
    }
    s.push(T[((n2 >> 18) & 63) as usize] as char);
    s.push(T[((n2 >> 12) & 63) as usize] as char);
    s.push_str("==");
    s
}

/// Check every pinned file in `dir`: size, sha256 and (if pinned) CRC32C.
pub fn verify_dir(dir: &Path, files: &'static [Pinned]) -> Vec<Checked> {
    files
        .iter()
        .map(|p| {
            let path = dir.join(p.name);
            match std::fs::read(&path) {
                Err(e) => Checked {
                    name: p.name,
                    sha256: String::new(),
                    ok: false,
                    problem: Some(format!("read {}: {e}", path.display())),
                },
                Ok(bytes) => {
                    let sha = sha256_hex(&bytes);
                    let mut problem = None;
                    if bytes.len() as u64 != p.size {
                        problem = Some(format!("size {} != pinned {}", bytes.len(), p.size));
                    } else if sha != p.sha256 {
                        problem = Some("sha256 differs from pinned value".into());
                    } else if !p.crc32c_b64.is_empty() && crc32c_b64(crc32c(&bytes)) != p.crc32c_b64
                    {
                        problem = Some("crc32c differs from vendor manifest".into());
                    }
                    Checked {
                        name: p.name,
                        sha256: sha,
                        ok: problem.is_none(),
                        problem,
                    }
                }
            }
        })
        .collect()
}

/// Verify both directories; `Err` lists every failing file.
pub fn verify_all(lib_dir: &Path, model_dir: &Path) -> Result<Vec<Checked>, String> {
    let mut all = verify_dir(lib_dir, LIB_FILES);
    all.extend(verify_dir(model_dir, MODEL_FILES));
    let bad: Vec<String> = all
        .iter()
        .filter(|c| !c.ok)
        .map(|c| format!("{}: {}", c.name, c.problem.as_deref().unwrap_or("?")))
        .collect();
    if bad.is_empty() {
        Ok(all)
    } else {
        Err(bad.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32c_known_vectors() {
        // RFC 3720 B.4 test vectors.
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
        assert_eq!(crc32c(&[0xFFu8; 32]), 0x62A8_AB43);
    }

    #[test]
    fn crc32c_base64_shape() {
        assert_eq!(crc32c_b64(0), "AAAAAA==");
        assert_eq!(crc32c_b64(0xE306_9283), "4waSgw==");
        for p in MODEL_FILES {
            assert_eq!(p.crc32c_b64.len(), 8, "{}", p.name);
            assert_eq!(p.sha256.len(), 64, "{}", p.name);
        }
    }

    #[test]
    fn sha256_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verify_reports_missing_files() {
        let r = verify_all(
            Path::new("/nonexistent-lib"),
            Path::new("/nonexistent-models"),
        );
        let e = r.unwrap_err();
        assert!(e.contains("libmoonshine.so"));
        assert!(e.contains("decoder_kv.ort"));
    }

    #[test]
    fn model_total_is_about_45_mb() {
        let total: u64 = MODEL_FILES.iter().map(|p| p.size).sum();
        assert_eq!(total, 45_233_659);
    }
}
