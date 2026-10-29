// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `galion-stt` binary.
//!
//! ```text
//! galion-stt [serve] [--bind 127.0.0.1:8651] [--lib-dir DIR] [--model-dir DIR]
//!            [--max-sessions 2] [--max-session-secs 600] [--partial-ms 300]
//!            [--ort-default-threads]
//! galion-stt verify     [--lib-dir DIR] [--model-dir DIR]
//! galion-stt transcribe FILE.wav [--realtime] [--frame-ms 20] [--partial-ms 300]
//!            [--tail-silence-ms 0] [--ort-default-threads]   (benchmark, prints JSON)
//! galion-stt probe      ws://127.0.0.1:8651/stream FILE.wav [--frame-ms 20]
//!            [--tail-silence-ms 0]                         (WS client, prints JSON)
//! ```
//! Env overrides: GALION_STT_BIND, GALION_STT_LIB_DIR, GALION_STT_MODEL_DIR,
//! GALION_STT_MAX_SESSIONS, GALION_STT_MAX_SESSION_SECS, GALION_STT_PARTIAL_MS.
//! ONNX Runtime always runs single-threaded per session
//! (`MOONSHINE_ORT_SINGLE_THREAD=1`, set by this binary; see `ort_threads`).

use galion_stt::audio::{f32_to_pcm16le, parse_wav, SAMPLE_RATE};
use galion_stt::engine::{EngineConfig, EnginePool, LineTracker, Transcriber};
use galion_stt::ffi::Api;
use galion_stt::models;
use galion_stt::protocol::ServerMsg;
use galion_stt::server::{self, AppState, ServerConfig};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_LIB_DIR: &str = "/opt/galion/stt-moonshine/lib";
const DEFAULT_MODEL_DIR: &str = "/opt/galion/stt-moonshine/models";

struct Args {
    rest: Vec<String>,
}

impl Args {
    fn flag(&mut self, name: &str) -> bool {
        if let Some(i) = self.rest.iter().position(|a| a == name) {
            self.rest.remove(i);
            true
        } else {
            false
        }
    }
    fn opt(&mut self, name: &str, env: Option<&str>) -> Option<String> {
        if let Some(i) = self.rest.iter().position(|a| a == name) {
            if i + 1 < self.rest.len() {
                let v = self.rest.remove(i + 1);
                self.rest.remove(i);
                return Some(v);
            }
        }
        env.and_then(|e| std::env::var(e).ok())
            .filter(|v| !v.is_empty())
    }
    fn num<T: std::str::FromStr>(
        &mut self,
        name: &str,
        env: Option<&str>,
        d: T,
    ) -> Result<T, String> {
        match self.opt(name, env) {
            None => Ok(d),
            Some(v) => v.parse().map_err(|_| format!("bad value for {name}: {v}")),
        }
    }
}

fn main() {
    let mut a = Args {
        rest: std::env::args().skip(1).collect(),
    };
    let cmd = if a.rest.first().is_some_and(|c| !c.starts_with('-')) {
        a.rest.remove(0)
    } else {
        "serve".to_string()
    };
    let r = match cmd.as_str() {
        "serve" => serve(a),
        "verify" => verify(a),
        "transcribe" => transcribe(a),
        "probe" => probe(a),
        "-h" | "--help" | "help" => {
            println!("galion-stt serve|verify|transcribe|probe (see crate docs)");
            Ok(())
        }
        other => Err(format!("unknown command {other}")),
    };
    if let Err(e) = r {
        eprintln!("galion-stt: error: {e}");
        std::process::exit(1);
    }
}

fn dirs(a: &mut Args) -> (PathBuf, PathBuf) {
    let lib = a
        .opt("--lib-dir", Some("GALION_STT_LIB_DIR"))
        .unwrap_or_else(|| DEFAULT_LIB_DIR.into());
    let model = a
        .opt("--model-dir", Some("GALION_STT_MODEL_DIR"))
        .unwrap_or_else(|| DEFAULT_MODEL_DIR.into());
    (PathBuf::from(lib), PathBuf::from(model))
}

/// Must run before any other thread exists (it sets an env var that
/// libmoonshine reads when it builds ONNX Runtime sessions).
///
/// Single-threaded ONNX Runtime is the enforced default: with ORT's own
/// thread pools, libmoonshine 0.1.5 gave garbled transcripts on about 40 %
/// of identical runs (see docs/galion-stt.md), single-thread gave the same
/// correct text 12 of 12 times. `--ort-default-threads` exists only to
/// reproduce that.
fn ort_threads(a: &mut Args) {
    if a.flag("--ort-default-threads") {
        std::env::remove_var("MOONSHINE_ORT_SINGLE_THREAD");
    } else {
        a.flag("--ort-single-thread");
        std::env::set_var("MOONSHINE_ORT_SINGLE_THREAD", "1");
    }
}

fn open_engine(lib_dir: &Path, model_dir: &Path) -> Result<Arc<Api>, String> {
    let t = Instant::now();
    models::verify_all(lib_dir, model_dir).map_err(|e| format!("pinned file check failed: {e}"))?;
    eprintln!(
        "galion-stt: verified {} pinned files (sha256) in {} ms",
        models::LIB_FILES.len() + models::MODEL_FILES.len(),
        t.elapsed().as_millis()
    );
    let api = Api::open(&lib_dir.join("libmoonshine.so"))?;
    // SAFETY: plain C function with no arguments.
    let v = unsafe { (api.get_version)() };
    eprintln!("galion-stt: libmoonshine version {v}");
    Ok(Arc::new(api))
}

fn verify(mut a: Args) -> Result<(), String> {
    let (lib, model) = dirs(&mut a);
    let mut all = models::verify_dir(&lib, models::LIB_FILES);
    all.extend(models::verify_dir(&model, models::MODEL_FILES));
    let mut bad = 0;
    for c in &all {
        println!(
            "{}  {}  {}",
            if c.ok { "OK  " } else { "FAIL" },
            if c.sha256.is_empty() { "-" } else { &c.sha256 },
            c.name
        );
        if let Some(p) = &c.problem {
            println!("      {p}");
            bad += 1;
        }
    }
    if bad > 0 {
        Err(format!("{bad} file(s) failed"))
    } else {
        Ok(())
    }
}

fn serve(mut a: Args) -> Result<(), String> {
    ort_threads(&mut a);
    let (lib, model) = dirs(&mut a);
    let bind: SocketAddr = a
        .opt("--bind", Some("GALION_STT_BIND"))
        .unwrap_or_else(|| "127.0.0.1:8651".into())
        .parse()
        .map_err(|e| format!("bad --bind: {e}"))?;
    server::check_bind(&bind)?;
    let max_sessions: usize = a.num("--max-sessions", Some("GALION_STT_MAX_SESSIONS"), 2)?;
    let max_secs: u64 = a.num(
        "--max-session-secs",
        Some("GALION_STT_MAX_SESSION_SECS"),
        600,
    )?;
    let partial_ms: u64 = a.num("--partial-ms", Some("GALION_STT_PARTIAL_MS"), 300)?;
    if !(1..=8).contains(&max_sessions) {
        return Err("--max-sessions must be 1..=8".into());
    }
    if !a.rest.is_empty() {
        return Err(format!("unexpected arguments: {:?}", a.rest));
    }
    let api = open_engine(&lib, &model)?;
    let ecfg = EngineConfig::default();
    let t = Instant::now();
    let mut ts = Vec::new();
    for _ in 0..max_sessions {
        ts.push(Transcriber::load(Arc::clone(&api), &model, &ecfg)?);
    }
    eprintln!(
        "galion-stt: loaded {max_sessions} Tiny Streaming transcriber(s) in {} ms (ort_single_thread={})",
        t.elapsed().as_millis(),
        std::env::var("MOONSHINE_ORT_SINGLE_THREAD").unwrap_or_default() == "1"
    );
    let pool = EnginePool::new(ts);
    let cfg = ServerConfig {
        bind,
        max_session: Duration::from_secs(max_secs),
        partial_interval: Duration::from_millis(partial_ms),
        ..ServerConfig::default()
    };
    let state = AppState::new(pool, cfg);
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| format!("tokio: {e}"))?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(bind)
            .await
            .map_err(|e| format!("bind {bind}: {e}"))?;
        eprintln!("galion-stt: listening on http://{bind} (GET /health, WS /stream)");
        axum::serve(listener, server::router(state))
            .with_graceful_shutdown(async {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {},
                    _ = async { match term.as_mut() { Some(t) => { t.recv().await; } None => std::future::pending::<()>().await } } => {},
                }
                eprintln!("galion-stt: shutting down");
            })
            .await
            .map_err(|e| format!("serve: {e}"))
    })
}

fn proc_status_kb(key: &str) -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    s.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
}

fn load_wav_16k(path: &str, tail_silence_ms: u64) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    let w = parse_wav(&bytes)?;
    if w.sample_rate != SAMPLE_RATE {
        return Err(format!(
            "{path} is {} Hz; the wire format is 16 kHz (resample first)",
            w.sample_rate
        ));
    }
    let mut s = w.samples;
    s.extend(std::iter::repeat_n(
        0.0f32,
        (tail_silence_ms * u64::from(SAMPLE_RATE) / 1000) as usize,
    ));
    Ok(s)
}

/// Offline / real-time-paced benchmark straight on the engine.
fn transcribe(mut a: Args) -> Result<(), String> {
    ort_threads(&mut a);
    let (lib, model) = dirs(&mut a);
    let realtime = a.flag("--realtime");
    let frame_ms: u64 = a.num("--frame-ms", None, 20)?;
    let partial_ms: u64 = a.num("--partial-ms", None, 300)?;
    let tail: u64 = a.num("--tail-silence-ms", None, 0)?;
    let file = a
        .rest
        .first()
        .cloned()
        .ok_or("transcribe needs a WAV path")?;
    let samples = load_wav_16k(&file, tail)?;
    let rss_before = proc_status_kb("VmRSS:");
    let api = open_engine(&lib, &model)?;
    let t_load = Instant::now();
    let mut tr = Transcriber::load(api, &model, &EngineConfig::default())?;
    let load_ms = t_load.elapsed().as_millis() as u64;
    let rss_loaded = proc_status_kb("VmRSS:");
    let mut st = tr.open_stream()?;
    let frame = (frame_ms * u64::from(SAMPLE_RATE) / 1000) as usize;
    let mut tracker = LineTracker::default();
    let mut infer_ms: Vec<f64> = Vec::new();
    let mut first_partial: Option<(u64, u64)> = None; // (audio ms fed, wall ms)
    let mut finals: Vec<serde_json::Value> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    let t0 = Instant::now();
    let mut fed: usize = 0;
    let mut last_tx_audio: usize = 0;
    let mut last_tx_wall = Instant::now();
    let audio_ms_at = |n: usize| (n as u64) * 1000 / u64::from(SAMPLE_RATE);
    for chunk in samples.chunks(frame) {
        if realtime {
            let due = t0 + Duration::from_millis(audio_ms_at(fed));
            let now = Instant::now();
            if due > now {
                std::thread::sleep(due - now);
            }
        }
        st.add_audio(chunk, SAMPLE_RATE)?;
        fed += chunk.len();
        let tick = if realtime {
            last_tx_wall.elapsed() >= Duration::from_millis(partial_ms)
        } else {
            audio_ms_at(fed - last_tx_audio) >= partial_ms
        };
        if tick {
            let t = Instant::now();
            let lines = st.transcribe(false)?;
            infer_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            last_tx_audio = fed;
            last_tx_wall = Instant::now();
            for ev in tracker.update(&lines, false) {
                match ev {
                    ServerMsg::Partial { .. } => {
                        if first_partial.is_none() {
                            first_partial =
                                Some((audio_ms_at(fed), t0.elapsed().as_millis() as u64));
                        }
                    }
                    ServerMsg::Final { line, text } => {
                        let l = &lines[line as usize];
                        let end_ms = ((l.start_time + l.duration) * 1000.0) as i64;
                        let lag_audio = audio_ms_at(fed) as i64 - end_ms;
                        let lag_wall = t0.elapsed().as_millis() as i64 - end_ms;
                        finals.push(serde_json::json!({
                            "line": line, "end_ms": end_ms,
                            "audio_fed_after_end_ms": lag_audio,
                            "wall_after_end_ms": if realtime { Some(lag_wall) } else { None },
                        }));
                        texts.push(text);
                    }
                    ServerMsg::Error { .. } => {}
                }
            }
        }
    }
    let t_stop = Instant::now();
    let lines = st.finish()?;
    let stop_ms = t_stop.elapsed().as_secs_f64() * 1000.0;
    infer_ms.push(stop_ms);
    for ev in tracker.update(&lines, true) {
        if let ServerMsg::Final { text, .. } = ev {
            texts.push(text);
        }
    }
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let audio_ms = audio_ms_at(samples.len()) as f64;
    let total: f64 = infer_ms.iter().sum();
    let max = infer_ms.iter().cloned().fold(0.0, f64::max);
    let out = serde_json::json!({
        "file": Path::new(&file).file_name().map(|f| f.to_string_lossy().into_owned()),
        "mode": if realtime { "realtime" } else { "fast" },
        "ort_single_thread": std::env::var("MOONSHINE_ORT_SINGLE_THREAD").unwrap_or_default() == "1",
        "audio_ms": audio_ms,
        "load_ms": load_ms,
        "wall_ms": wall_ms.round(),
        "infer_calls": infer_ms.len(),
        "infer_ms_total": total.round(),
        "infer_ms_max": max.round(),
        "infer_ms_avg": (total / infer_ms.len().max(1) as f64).round(),
        "rtf_compute": total / audio_ms,
        "rtf_wall_fast": if realtime { None } else { Some(wall_ms / audio_ms) },
        "first_partial_audio_ms": first_partial.map(|f| f.0),
        "first_partial_wall_ms": first_partial.map(|f| f.1),
        "vad_finals": finals,
        "stop_to_final_ms": stop_ms.round(),
        "rss_kb_start": rss_before,
        "rss_kb_after_load": rss_loaded,
        "rss_kb_now": proc_status_kb("VmRSS:"),
        "rss_kb_peak": proc_status_kb("VmHWM:"),
        "partials": tracker.partials,
        "finals": tracker.finals,
        "text": texts.join(" "),
    });
    println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    Ok(())
}

/// Real-time WS client against a running service.
fn probe(mut a: Args) -> Result<(), String> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let frame_ms: u64 = a.num("--frame-ms", None, 20)?;
    let tail: u64 = a.num("--tail-silence-ms", None, 0)?;
    if a.rest.len() < 2 {
        return Err("probe needs URL and WAV".into());
    }
    let url = a.rest[0].clone();
    let samples = load_wav_16k(&a.rest[1], tail)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let (ws, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .map_err(|e| format!("connect {url}: {e}"))?;
        let (mut tx, mut rx) = ws.split();
        let frame = (frame_ms * u64::from(SAMPLE_RATE) / 1000) as usize;
        let t0 = Instant::now();
        let audio_ms = samples.len() as u64 * 1000 / u64::from(SAMPLE_RATE);
        let sender = tokio::spawn(async move {
            let mut fed = 0usize;
            for c in samples.chunks(frame) {
                let due = t0 + Duration::from_millis(fed as u64 * 1000 / u64::from(SAMPLE_RATE));
                tokio::time::sleep_until(due.into()).await;
                if tx.send(Message::Binary(f32_to_pcm16le(c).into())).await.is_err() {
                    return None;
                }
                fed += c.len();
            }
            let stop_at = t0.elapsed().as_millis() as u64;
            let _ = tx
                .send(Message::Text(r#"{"type":"stop"}"#.into()))
                .await;
            Some((stop_at, tx))
        });
        let mut events = Vec::new();
        let mut first_partial = None;
        let mut texts = Vec::new();
        let mut last_final_ms = None;
        let mut close_code = None;
        while let Some(Ok(m)) = rx.next().await {
            let ms = t0.elapsed().as_millis() as u64;
            match m {
                Message::Text(t) => {
                    let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                    let ty = v["type"].as_str().unwrap_or("").to_string();
                    if ty == "partial" && first_partial.is_none() {
                        first_partial = Some(ms);
                    }
                    if ty == "final" {
                        texts.push(v["text"].as_str().unwrap_or("").to_string());
                        last_final_ms = Some(ms);
                    }
                    events.push(serde_json::json!({"t_ms": ms, "type": ty, "line": v["line"], "text": v["text"], "message": v["message"]}));
                }
                Message::Close(f) => {
                    close_code = f.map(|f| u16::from(f.code));
                    break;
                }
                _ => {}
            }
        }
        let stop_at = sender.await.ok().flatten().map(|s| s.0);
        let out = serde_json::json!({
            "audio_ms": audio_ms,
            "first_partial_ms": first_partial,
            "stop_sent_ms": stop_at,
            "last_final_ms": last_final_ms,
            "stop_to_last_final_ms": match (stop_at, last_final_ms) { (Some(s), Some(f)) => Some(f as i64 - s as i64), _ => None },
            "partials": events.iter().filter(|e| e["type"] == "partial").count(),
            "finals": texts.len(),
            "close_code": close_code,
            "text": texts.join(" "),
            "events": events,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
        Ok(())
    })
}
