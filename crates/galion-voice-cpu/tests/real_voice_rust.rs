// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the Rust speech service (swarm slug voice-rust), the
//! Rust port of the former scripts/real/voice-rust.sh. GALION_REAL=1 only, and
//! run it with `--release` (candle in a debug build is far too slow):
//!
//! ```text
//! GALION_REAL=1 [ONE_BIN=/path/to/galion-one] [MOUTH_ARGS="--flow-steps 5 --prompt-seconds 4"] \
//!   cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox \
//!   --test real_voice_rust -- --nocapture
//! ```
//!
//! * ear   127.0.0.1:19371  `--stt whisper-large-v3-turbo --tts none`
//! * mouth 127.0.0.1:19372  `--tts chatterbox --stt none --voices $GALION_VOICE_VOICES`
//! * with `ONE_BIN`: galion-one (integration-1 8fd26f6) on 19374, its mouth
//!   proxy on 19375, `GALION_MOUTH`/`GALION_EAR` pointed at the two above, a
//!   temp `GALION_HOME` and a throw-away pairing store so `/hear` can be used.
//!
//! Checks: 503 "loading" while loading, 200 when loaded with the legacy
//! fields; Samantha lines through `/synthesize` and back through the ear
//! (word match, latency, X-RTF/X-Duration); legacy error contract; Galion One
//! end to end. Prints a markdown report (also written to
//! `$GALION_REAL_REPORT` when set). Every child process is killed on exit.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EAR: &str = "127.0.0.1:19371";
const MOUTH: &str = "127.0.0.1:19372";
const ONE: &str = "127.0.0.1:19374";
const ONE_MOUTH: &str = "127.0.0.1:19375";

/// Kills its children when dropped (also on panic).
struct Kids(Vec<Child>);
impl Drop for Kids {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

struct Resp {
    code: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}
impl Resp {
    fn header(&self, k: &str) -> String {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(k))
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).to_string()
    }
}

fn http(addr: &str, method: &str, path: &str, extra: &[(&str, &str)], body: &[u8]) -> Option<Resp> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(180))).ok()?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for (k, v) in extra {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes()).ok()?;
    s.write_all(body).ok()?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).ok()?;
    let end = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&buf[..end]).to_string();
    let mut lines = head.lines();
    let code = lines.next()?.split_whitespace().nth(1)?.parse().ok()?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Some(Resp {
        code,
        headers,
        body: buf[end + 4..].to_vec(),
    })
}

fn get(addr: &str, path: &str) -> Option<Resp> {
    http(addr, "GET", path, &[], b"")
}

fn multipart(wav: &[u8]) -> (String, Vec<u8>) {
    let mut b = b"--rtb\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n".to_vec();
    b.extend_from_slice(wav);
    b.extend_from_slice(b"\r\n--rtb\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n--rtb\r\nContent-Disposition: form-data; name=\"fast\"\r\n\r\n1\r\n--rtb--\r\n");
    ("multipart/form-data; boundary=rtb".into(), b)
}

fn json_text(body: &str, key: &str) -> String {
    galion_voice_cpu::serve::json_str(body, key).unwrap_or_default()
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

/// Share of the script's words present in the transcript (bag of words).
fn word_match(script: &str, heard: &str) -> f64 {
    let want = words(script);
    let mut got = words(heard);
    let mut hit = 0;
    for w in &want {
        if let Some(i) = got.iter().position(|g| g == w) {
            got.swap_remove(i);
            hit += 1;
        }
    }
    100.0 * hit as f64 / want.len().max(1) as f64
}

fn wait_healthy(addr: &str, secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if get(addr, "/health").is_some_and(|r| r.code == 200) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    false
}

fn short(body: &str) -> String {
    body.split(",\"rss_kb\"").next().unwrap_or(body).to_string()
}

/// SHA-256 (FIPS 180-4), for the throw-away Galion One pairing store.
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_known_answers() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert!((word_match("Hi. I am here", "hi i am HERE") - 100.0).abs() < 1e-9);
}

const LINES: [(&str, &str); 4] = [
    (
        "samantha",
        "Hi. I am right here with you, and I am listening.",
    ),
    (
        "samantha",
        "That sounds like a lot to carry. Do you want to talk about it?",
    ),
    ("jarvis", "Good evening. All systems are running normally."),
    (
        "samantha",
        "I love the way the light looks this time of day.",
    ),
];

#[test]
fn real_mouth_ear_and_galion_one() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 (and build with --release)");
        return;
    }
    let env = |k: &str, d: &str| {
        std::env::var(k)
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| d.into())
    };
    let models = env("GALION_VOICE_MODELS", "/workspace/oom-models");
    let voices = env(
        "GALION_VOICE_VOICES",
        "/workspace/swarm/target/voice-rust/voices",
    );
    let mouth_args = env("MOUTH_ARGS", "");
    let bin = env!("CARGO_BIN_EXE_galion-voice-serve");
    let tmp = std::env::temp_dir().join(format!("voice-rust-real-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let mut rep = String::new();
    let mut out = |s: String| {
        eprintln!("{s}");
        rep.push_str(&s);
        rep.push('\n');
    };
    out(format!(
        "# voice-rust real test, {} CPUs, load {}",
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        std::fs::read_to_string("/proc/loadavg")
            .unwrap_or_default()
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    ));
    out(format!(
        "mouth args: {}",
        if mouth_args.is_empty() {
            "<reference: 10 flow steps, 10 s prompt>"
        } else {
            &mouth_args
        }
    ));

    let mut kids = Kids(Vec::new());
    kids.0.push(
        Command::new(bin)
            .args([
                "--bind",
                EAR,
                "--models",
                &models,
                "--stt",
                "whisper-large-v3-turbo",
                "--tts",
                "none",
            ])
            .stderr(Stdio::null())
            .spawn()
            .expect("ear"),
    );
    kids.0.push(
        Command::new(bin)
            .args([
                "--bind",
                MOUTH,
                "--models",
                &models,
                "--stt",
                "none",
                "--tts",
                "chatterbox",
                "--voices",
                &voices,
            ])
            .args(mouth_args.split_whitespace())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("mouth"),
    );
    std::thread::sleep(Duration::from_millis(800));
    out("## health while loading".into());
    for a in [EAR, MOUTH] {
        let r = get(a, "/health").expect("health while loading");
        out(format!("- {a} -> HTTP {} {}", r.code, short(&r.text())));
        assert_eq!(r.code, 503);
        assert!(r.text().contains("\"status\":\"loading\""));
    }
    let t = Instant::now();
    assert!(
        wait_healthy(EAR, 300) && wait_healthy(MOUTH, 300),
        "never healthy"
    );
    out(format!("both healthy after {} ms", t.elapsed().as_millis()));
    out("## health when loaded".into());
    for a in [EAR, MOUTH] {
        let r = get(a, "/health").unwrap();
        out(format!("- {a} -> HTTP {} {}", r.code, short(&r.text())));
        for f in [
            "\"ok\":true",
            "\"status\":\"ok\"",
            "\"provider\":\"local\"",
            "\"loaded\":true",
            "\"error\":null",
            "\"device\":\"cpu\"",
        ] {
            assert!(r.text().contains(f), "{f}");
        }
    }

    out("## Samantha lines: mouth -> WAV -> ear".into());
    out("| # | voice | text | audio s | synth ms | X-RTF | ear ms | heard | words |".into());
    out("|---|---|---|---|---|---|---|---|---|".into());
    let mut first_wav = Vec::new();
    let mut matches = Vec::new();
    for (i, (voice, line)) in LINES.iter().enumerate() {
        let body = format!(
            "{{\"text\":{},\"voice\":\"{voice}\"}}",
            galion_voice_cpu::serve::quote(line)
        );
        let t = Instant::now();
        let r = http(
            MOUTH,
            "POST",
            "/synthesize",
            &[("Content-Type", "application/json")],
            body.as_bytes(),
        )
        .expect("synth");
        let synth_ms = t.elapsed().as_millis();
        assert_eq!(r.code, 200, "{}", r.text());
        assert_eq!(&r.body[..4], b"RIFF");
        assert_eq!(
            u32::from_le_bytes(r.body[24..28].try_into().unwrap()),
            24_000
        );
        let (ct, mp) = multipart(&r.body);
        let t = Instant::now();
        let e = http(
            EAR,
            "POST",
            "/v1/audio/transcriptions",
            &[("Content-Type", &ct)],
            &mp,
        )
        .expect("ear");
        let ear_ms = t.elapsed().as_millis();
        assert_eq!(e.code, 200, "{}", e.text());
        let heard = json_text(&e.text(), "text");
        let m = word_match(line, &heard);
        matches.push(m);
        out(format!(
            "| {} | {voice} | {line} | {} | {synth_ms} | {} | {ear_ms} | {heard} | {m:.0}% |",
            i + 1,
            r.header("X-Duration"),
            r.header("X-RTF")
        ));
        if let Ok(dir) = std::env::var("KEEP_WAVS") {
            let _ = std::fs::write(
                PathBuf::from(dir).join(format!("line{}-{voice}.wav", i + 1)),
                &r.body,
            );
        }
        if i == 0 {
            first_wav = r.body.clone();
        }
    }
    let mean = matches.iter().sum::<f64>() / matches.len() as f64;
    out(format!("mean word match {mean:.1}%"));
    assert!(mean >= 80.0, "transcripts do not match the scripts");

    out("## contract checks".into());
    let r = http(
        MOUTH,
        "POST",
        "/synthesize",
        &[("Content-Type", "application/json")],
        br#"{"text":"  "}"#,
    )
    .unwrap();
    out(format!("- empty text -> HTTP {} {}", r.code, r.text()));
    assert_eq!(
        (r.code, r.text().as_str()),
        (400, "{\"error\":\"text required\"}")
    );
    let r = http(MOUTH, "POST", "/synthesize", &[], b"nope").unwrap();
    out(format!("- non-JSON body -> HTTP {} {}", r.code, r.text()));
    assert_eq!(r.code, 400);
    let r = http(
        EAR,
        "POST",
        "/v1/audio/transcriptions",
        &[("Content-Type", "multipart/form-data; boundary=x")],
        b"--x--\r\n",
    )
    .unwrap();
    out(format!(
        "- ear without a file part -> HTTP {} {}",
        r.code,
        r.text()
    ));
    assert_eq!(r.code, 400);

    if let Some(one) = std::env::var("ONE_BIN")
        .ok()
        .filter(|p| std::path::Path::new(p).is_file())
    {
        out("## Galion One end to end".into());
        let (home, hermes, store) = (tmp.join("home"), tmp.join("hermes"), tmp.join("store.json"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&hermes).unwrap();
        let code = "VOICERUSTTEST";
        std::fs::write(
            &store,
            format!("{{\"codeHash\":\"{}\",\"tokenHash\":\"\",\"tokenHashes\":[],\"deviceHash\":\"\",\"fails\":0,\"lockedUntil\":0}}\n", sha256_hex(code.as_bytes())),
        )
        .unwrap();
        kids.0.push(
            Command::new(&one)
                .env("GALION_HOME", &home)
                .env("ONE_HERMES_DIR", &hermes)
                .env("ONE_DEVICE_STORE", &store)
                .env("ONE_BIND", ONE)
                .env("ONE_MOUTH_BIND", ONE_MOUTH)
                .env("ONE_NO_WARM", "1")
                .env("ONE_WARM_MIND", "0")
                .env("GALION_MOUTH", MOUTH)
                .env("GALION_EAR", EAR)
                .env("GALION_MIND", "127.0.0.1:19376")
                .env("GALION_EYE", "127.0.0.1:19377")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("galion-one"),
        );
        let t = Instant::now();
        while get(ONE_MOUTH, "/health").is_none() && t.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(200));
        }
        let r = get(ONE_MOUTH, "/health").expect("one mouth health");
        out(format!(
            "- one mouth /health pass-through -> HTTP {} {}",
            r.code,
            short(&r.text())
        ));
        assert_eq!(r.code, 200);
        assert!(r.text().contains("\"model\":\"chatterbox\""));
        let t = Instant::now();
        let r = http(
            ONE_MOUTH,
            "POST",
            "/synthesize",
            &[("Content-Type", "application/json")],
            br#"{"text":"Hello. I can hear you now."}"#,
        )
        .unwrap();
        let ms = t.elapsed().as_millis();
        out(format!("- one mouth /synthesize -> HTTP {}, {} bytes, {ms} ms (Galion One's mouth timeout: 30 s)", r.code, r.body.len()));
        assert_eq!(r.code, 200, "Galion One got no audio (timeout or error)");
        let (ct, mp) = multipart(&r.body);
        let e = http(
            EAR,
            "POST",
            "/v1/audio/transcriptions",
            &[("Content-Type", &ct)],
            &mp,
        )
        .unwrap();
        out(format!(
            "  - that WAV through the ear: \"{}\"",
            json_text(&e.text(), "text")
        ));
        let p = http(
            ONE,
            "POST",
            "/pair",
            &[("Content-Type", "application/json")],
            format!("{{\"code\":\"{code}\"}}").as_bytes(),
        )
        .unwrap();
        let cookie = p
            .header("Set-Cookie")
            .split(';')
            .next()
            .unwrap_or("")
            .to_string();
        assert!(!cookie.is_empty(), "pairing failed: {}", p.text());
        let t = Instant::now();
        let h = http(
            ONE,
            "POST",
            "/hear",
            &[("Cookie", &cookie), ("Content-Type", "audio/wav")],
            &first_wav,
        )
        .unwrap();
        let ms = t.elapsed().as_millis();
        out(format!(
            "- one /hear (paired, line 1 WAV) -> {} in {ms} ms (Galion One's ear timeout: 20 s)",
            h.text()
        ));
        let said = json_text(&h.text(), "say");
        assert!(word_match(LINES[0].1, &said) >= 80.0, "{said}");
    } else {
        out("(Galion One stage skipped: set ONE_BIN)".into());
    }
    for a in [EAR, MOUTH] {
        let b = get(a, "/health").unwrap().text();
        let hwm = b
            .split("\"hwm_kb\":")
            .nth(1)
            .unwrap_or("?")
            .trim_end_matches('}');
        out(format!("peak RSS {a}: {hwm} kB"));
    }
    drop(kids);
    let _ = std::fs::remove_dir_all(&tmp);
    if let Ok(p) = std::env::var("GALION_REAL_REPORT") {
        std::fs::write(p, &rep).unwrap();
    }
}
