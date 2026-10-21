// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Port of `scripts/real/voice-mouth.sh` (deleted). Release
//! `galion-voice-serve` (whisper + kitten, CPU only) on 127.0.0.1:19840 with
//! 1 and then 4 candle threads: a real 11.0 s JFK WAV in (STT x3), text in
//! (TTS x2), the `/v1/voice` round trip; latency, CPU (X-Cpu-Ms) and memory
//! (VmRSS / VmHWM) printed. Then a release `galion-one` on 19841 (text) /
//! 19842 (mouth) with `GALION_MOUTH=127.0.0.1:19840` (mouth proxies to the
//! voice service) and without it (the live configuration: honest 503).
//! Nothing paid, nothing remote: GALION_MIND points at the dead 19849.
//!
//!   GALION_REAL=1 [GALION_VOICE_MODELS=/workspace/oom-models] \
//!     cargo test -p galion-realtest --test real_sh_voice_mouth -- --nocapture
//!
//! Skips (passes with a note) when the models dir is missing.

mod sh3;

use galion_realtest::http::{self, Resp};
use sh3::*;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const VOICE: u16 = PORT_BASE;
const ONE: u16 = PORT_BASE + 1;
const MOUTH: u16 = PORT_BASE + 2;
const DEAD_MIND: u16 = PORT_BASE + 9;
const TEXT: &str = "Hello Slavik. The mouth can speak again, on the CPU only.";

fn post(port: u16, path: &str, ctype: &str, body: &[u8]) -> (Resp, f64) {
    let t = Instant::now();
    let r = http::request(
        &format!("127.0.0.1:{port}"),
        "POST",
        path,
        &[("Content-Type", ctype)],
        body,
        Duration::from_secs(180),
    )
    .unwrap_or_default();
    (r, t.elapsed().as_secs_f64())
}

fn hdr(r: &Resp, k: &str) -> String {
    r.header(k).unwrap_or("-").to_string()
}

fn wait_health(port: u16) -> bool {
    wait_until(Duration::from_secs(30), || {
        http::get(&format!("127.0.0.1:{port}"), "/health").is_ok()
    })
}

#[test]
fn real_sh_voice_mouth() {
    if !enabled("real_sh_voice_mouth") {
        return;
    }
    let models = std::env::var_os("GALION_VOICE_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| "/workspace/oom-models".into());
    let wav_path = models.join("whisper-tiny.en/samples_jfk.wav");
    if !wav_path.exists() {
        eprintln!(
            "real_sh_voice_mouth: skipped ({} missing; scripts/voice/fetch-models.sh)",
            wav_path.display()
        );
        return;
    }
    let rel = release_bins(
        None,
        &[
            "-p",
            "galion-voice-cpu",
            "--features",
            "whisper,kitten",
            "--bin",
            "galion-voice-serve",
        ],
        &[],
    );
    let rel_one = release_bins(None, &["-p", "galion-one"], &[]);
    let voice_bin = need(&rel.join("galion-voice-serve"));
    let one_bin = need(&rel_one.join("galion-one"));
    let wav = std::fs::read(&wav_path).unwrap();
    require_free(&[VOICE, ONE, MOUTH]);
    let tmp = Scratch::new("voice-mouth-real");
    let load = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    say(format!(
        "{} CPUs, load {}",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(0),
        load.split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ")
    ));
    let mut ck = Checks::default();
    for threads in [1, 4] {
        let mut procs = Procs::default();
        let vp = procs.spawn(
            "voice",
            Command::new("nice")
                .args(["-n", "10"])
                .arg(&voice_bin)
                .arg("--bind")
                .arg(format!("127.0.0.1:{VOICE}"))
                .arg("--models")
                .arg(&models)
                .env("RAYON_NUM_THREADS", threads.to_string())
                .stdout(Stdio::null())
                .stderr(std::fs::File::create(tmp.path("voice.log")).unwrap()),
        );
        ck.ok(&format!("t{threads}: voice up"), wait_health(VOICE), "");
        println!("## RAYON_NUM_THREADS={threads}");
        let h = http::get(&format!("127.0.0.1:{VOICE}"), "/health").unwrap_or_default();
        println!("health: {} -> {}", h.text(), h.status);
        ck.ok(
            &format!("t{threads}: health ok"),
            h.status == 200 && h.text().contains("\"ok\":true"),
            "",
        );
        for i in 1..=3 {
            let (r, s) = post(VOICE, "/v1/audio/transcriptions", "audio/wav", &wav);
            println!(
                "stt #{i} (11.0 s JFK WAV): {} -> {} {s:.3}s",
                r.text(),
                r.status
            );
            ck.ok(
                &format!("t{threads}: stt #{i}"),
                r.status == 200
                    && r.text()
                        .contains("ask not what your country can do for you"),
                "",
            );
        }
        for i in 1..=2 {
            let body = format!("{{\"text\":\"{TEXT}\"}}");
            let (r, s) = post(VOICE, "/synthesize", "application/json", body.as_bytes());
            println!(
                "tts #{i}: {} {} B {s:.3}s audio {} s, wall {} ms, cpu {} ms",
                r.status,
                r.body.len(),
                hdr(&r, "X-Audio-S"),
                hdr(&r, "X-Wall-Ms"),
                hdr(&r, "X-Cpu-Ms")
            );
            ck.ok(
                &format!("t{threads}: tts #{i} RIFF WAV"),
                r.status == 200 && r.body.starts_with(b"RIFF"),
                "",
            );
        }
        let (r, s) = post(VOICE, "/v1/voice", "audio/wav", &wav);
        println!(
            "round trip /v1/voice: {} {} B {s:.3}s stt {} ms, tts {} ms, reply audio {} s, cpu {} ms",
            r.status,
            r.body.len(),
            hdr(&r, "X-Stt-Ms"),
            hdr(&r, "X-Tts-Ms"),
            hdr(&r, "X-Audio-S"),
            hdr(&r, "X-Cpu-Ms")
        );
        println!("transcript header: {}", hdr(&r, "X-Transcript"));
        ck.ok(
            &format!("t{threads}: round trip"),
            r.status == 200
                && r.body.starts_with(b"RIFF")
                && hdr(&r, "X-Transcript").contains("ask not"),
            "",
        );
        println!(
            "process: VmHWM {} VmRSS {}",
            proc_field(vp, "VmHWM").unwrap_or_default(),
            proc_field(vp, "VmRSS").unwrap_or_default()
        );
        if threads == 4 {
            let one = |mouth: bool| {
                let mut c = Command::new("nice");
                c.args(["-n", "10"])
                    .arg(&one_bin)
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .env("HOME", &tmp.0)
                    .env("GALION_HOME", &tmp.0)
                    .env("ONE_NO_WARM", "1")
                    .env("ONE_WARM_MIND", "0")
                    .env("ONE_DEVICE_STORE", tmp.path("none.json"))
                    .env("ONE_HERMES_DIR", &tmp.0)
                    .env("ONE_HERMES_ENV", tmp.path("none.env"))
                    .env("OPENROUTER_KEY_FILE", tmp.path("none.env"))
                    .env("GALION_MIND", format!("127.0.0.1:{DEAD_MIND}"))
                    .env("GALION_EAR", format!("127.0.0.1:{VOICE}"))
                    .env("ONE_BIND", format!("127.0.0.1:{ONE}"))
                    .env("ONE_MOUTH_BIND", format!("127.0.0.1:{MOUTH}"))
                    .stdout(Stdio::null())
                    .stderr(std::fs::File::create(tmp.path("one.log")).unwrap());
                if mouth {
                    c.env("GALION_MOUTH", format!("127.0.0.1:{VOICE}"));
                }
                c
            };
            procs.spawn("one-mouth", &mut one(true));
            ck.ok("galion-one mouth up", wait_health(MOUTH), "");
            println!("## galion-one with GALION_MOUTH=127.0.0.1:{VOICE}");
            let mh = http::get(&format!("127.0.0.1:{MOUTH}"), "/health").unwrap_or_default();
            println!("mouth /health: {} -> {}", mh.text(), mh.status);
            ck.ok(
                "mouth /health proxied 200",
                mh.status == 200 && mh.text().contains("galion-voice-cpu"),
                "",
            );
            let (r, s) = post(
                MOUTH,
                "/synthesize",
                "application/json",
                b"{\"text\":\"Galion One speaking through the CPU voice service.\"}",
            );
            println!("mouth /synthesize: {} {} B {s:.3}s", r.status, r.body.len());
            ck.ok(
                "mouth /synthesize RIFF WAV",
                r.status == 200 && r.body.starts_with(b"RIFF"),
                "",
            );
            procs.stop("one-mouth");
            ck.ok(
                "galion-one stopped",
                wait_until(Duration::from_secs(5), || !port_open(MOUTH)),
                "",
            );
            procs.spawn("one-plain", &mut one(false));
            wait_health(MOUTH);
            println!("## galion-one without GALION_MOUTH (the live configuration)");
            let mh = http::get(&format!("127.0.0.1:{MOUTH}"), "/health").unwrap_or_default();
            println!("mouth /health: {} -> {}", mh.text(), mh.status);
            ck.ok(
                "no GALION_MOUTH: honest 503",
                mh.status == 503 && mh.text().contains("no speech backend configured"),
                "",
            );
        }
        procs.stop_all();
        ck.ok(
            &format!("t{threads}: teardown"),
            wait_until(Duration::from_secs(5), || {
                ![VOICE, ONE, MOUTH].iter().any(|p| port_open(*p))
            }),
            "",
        );
    }
    let left = pids_running(&voice_bin);
    ck.ok(
        "no galion-voice-serve left",
        left.is_empty(),
        &format!("{left:?}"),
    );
    ck.finish("real_sh_voice_mouth");
}
