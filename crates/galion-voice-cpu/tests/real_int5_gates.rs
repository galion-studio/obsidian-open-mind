// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (slot 211, int5-candle-repocheck; ports 20810-20819): the
//! voice service still hears and speaks with the SIMD-free vendored
//! candle-core, and the release binary is stripped.
//!
//! Starts the real `galion-voice-serve` on 127.0.0.1:20810 (or
//! `GALION_INT5_PORT`), waits for `/health` 200, transcribes the JFK sample
//! (Whisper tiny: candle matmul + softmax + vec kernels) and synthesises a
//! sentence (KittenTTS), then kills it. With `GALION_VOICE_SERVE_BIN` and
//! `GALION_VOICE_SERVE_RUNNER` (for example an aarch64 musl build run under
//! `qemu-aarch64`) the same checks run against another architecture's binary,
//! which is where the NEON kernels used to be compiled in.
//!
//! The binary must have no `.symtab` section when it is a release build.
//! Runs only with GALION_REAL=1 and the models in GALION_VOICE_MODELS
//! (default /workspace/oom-models).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn http(addr: &str, method: &str, path: &str, ctype: &str, body: &[u8]) -> (u16, Vec<u8>) {
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(1800))).unwrap();
    let head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes()).unwrap();
    s.write_all(body).unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n").expect("head") + 4;
    let code = String::from_utf8_lossy(&buf[9..12]).parse().unwrap();
    (code, buf[end..].to_vec())
}

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}
fn u32_at(b: &[u8], o: usize) -> usize {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize
}
fn u64_at(b: &[u8], o: usize) -> usize {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) as usize
}

/// Section names of a 64-bit little-endian ELF file (x86_64, aarch64).
fn elf64_section_names(elf: &[u8]) -> Vec<String> {
    assert_eq!(&elf[..4], b"\x7fELF", "not an ELF file");
    assert_eq!(elf[4], 2, "not ELF64");
    assert_eq!(elf[5], 1, "not little-endian");
    let shoff = u64_at(elf, 0x28);
    let shentsize = u16_at(elf, 0x3a);
    let shnum = u16_at(elf, 0x3c);
    let shstrndx = u16_at(elf, 0x3e);
    let sh = |i: usize| shoff + i * shentsize;
    let strtab = u64_at(elf, sh(shstrndx) + 0x18);
    (0..shnum)
        .map(|i| {
            let name = strtab + u32_at(elf, sh(i));
            let len = elf[name..].iter().position(|&c| c == 0).unwrap();
            String::from_utf8_lossy(&elf[name..name + len]).into_owned()
        })
        .collect()
}

#[test]
fn elf_reader_sees_symtab_in_an_unstripped_file() {
    // The test binary itself is built with debug info / symbols.
    let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    if me[4] == 2 && me[5] == 1 && cfg!(debug_assertions) {
        assert!(elf64_section_names(&me).iter().any(|n| n == ".symtab"));
    }
}

#[test]
fn real_simd_free_voice_hears_and_speaks() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let models =
        std::env::var("GALION_VOICE_MODELS").unwrap_or_else(|_| "/workspace/oom-models".into());
    let port = std::env::var("GALION_INT5_PORT").unwrap_or_else(|_| "20810".into());
    let p: u16 = port.parse().expect("port");
    assert!(
        (20810..=20819).contains(&p),
        "slot 211 owns 20810-20819 only"
    );
    let addr = format!("127.0.0.1:{p}");
    let bin = std::env::var("GALION_VOICE_SERVE_BIN")
        .unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-voice-serve").into());
    let runner = std::env::var("GALION_VOICE_SERVE_RUNNER").unwrap_or_default();

    let elf = std::fs::read(&bin).expect("read binary");
    let sections = elf64_section_names(&elf);
    let release = std::env::var("GALION_VOICE_SERVE_BIN").is_ok() || cfg!(not(debug_assertions));
    eprintln!(
        "binary {bin}: {} bytes, {} sections, .symtab {}",
        elf.len(),
        sections.len(),
        sections.iter().any(|n| n == ".symtab")
    );
    if release {
        assert!(
            !sections.iter().any(|n| n == ".symtab"),
            "release binary not stripped"
        );
    }

    let mut cmd = if runner.is_empty() {
        Command::new(&bin)
    } else {
        let mut c = Command::new(&runner);
        c.arg(&bin);
        c
    };
    let mut child = cmd
        .args(["--bind", &addr, "--models", &models])
        .env("RAYON_NUM_THREADS", "2")
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn galion-voice-serve");
    let t0 = Instant::now();
    let result = std::panic::catch_unwind(|| {
        let mut up = false;
        while t0.elapsed() < Duration::from_secs(60) {
            if TcpStream::connect(&addr).is_ok() {
                up = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(up, "service did not start");
        let (mut code, mut body) = http(&addr, "GET", "/health", "text/plain", b"");
        while code == 503 && t0.elapsed() < Duration::from_secs(900) {
            std::thread::sleep(Duration::from_millis(500));
            (code, body) = http(&addr, "GET", "/health", "text/plain", b"");
        }
        assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
        eprintln!("health 200 after {} ms", t0.elapsed().as_millis());

        let wav = std::fs::read(format!("{models}/whisper-tiny.en/samples_jfk.wav")).unwrap();
        let t = Instant::now();
        let (code, body) = http(&addr, "POST", "/v1/audio/transcriptions", "audio/wav", &wav);
        let text = String::from_utf8_lossy(&body).to_string();
        eprintln!("ear {} ms: {code} {text}", t.elapsed().as_millis());
        assert_eq!(code, 200, "{text}");
        assert!(
            text.contains("ask not what your country can do for you"),
            "{text}"
        );

        let t = Instant::now();
        let (code, body) = http(
            &addr,
            "POST",
            "/synthesize",
            "application/json",
            br#"{"text":"Hello from the Galion mouth."}"#,
        );
        eprintln!(
            "mouth {} ms: {code}, {} bytes",
            t.elapsed().as_millis(),
            body.len()
        );
        assert_eq!(code, 200);
        assert_eq!(&body[..4], b"RIFF");
        assert!(body.len() > 44 + 24_000, "under half a second of audio");
    });
    let _ = child.kill();
    let _ = child.wait();
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}
