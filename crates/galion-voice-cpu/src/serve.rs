// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! A small std-only HTTP voice service around the [`SpeechToText`] and
//! [`TextToSpeech`] traits. It speaks the two wire contracts Galion One
//! already uses, so one process can stand in for both of them:
//!
//! * the **ear** (`GALION_EAR`): `POST /v1/audio/transcriptions`, an
//!   OpenAI-style multipart upload (field `file`) or a raw `audio/wav` body,
//!   answered with `{"text":"..."}`;
//! * the **mouth** (`GALION_MOUTH`): `POST /synthesize` with
//!   `{"text":"...","voice":"..."}`, answered with a 16-bit mono WAV;
//! * `POST /v1/voice`: WAV in, transcript, echo reply, WAV out (round trip for
//!   tests; no text model is wired in, so no paid API can be reached);
//! * `GET /health`: 200 only when every engine this process was asked to
//!   run is loaded, 503 with the reason otherwise (also while loading). The
//!   body keeps the legacy `tts_server.py` fields (`ok`, `status`, `provider`,
//!   `model`, `loaded`, `error`, `device`, `voices`, `bind`) and never claims a
//!   model that is not loaded.
//!
//! Engines run one call at a time each (a `Mutex`), which keeps CPU use
//! bounded on a shared box. There is no async runtime and no new crate.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::resample::{i16_to_f32, linear};
use crate::stt::{SpeechToText, TextToSpeech};
use crate::wav;

/// Largest request body (a WAV upload). 16 MB is about 8 min of 16 kHz mono.
pub const MAX_BODY: usize = 16 * 1024 * 1024;
/// Longest text one synth call accepts (chars), as Galion One's mouth.
pub const MAX_CHARS: usize = 300;
/// Longest audio one transcription call accepts (seconds).
pub const MAX_AUDIO_S: f32 = 120.0;
/// Concurrent connections served; extra ones get 503 at once.
pub const MAX_CONNS: usize = 16;

/// One loaded speech-to-text engine.
pub struct SttSlot {
    pub name: String,
    pub engine: Mutex<Box<dyn SpeechToText + Send>>,
}

/// One loaded text-to-speech engine.
pub struct TtsSlot {
    pub name: String,
    pub voices: Vec<String>,
    pub default_voice: String,
    /// Voices listed by `/health` when they differ from `voices` (legacy
    /// Chatterbox lists only voices that have a reference clip).
    pub health_voices: Option<Vec<String>>,
    pub engine: Mutex<Box<dyn TextToSpeech + Send>>,
}

/// What the service has loaded. A missing engine keeps its reason so
/// `/health` can say why instead of pretending to be "loading".
#[derive(Default)]
pub struct Engines {
    pub stt: Option<SttSlot>,
    pub tts: Option<TtsSlot>,
    pub stt_missing: String,
    pub tts_missing: String,
    /// Engines are still loading (health says "loading", 503).
    pub loading: bool,
    /// Engines this process was asked to run (a missing one fails health).
    pub want_stt: bool,
    pub want_tts: bool,
    /// Listen address, echoed by `/health` like the legacy server.
    pub bind: String,
}

/// The engines a running server answers with; swapped once loading ends.
pub type EngineCell = RwLock<Arc<Engines>>;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    fn json(status: u16, body: String) -> Response {
        Response {
            status,
            content_type: "application/json",
            headers: Vec::new(),
            body: body.into_bytes(),
        }
    }
    fn error(status: u16, msg: &str) -> Response {
        Response::json(status, format!("{{\"error\":{}}}", quote(msg)))
    }
}

/// JSON string literal for `s`.
pub fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// The string value of top-level-ish key `key` in a small JSON object, with
/// the usual escapes decoded. `None` when the key is absent or not a string.
pub fn json_str(body: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let mut from = 0;
    while let Some(i) = body[from..].find(&pat) {
        let after = from + i + pat.len();
        let rest = body[after..].trim_start();
        if let Some(rest) = rest.strip_prefix(':') {
            let rest = rest.trim_start();
            let mut chars = rest.strip_prefix('"')?.chars();
            let mut out = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '"' => return Some(out),
                    '\\' => match chars.next()? {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'b' | 'f' => out.push(' '),
                        'u' => {
                            let hex: String = chars.by_ref().take(4).collect();
                            let cp = u32::from_str_radix(&hex, 16).ok()?;
                            out.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                        }
                        other => out.push(other),
                    },
                    c => out.push(c),
                }
            }
            return None;
        }
        from = after;
    }
    None
}

/// The audio bytes of a request: the `file` part of a multipart upload, or
/// the raw body for `audio/*` and `application/octet-stream`.
pub fn audio_part<'a>(content_type: &str, body: &'a [u8]) -> Result<&'a [u8], String> {
    let ct = content_type.to_ascii_lowercase();
    if !ct.starts_with("multipart/form-data") {
        return Ok(body);
    }
    let boundary = content_type
        .split(';')
        .filter_map(|p| p.trim().strip_prefix("boundary="))
        .next()
        .map(|b| b.trim_matches('"'))
        .ok_or("multipart without boundary")?;
    let delim = format!("--{boundary}");
    let d = delim.as_bytes();
    let mut pos = 0;
    while let Some(start) = find(&body[pos..], d).map(|i| pos + i + d.len()) {
        let Some(head_end) = find(&body[start..], b"\r\n\r\n").map(|i| start + i) else {
            break;
        };
        let head = String::from_utf8_lossy(&body[start..head_end]).to_ascii_lowercase();
        let data = head_end + 4;
        let end = find(&body[data..], format!("\r\n{delim}").as_bytes())
            .map(|i| data + i)
            .unwrap_or(body.len());
        if head.contains("name=\"file\"") {
            return Ok(&body[data..end]);
        }
        pos = end;
    }
    Err("multipart has no \"file\" part".into())
}

/// The text value of multipart form field `name` (e.g. `language`), if any.
pub fn form_field(content_type: &str, body: &[u8], name: &str) -> Option<String> {
    let boundary = content_type
        .split(';')
        .filter_map(|p| p.trim().strip_prefix("boundary="))
        .next()?
        .trim_matches('"');
    let delim = format!("--{boundary}");
    let d = delim.as_bytes();
    let want = format!("name=\"{}\"", name.to_ascii_lowercase());
    let mut pos = 0;
    while let Some(start) = find(&body[pos..], d).map(|i| pos + i + d.len()) {
        let head_end = find(&body[start..], b"\r\n\r\n").map(|i| start + i)?;
        let head = String::from_utf8_lossy(&body[start..head_end]).to_ascii_lowercase();
        let data = head_end + 4;
        let end = find(&body[data..], format!("\r\n{delim}").as_bytes())
            .map(|i| data + i)
            .unwrap_or(body.len());
        if head.contains(&want) {
            let v = String::from_utf8_lossy(&body[data..end]).trim().to_string();
            return Some(v).filter(|v| !v.is_empty() && v.len() <= 32);
        }
        pos = end;
    }
    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Process CPU time (user + system) in ms, from /proc (Linux); 0 elsewhere.
pub fn proc_cpu_ms() -> u64 {
    let Ok(s) = std::fs::read_to_string("/proc/self/stat") else {
        return 0;
    };
    // Fields after the ")" of the command name: utime is the 12th, stime 13th.
    let Some(rest) = s.rsplit(')').next() else {
        return 0;
    };
    let f: Vec<&str> = rest.split_whitespace().collect();
    let tick = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    // USER_HZ is 100 on every Linux target we build for.
    (tick(11) + tick(12)) * 10
}

/// `VmRSS`/`VmHWM` of this process in kB, from /proc (Linux); 0 elsewhere.
pub fn mem_kb(field: &str) -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with(field))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse().ok())
        })
        .unwrap_or(0)
}

/// Header-safe form of a transcript: printable ASCII kept, the rest as %XX.
pub fn header_safe(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if (0x20..0x7f).contains(&b) && b != b'%' {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}

impl Engines {
    fn health(&self) -> Response {
        let stt = self.stt.as_ref().map(|s| quote(&s.name));
        let tts = self.tts.as_ref().map(|t| quote(&t.name));
        let voices = self
            .tts
            .as_ref()
            .map(|t| {
                t.health_voices
                    .as_ref()
                    .unwrap_or(&t.voices)
                    .iter()
                    .map(|v| quote(v))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let any = stt.is_some() || tts.is_some();
        let stt_ok = !self.want_stt || stt.is_some();
        let tts_ok = !self.want_tts || tts.is_some();
        let ok = !self.loading && any && stt_ok && tts_ok;
        let status = if self.loading {
            "loading"
        } else if ok {
            "ok"
        } else if any {
            "partial"
        } else {
            "unavailable"
        };
        let mut errors = Vec::new();
        if self.stt.is_none() && !self.stt_missing.is_empty() {
            errors.push(format!("stt: {}", self.stt_missing));
        }
        if self.tts.is_none() && !self.tts_missing.is_empty() {
            errors.push(format!("tts: {}", self.tts_missing));
        }
        let error = if self.loading || errors.is_empty() {
            "null".to_string()
        } else {
            quote(&errors.join("; "))
        };
        // Legacy "model" names the mouth's model; an ear-only process names
        // its STT model.
        let model = tts
            .clone()
            .or_else(|| stt.clone())
            .unwrap_or_else(|| "null".into());
        let body = format!(
            "{{\"ok\":{ok},\"status\":\"{status}\",\"provider\":\"local\",\"model\":{model},\
             \"loaded\":{loaded},\"error\":{error},\"device\":\"cpu\",\"voices\":[{voices}],\
             \"bind\":{bind},\"runtime\":\"galion-voice-cpu (Rust, candle)\",\"stt\":{},\"tts\":{},\
             \"stt_missing\":{},\"tts_missing\":{},\"rss_kb\":{},\"hwm_kb\":{}}}",
            stt.unwrap_or_else(|| "null".into()),
            tts.unwrap_or_else(|| "null".into()),
            quote(&self.stt_missing),
            quote(&self.tts_missing),
            mem_kb("VmRSS"),
            mem_kb("VmHWM"),
            loaded = ok,
            bind = quote(&self.bind),
        );
        Response::json(if ok { 200 } else { 503 }, body)
    }

    /// WAV bytes to text. Returns (text, audio seconds).
    pub fn transcribe_wav(&self, bytes: &[u8]) -> Result<(String, f32), (u16, String)> {
        self.transcribe_wav_lang(bytes, None)
    }

    /// As [`Self::transcribe_wav`] with the request's `language` form field.
    pub fn transcribe_wav_lang(
        &self,
        bytes: &[u8],
        language: Option<&str>,
    ) -> Result<(String, f32), (u16, String)> {
        let slot = self
            .stt
            .as_ref()
            .ok_or_else(|| (503, format!("no speech-to-text: {}", self.stt_missing)))?;
        let w = wav::parse(bytes).map_err(|e| (400, format!("wav: {e}")))?;
        if w.sample_rate == 0 {
            return Err((400, "wav: sample rate 0".into()));
        }
        let secs = w.samples.len() as f32 / w.sample_rate as f32;
        if secs > MAX_AUDIO_S {
            return Err((413, format!("audio longer than {MAX_AUDIO_S} s")));
        }
        let mut eng = slot
            .engine
            .lock()
            .map_err(|_| (500, "stt engine poisoned".to_string()))?;
        if let Some(l) = language {
            eng.set_language(l);
        }
        let rate = eng.sample_rate();
        let pcm = linear(&i16_to_f32(&w.samples), w.sample_rate, rate);
        // Whisper sees at most 30 s per call; longer audio goes in windows.
        let win = rate as usize * 30;
        let mut parts = Vec::new();
        for chunk in pcm.chunks(win.max(1)) {
            let t = eng
                .transcribe(chunk)
                .map_err(|e| (500, format!("stt: {e}")))?;
            if !t.trim().is_empty() {
                parts.push(t.trim().to_string());
            }
        }
        Ok((parts.join(" "), secs))
    }

    /// Text to WAV bytes. Returns (wav, audio seconds, voice used).
    pub fn synth_wav(
        &self,
        text: &str,
        voice: &str,
    ) -> Result<(Vec<u8>, f32, String), (u16, String)> {
        let slot = self
            .tts
            .as_ref()
            .ok_or_else(|| (503, format!("no text-to-speech: {}", self.tts_missing)))?;
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            return Err((400, "text required".into()));
        }
        let voice = voice.trim().to_lowercase();
        let voice = if slot.voices.contains(&voice) {
            voice
        } else {
            slot.default_voice.clone()
        };
        let mut eng = slot
            .engine
            .lock()
            .map_err(|_| (500, "tts engine poisoned".to_string()))?;
        let text: String = text.chars().take(eng.max_chars()).collect();
        let rate = eng.sample_rate();
        let mut pcm: Vec<i16> = Vec::new();
        for chunk in eng.chunks(&text) {
            let part = eng
                .synthesize(chunk, &voice)
                .map_err(|e| (500, format!("tts: {e}")))?;
            pcm.extend_from_slice(&part);
        }
        let secs = pcm.len() as f32 / rate.max(1) as f32;
        Ok((wav::encode(&pcm, rate), secs, voice))
    }

    pub fn handle(&self, req: &Request) -> Response {
        let t0 = Instant::now();
        let c0 = proc_cpu_ms();
        let mut resp = match (req.method.as_str(), req.path.as_str()) {
            ("GET", "/health") => return self.health(),
            ("POST", "/v1/audio/transcriptions") | ("POST", "/stt") => {
                let lang = form_field(&req.content_type, &req.body, "language");
                match audio_part(&req.content_type, &req.body)
                    .map_err(|e| (400, e))
                    .and_then(|a| self.transcribe_wav_lang(a, lang.as_deref()))
                {
                    Ok((text, secs)) => Response::json(
                        200,
                        format!(
                            "{{\"text\":{},\"audio_s\":{secs:.2},\"ms\":{},\"cpu_ms\":{}}}",
                            quote(&text),
                            t0.elapsed().as_millis(),
                            proc_cpu_ms().saturating_sub(c0)
                        ),
                    ),
                    Err((code, e)) => Response::error(code, &e),
                }
            }
            ("POST", "/synthesize") | ("POST", "/tts") => {
                let body = String::from_utf8_lossy(&req.body);
                let text = json_str(&body, "text").unwrap_or_default();
                let voice = json_str(&body, "voice").unwrap_or_default();
                match self.synth_wav(&text, &voice) {
                    Ok((wav, secs, voice)) => Response {
                        status: 200,
                        content_type: "audio/wav",
                        headers: vec![
                            // Legacy headers: wall/audio seconds, audio seconds.
                            (
                                "X-RTF".into(),
                                format!(
                                    "{:.3}",
                                    t0.elapsed().as_secs_f64() / f64::from(secs.max(0.001))
                                ),
                            ),
                            ("X-Duration".into(), format!("{:.3}", secs.max(0.001))),
                            ("X-Audio-S".into(), format!("{secs:.2}")),
                            ("X-Voice".into(), voice),
                        ],
                        body: wav,
                    },
                    Err((code, e)) => Response::error(code, &e),
                }
            }
            ("POST", "/v1/voice") => self.round_trip(req),
            (_, "/health" | "/v1/audio/transcriptions" | "/synthesize" | "/v1/voice") => {
                Response::error(405, "method not allowed")
            }
            _ => Response::error(404, "not found"),
        };
        resp.headers
            .push(("X-Wall-Ms".into(), t0.elapsed().as_millis().to_string()));
        resp.headers.push((
            "X-Cpu-Ms".into(),
            proc_cpu_ms().saturating_sub(c0).to_string(),
        ));
        resp
    }

    fn round_trip(&self, req: &Request) -> Response {
        let t0 = Instant::now();
        let audio = match audio_part(&req.content_type, &req.body) {
            Ok(a) => a,
            Err(e) => return Response::error(400, &e),
        };
        let (heard, _) = match self.transcribe_wav(audio) {
            Ok(v) => v,
            Err((code, e)) => return Response::error(code, &e),
        };
        let stt_ms = t0.elapsed().as_millis();
        let reply = if heard.is_empty() {
            "I did not catch that.".to_string()
        } else {
            format!("I heard: {heard}")
        };
        let t1 = Instant::now();
        match self.synth_wav(&reply, "") {
            Ok((wav, secs, _)) => Response {
                status: 200,
                content_type: "audio/wav",
                headers: vec![
                    ("X-Transcript".into(), header_safe(&heard)),
                    ("X-Stt-Ms".into(), stt_ms.to_string()),
                    ("X-Tts-Ms".into(), t1.elapsed().as_millis().to_string()),
                    ("X-Audio-S".into(), format!("{secs:.2}")),
                ],
                body: wav,
            },
            // No mouth: still give the ear's answer.
            Err((503, _)) => Response::json(
                200,
                format!(
                    "{{\"text\":{},\"stt_ms\":{stt_ms},\"audio\":null}}",
                    quote(&heard)
                ),
            ),
            Err((code, e)) => Response::error(code, &e),
        }
    }
}

fn reason(code: u16) -> &'static str {
    match code {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        _ => "Service Unavailable",
    }
}

pub fn write_response(s: &mut impl Write, r: &Response) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        r.status,
        reason(r.status),
        r.content_type,
        r.body.len()
    );
    for (k, v) in &r.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    s.write_all(head.as_bytes())?;
    s.write_all(&r.body)?;
    s.flush()
}

/// Read one HTTP/1.1 request (headers up to 64 KB, body up to [`MAX_BODY`]).
pub fn read_request(s: &mut impl Read) -> Result<Request, Response> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 8192];
    let head_end = loop {
        let n = s.read(&mut tmp).map_err(|_| Response::error(400, "read"))?;
        if n == 0 {
            return Err(Response::error(400, "eof"));
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 64 * 1024 {
            return Err(Response::error(400, "headers too large"));
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let mut first = lines.next().unwrap_or("").split_whitespace();
    let method = first.next().unwrap_or("").to_string();
    let path = first
        .next()
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let mut length = 0usize;
    let mut content_type = String::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            match k.trim().to_ascii_lowercase().as_str() {
                "content-length" => length = v.trim().parse().unwrap_or(usize::MAX),
                "content-type" => content_type = v.trim().to_string(),
                _ => {}
            }
        }
    }
    if length > MAX_BODY {
        return Err(Response::error(413, "body too large"));
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < length {
        let n = s.read(&mut tmp).map_err(|_| Response::error(400, "read"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(length);
    Ok(Request {
        method,
        path,
        content_type,
        body,
    })
}

fn serve_conn(cell: &EngineCell, mut s: TcpStream) {
    let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = s.set_write_timeout(Some(Duration::from_secs(30)));
    let resp = match read_request(&mut s) {
        Ok(req) => {
            // Snapshot: a request keeps the engines it started with.
            let engines = cell.read().map(|g| g.clone());
            let r = match engines {
                Ok(e) => e.handle(&req),
                Err(_) => Response::error(500, "engine table poisoned"),
            };
            eprintln!(
                "voice: {} {} -> {} ({} B in, {} B out)",
                req.method,
                req.path,
                r.status,
                req.body.len(),
                r.body.len()
            );
            r
        }
        Err(r) => r,
    };
    let _ = write_response(&mut s, &resp);
}

/// Accept loop: one thread per connection, at most [`MAX_CONNS`] at a time.
pub fn serve(listener: TcpListener, engines: Arc<Engines>) {
    serve_cell(listener, Arc::new(RwLock::new(engines)));
}

/// [`serve`] over a swappable engine table: start with `loading: true`, bind
/// at once so `/health` answers truthfully, and store the loaded engines
/// when they are ready.
pub fn serve_cell(listener: TcpListener, engines: Arc<EngineCell>) {
    let live = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        if live.load(Ordering::SeqCst) >= MAX_CONNS {
            let _ = write_response(&mut s, &Response::error(503, "busy"));
            continue;
        }
        live.fetch_add(1, Ordering::SeqCst);
        let (e, l) = (engines.clone(), live.clone());
        std::thread::spawn(move || {
            serve_conn(&e, s);
            l.fetch_sub(1, Ordering::SeqCst);
        });
    }
}

/// Licences a model file may carry before this service loads it (SPDX ids,
/// lower case). Same list as the swarm rules for code.
pub const ALLOWED_MODEL_LICENCES: [&str; 10] = [
    "mit",
    "apache-2.0",
    "bsd-2-clause",
    "bsd-3-clause",
    "isc",
    "zlib",
    "unicode-3.0",
    "0bsd",
    "cc0-1.0",
    "mit or apache-2.0",
];

/// The licence a model directory declares: the `license:` line of a Hugging
/// Face model card (`README.md` front matter), or a recognised `LICENSE`
/// text. `None` when nothing is declared (which means: do not load).
pub fn declared_licence(readme: Option<&str>, license_file: Option<&str>) -> Option<String> {
    if let Some(r) = readme {
        if let Some(front) = r.strip_prefix("---") {
            let front = front.split("\n---").next().unwrap_or("");
            for line in front.lines() {
                if let Some(v) = line.trim().strip_prefix("license:") {
                    let v = v.trim().trim_matches('"').to_ascii_lowercase();
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
        }
    }
    let l = license_file?;
    let head: String = l.chars().take(4000).collect();
    if head.contains("MIT License") || head.contains("Permission is hereby granted, free of charge")
    {
        Some("mit".into())
    } else if head.contains("Apache License") && head.contains("Version 2.0") {
        Some("apache-2.0".into())
    } else if head.contains("Redistribution and use in source and binary forms") {
        // BSD-2 has two numbered conditions, BSD-3 adds the endorsement clause.
        if head.contains("endorse or promote") {
            Some("bsd-3-clause".into())
        } else {
            Some("bsd-2-clause".into())
        }
    } else {
        None
    }
}

/// Ok(licence) when the directory's declared licence is on the allow list.
pub fn check_model_dir(dir: &std::path::Path) -> Result<String, String> {
    let readme = std::fs::read_to_string(dir.join("README.md")).ok();
    let lic = std::fs::read_to_string(dir.join("LICENSE")).ok();
    match declared_licence(readme.as_deref(), lic.as_deref()) {
        Some(l) if ALLOWED_MODEL_LICENCES.contains(&l.as_str()) => Ok(l),
        Some(l) => Err(format!(
            "{}: licence {l} is not on the allow list",
            dir.display()
        )),
        None => Err(format!(
            "{}: no declared licence (README.md front matter or LICENSE); not loading",
            dir.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeStt;
    impl SpeechToText for FakeStt {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
            Ok(format!("{} samples", pcm.len()))
        }
    }
    struct FakeTts;
    impl TextToSpeech for FakeTts {
        fn sample_rate(&self) -> u32 {
            24_000
        }
        fn synthesize(&mut self, text: &str, _: &str) -> Result<Vec<i16>, String> {
            Ok(vec![1; text.len() * 10])
        }
    }

    fn both() -> Engines {
        Engines {
            stt: Some(SttSlot {
                name: "fake-stt".into(),
                engine: Mutex::new(Box::new(FakeStt)),
            }),
            tts: Some(TtsSlot {
                name: "fake-tts".into(),
                voices: vec!["a".into(), "b".into()],
                default_voice: "a".into(),
                health_voices: None,
                engine: Mutex::new(Box::new(FakeTts)),
            }),
            ..Default::default()
        }
    }

    fn req(method: &str, path: &str, ct: &str, body: &[u8]) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            content_type: ct.into(),
            body: body.to_vec(),
        }
    }

    #[test]
    fn health_is_honest() {
        let none = Engines {
            stt_missing: "no model dir".into(),
            tts_missing: "licence".into(),
            ..Default::default()
        };
        let r = none.handle(&req("GET", "/health", "", b""));
        assert_eq!(r.status, 503);
        let b = String::from_utf8(r.body).unwrap();
        assert!(b.contains("\"status\":\"unavailable\"") && b.contains("no model dir"));
        assert!(!b.contains("chatterbox") && !b.contains("cuda"));
        let r = both().handle(&req("GET", "/health", "", b""));
        assert_eq!(r.status, 200);
        assert!(String::from_utf8(r.body)
            .unwrap()
            .contains("\"stt\":\"fake-stt\""));
    }

    #[test]
    fn stt_raw_and_multipart() {
        let wav = wav::encode(&vec![0i16; 8000], 8000); // 1 s at 8 kHz
        let e = both();
        let r = e.handle(&req("POST", "/v1/audio/transcriptions", "audio/wav", &wav));
        assert_eq!(r.status, 200);
        assert!(String::from_utf8(r.body)
            .unwrap()
            .contains("\"text\":\"16000 samples\""));
        let mut mp = b"--XyZ\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nw\r\n--XyZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"a.wav\"\r\nContent-Type: audio/wav\r\n\r\n".to_vec();
        mp.extend_from_slice(&wav);
        mp.extend_from_slice(b"\r\n--XyZ--\r\n");
        let r = e.handle(&req(
            "POST",
            "/v1/audio/transcriptions",
            "multipart/form-data; boundary=XyZ",
            &mp,
        ));
        assert_eq!(r.status, 200, "{:?}", String::from_utf8_lossy(&r.body));
        let r = e.handle(&req("POST", "/stt", "audio/wav", b"nope"));
        assert_eq!(r.status, 400);
    }

    #[test]
    fn synth_and_round_trip() {
        let e = both();
        let r = e.handle(&req(
            "POST",
            "/synthesize",
            "application/json",
            br#"{"text":"Hi \"there\"","voice":"b"}"#,
        ));
        assert_eq!(r.status, 200);
        assert_eq!(&r.body[..4], b"RIFF");
        assert!(r.headers.iter().any(|(k, v)| k == "X-Voice" && v == "b"));
        let r = e.handle(&req(
            "POST",
            "/synthesize",
            "application/json",
            br#"{"text":"  "}"#,
        ));
        assert_eq!(r.status, 400);
        let wav = wav::encode(&vec![0i16; 1600], 16000);
        let r = e.handle(&req("POST", "/v1/voice", "audio/wav", &wav));
        assert_eq!(r.status, 200);
        assert!(r
            .headers
            .iter()
            .any(|(k, v)| k == "X-Transcript" && v == "1600 samples"));
        let none = Engines::default();
        let r = none.handle(&req("POST", "/synthesize", "", br#"{"text":"hi"}"#));
        assert_eq!(r.status, 503);
        assert_eq!(none.handle(&req("GET", "/x", "", b"")).status, 404);
        assert_eq!(none.handle(&req("GET", "/synthesize", "", b"")).status, 405);
    }

    #[test]
    fn json_helpers() {
        assert_eq!(
            json_str(r#"{"a":1,"text" : "x\ny\u0041"}"#, "text").unwrap(),
            "x\nyA"
        );
        assert_eq!(json_str(r#"{"text":5}"#, "text"), None);
        assert_eq!(quote("a\"b\\\n"), "\"a\\\"b\\\\\\n\"");
        assert_eq!(header_safe("ok é"), "ok %C3%A9");
    }

    #[test]
    fn licence_gate() {
        let card = "---\nlanguage: en\nlicense: apache-2.0\n---\n# x";
        assert_eq!(declared_licence(Some(card), None).unwrap(), "apache-2.0");
        let nc = "---\nlicense: cc-by-nc-4.0\n---\n";
        assert_eq!(declared_licence(Some(nc), None).unwrap(), "cc-by-nc-4.0");
        assert!(!ALLOWED_MODEL_LICENCES.contains(&"cc-by-nc-4.0"));
        let cmu = "Copyright (C) 1993-2015 Carnegie Mellon University.\nRedistribution and use in source and binary forms, with or without";
        assert_eq!(declared_licence(None, Some(cmu)).unwrap(), "bsd-2-clause");
        assert_eq!(declared_licence(Some("# no front matter"), None), None);
        assert_eq!(
            declared_licence(None, Some("MIT License\n\nCopyright")).unwrap(),
            "mit"
        );
    }

    #[test]
    fn http_parse() {
        let raw = b"POST /synthesize?x=1 HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 4\r\n\r\nabcdEXTRA";
        let r = read_request(&mut &raw[..]).unwrap();
        assert_eq!(
            (r.method.as_str(), r.path.as_str()),
            ("POST", "/synthesize")
        );
        assert_eq!(r.body, b"abcd");
        let big = format!(
            "POST / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        assert_eq!(read_request(&mut big.as_bytes()).unwrap_err().status, 413);
    }
}
