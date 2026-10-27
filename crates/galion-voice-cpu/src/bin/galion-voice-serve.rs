// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! galion-voice-serve: the CPU voice service (ear + mouth) for Galion One.
//!
//! usage: galion-voice-serve [--bind ADDR] [--models DIR] [--stt NAME|none]
//!        [--tts kitten|chatterbox|none] [--voices DIR] [--flow-steps N]
//!        [--prompt-seconds S] [--seed N] [--worker]
//!
//! * `--bind` / `GALION_VOICE_BIND` (default 127.0.0.1:8003)
//! * `--models` / `GALION_VOICE_MODELS` (default /var/lib/galion/voice-models),
//!   laid out as `whisper-tiny.en/`, `whisper-large-v3-turbo/`,
//!   `kitten-tts-nano/`, `cmudict/`, `chatterbox/`.
//! * `--stt` / `GALION_VOICE_STT`: the Whisper directory to load
//!   (default `whisper-tiny.en`; `whisper-large-v3-turbo` is the legacy ear's
//!   model), or `none`.
//! * `--tts` / `GALION_VOICE_TTS`: `kitten` (default), `chatterbox` (the
//!   legacy mouth's model, with the legacy voice table and chunking) or `none`.
//! * `--voices` / `GALION_VOICE_VOICES`: directory with `samantha.wav` /
//!   `jarvis.wav` reference clips for Chatterbox.
//! * `--flow-steps` (default 10) and `--prompt-seconds` (default 10): the
//!   Chatterbox speed/quality knobs (the reference values are 10 and 10).
//! * `--worker`: instead of HTTP, speak the legacy `worker.py` frame protocol
//!   on stdin/stdout (Chatterbox only).
//!
//! The port is bound before the models load, so `/health` answers
//! `"status":"loading"` (503) until they are ready. It never downloads
//! anything. Each model directory is loaded only when its declared licence
//! (model card front matter or LICENSE) is on the allow list in
//! `serve::ALLOWED_MODEL_LICENCES`. Threads: candle uses `RAYON_NUM_THREADS`.

use std::net::TcpListener;
use std::path::PathBuf;
#[cfg(any(feature = "whisper", feature = "kitten", feature = "chatterbox"))]
use std::sync::Mutex;
use std::sync::{Arc, RwLock};
#[cfg(any(feature = "whisper", feature = "kitten", feature = "chatterbox"))]
use std::time::Instant;

#[cfg(any(feature = "whisper", feature = "kitten", feature = "chatterbox"))]
use galion_voice_cpu::serve::check_model_dir;
use galion_voice_cpu::serve::{serve_cell, Engines};

fn arg(args: &[String], flag: &str, env: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var(env).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| default.to_string())
}

/// Settings shared by the loaders.
#[allow(dead_code)]
struct Opts {
    models: PathBuf,
    stt: String,
    tts: String,
    voices: Option<PathBuf>,
    flow_steps: usize,
    prompt_seconds: f32,
    seed: Option<u64>,
}

#[cfg(feature = "whisper")]
fn load_stt(o: &Opts, e: &mut Engines) {
    use galion_voice_cpu::serve::SttSlot;
    let name = o.stt.clone();
    if name.contains('/') || name.contains("..") || !name.starts_with("whisper") {
        e.stt_missing = format!("--stt {name}: expected a whisper-* model directory name");
        return;
    }
    let dir = o.models.join(&name);
    let t = Instant::now();
    let r = check_model_dir(&dir)
        .and_then(|lic| galion_voice_cpu::whisper_stt::WhisperStt::load(&dir).map(|m| (lic, m)));
    match r {
        Ok((lic, m)) => {
            eprintln!(
                "voice: stt {name} ({lic}, multilingual {}) loaded in {} ms",
                m.is_multilingual(),
                t.elapsed().as_millis()
            );
            e.stt = Some(SttSlot {
                name,
                engine: Mutex::new(Box::new(m)),
            });
        }
        Err(err) => {
            eprintln!("voice: stt not loaded: {err}");
            e.stt_missing = err;
        }
    }
}

#[cfg(not(feature = "whisper"))]
fn load_stt(_: &Opts, e: &mut Engines) {
    e.stt_missing = "built without the whisper feature".into();
}

#[cfg(feature = "kitten")]
fn load_kitten(o: &Opts, e: &mut Engines) {
    use galion_voice_cpu::serve::TtsSlot;
    let dir = o.models.join("kitten-tts-nano");
    let dict = o.models.join("cmudict");
    let t = Instant::now();
    let r = check_model_dir(&dir).and_then(|lic| {
        let dlic = check_model_dir(&dict)?;
        galion_voice_cpu::kitten::KittenTts::load(&dir, &dict.join("cmudict.dict"))
            .map(|m| (lic, dlic, m))
    });
    match r {
        Ok((lic, dlic, m)) => {
            eprintln!(
                "voice: tts kitten-tts-nano-0.8 ({lic}) + cmudict ({dlic}) loaded in {} ms",
                t.elapsed().as_millis()
            );
            let voices: Vec<String> = m.voice_names().into_iter().map(String::from).collect();
            let default_voice = m.default_voice.clone();
            e.tts = Some(TtsSlot {
                name: "kitten-tts-nano-0.8".into(),
                voices,
                default_voice,
                health_voices: None,
                engine: Mutex::new(Box::new(m)),
            });
        }
        Err(err) => {
            eprintln!("voice: tts not loaded: {err}");
            e.tts_missing = err;
        }
    }
}

#[cfg(not(feature = "kitten"))]
fn load_kitten(_: &Opts, e: &mut Engines) {
    e.tts_missing = "built without the kitten feature".into();
}

#[cfg(feature = "chatterbox")]
fn chatterbox_engine(o: &Opts) -> Result<galion_voice_cpu::chatterbox_tts::ChatterboxTts, String> {
    use galion_voice_cpu::chatterbox_tts::{ChatterboxConfig, ChatterboxTts};
    let dir = o.models.join("chatterbox");
    let lic = check_model_dir(&dir)?;
    let mut cfg = ChatterboxConfig {
        voices_dir: o.voices.clone(),
        prompt_seconds: o.prompt_seconds,
        ..Default::default()
    };
    cfg.options.flow_steps = o.flow_steps;
    cfg.options.seed = o.seed;
    let t = Instant::now();
    let m = ChatterboxTts::load(&dir, &cfg)?;
    eprintln!(
        "voice: tts chatterbox ({lic}) ready in {} ms: {}",
        t.elapsed().as_millis(),
        m.notes.join("; ")
    );
    Ok(m)
}

#[cfg(feature = "chatterbox")]
fn load_chatterbox(o: &Opts, e: &mut Engines) {
    use galion_voice_cpu::chatterbox_tts::LEGACY_VOICES;
    use galion_voice_cpu::serve::TtsSlot;
    match chatterbox_engine(o) {
        Ok(m) => {
            e.tts = Some(TtsSlot {
                name: "chatterbox".into(),
                voices: LEGACY_VOICES.iter().map(|v| v.to_string()).collect(),
                default_voice: LEGACY_VOICES[0].into(),
                health_voices: Some(m.ready_voices()),
                engine: Mutex::new(Box::new(m)),
            });
        }
        Err(err) => {
            eprintln!("voice: tts not loaded: {err}");
            e.tts_missing = err;
        }
    }
}

#[cfg(not(feature = "chatterbox"))]
fn load_chatterbox(_: &Opts, e: &mut Engines) {
    e.tts_missing = "built without the chatterbox feature".into();
}

fn load(o: &Opts, e: &mut Engines) {
    if e.want_stt {
        load_stt(o, e);
    } else {
        e.stt_missing = "disabled (--stt none)".into();
    }
    match o.tts.as_str() {
        "none" => e.tts_missing = "disabled (--tts none)".into(),
        "chatterbox" => load_chatterbox(o, e),
        "kitten" => load_kitten(o, e),
        other => e.tts_missing = format!("--tts {other}: expected kitten, chatterbox or none"),
    }
}

/// The legacy `worker.py` protocol on stdin/stdout: `R` once loaded, then per
/// request `u32 LE n` + `[voice byte][utf-8 text]`; reply one PCM frame
/// (24 kHz s16le) and a zero-length end frame.
#[cfg(feature = "chatterbox")]
fn worker(o: &Opts) -> i32 {
    use galion_voice_cpu::chatterbox_tts::{to_pcm16, LEGACY_VOICES};
    use std::io::{Read, Write};
    let m = match chatterbox_engine(o) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("mouth: {e}");
            return 1;
        }
    };
    // worker.py used exaggeration 0.4 and cfg 0.4 (the HTTP server 0.5/0.5).
    let mut opts = m.options.clone();
    opts.exaggeration = 0.4;
    opts.cfg_weight = 0.4;
    let mut out = std::io::stdout().lock();
    let mut inp = std::io::stdin().lock();
    if out.write_all(b"R").and_then(|_| out.flush()).is_err() {
        return 1;
    }
    loop {
        let mut len = [0u8; 4];
        if inp.read_exact(&mut len).is_err() {
            return 0;
        }
        let n = u32::from_le_bytes(len) as usize;
        let mut clip: Option<Vec<u8>> = None;
        if (2..=2000).contains(&n) {
            let mut buf = vec![0u8; n];
            if inp.read_exact(&mut buf).is_err() {
                return 0;
            }
            let voice = LEGACY_VOICES[if buf[0] == 1 { 1 } else { 0 }];
            let text = String::from_utf8_lossy(&buf[1..]);
            let text: String = text.trim().chars().take(300).collect();
            if !text.is_empty() {
                match m.generate(&text, voice, &opts) {
                    Ok(wav) => {
                        clip = Some(
                            to_pcm16(&wav)
                                .iter()
                                .flat_map(|s| s.to_le_bytes())
                                .collect(),
                        )
                    }
                    Err(e) => eprintln!("mouth: {e}"),
                }
            }
        } else {
            // Out-of-range length: drain it (bounded) and answer with the end frame.
            let _ = std::io::copy(&mut (&mut inp).take(n as u64), &mut std::io::sink());
        }
        let mut reply = Vec::new();
        if let Some(pcm) = clip {
            reply.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
            reply.extend_from_slice(&pcm);
        }
        reply.extend_from_slice(&0u32.to_le_bytes());
        if out.write_all(&reply).and_then(|_| out.flush()).is_err() {
            return 0;
        }
    }
}

#[cfg(not(feature = "chatterbox"))]
fn worker(_: &Opts) -> i32 {
    eprintln!("mouth: built without the chatterbox feature");
    1
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!(
            "usage: galion-voice-serve [--bind ADDR] [--models DIR] [--stt NAME|none] \
             [--tts kitten|chatterbox|none] [--voices DIR] [--flow-steps N] \
             [--prompt-seconds S] [--seed N] [--worker]"
        );
        return;
    }
    let bind = arg(&args, "--bind", "GALION_VOICE_BIND", "127.0.0.1:8003");
    let o = Opts {
        models: PathBuf::from(arg(
            &args,
            "--models",
            "GALION_VOICE_MODELS",
            "/var/lib/galion/voice-models",
        )),
        stt: arg(&args, "--stt", "GALION_VOICE_STT", "whisper-tiny.en"),
        tts: arg(&args, "--tts", "GALION_VOICE_TTS", "kitten"),
        voices: Some(arg(&args, "--voices", "GALION_VOICE_VOICES", ""))
            .filter(|v| !v.is_empty())
            .map(PathBuf::from),
        flow_steps: arg(&args, "--flow-steps", "GALION_VOICE_FLOW_STEPS", "10")
            .parse()
            .unwrap_or(10)
            .clamp(1, 50),
        prompt_seconds: arg(
            &args,
            "--prompt-seconds",
            "GALION_VOICE_PROMPT_SECONDS",
            "10",
        )
        .parse()
        .unwrap_or(10.0f32)
        .clamp(1.0, 10.0),
        seed: arg(&args, "--seed", "GALION_VOICE_SEED", "").parse().ok(),
    };
    if args.iter().any(|a| a == "--worker") {
        std::process::exit(worker(&o));
    }
    let listener = match TcpListener::bind(&bind) {
        Ok(l) => l,
        Err(err) => {
            eprintln!("voice: bind {bind}: {err}");
            std::process::exit(1);
        }
    };
    // NERVE push (no-op unless NERVE_URL is set): started + heartbeat.
    nerve_client::event(
        "galion-voice-serve",
        "started",
        &format!("listening on {bind}"),
    );
    nerve_client::start_heartbeat("galion-voice-serve", 60);
    let pending = Engines {
        loading: true,
        want_stt: o.stt != "none",
        want_tts: o.tts != "none",
        bind: bind.clone(),
        ..Default::default()
    };
    let (want_stt, want_tts) = (pending.want_stt, pending.want_tts);
    let cell = Arc::new(RwLock::new(Arc::new(pending)));
    let c = cell.clone();
    std::thread::spawn(move || {
        let mut e = Engines {
            want_stt,
            want_tts,
            bind: bind.clone(),
            ..Default::default()
        };
        load(&o, &mut e);
        eprintln!(
            "voice: ready on {bind} (stt {}, tts {})",
            e.stt.as_ref().map_or("none", |s| s.name.as_str()),
            e.tts.as_ref().map_or("none", |t| t.name.as_str())
        );
        if let Ok(mut g) = c.write() {
            *g = Arc::new(e);
        }
    });
    eprintln!("voice: listening (loading models)");
    serve_cell(listener, cell);
}
