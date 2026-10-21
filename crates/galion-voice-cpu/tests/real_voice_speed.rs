// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! voice-speed real-life test (slot 147, ports 20170-20179). GALION_REAL=1 only.
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-voice-cpu --features whisper,kitten,chatterbox \
//!     --test real_voice_speed -- --nocapture --test-threads 1
//! ```
//!
//! For each mouth profile in `VS_PROFILES` (default `q8:10,f32:10,q8:5`, i.e.
//! T3 weights : flow steps) it starts the release `galion-voice-serve` as the
//! legacy mouth on 127.0.0.1:20171 (`--tts chatterbox --stt none --seed 1`),
//! checks /health 503 -> 200 and the 400 contract, speaks five lines over
//! `POST /synthesize` and stops it. Then the legacy ear (whisper
//! large-v3-turbo, `--stt` on 127.0.0.1:20170) transcribes every WAV. One
//! model runs at a time to keep RAM low on the shared box. Voices: jarvis
//! is conditioned on a reference clip (`$VS_JARVIS_WAV`, default the
//! KittenTTS male sample in the models dir), samantha uses the built-in
//! voice. Prints a markdown report (also to `$GALION_REAL_REPORT`).
//!
//! Asserts: every line 200 with >= 0.5 s audio, every line under Galion One's
//! 30 s mouth timeout, mean word match >= 90 % per profile. The < 15 s for 3 s
//! of speech target is reported per line; `VS_ASSERT_TARGET=1` also asserts
//! it for the `q8:10` profile (off by default: the box is shared, so wall
//! time depends on other agents' load, which the report records).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const EAR: &str = "127.0.0.1:20170";
const MOUTH: &str = "127.0.0.1:20171";

const LINES: [(&str, &str); 5] = [
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
    (
        "jarvis",
        "Your meeting starts in ten minutes, and the coffee is ready.",
    ),
];

struct Kid(Child);
impl Drop for Kid {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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

fn http(addr: &str, method: &str, path: &str, ctype: &str, body: &[u8]) -> Option<Resp> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(240))).ok()?;
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if !ctype.is_empty() {
        head.push_str(&format!("Content-Type: {ctype}\r\n"));
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

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(String::from)
        .collect()
}

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

fn load_avg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// Wait for /health 200; returns (first status seen, ms until healthy).
fn wait_healthy(addr: &str, kid: &mut Kid, secs: u64) -> (Option<u16>, Option<u128>) {
    let t = Instant::now();
    let mut first = None;
    while t.elapsed() < Duration::from_secs(secs) {
        if let Ok(Some(_)) = kid.0.try_wait() {
            return (first, None);
        }
        if let Some(r) = http(addr, "GET", "/health", "", b"") {
            first.get_or_insert(r.code);
            if r.code == 200 {
                return (first, Some(t.elapsed().as_millis()));
            }
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    (first, None)
}

fn json_num(body: &str, key: &str) -> Option<f64> {
    let i = body.find(&format!("\"{key}\":"))? + key.len() + 3;
    let rest = &body[i..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

struct Line {
    profile: String,
    voice: &'static str,
    text: &'static str,
    audio_s: f64,
    wall_ms: u128,
    cpu_ms: String,
    wav: Vec<u8>,
    heard: String,
    ear_ms: u128,
}

#[test]
fn mouth_speed_profiles_with_ear_round_trip() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_galion-voice-serve");
    let models =
        std::env::var("GALION_VOICE_MODELS").unwrap_or_else(|_| "/workspace/oom-models".into());
    let jarvis = std::env::var("VS_JARVIS_WAV")
        .unwrap_or_else(|_| format!("{models}/kitten-out/kitten-official-jasper-2.wav"));
    let voices = std::env::temp_dir().join(format!("voice-speed-{}", std::process::id()));
    std::fs::create_dir_all(&voices).unwrap();
    std::fs::copy(&jarvis, voices.join("jarvis.wav")).expect("jarvis reference clip");
    let profiles = std::env::var("VS_PROFILES").unwrap_or_else(|_| "q8:10,f32:10,q8:5".to_string());
    let mut rep = String::new();
    let mut say = |s: String| {
        println!("{s}");
        rep.push_str(&s);
        rep.push('\n');
    };
    say(format!(
        "# voice-speed real test, {} CPUs, load {}",
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        load_avg()
    ));
    let mut all: Vec<Line> = Vec::new();
    for prof in profiles.split(',') {
        let (w, steps) = prof.split_once(':').unwrap_or((prof, "10"));
        say(format!(
            "## mouth profile t3={w} flow-steps={steps} (load {})",
            load_avg()
        ));
        let mut kid = Kid(Command::new(bin)
            .args([
                "--bind",
                MOUTH,
                "--models",
                &models,
                "--tts",
                "chatterbox",
                "--stt",
                "none",
            ])
            .args(["--voices", voices.to_str().unwrap(), "--seed", "1"])
            .args(["--t3-weights", w, "--flow-steps", steps])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start mouth"));
        let (first, ready) = wait_healthy(MOUTH, &mut kid, 300);
        say(format!("- /health first {first:?}, 200 after {ready:?} ms"));
        assert_eq!(first, Some(503), "mouth must answer 503 while loading");
        assert!(ready.is_some(), "mouth never became healthy");
        let h = http(MOUTH, "GET", "/health", "", b"").unwrap();
        let ht = h.text();
        assert!(ht.contains("\"model\":\"chatterbox\""), "{ht}");
        let bad = http(
            MOUTH,
            "POST",
            "/synthesize",
            "application/json",
            b"{\"text\":\"  \"}",
        );
        let bad = bad.expect("400 probe");
        say(format!("- empty text -> HTTP {} {}", bad.code, bad.text()));
        assert_eq!(bad.code, 400);
        for (voice, text) in LINES {
            let body = format!("{{\"text\":\"{text}\",\"voice\":\"{voice}\"}}");
            let t = Instant::now();
            let r = http(
                MOUTH,
                "POST",
                "/synthesize",
                "application/json",
                body.as_bytes(),
            )
            .expect("synthesize");
            let wall_ms = t.elapsed().as_millis();
            assert_eq!(r.code, 200, "{}", r.text());
            assert_eq!(r.header("Content-Type"), "audio/wav");
            let audio_s = (r.body.len().saturating_sub(44)) as f64 / 48000.0;
            assert!(audio_s >= 0.5, "too little audio: {audio_s}");
            all.push(Line {
                profile: prof.to_string(),
                voice,
                text,
                audio_s,
                wall_ms,
                cpu_ms: r.header("X-Cpu-Ms"),
                wav: r.body,
                heard: String::new(),
                ear_ms: 0,
            });
        }
        let h = http(MOUTH, "GET", "/health", "", b"").unwrap().text();
        say(format!(
            "- mouth peak RSS {:.0} MB",
            json_num(&h, "hwm_kb").unwrap_or(0.0) / 1024.0
        ));
        drop(kid);
    }
    // The ear, alone in RAM.
    let mut ear = Kid(Command::new(bin)
        .args([
            "--bind",
            EAR,
            "--models",
            &models,
            "--stt",
            "whisper-large-v3-turbo",
        ])
        .args(["--tts", "none"])
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("start ear"));
    let (_, ready) = wait_healthy(EAR, &mut ear, 300);
    assert!(ready.is_some(), "ear never became healthy");
    for l in all.iter_mut() {
        let mut b = b"--vsb\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n".to_vec();
        b.extend_from_slice(&l.wav);
        b.extend_from_slice(
            b"\r\n--vsb\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n--vsb--\r\n",
        );
        let t = Instant::now();
        let r = http(
            EAR,
            "POST",
            "/v1/audio/transcriptions",
            "multipart/form-data; boundary=vsb",
            &b,
        )
        .expect("transcribe");
        l.ear_ms = t.elapsed().as_millis();
        assert_eq!(r.code, 200, "{}", r.text());
        l.heard = galion_voice_cpu::serve::json_str(&r.text(), "text").unwrap_or_default();
    }
    drop(ear);
    let _ = std::fs::remove_dir_all(&voices);
    say("## lines (mouth -> WAV -> ear)".into());
    say("| profile | voice | text | audio s | synth ms | cpu ms | s per 3 s speech | < 15 s | heard | words |".into());
    say("|---|---|---|---|---|---|---|---|---|---|".into());
    let mut per: Vec<(String, f64, usize, f64)> = Vec::new();
    for l in &all {
        let m = word_match(l.text, &l.heard);
        let per3 = l.wall_ms as f64 / 1000.0 / l.audio_s * 3.0;
        say(format!(
            "| {} | {} | {} | {:.2} | {} | {} | {:.1} | {} | {} | {:.0}% |",
            l.profile,
            l.voice,
            l.text,
            l.audio_s,
            l.wall_ms,
            l.cpu_ms,
            per3,
            if per3 < 15.0 { "yes" } else { "no" },
            l.heard,
            m
        ));
        assert!(
            l.wall_ms < 30_000,
            "{}: {} ms over Galion One's 30 s",
            l.profile,
            l.wall_ms
        );
        if std::env::var("VS_ASSERT_TARGET").as_deref() == Ok("1") && l.profile == "q8:10" {
            assert!(per3 < 15.0, "{}: {per3:.1} s per 3 s of speech", l.text);
        }
        match per.iter_mut().find(|p| p.0 == l.profile) {
            Some(p) => {
                p.1 += m;
                p.2 += 1;
                p.3 += per3;
            }
            None => per.push((l.profile.clone(), m, 1, per3)),
        }
    }
    say("## per profile".into());
    for (p, m, n, s) in &per {
        let (m, s) = (m / *n as f64, s / *n as f64);
        say(format!(
            "- {p}: mean words {m:.1}%, mean {s:.1} s per 3 s of speech"
        ));
        assert!(m >= 90.0, "{p}: mean word match {m:.1}%");
    }
    say(format!("load at end {}", load_avg()));
    if let Ok(p) = std::env::var("GALION_REAL_REPORT") {
        let _ = std::fs::write(p, &rep);
    }
}
