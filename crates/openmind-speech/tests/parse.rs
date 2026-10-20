// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use openmind_speech::{
    Engine, FixtureError, OutputKind, ParsedOutput, SpeechProvider, TranscriptSource, WaveInfo,
    USES_GPU, WHISPER_SERVER_PORT,
};

fn whisper() -> SpeechProvider {
    SpeechProvider::new("cpu-stt", "http://127.0.0.1:8080", Engine::WhisperCpp).unwrap()
}

fn piper() -> SpeechProvider {
    SpeechProvider::new("cpu-tts", "local://piper", Engine::Piper).unwrap()
}

fn key_prefix() -> String {
    ['s', 'k', '-'].into_iter().collect()
}

#[test]
fn provider_flags_never_use_gpu_or_spawn() {
    let stt = whisper();
    assert_eq!(stt.id(), "cpu-stt");
    assert_eq!(stt.endpoint(), "http://127.0.0.1:8080");
    assert_eq!(stt.engine(), Engine::WhisperCpp);
    assert!(stt.speaks_stt());
    assert!(!stt.speaks_tts());
    assert!(!stt.dials_network());
    assert!(!stt.spawns_process());
    assert!(!stt.downloads_models());
    assert!(!stt.uses_gpu());
    assert_eq!(stt.uses_gpu(), USES_GPU);
    assert!(stt.endpoint().contains(&WHISPER_SERVER_PORT.to_string()));

    let tts = piper();
    assert_eq!(tts.engine().as_str(), "piper");
    assert!(!tts.speaks_stt());
    assert!(tts.speaks_tts());
    assert!(!tts.dials_network());
    assert!(!tts.spawns_process());
    assert!(!tts.downloads_models());
    assert!(!tts.uses_gpu());
}

#[test]
fn rejects_blank_and_key_shaped_rows() {
    let blank = SpeechProvider::new("  ", "http://127.0.0.1:8080", Engine::WhisperCpp);
    assert!(matches!(
        blank,
        Err(FixtureError::BadField { field, .. }) if field == "id"
    ));
    let spaced = SpeechProvider::new("cpu stt", "http://127.0.0.1:8080", Engine::WhisperCpp);
    assert!(matches!(spaced, Err(FixtureError::BadField { .. })));
    let keyed = format!("{}live", key_prefix());
    let err = SpeechProvider::new(keyed, "http://127.0.0.1:8080", Engine::WhisperCpp);
    assert_eq!(err, Err(FixtureError::SecretShape("id")));
    let err = SpeechProvider::new(
        "cpu-stt",
        format!("http://127.0.0.1:8080/{}", key_prefix()),
        Engine::WhisperCpp,
    );
    assert_eq!(err, Err(FixtureError::SecretShape("endpoint")));
    let empty_endpoint = SpeechProvider::new("cpu-stt", "", Engine::WhisperCpp);
    assert!(matches!(
        empty_endpoint,
        Err(FixtureError::BadField { field, .. }) if field == "endpoint"
    ));
}

#[test]
fn whisper_cli_fixture() {
    let body = include_str!("../fixtures/whisper_cli.json");
    let heard = whisper().parse_whisper(body).unwrap();
    assert_eq!(heard.source, TranscriptSource::CliJson);
    assert_eq!(heard.text, " Hello there.");
    assert_eq!(heard.language.as_deref(), Some("en"));
    assert_eq!(heard.translate, Some(false));
    assert_eq!(heard.duration_ms, Some(1200));
    assert_eq!(heard.model_path.as_deref(), Some("models/ggml-tiny.en.bin"));
    let model = heard.model.unwrap();
    assert_eq!(model.model_type.as_deref(), Some("tiny.en"));
    assert_eq!(model.multilingual, Some(false));
    assert_eq!(model.vocab, Some(51864));
    assert_eq!(model.mels, Some(80));
    assert_eq!(model.ftype, Some(1));
    assert_eq!(model.audio.unwrap().layer, Some(4));
    assert_eq!(model.text.unwrap().ctx, Some(448));
    assert_eq!(heard.segments.len(), 2);
    assert_eq!(heard.segments[0].from_ms, Some(0));
    assert_eq!(heard.segments[0].to_ms, Some(400));
    assert_eq!(
        heard.segments[0].from_stamp.as_deref(),
        Some("00:00:00,000")
    );
    assert_eq!(heard.segments[1].text, " there.");
    assert_eq!(heard.segments[1].to_ms, Some(1200));
    assert!(heard.segments[0].tokens.is_empty());
    assert!(!whisper().uses_gpu());
    assert!(!whisper().downloads_models());
}

#[test]
fn whisper_full_fixture_ignores_named_gpu() {
    let body = include_str!("../fixtures/whisper_cli_full.json");
    assert!(body.contains("CUDA = 1"));
    let row = whisper();
    let heard = row.parse_whisper(body).unwrap();
    assert!(!row.uses_gpu());
    assert!(!row.spawns_process());
    assert_eq!(heard.text, " Hello there.");
    assert_eq!(heard.model.unwrap().model_type.as_deref(), Some("base.en"));
    assert_eq!(heard.segments.len(), 1);
    assert_eq!(heard.segments[0].speaker.as_deref(), Some("0"));
    assert_eq!(heard.segments[0].tokens.len(), 2);
    assert_eq!(heard.segments[0].tokens[0].text, " Hello");
    assert_eq!(heard.segments[0].tokens[0].id, 2425);
    assert_eq!(heard.segments[0].tokens[0].from_ms, Some(0));
    assert_eq!(heard.segments[0].tokens[0].to_ms, Some(400));
    assert_eq!(heard.segments[0].tokens[1].text, " there.");
    assert_eq!(heard.segments[0].tokens[1].id, 456);
    assert_eq!(heard.segments[0].tokens[1].from_ms, None);
    assert_eq!(heard.segments[0].tokens[1].to_ms, None);
}

#[test]
fn whisper_server_fixture() {
    let body = include_str!("../fixtures/whisper_server.json");
    let heard = whisper().parse_whisper(body).unwrap();
    assert_eq!(heard.source, TranscriptSource::ServerJson);
    assert_eq!(heard.text, " Hello there.");
    assert_eq!(heard.language.as_deref(), Some("en"));
    assert_eq!(heard.duration_ms, Some(1500));
    assert!(heard.model.is_none());
    assert_eq!(heard.segments.len(), 1);
    assert_eq!(heard.segments[0].from_ms, Some(0));
    assert_eq!(heard.segments[0].to_ms, Some(1500));
    assert!(heard.segments[0].tokens.is_empty());
    assert!(!whisper().uses_gpu());
}

#[test]
fn whisper_rejects_bad_bodies() {
    let row = whisper();
    assert_eq!(row.parse_whisper("   "), Err(FixtureError::Empty));
    assert!(matches!(row.parse_whisper("["), Err(FixtureError::Json(_))));
    assert_eq!(row.parse_whisper("[]"), Err(FixtureError::NotAnObject));
    assert_eq!(
        row.parse_whisper("{}"),
        Err(FixtureError::MissingField("transcription".to_owned()))
    );
    let empty = row.parse_whisper(r#"{"transcription":[]}"#).unwrap();
    assert_eq!(empty.text, "");
    assert!(empty.segments.is_empty());
    assert!(matches!(
        row.parse_whisper(r#"{"transcription":[{"offsets":{"from":0,"to":1}}]}"#),
        Err(FixtureError::MissingField(_))
    ));
    assert!(matches!(
        row.parse_whisper(r#"{"transcription":[{"text":" hi","offsets":{"from":5,"to":1}}]}"#),
        Err(FixtureError::BadField { .. })
    ));
    assert!(matches!(
        row.parse_whisper(r#"{"model":{"vocab":"no"},"transcription":[]}"#),
        Err(FixtureError::BadField { .. })
    ));
    assert!(matches!(
        row.parse_whisper(
            r#"{"transcription":[{"text":"x","tokens":[{"text":"x","id":1,"p":"no"}]}]}"#
        ),
        Err(FixtureError::BadField { .. })
    ));
    let turned = row
        .parse_whisper(
            r#"{"transcription":[{"text":" hi","offsets":{"from":0,"to":10},"speaker_turn_next":true}]}"#,
        )
        .unwrap();
    assert_eq!(turned.segments[0].speaker_turn_next, Some(true));
}

#[test]
fn model_path_is_stored_and_not_fetched() {
    let row = whisper();
    let heard = row
        .parse_whisper(
            r#"{"params":{"model":"http://127.0.0.1:9/models/ggml-tiny.en.bin"},"transcription":[{"text":" hi","offsets":{"from":0,"to":10}}]}"#,
        )
        .unwrap();
    assert_eq!(
        heard.model_path.as_deref(),
        Some("http://127.0.0.1:9/models/ggml-tiny.en.bin")
    );
    assert!(!row.downloads_models());
    assert!(!row.dials_network());
    let keyed = format!("{}live.bin", key_prefix());
    let body = format!(r#"{{"params":{{"model":"{keyed}"}},"transcription":[]}}"#);
    let err = row.parse_whisper(&body);
    assert_eq!(err, Err(FixtureError::SecretShape("params.model")));
    let rendered = err.unwrap_err().to_string();
    assert!(!rendered.contains(&key_prefix()));
}

#[test]
fn piper_log_fixture() {
    let body = include_str!("../fixtures/piper_log.txt");
    let report = piper().parse_piper_log(body).unwrap();
    assert_eq!(report.real_time_factor, "0.15");
    assert_eq!(report.infer_seconds, "0.03");
    assert_eq!(report.audio_seconds, "0.20");
    assert_eq!(report.infer_ms, 30);
    assert_eq!(report.audio_ms, 200);
    assert_eq!(report.load_seconds.as_deref(), Some("0.12"));
    assert_eq!(report.load_ms, Some(120));
    assert!(!piper().uses_gpu());
    assert!(!piper().spawns_process());

    let bare = piper()
        .parse_piper_log("Real-time factor: 1 (infer=0.5 sec, audio=2 sec)\n")
        .unwrap();
    assert_eq!(bare.real_time_factor, "1");
    assert_eq!(bare.infer_ms, 500);
    assert_eq!(bare.audio_ms, 2000);
    assert!(bare.load_seconds.is_none());
}

#[test]
fn piper_log_rejects_bad_bodies() {
    let row = piper();
    assert_eq!(row.parse_piper_log("  \n"), Err(FixtureError::Empty));
    assert_eq!(
        row.parse_piper_log("Loaded voice in 0.12 second(s)\n"),
        Err(FixtureError::Shape(
            "piper log has no Real-time factor line"
        ))
    );
    assert!(matches!(
        row.parse_piper_log("Real-time factor: fast\n"),
        Err(FixtureError::BadField { .. })
    ));
    let twice = "\
Real-time factor: 0.1 (infer=0.1 sec, audio=1 sec)
Real-time factor: 0.2 (infer=0.2 sec, audio=1 sec)
";
    assert!(matches!(
        row.parse_piper_log(twice),
        Err(FixtureError::BadField { field, .. }) if field == "real_time_factor"
    ));
}

#[test]
fn piper_wav_fixture() {
    let bytes = include_bytes!("../fixtures/piper_silence.wav");
    let wave = piper().parse_piper_wav(bytes).unwrap();
    assert_eq!(
        wave,
        WaveInfo {
            audio_format: 1,
            channels: 1,
            sample_rate: 22050,
            byte_rate: 44100,
            block_align: 2,
            bits_per_sample: 16,
            data_bytes: 882,
            duration_ms: 20,
        }
    );
    assert!(!piper().uses_gpu());
    assert!(!piper().downloads_models());
}

#[test]
fn piper_wav_rejects_bad_bytes() {
    let row = piper();
    assert!(matches!(
        row.parse_piper_wav(b"not a wave"),
        Err(FixtureError::BadField { .. })
    ));
    assert!(matches!(
        row.parse_piper_wav(b"RIFF\x00\x00\x00\x00NOPE"),
        Err(FixtureError::Shape(_))
    ));
    let mut bytes = include_bytes!("../fixtures/piper_silence.wav").to_vec();
    bytes[20] = 3;
    assert!(matches!(
        row.parse_piper_wav(&bytes),
        Err(FixtureError::BadField { .. })
    ));
    bytes = include_bytes!("../fixtures/piper_silence.wav").to_vec();
    bytes.truncate(20);
    assert!(matches!(
        row.parse_piper_wav(&bytes),
        Err(FixtureError::BadField { .. })
    ));
}

#[test]
fn engine_mismatch_does_not_parse_the_other_tool() {
    let stt = whisper();
    let tts = piper();
    assert_eq!(
        stt.parse_piper_log("nope"),
        Err(FixtureError::EngineMismatch {
            row: "whisper.cpp",
            output: "piper log",
        })
    );
    assert_eq!(
        stt.parse_piper_wav(&[0xff, 0xfe]),
        Err(FixtureError::EngineMismatch {
            row: "whisper.cpp",
            output: "piper wav",
        })
    );
    assert_eq!(
        tts.parse_whisper("{"),
        Err(FixtureError::EngineMismatch {
            row: "piper",
            output: "whisper.cpp json",
        })
    );
}

#[test]
fn parse_dispatch_uses_fixtures() {
    let stt = whisper();
    let cli = include_bytes!("../fixtures/whisper_cli.json");
    assert!(matches!(
        stt.parse(OutputKind::WhisperJson, cli).unwrap(),
        ParsedOutput::Transcript(_)
    ));
    let tts = piper();
    let log = include_bytes!("../fixtures/piper_log.txt");
    assert!(matches!(
        tts.parse(OutputKind::PiperLog, log).unwrap(),
        ParsedOutput::Synthesis(_)
    ));
    let wav = include_bytes!("../fixtures/piper_silence.wav");
    assert!(matches!(
        tts.parse(OutputKind::PiperWav, wav).unwrap(),
        ParsedOutput::Wave(_)
    ));
    assert_eq!(
        stt.parse(OutputKind::WhisperJson, &[0xff, 0xfe])
            .unwrap_err(),
        FixtureError::NotUtf8
    );
}

#[test]
fn crate_source_has_no_process_or_client() {
    let src = include_str!("../src/lib.rs");
    for banned in [
        "std::process",
        "std::net",
        "std::fs",
        "TcpStream",
        "UdpSocket",
        "reqwest",
        "hyper",
        "tokio",
        "ureq",
        "Command::",
        ".spawn(",
        "onnxruntime",
        "whisper_rs",
        "libc",
    ] {
        assert!(!src.contains(banned), "{banned}");
    }
    assert!(src.contains("fn uses_gpu"));
    assert!(src.contains("USES_GPU"));
}

#[test]
fn parsing_a_fixture_does_not_use_a_gpu_spawn_or_open_a_socket() {
    let before_fd = fd_targets();
    let before_sockets = socket_fds(&before_fd);
    assert!(!before_fd.is_empty(), "proc fd table was not readable");
    let before_children = child_pids();

    let stt = whisper();
    assert!(!stt.uses_gpu());
    assert_eq!(stt.uses_gpu(), USES_GPU);
    let heard = stt
        .parse_whisper(include_str!("../fixtures/whisper_cli.json"))
        .unwrap();
    assert!(!heard.text.is_empty());
    assert!(!stt.uses_gpu());
    assert!(!stt.spawns_process());
    assert!(!stt.dials_network());

    let tts = piper();
    assert!(!tts.uses_gpu());
    let report = tts
        .parse_piper_log(include_str!("../fixtures/piper_log.txt"))
        .unwrap();
    assert!(!report.real_time_factor.is_empty());
    assert!(!tts.uses_gpu());
    assert!(!tts.spawns_process());
    assert!(!tts.dials_network());

    let src = include_str!("../src/lib.rs");
    for banned in [
        "std::process",
        "std::net",
        "Command::",
        ".spawn(",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
    ] {
        assert!(!src.contains(banned), "{banned}");
    }

    let after_fd = fd_targets();
    assert_eq!(before_sockets, socket_fds(&after_fd));
    assert_eq!(before_fd, after_fd);
    assert_eq!(before_children, child_pids());
}

fn socket_fds(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| target.starts_with("socket:"))
        .cloned()
        .collect()
}

fn fd_targets() -> Vec<String> {
    let mut names = Vec::new();
    let dir = std::fs::read_dir("/proc/self/fd").expect("fd");
    for entry in dir.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            names.push(target.to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}

fn child_pids() -> Vec<String> {
    let mut kids = Vec::new();
    let dir = std::fs::read_dir("/proc/self/task").expect("task");
    for entry in dir.flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path().join("children")) else {
            continue;
        };
        kids.extend(text.split_whitespace().map(str::to_owned));
    }
    kids.sort();
    kids
}
