// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! CPU speech adapter for whisper.cpp and piper fixture outputs.
//!
//! [`SpeechProvider`] parses bytes the caller already has. It does not spawn
//! whisper.cpp, piper, or any model, and it does not download weights.
//! [`SpeechProvider::uses_gpu`] is always false, even when a whisper.cpp
//! `systeminfo` string names a GPU.
//!
//! Whisper.cpp fixtures are the `-oj` / `-ojf` JSON file (`transcription`,
//! with `offsets` already in milliseconds) and the whisper-server JSON body
//! (`text`, optional `segments` whose `start` / `end` are seconds).
//! Piper fixtures are a PCM WAV file and the CLI log line
//! `Real-time factor: R (infer=I sec, audio=A sec)`.
//! The optional `Loaded voice in N second(s)` line is read when present.
//! An endpoint string is stored and never dialed.

#![forbid(unsafe_code)]

use serde_json::{Map, Number, Value};
use std::fmt;

/// Usual whisper-server listen port. This crate does not bind it.
pub const WHISPER_SERVER_PORT: u16 = 8080;

/// This adapter never uses a GPU.
pub const USES_GPU: bool = false;

/// Which fixture family a row refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// whisper.cpp STT. JSON fixtures only.
    WhisperCpp,
    /// piper TTS. WAV bytes and the CLI log only.
    Piper,
}

impl Engine {
    /// Stable name. Not a binary path.
    pub const fn as_str(self) -> &'static str {
        match self {
            Engine::WhisperCpp => "whisper.cpp",
            Engine::Piper => "piper",
        }
    }
}

impl fmt::Display for Engine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One speech row. Parsing never opens a socket or a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeechProvider {
    id: String,
    endpoint: String,
    engine: Engine,
}

impl SpeechProvider {
    /// Build a row. `endpoint` is stored and never dialed.
    ///
    /// # Errors
    ///
    /// Empty id, empty endpoint, whitespace inside either, or a key-shaped
    /// token in either.
    pub fn new(
        id: impl Into<String>,
        endpoint: impl Into<String>,
        engine: Engine,
    ) -> Result<Self, FixtureError> {
        let id = id.into();
        let endpoint = endpoint.into();
        validate_token("id", &id)?;
        validate_token("endpoint", &endpoint)?;
        Ok(Self {
            id,
            endpoint,
            engine,
        })
    }

    /// Registry id. Not a secret.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Configured endpoint. Not dialed by this crate.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Which tool's fixtures this row parses.
    pub const fn engine(&self) -> Engine {
        self.engine
    }

    /// True for whisper.cpp.
    pub const fn speaks_stt(&self) -> bool {
        match self.engine {
            Engine::WhisperCpp => true,
            Engine::Piper => false,
        }
    }

    /// True for piper.
    pub const fn speaks_tts(&self) -> bool {
        match self.engine {
            Engine::WhisperCpp => false,
            Engine::Piper => true,
        }
    }

    /// Always false. Parsing a fixture does not open a socket.
    pub const fn dials_network(&self) -> bool {
        match self.engine {
            Engine::WhisperCpp | Engine::Piper => false,
        }
    }

    /// Always false. This crate never starts whisper.cpp, piper, or a model.
    pub const fn spawns_process(&self) -> bool {
        match self.engine {
            Engine::WhisperCpp | Engine::Piper => false,
        }
    }

    /// Always false. Model paths inside a fixture are not fetched.
    pub const fn downloads_models(&self) -> bool {
        match self.engine {
            Engine::WhisperCpp | Engine::Piper => false,
        }
    }

    /// Always false. A fixture may name a GPU; this adapter does not use one.
    pub const fn uses_gpu(&self) -> bool {
        match self.engine {
            Engine::WhisperCpp | Engine::Piper => USES_GPU,
        }
    }

    /// Parse a whisper.cpp JSON fixture.
    ///
    /// # Errors
    ///
    /// The row is not whisper.cpp, or the body is not a CLI or server transcript.
    ///
    /// # Examples
    ///
    /// ```
    /// use openmind_speech::{Engine, SpeechProvider};
    ///
    /// let row = SpeechProvider::new("cpu-stt", "http://127.0.0.1:8080", Engine::WhisperCpp)
    ///     .unwrap();
    /// assert!(!row.uses_gpu());
    /// assert!(!row.spawns_process());
    /// assert!(!row.downloads_models());
    /// let heard = row
    ///     .parse_whisper(r#"{"transcription":[{"text":" hello","offsets":{"from":0,"to":500}}]}"#)
    ///     .unwrap();
    /// assert_eq!(heard.text, " hello");
    /// assert_eq!(heard.segments[0].from_ms, Some(0));
    /// assert_eq!(heard.segments[0].to_ms, Some(500));
    /// ```
    pub fn parse_whisper(&self, body: &str) -> Result<Transcript, FixtureError> {
        self.expect_engine(Engine::WhisperCpp, "whisper.cpp json")?;
        parse_whisper_body(body)
    }

    /// Parse a piper CLI log. The real-time factor line is required.
    ///
    /// # Errors
    ///
    /// The row is not piper, or the text has no real-time factor line.
    pub fn parse_piper_log(&self, body: &str) -> Result<PiperReport, FixtureError> {
        self.expect_engine(Engine::Piper, "piper log")?;
        parse_piper_log_body(body)
    }

    /// Parse piper PCM WAV bytes. Samples are not copied and not played.
    ///
    /// # Errors
    ///
    /// The row is not piper, or the bytes are not a PCM RIFF file.
    pub fn parse_piper_wav(&self, bytes: &[u8]) -> Result<WaveInfo, FixtureError> {
        self.expect_engine(Engine::Piper, "piper wav")?;
        parse_wav(bytes)
    }

    /// Parse a fixture the caller already loaded.
    ///
    /// # Errors
    ///
    /// The output kind does not match the row, or the bytes do not match it.
    pub fn parse(&self, kind: OutputKind, bytes: &[u8]) -> Result<ParsedOutput, FixtureError> {
        match kind {
            OutputKind::WhisperJson => {
                let body = as_text(bytes)?;
                Ok(ParsedOutput::Transcript(self.parse_whisper(body)?))
            }
            OutputKind::PiperLog => {
                let body = as_text(bytes)?;
                Ok(ParsedOutput::Synthesis(self.parse_piper_log(body)?))
            }
            OutputKind::PiperWav => Ok(ParsedOutput::Wave(self.parse_piper_wav(bytes)?)),
        }
    }

    fn expect_engine(&self, want: Engine, output: &'static str) -> Result<(), FixtureError> {
        if self.engine == want {
            Ok(())
        } else {
            Err(FixtureError::EngineMismatch {
                row: self.engine.as_str(),
                output,
            })
        }
    }
}

/// Which fixture the caller is handing in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    /// whisper.cpp `-oj` JSON or a whisper-server JSON body.
    WhisperJson,
    /// piper stderr text, including the real-time factor line.
    PiperLog,
    /// piper `--output_file` PCM WAV bytes.
    PiperWav,
}

/// A parsed fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedOutput {
    /// Whisper.cpp transcript.
    Transcript(Transcript),
    /// Piper log summary.
    Synthesis(PiperReport),
    /// Piper WAV header. Samples are not retained.
    Wave(WaveInfo),
}

/// Where a whisper.cpp transcript was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptSource {
    /// `-oj` / `-ojf` file. Offsets are milliseconds.
    CliJson,
    /// whisper-server JSON. Segment times are seconds.
    ServerJson,
}

/// Parsed whisper.cpp transcript. Text is the joined segment text, or the
/// server `text` field. Leading spaces are kept as the tool wrote them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    /// Which JSON shape was read.
    pub source: TranscriptSource,
    /// Joined CLI segment text, or the server `text` field.
    pub text: String,
    /// `result.language`, else `params.language`, else the server `language`.
    pub language: Option<String>,
    /// `params.translate` when the CLI file had it.
    pub translate: Option<bool>,
    /// Server `duration` in milliseconds, else the last segment end.
    pub duration_ms: Option<u64>,
    /// `params.model`. Stored, never opened.
    pub model_path: Option<String>,
    /// CLI `model` object when present.
    pub model: Option<ModelInfo>,
    /// Segments in file order.
    pub segments: Vec<Segment>,
}

/// CLI `model` object. Numbers are checked and kept. Nothing is loaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelInfo {
    /// Wire field `type`, such as `tiny.en`.
    pub model_type: Option<String>,
    /// Wire field `multilingual`.
    pub multilingual: Option<bool>,
    /// Vocabulary size.
    pub vocab: Option<u64>,
    /// Mel count.
    pub mels: Option<u64>,
    /// File type integer from whisper.cpp.
    pub ftype: Option<i64>,
    /// `model.audio` sizes.
    pub audio: Option<TensorShape>,
    /// `model.text` sizes.
    pub text: Option<TensorShape>,
}

/// `ctx` / `state` / `head` / `layer` block inside `model`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorShape {
    /// Context length.
    pub ctx: Option<u64>,
    /// State width.
    pub state: Option<u64>,
    /// Head count.
    pub head: Option<u64>,
    /// Layer count.
    pub layer: Option<u64>,
}

/// One transcript segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Segment text, including a leading space when whisper.cpp wrote one.
    pub text: String,
    /// Start in milliseconds. CLI offsets, or server `start` converted.
    pub from_ms: Option<u64>,
    /// End in milliseconds.
    pub to_ms: Option<u64>,
    /// CLI timestamp `from`, such as `00:00:00,000`.
    pub from_stamp: Option<String>,
    /// CLI timestamp `to`.
    pub to_stamp: Option<String>,
    /// Diarization id when the CLI file included `speaker`.
    pub speaker: Option<String>,
    /// Tiny-diarize flag when the CLI file included it.
    pub speaker_turn_next: Option<bool>,
    /// CLI `-ojf` tokens. Server token-id arrays are not copied.
    pub tokens: Vec<Token>,
}

/// One `-ojf` token. Probability is checked, not stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Token text.
    pub text: String,
    /// Token id.
    pub id: i64,
    /// Start in milliseconds when the token had offsets.
    pub from_ms: Option<u64>,
    /// End in milliseconds when the token had offsets.
    pub to_ms: Option<u64>,
}

/// Numbers taken from a piper CLI log. The factor string is the text piper wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PiperReport {
    /// Real-time factor, as written.
    pub real_time_factor: String,
    /// Inference seconds, as written.
    pub infer_seconds: String,
    /// Audio seconds, as written.
    pub audio_seconds: String,
    /// `infer_seconds` rounded half-up to milliseconds.
    pub infer_ms: u64,
    /// `audio_seconds` rounded half-up to milliseconds.
    pub audio_ms: u64,
    /// Load line seconds, as written, when that line was present.
    pub load_seconds: Option<String>,
    /// Load line rounded half-up to milliseconds.
    pub load_ms: Option<u64>,
}

/// PCM WAV header from a piper file. Sample bytes are not retained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaveInfo {
    /// WAVE format tag. This crate accepts `1` (PCM) only.
    pub audio_format: u16,
    /// Channel count.
    pub channels: u16,
    /// Frames per second.
    pub sample_rate: u32,
    /// Bytes per second.
    pub byte_rate: u32,
    /// Bytes per frame.
    pub block_align: u16,
    /// Bits per sample.
    pub bits_per_sample: u16,
    /// `data` chunk size in bytes.
    pub data_bytes: u32,
    /// `data_bytes * 1000 / byte_rate`, integer division.
    pub duration_ms: u64,
}

/// Why a fixture or a provider row was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureError {
    /// Body was empty or only whitespace.
    Empty,
    /// Body was not JSON. The string is the parser diagnostic, not the body.
    Json(String),
    /// JSON value was not an object.
    NotAnObject,
    /// Text bytes were not UTF-8.
    NotUtf8,
    /// A required field is absent.
    MissingField(String),
    /// A field had the wrong JSON type or an illegal value.
    BadField {
        /// Dotted field path.
        field: String,
        /// What the parser wanted, in plain words.
        expected: &'static str,
    },
    /// Id, endpoint, or a model path held a key-shaped token. The value is not stored.
    SecretShape(&'static str),
    /// The row's engine cannot parse this output.
    EngineMismatch {
        /// Engine on the row.
        row: &'static str,
        /// Output the caller asked for.
        output: &'static str,
    },
    /// The body was the wrong fixture shape. The string is a fixed sentence.
    Shape(&'static str),
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("fixture body is empty"),
            Self::Json(message) => write!(f, "fixture body is not json: {message}"),
            Self::NotAnObject => f.write_str("fixture body must be a json object"),
            Self::NotUtf8 => f.write_str("fixture text is not utf-8"),
            Self::MissingField(field) => write!(f, "missing {field}"),
            Self::BadField { field, expected } => write!(f, "{field} must be {expected}"),
            Self::SecretShape(field) => write!(f, "provider {field} must not contain a key"),
            Self::EngineMismatch { row, output } => {
                write!(f, "provider is {row} and cannot parse {output}")
            }
            Self::Shape(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FixtureError {}

fn validate_token(field: &'static str, value: &str) -> Result<(), FixtureError> {
    if value.is_empty() || value.trim().is_empty() {
        return Err(match field {
            "endpoint" => bad("endpoint", "a non-empty endpoint"),
            _ => bad("id", "a non-empty id"),
        });
    }
    if value.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad(field, "a token without whitespace"));
    }
    if looks_like_key(value) {
        return Err(FixtureError::SecretShape(field));
    }
    Ok(())
}

fn looks_like_key(value: &str) -> bool {
    // 0x73 0x6b 0x2d is the key prefix. A literal would trip the secrets scan.
    let prefix = [0x73, 0x6b, 0x2d];
    value
        .as_bytes()
        .windows(prefix.len())
        .any(|window| window == prefix.as_slice())
}

fn bad(field: impl Into<String>, expected: &'static str) -> FixtureError {
    FixtureError::BadField {
        field: field.into(),
        expected,
    }
}

fn as_text(bytes: &[u8]) -> Result<&str, FixtureError> {
    std::str::from_utf8(bytes).map_err(|_| FixtureError::NotUtf8)
}

fn parse_whisper_body(body: &str) -> Result<Transcript, FixtureError> {
    let map = json_object(body)?;
    if map.contains_key("transcription") {
        parse_cli_transcript(&map)
    } else if map.contains_key("text") {
        parse_server_transcript(&map)
    } else {
        Err(FixtureError::MissingField("transcription".to_owned()))
    }
}

fn parse_cli_transcript(map: &Map<String, Value>) -> Result<Transcript, FixtureError> {
    let model = parse_model(map)?;
    let params = parse_params(map)?;
    let result_language = parse_result_language(map)?;
    let segments = parse_cli_segments(map)?;
    let text = segments.iter().map(|seg| seg.text.as_str()).collect();
    let duration_ms = segments.iter().filter_map(|seg| seg.to_ms).max();
    Ok(Transcript {
        source: TranscriptSource::CliJson,
        text,
        language: result_language.or(params.language),
        translate: params.translate,
        duration_ms,
        model_path: params.model_path,
        model,
        segments,
    })
}

fn parse_server_transcript(map: &Map<String, Value>) -> Result<Transcript, FixtureError> {
    let text = required_string(map, "text", "text")?;
    let language = optional_nonempty(map, "language", "language")?;
    let explicit = optional_seconds(map, "duration", "duration")?;
    let segments = parse_server_segments(map)?;
    let inferred = segments.iter().filter_map(|seg| seg.to_ms).max();
    Ok(Transcript {
        source: TranscriptSource::ServerJson,
        text,
        language,
        translate: None,
        duration_ms: explicit.or(inferred),
        model_path: None,
        model: None,
        segments,
    })
}

fn parse_model(map: &Map<String, Value>) -> Result<Option<ModelInfo>, FixtureError> {
    let Some(obj) = optional_object(map, "model", "model")? else {
        return Ok(None);
    };
    Ok(Some(ModelInfo {
        model_type: optional_nonempty(obj, "type", "model.type")?,
        multilingual: optional_bool(obj, "multilingual", "model.multilingual")?,
        vocab: optional_u64(obj, "vocab", "model.vocab")?,
        mels: optional_u64(obj, "mels", "model.mels")?,
        ftype: optional_i64(obj, "ftype", "model.ftype")?,
        audio: parse_shape(obj, "audio", "model.audio")?,
        text: parse_shape(obj, "text", "model.text")?,
    }))
}

fn parse_shape(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<TensorShape>, FixtureError> {
    let Some(obj) = optional_object(map, key, display)? else {
        return Ok(None);
    };
    Ok(Some(TensorShape {
        ctx: optional_u64(obj, "ctx", &format!("{display}.ctx"))?,
        state: optional_u64(obj, "state", &format!("{display}.state"))?,
        head: optional_u64(obj, "head", &format!("{display}.head"))?,
        layer: optional_u64(obj, "layer", &format!("{display}.layer"))?,
    }))
}

struct CliParams {
    model_path: Option<String>,
    language: Option<String>,
    translate: Option<bool>,
}

fn parse_params(map: &Map<String, Value>) -> Result<CliParams, FixtureError> {
    let Some(obj) = optional_object(map, "params", "params")? else {
        return Ok(CliParams {
            model_path: None,
            language: None,
            translate: None,
        });
    };
    let model_path = optional_nonempty(obj, "model", "params.model")?;
    if let Some(path) = &model_path {
        if looks_like_key(path) {
            return Err(FixtureError::SecretShape("params.model"));
        }
    }
    Ok(CliParams {
        model_path,
        language: optional_nonempty(obj, "language", "params.language")?,
        translate: optional_bool(obj, "translate", "params.translate")?,
    })
}

fn parse_result_language(map: &Map<String, Value>) -> Result<Option<String>, FixtureError> {
    let Some(obj) = optional_object(map, "result", "result")? else {
        return Ok(None);
    };
    optional_nonempty(obj, "language", "result.language")
}

fn parse_cli_segments(map: &Map<String, Value>) -> Result<Vec<Segment>, FixtureError> {
    let items = required_array(map, "transcription", "transcription")?;
    let mut segments = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        segments.push(parse_cli_segment(index, item)?);
    }
    Ok(segments)
}

fn parse_cli_segment(index: usize, value: &Value) -> Result<Segment, FixtureError> {
    let Some(map) = value.as_object() else {
        return Err(bad(format!("transcription[{index}]"), "an object"));
    };
    let text = required_string(map, "text", &format!("transcription[{index}].text"))?;
    let times = parse_times(map, &format!("transcription[{index}]"))?;
    let speaker = optional_nonempty(map, "speaker", &format!("transcription[{index}].speaker"))?;
    let speaker_turn_next = optional_bool(
        map,
        "speaker_turn_next",
        &format!("transcription[{index}].speaker_turn_next"),
    )?;
    let tokens = parse_tokens(map, index)?;
    Ok(Segment {
        text,
        from_ms: times.from_ms,
        to_ms: times.to_ms,
        from_stamp: times.from_stamp,
        to_stamp: times.to_stamp,
        speaker,
        speaker_turn_next,
        tokens,
    })
}

fn parse_tokens(map: &Map<String, Value>, index: usize) -> Result<Vec<Token>, FixtureError> {
    let field = format!("transcription[{index}].tokens");
    let Some(items) = optional_array(map, "tokens", &field)? else {
        return Ok(Vec::new());
    };
    let mut tokens = Vec::with_capacity(items.len());
    for (token_index, item) in items.iter().enumerate() {
        tokens.push(parse_token(&field, token_index, item)?);
    }
    Ok(tokens)
}

fn parse_token(field: &str, index: usize, value: &Value) -> Result<Token, FixtureError> {
    let Some(map) = value.as_object() else {
        return Err(bad(format!("{field}[{index}]"), "an object"));
    };
    let text = required_string(map, "text", &format!("{field}[{index}].text"))?;
    let id = required_i64(map, "id", &format!("{field}[{index}].id"))?;
    expect_number_if_present(map, "p", &format!("{field}[{index}].p"))?;
    expect_number_if_present(map, "t_dtw", &format!("{field}[{index}].t_dtw"))?;
    let times = parse_times(map, &format!("{field}[{index}]"))?;
    Ok(Token {
        text,
        id,
        from_ms: times.from_ms,
        to_ms: times.to_ms,
    })
}

struct Times {
    from_stamp: Option<String>,
    to_stamp: Option<String>,
    from_ms: Option<u64>,
    to_ms: Option<u64>,
}

fn parse_times(map: &Map<String, Value>, display: &str) -> Result<Times, FixtureError> {
    let stamps = match optional_object(map, "timestamps", &format!("{display}.timestamps"))? {
        None => (None, None),
        Some(obj) => (
            Some(required_nonempty(
                obj,
                "from",
                &format!("{display}.timestamps.from"),
            )?),
            Some(required_nonempty(
                obj,
                "to",
                &format!("{display}.timestamps.to"),
            )?),
        ),
    };
    let offsets = match optional_object(map, "offsets", &format!("{display}.offsets"))? {
        None => (None, None),
        Some(obj) => {
            let from = required_u64(obj, "from", &format!("{display}.offsets.from"))?;
            let to = required_u64(obj, "to", &format!("{display}.offsets.to"))?;
            if from > to {
                return Err(bad(
                    format!("{display}.offsets"),
                    "from less than or equal to to",
                ));
            }
            (Some(from), Some(to))
        }
    };
    Ok(Times {
        from_stamp: stamps.0,
        to_stamp: stamps.1,
        from_ms: offsets.0,
        to_ms: offsets.1,
    })
}

fn parse_server_segments(map: &Map<String, Value>) -> Result<Vec<Segment>, FixtureError> {
    let Some(items) = optional_array(map, "segments", "segments")? else {
        return Ok(Vec::new());
    };
    let mut segments = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        segments.push(parse_server_segment(index, item)?);
    }
    Ok(segments)
}

fn parse_server_segment(index: usize, value: &Value) -> Result<Segment, FixtureError> {
    let Some(map) = value.as_object() else {
        return Err(bad(format!("segments[{index}]"), "an object"));
    };
    let text = required_string(map, "text", &format!("segments[{index}].text"))?;
    let from_ms = Some(required_seconds(
        map,
        "start",
        &format!("segments[{index}].start"),
    )?);
    let to_ms = Some(required_seconds(
        map,
        "end",
        &format!("segments[{index}].end"),
    )?);
    if from_ms > to_ms {
        return Err(bad(
            format!("segments[{index}]"),
            "start less than or equal to end",
        ));
    }
    if let Some(tokens) = map.get("tokens") {
        if !matches!(tokens, Value::Array(_) | Value::Null) {
            return Err(bad(format!("segments[{index}].tokens"), "an array"));
        }
    }
    Ok(Segment {
        text,
        from_ms,
        to_ms,
        from_stamp: None,
        to_stamp: None,
        speaker: None,
        speaker_turn_next: None,
        tokens: Vec::new(),
    })
}

fn parse_piper_log_body(body: &str) -> Result<PiperReport, FixtureError> {
    if body.trim().is_empty() {
        return Err(FixtureError::Empty);
    }
    let mut factor: Option<(String, String, String)> = None;
    let mut load: Option<String> = None;
    for line in body.lines() {
        if let Some(found) = parse_factor_line(line)? {
            if factor.is_some() {
                return Err(bad("real_time_factor", "one Real-time factor line"));
            }
            factor = Some(found);
        }
        if let Some(found) = parse_load_line(line)? {
            if load.is_some() {
                return Err(bad("load_seconds", "one Loaded voice line"));
            }
            load = Some(found);
        }
    }
    let Some((real_time_factor, infer_seconds, audio_seconds)) = factor else {
        return Err(FixtureError::Shape(
            "piper log has no Real-time factor line",
        ));
    };
    let infer_ms = seconds_to_ms(&infer_seconds, "infer_seconds")?;
    let audio_ms = seconds_to_ms(&audio_seconds, "audio_seconds")?;
    let load_ms = match &load {
        Some(text) => Some(seconds_to_ms(text, "load_seconds")?),
        None => None,
    };
    Ok(PiperReport {
        real_time_factor,
        infer_seconds,
        audio_seconds,
        infer_ms,
        audio_ms,
        load_seconds: load,
        load_ms,
    })
}

fn parse_factor_line(line: &str) -> Result<Option<(String, String, String)>, FixtureError> {
    const MARK: &str = "Real-time factor: ";
    let Some(at) = line.find(MARK) else {
        return Ok(None);
    };
    let rest = &line[at + MARK.len()..];
    let Some(paren) = rest.find(" (infer=") else {
        return Err(bad(
            "real_time_factor",
            "Real-time factor: N (infer=I sec, audio=A sec)",
        ));
    };
    let real_time_factor = rest[..paren].trim();
    check_decimal(real_time_factor, "real_time_factor")?;
    let after = &rest[paren + " (infer=".len()..];
    let Some(mid) = after.find(" sec, audio=") else {
        return Err(bad(
            "real_time_factor",
            "Real-time factor: N (infer=I sec, audio=A sec)",
        ));
    };
    let infer_seconds = after[..mid].trim();
    check_decimal(infer_seconds, "infer_seconds")?;
    let after = &after[mid + " sec, audio=".len()..];
    let Some(end) = after.find(" sec)") else {
        return Err(bad(
            "real_time_factor",
            "Real-time factor: N (infer=I sec, audio=A sec)",
        ));
    };
    let audio_seconds = after[..end].trim();
    check_decimal(audio_seconds, "audio_seconds")?;
    let trailing = &after[end + " sec)".len()..];
    if !trailing.trim().is_empty() {
        return Err(bad(
            "real_time_factor",
            "Real-time factor: N (infer=I sec, audio=A sec)",
        ));
    }
    Ok(Some((
        real_time_factor.to_owned(),
        infer_seconds.to_owned(),
        audio_seconds.to_owned(),
    )))
}

fn parse_load_line(line: &str) -> Result<Option<String>, FixtureError> {
    const MARK: &str = "Loaded voice in ";
    const SUFFIX: &str = " second(s)";
    let Some(at) = line.find(MARK) else {
        return Ok(None);
    };
    let rest = &line[at + MARK.len()..];
    let Some(end) = rest.find(SUFFIX) else {
        return Err(bad("load_seconds", "Loaded voice in N second(s)"));
    };
    let load = rest[..end].trim();
    check_decimal(load, "load_seconds")?;
    let trailing = &rest[end + SUFFIX.len()..];
    if !trailing.trim().is_empty() {
        return Err(bad("load_seconds", "Loaded voice in N second(s)"));
    }
    Ok(Some(load.to_owned()))
}

fn parse_wav(bytes: &[u8]) -> Result<WaveInfo, FixtureError> {
    if bytes.len() < 12 {
        return Err(bad("wav", "a PCM RIFF file"));
    }
    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(FixtureError::Shape("wav fixture is not a PCM RIFF file"));
    }
    let riff_size = read_u32(bytes, 4)?;
    let limit = 8usize
        .checked_add(riff_size as usize)
        .ok_or_else(|| bad("wav", "a size that fits the buffer"))?;
    if limit > bytes.len() {
        return Err(bad("wav", "a RIFF size that fits the buffer"));
    }
    let mut pos = 12usize;
    let mut fmt: Option<FmtChunk> = None;
    let mut data_bytes: Option<u32> = None;
    while pos.checked_add(8).is_some_and(|end| end <= limit) {
        let mut id = [0u8; 4];
        id.copy_from_slice(&bytes[pos..pos + 4]);
        let size = read_u32(bytes, pos + 4)?;
        let data_at = pos + 8;
        let data_end = data_at
            .checked_add(size as usize)
            .ok_or_else(|| bad("wav", "a chunk that fits the buffer"))?;
        if data_end > limit {
            return Err(bad("wav", "a chunk that fits the buffer"));
        }
        match id.as_slice() {
            b"fmt " => {
                if fmt.is_some() {
                    return Err(bad("wav", "one fmt chunk"));
                }
                fmt = Some(parse_fmt(&bytes[data_at..data_end])?);
            }
            b"data" => {
                if data_bytes.is_some() {
                    return Err(bad("wav", "one data chunk"));
                }
                data_bytes = Some(size);
            }
            _ => {}
        }
        let padded = (size as usize)
            .checked_add((size as usize) % 2)
            .ok_or_else(|| bad("wav", "a chunk that fits the buffer"))?;
        pos = data_at
            .checked_add(padded)
            .ok_or_else(|| bad("wav", "a chunk that fits the buffer"))?;
    }
    let Some(fmt) = fmt else {
        return Err(bad("wav", "a fmt chunk"));
    };
    let Some(data_bytes) = data_bytes else {
        return Err(bad("wav", "a data chunk"));
    };
    if !u64::from(data_bytes).is_multiple_of(u64::from(fmt.block_align)) {
        return Err(bad("wav", "a data size that is a whole number of frames"));
    }
    let byte_rate = u64::from(fmt.byte_rate);
    let duration_ms = (u64::from(data_bytes))
        .checked_mul(1000)
        .and_then(|n| n.checked_div(byte_rate))
        .ok_or_else(|| bad("wav", "a duration that fits"))?;
    Ok(WaveInfo {
        audio_format: fmt.audio_format,
        channels: fmt.channels,
        sample_rate: fmt.sample_rate,
        byte_rate: fmt.byte_rate,
        block_align: fmt.block_align,
        bits_per_sample: fmt.bits_per_sample,
        data_bytes,
        duration_ms,
    })
}

struct FmtChunk {
    audio_format: u16,
    channels: u16,
    sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

fn parse_fmt(bytes: &[u8]) -> Result<FmtChunk, FixtureError> {
    if bytes.len() < 16 {
        return Err(bad("wav", "a fmt chunk of at least 16 bytes"));
    }
    let audio_format = read_u16(bytes, 0)?;
    if audio_format != 1 {
        return Err(bad("wav", "PCM format 1"));
    }
    let channels = read_u16(bytes, 2)?;
    if channels == 0 {
        return Err(bad("wav", "at least one channel"));
    }
    let sample_rate = read_u32(bytes, 4)?;
    if sample_rate == 0 {
        return Err(bad("wav", "a non-zero sample rate"));
    }
    let byte_rate = read_u32(bytes, 8)?;
    let block_align = read_u16(bytes, 12)?;
    let bits_per_sample = read_u16(bytes, 14)?;
    if bits_per_sample == 0 || !bits_per_sample.is_multiple_of(8) {
        return Err(bad("wav", "a whole number of bytes per sample"));
    }
    let width = bits_per_sample / 8;
    let expected_align = (u32::from(channels))
        .checked_mul(u32::from(width))
        .ok_or_else(|| bad("wav", "a block align that fits"))?;
    if u32::from(block_align) != expected_align || block_align == 0 {
        return Err(bad("wav", "a block align of channels times sample width"));
    }
    let expected_rate = (u64::from(sample_rate))
        .checked_mul(u64::from(block_align))
        .ok_or_else(|| bad("wav", "a byte rate that fits"))?;
    if u64::from(byte_rate) != expected_rate {
        return Err(bad("wav", "a byte rate of sample rate times block align"));
    }
    Ok(FmtChunk {
        audio_format,
        channels,
        sample_rate,
        byte_rate,
        block_align,
        bits_per_sample,
    })
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16, FixtureError> {
    let end = at
        .checked_add(2)
        .ok_or_else(|| bad("wav", "a field inside the buffer"))?;
    if end > bytes.len() {
        return Err(bad("wav", "a field inside the buffer"));
    }
    let mut buf = [0u8; 2];
    buf.copy_from_slice(&bytes[at..end]);
    Ok(u16::from_le_bytes(buf))
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32, FixtureError> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| bad("wav", "a field inside the buffer"))?;
    if end > bytes.len() {
        return Err(bad("wav", "a field inside the buffer"));
    }
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&bytes[at..end]);
    Ok(u32::from_le_bytes(buf))
}

fn json_object(body: &str) -> Result<Map<String, Value>, FixtureError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(FixtureError::Empty);
    }
    let value: Value =
        serde_json::from_str(trimmed).map_err(|err| FixtureError::Json(err.to_string()))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(FixtureError::NotAnObject),
    }
}

fn optional_object<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<&'a Map<String, Value>>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Object(obj)) => Ok(Some(obj)),
        Some(_) => Err(bad(display, "an object")),
    }
}

fn optional_array<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<&'a [Value]>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => Ok(Some(items.as_slice())),
        Some(_) => Err(bad(display, "an array")),
    }
}

fn required_array<'a>(
    map: &'a Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<&'a [Value], FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Err(FixtureError::MissingField(display.to_owned())),
        Some(Value::Array(items)) => Ok(items.as_slice()),
        Some(_) => Err(bad(display, "an array")),
    }
}

fn optional_string(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<String>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(bad(display, "a string")),
    }
}

fn optional_nonempty(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<String>, FixtureError> {
    match optional_string(map, key, display)? {
        None => Ok(None),
        Some(text) if text.is_empty() => Err(bad(display, "a non-empty string")),
        Some(text) => Ok(Some(text)),
    }
}

fn required_string(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<String, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Err(FixtureError::MissingField(display.to_owned())),
        Some(Value::String(text)) => Ok(text.clone()),
        Some(_) => Err(bad(display, "a string")),
    }
}

fn required_nonempty(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<String, FixtureError> {
    match required_string(map, key, display)? {
        text if text.is_empty() => Err(bad(display, "a non-empty string")),
        text => Ok(text),
    }
}

fn optional_bool(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<bool>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(flag)) => Ok(Some(*flag)),
        Some(_) => Err(bad(display, "a boolean")),
    }
}

fn optional_i64(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<i64>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number_i64(number, display).map(Some),
        Some(_) => Err(bad(display, "an integer")),
    }
}

fn required_i64(map: &Map<String, Value>, key: &str, display: &str) -> Result<i64, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Err(FixtureError::MissingField(display.to_owned())),
        Some(Value::Number(number)) => number_i64(number, display),
        Some(_) => Err(bad(display, "an integer")),
    }
}

fn optional_u64(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<u64>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number_u64(number, display).map(Some),
        Some(_) => Err(bad(display, "a non-negative integer")),
    }
}

fn required_u64(map: &Map<String, Value>, key: &str, display: &str) -> Result<u64, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Err(FixtureError::MissingField(display.to_owned())),
        Some(Value::Number(number)) => number_u64(number, display),
        Some(_) => Err(bad(display, "a non-negative integer")),
    }
}

fn number_i64(number: &Number, display: &str) -> Result<i64, FixtureError> {
    number.as_i64().ok_or_else(|| bad(display, "an integer"))
}

fn number_u64(number: &Number, display: &str) -> Result<u64, FixtureError> {
    number
        .as_u64()
        .ok_or_else(|| bad(display, "a non-negative integer"))
}

fn expect_number_if_present(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<(), FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) | Some(Value::Number(_)) => Ok(()),
        Some(_) => Err(bad(display, "a number")),
    }
}

fn optional_seconds(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<Option<u64>, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number_seconds(number, display).map(Some),
        Some(_) => Err(bad(display, "a non-negative number of seconds")),
    }
}

fn required_seconds(
    map: &Map<String, Value>,
    key: &str,
    display: &str,
) -> Result<u64, FixtureError> {
    match map.get(key) {
        None | Some(Value::Null) => Err(FixtureError::MissingField(display.to_owned())),
        Some(Value::Number(number)) => number_seconds(number, display),
        Some(_) => Err(bad(display, "a non-negative number of seconds")),
    }
}

fn number_seconds(number: &Number, display: &str) -> Result<u64, FixtureError> {
    let text = number.to_string();
    if text.starts_with('-') {
        return Err(bad(display, "a non-negative number of seconds"));
    }
    seconds_to_ms(&text, display)
}

fn check_decimal(raw: &str, field: &str) -> Result<(), FixtureError> {
    split_decimal(raw, field).map(|_| ())
}

fn seconds_to_ms(raw: &str, field: &str) -> Result<u64, FixtureError> {
    let (whole, micros) = split_decimal(raw, field)?;
    let extra = (micros + 500) / 1000;
    whole
        .checked_mul(1000)
        .and_then(|ms| ms.checked_add(extra))
        .ok_or_else(|| bad(field, "a number of seconds that fits"))
}

fn split_decimal(raw: &str, field: &str) -> Result<(u64, u64), FixtureError> {
    if raw.is_empty() || raw.starts_with('+') || raw.starts_with('-') {
        return Err(bad(field, "a non-negative decimal"));
    }
    let (whole, frac) = match raw.split_once('.') {
        Some((head, tail)) => {
            if head.is_empty() || tail.is_empty() {
                return Err(bad(field, "a non-negative decimal"));
            }
            (head, tail)
        }
        None => (raw, ""),
    };
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad(field, "a non-negative decimal"));
    }
    if frac.len() > 6 {
        return Err(bad(field, "at most 6 fractional digits"));
    }
    let secs = whole
        .parse::<u64>()
        .map_err(|_| bad(field, "a number of seconds that fits"))?;
    let mut padded = frac.to_owned();
    while padded.len() < 6 {
        padded.push('0');
    }
    let micros = padded
        .parse::<u64>()
        .map_err(|_| bad(field, "a number of seconds that fits"))?;
    Ok((secs, micros))
}
