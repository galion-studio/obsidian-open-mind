// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Small helpers: hex, randomness, bounded subprocess calls, file reads.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

pub fn random_bytes<const N: usize>() -> std::io::Result<[u8; N]> {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(b)
}

/// Run a helper program (iw, nmcli, ip, termux-*) with a hard timeout.
/// Returns stdout on exit status 0; None if missing, failing or too slow.
pub fn run_cmd(prog: &str, args: &[&str], timeout: Duration) -> Option<String> {
    let mut child = Command::new(prog)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let mut out = String::new();
                child
                    .stdout
                    .take()?
                    .take(1 << 20)
                    .read_to_string(&mut out)
                    .ok()?;
                return st.success().then_some(out);
            }
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

pub fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// True when running inside Termux on Android.
pub fn is_termux() -> bool {
    std::env::var("PREFIX")
        .map(|p| p.contains("com.termux"))
        .unwrap_or(false)
        || std::env::var_os("TERMUX_VERSION").is_some()
        || std::path::Path::new("/system/build.prop").exists()
}

/// Peak resident set size of this process (VmHWM) in KiB, from /proc.
pub fn vm_hwm_kb() -> Option<u64> {
    read_file("/proc/self/status")?
        .lines()
        .find(|l| l.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hex_roundtrip() {
        let b = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&b), "0001abff");
        assert_eq!(unhex("0001abff").unwrap(), b);
        assert!(unhex("abc").is_none());
        assert!(unhex("zz").is_none());
    }
    #[test]
    fn run_cmd_missing_program_is_none() {
        assert!(run_cmd("/nonexistent/omnode-test", &[], Duration::from_secs(1)).is_none());
    }
    #[test]
    fn run_cmd_timeout() {
        let t = Instant::now();
        assert!(run_cmd("sleep", &["5"], Duration::from_millis(200)).is_none());
        assert!(t.elapsed() < Duration::from_secs(3));
    }
}

/// Unpadded base64url (RFC 4648 section 5), used to keep QR payloads small.
pub fn b64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    s
}

#[cfg(test)]
mod b64_tests {
    #[test]
    fn rfc4648_vectors() {
        assert_eq!(super::b64url(b""), "");
        assert_eq!(super::b64url(b"f"), "Zg");
        assert_eq!(super::b64url(b"fo"), "Zm8");
        assert_eq!(super::b64url(b"foo"), "Zm9v");
        assert_eq!(super::b64url(b"foobar"), "Zm9vYmFy");
        assert_eq!(super::b64url(&[0xfb, 0xff]), "-_8");
    }
}
