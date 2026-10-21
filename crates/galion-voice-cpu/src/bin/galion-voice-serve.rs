// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! galion-voice-serve: the CPU voice service (ear + mouth) for Galion One.
//!
//! usage: galion-voice-serve [--bind ADDR] [--models DIR]
//!
//! * `--bind` / `GALION_VOICE_BIND` (default 127.0.0.1:8003)
//! * `--models` / `GALION_VOICE_MODELS` (default /var/lib/galion/voice-models),
//!   laid out as `whisper-tiny.en/`, `kitten-tts-nano/`, `cmudict/`.
//!
//! It never downloads anything. Each model directory is loaded only when its
//! declared licence (model card front matter or LICENSE) is on the allow list
//! in `serve::ALLOWED_MODEL_LICENCES`. Whisper needs the `whisper` feature,
//! KittenTTS the `kitten` feature; without them the engine is reported as
//! missing and `/health` answers 503 with the reason.
//! Threads: candle uses `RAYON_NUM_THREADS` (set it, e.g. 2, on a shared box).

use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(any(feature = "whisper", feature = "kitten"))]
use std::sync::Mutex;
#[cfg(any(feature = "whisper", feature = "kitten"))]
use std::time::Instant;

#[cfg(any(feature = "whisper", feature = "kitten"))]
use galion_voice_cpu::serve::check_model_dir;
use galion_voice_cpu::serve::{serve, Engines};

fn arg(args: &[String], flag: &str, env: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var(env).ok().filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| default.to_string())
}

#[cfg(feature = "whisper")]
fn load_stt(models: &std::path::Path, e: &mut Engines) {
    use galion_voice_cpu::serve::SttSlot;
    let dir = models.join("whisper-tiny.en");
    let t = Instant::now();
    let r = check_model_dir(&dir)
        .and_then(|lic| galion_voice_cpu::whisper_stt::WhisperStt::load(&dir).map(|m| (lic, m)));
    match r {
        Ok((lic, m)) => {
            eprintln!(
                "voice: stt whisper-tiny.en ({lic}) loaded in {} ms",
                t.elapsed().as_millis()
            );
            e.stt = Some(SttSlot {
                name: "whisper-tiny.en".into(),
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
fn load_stt(_: &std::path::Path, e: &mut Engines) {
    e.stt_missing = "built without the whisper feature".into();
}

#[cfg(feature = "kitten")]
fn load_tts(models: &std::path::Path, e: &mut Engines) {
    use galion_voice_cpu::serve::TtsSlot;
    let dir = models.join("kitten-tts-nano");
    let dict = models.join("cmudict");
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
fn load_tts(_: &std::path::Path, e: &mut Engines) {
    e.tts_missing = "built without the kitten feature".into();
}

const USAGE: &str = "usage: galion-voice-serve [--bind ADDR] [--models DIR]\nenv: GALION_VOICE_BIND (default 127.0.0.1:8003), GALION_VOICE_MODELS (default /var/lib/galion/voice-models)\n";

/// `--check`: bind address and model layout only; loads no model, never binds.
fn check() -> Result<String, String> {
    use galion_common::flags::{bind_addr, line, path_exists};
    let mut r = String::new();
    let none: Vec<String> = Vec::new();
    bind_addr(
        &mut r,
        "bind",
        &arg(&none, "--bind", "GALION_VOICE_BIND", "127.0.0.1:8003"),
    )?;
    let models = PathBuf::from(arg(
        &none,
        "--models",
        "GALION_VOICE_MODELS",
        "/var/lib/galion/voice-models",
    ));
    let ok = path_exists(&mut r, "models", &models, true);
    for d in ["whisper-tiny.en", "kitten-tts-nano", "cmudict"] {
        path_exists(&mut r, "model", &models.join(d), true);
    }
    line(
        &mut r,
        "features",
        format!(
            "whisper={} kitten={}",
            cfg!(feature = "whisper"),
            cfg!(feature = "kitten")
        ),
    );
    if ok {
        Ok(r)
    } else {
        Err(format!("{r}models dir missing (/health would answer 503)"))
    }
}

fn main() {
    galion_common::flags::early(
        "galion-voice-serve",
        env!("CARGO_PKG_VERSION"),
        USAGE,
        check,
    );
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("usage: galion-voice-serve [--bind ADDR] [--models DIR]");
        return;
    }
    let bind = arg(&args, "--bind", "GALION_VOICE_BIND", "127.0.0.1:8003");
    let models = PathBuf::from(arg(
        &args,
        "--models",
        "GALION_VOICE_MODELS",
        "/var/lib/galion/voice-models",
    ));
    let mut e = Engines::default();
    load_stt(&models, &mut e);
    load_tts(&models, &mut e);
    let listener = match TcpListener::bind(&bind) {
        Ok(l) => l,
        Err(err) => {
            eprintln!("voice: bind {bind}: {err}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "voice: listening on {bind} (stt {}, tts {})",
        e.stt.as_ref().map_or("none", |s| s.name.as_str()),
        e.tts.as_ref().map_or("none", |t| t.name.as_str())
    );
    serve(listener, Arc::new(e));
}
