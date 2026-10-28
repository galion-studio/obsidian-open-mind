// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// PDF "Standard" security handler, RC4 revisions 2 and 3 (/V 1 or 2), for
// files that open WITHOUT a password (empty user password): the common
// "protected" PDF that only restricts printing or copying. Std only,
// portable Rust: MD5 (RFC 1321) and RC4 written from their public
// descriptions; ISO 32000-1 section 7.6.3, algorithms 1, 2, 4 and 5.
//
// This is decoding, not security: the key comes from the file itself. A file
// that needs a real user password, or uses AES (/V 4 or 5), is reported as
// encrypted and left to the optional pdftotext fallback.

const PAD: [u8; 32] = [
    0x28, 0xbf, 0x4e, 0x5e, 0x4e, 0x75, 0x8a, 0x41, 0x64, 0x00, 0x4e, 0x56, 0xff, 0xfa, 0x01, 0x08,
    0x2e, 0x2e, 0x00, 0xb6, 0xd0, 0x68, 0x3e, 0x80, 0x2f, 0x0c, 0xa9, 0xfe, 0x64, 0x53, 0x69, 0x7a,
];

/// MD5 (RFC 1321). The sine table is computed the way the RFC defines it
/// (floor(abs(sin(i + 1)) * 2^32)); the tests pin the RFC test vectors.
pub fn md5(data: &[u8]) -> [u8; 16] {
    const S: [u32; 16] = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21];
    let k: Vec<u32> = (0..64)
        .map(|i| ((i as f64 + 1.0).sin().abs() * 4_294_967_296.0) as u32)
        .collect();
    let mut st: [u32; 4] = [0x6745_2301, 0xefcd_ab89, 0x98ba_dcfe, 0x1032_5476];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    for blk in 0..msg.len() / 64 {
        let block = &msg[blk * 64..blk * 64 + 64];
        let m: [u32; 16] = std::array::from_fn(|i| {
            u32::from_le_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ])
        });
        let [mut a, mut b, mut c, mut d] = st;
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | (!b & d), i),
                1 => ((d & b) | (!d & c), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let t = d;
            d = c;
            c = b;
            let s = S[(i / 16) * 4 + i % 4];
            b = b.wrapping_add(
                a.wrapping_add(f)
                    .wrapping_add(k[i])
                    .wrapping_add(m[g])
                    .rotate_left(s),
            );
            a = t;
        }
        st[0] = st[0].wrapping_add(a);
        st[1] = st[1].wrapping_add(b);
        st[2] = st[2].wrapping_add(c);
        st[3] = st[3].wrapping_add(d);
    }
    let mut out = [0u8; 16];
    for (i, w) in st.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

/// RC4 keystream XOR (encrypt == decrypt).
pub fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    if key.is_empty() {
        return data.to_vec();
    }
    let mut s: [u8; 256] = [0; 256];
    for (i, v) in s.iter_mut().enumerate() {
        *v = i as u8;
    }
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(key[i % key.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    data.iter()
        .map(|b| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(s[i as usize]);
            s.swap(i as usize, j as usize);
            b ^ s[s[i as usize].wrapping_add(s[j as usize]) as usize]
        })
        .collect()
}

/// The file key for the empty user password, if the file really opens with
/// it (algorithm 2 + check by algorithm 4 / 5). `None` = a password is
/// needed or the parameters are unsupported.
pub fn empty_password_key(
    r: i64,
    length_bits: i64,
    o: &[u8],
    u: &[u8],
    p: i64,
    id0: &[u8],
) -> Option<Vec<u8>> {
    if !(2..=3).contains(&r) || o.len() < 32 || u.len() < 32 {
        return None;
    }
    let n = if r == 2 {
        5
    } else {
        (length_bits / 8).clamp(5, 16) as usize
    };
    let mut buf = PAD.to_vec();
    buf.extend_from_slice(&o[..32]);
    buf.extend_from_slice(&(p as i32).to_le_bytes());
    buf.extend_from_slice(id0);
    let mut key = md5(&buf).to_vec();
    if r >= 3 {
        for _ in 0..50 {
            key = md5(&key[..n]).to_vec();
        }
    }
    key.truncate(n);
    let ok = if r == 2 {
        rc4(&key, &PAD) == u[..32]
    } else {
        let mut b = PAD.to_vec();
        b.extend_from_slice(id0);
        let mut x = rc4(&key, &md5(&b));
        for i in 1..=19u8 {
            let k: Vec<u8> = key.iter().map(|v| v ^ i).collect();
            x = rc4(&k, &x);
        }
        x[..16] == u[..16]
    };
    ok.then_some(key)
}

/// Per-object key (algorithm 1, RC4).
pub fn object_key(file_key: &[u8], num: u32, gen: u16) -> Vec<u8> {
    let mut b = file_key.to_vec();
    b.extend_from_slice(&num.to_le_bytes()[..3]);
    b.extend_from_slice(&gen.to_le_bytes());
    let h = md5(&b);
    h[..(file_key.len() + 5).min(16)].to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn md5_rfc1321_vectors() {
        for (input, want) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(hex(&md5(input.as_bytes())), want, "{input:?}");
        }
    }

    #[test]
    fn md5_matches_md5sum_when_installed() {
        let _exec = crate::host::EXEC_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        use std::io::Write;
        use std::process::{Command, Stdio};
        let data: Vec<u8> = (0..10_000u32).map(|i| (i * 7 + i / 13) as u8).collect();
        let Ok(mut child) = Command::new("md5sum")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
        else {
            eprintln!("SKIP: md5sum not installed");
            return;
        };
        child.stdin.take().unwrap().write_all(&data).unwrap();
        let out = child.wait_with_output().unwrap();
        let want = String::from_utf8_lossy(&out.stdout);
        assert_eq!(hex(&md5(&data)), want.split_whitespace().next().unwrap());
    }

    #[test]
    fn rc4_known_vectors() {
        // Classic test vectors (Wikipedia "RC4", Test vectors).
        assert_eq!(hex(&rc4(b"Key", b"Plaintext")), "bbf316e8d940af0ad3");
        assert_eq!(hex(&rc4(b"Wiki", b"pedia")), "1021bf0420");
        assert_eq!(
            hex(&rc4(b"Secret", b"Attack at dawn")),
            "45a01f645fc35b383552544b9bf5"
        );
        let x = rc4(b"k", b"round trip");
        assert_eq!(rc4(b"k", &x), b"round trip");
    }
}
