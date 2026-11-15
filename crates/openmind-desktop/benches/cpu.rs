// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! CPU timings for the desktop hot paths: request handling, the file gate,
//! and probe parsing. No sockets. `cargo bench -p openmind-desktop --bench cpu`.

use galion_common::http::{parse_head, Head, Limits, Request};
use openmind_desktop::files::{classify, json_path, sanitize_name, Files};
use openmind_desktop::http::reply;
use openmind_desktop::probe::{
    observed_mode, probe_one, ProbeConfig, ProbeReport, ProbeSet, Service, State, SERVICES,
};
use std::hint::black_box;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

fn measure(name: &str, iters: u64, mut body: impl FnMut()) {
    for _ in 0..32 {
        body();
    }
    let mut samples = [0u128; 5];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..iters {
            body();
        }
        *sample = start.elapsed().as_nanos() / u128::from(iters);
    }
    samples.sort_unstable();
    println!(
        "BENCH {name} iters={iters} median_ns={} min_ns={} samples_ns={samples:?}",
        samples[2], samples[0]
    );
}

fn request(method: &str, target: &str, body: &[u8]) -> Request {
    Request {
        head: Head {
            method: method.into(),
            target: target.into(),
            minor: 1,
            headers: vec![
                ("host".into(), "127.0.0.1:18181".into()),
                ("content-type".into(), "application/json".into()),
            ],
            content_length: Some(body.len() as u64),
            chunked: false,
            len: 0,
        },
        raw_head: Vec::new(),
        body: body.to_vec(),
        peer: Some(SocketAddr::from(([127, 0, 0, 1], 9))),
    }
}

fn probe_set() -> ProbeSet {
    ProbeSet {
        timeout: Duration::from_millis(2_000),
        probes: SERVICES
            .into_iter()
            .enumerate()
            .map(|(i, service)| ProbeReport {
                service,
                enabled: service != Service::Cortex,
                state: if i == 3 { State::Down } else { State::Up },
                host: "localhost".into(),
                port: service.default_port(),
                path: service.default_path().into(),
                http_status: Some(if i == 3 { 503 } else { 200 }),
                detail: if i == 3 {
                    "http 503".into()
                } else {
                    "200".into()
                },
                observed_mode: None,
            })
            .collect(),
    }
}

#[test]
fn cpu_hot_paths() {
    let limits = Limits::default();
    let raw = b"POST /files HTTP/1.1\r\nHost: 127.0.0.1:18181\r\nContent-Type: application/json\r\nContent-Length: 20\r\nConnection: close\r\n\r\n";
    let parsed = parse_head(raw, &limits).expect("head");
    assert_eq!(parsed.method, "POST");
    assert_eq!(parsed.headers.len(), 4);
    measure("request_parse_head", 100_000, || {
        black_box(parse_head(black_box(raw), &limits).expect("head"));
    });

    let wide = {
        let mut buf = b"GET /files?kind=text HTTP/1.1\r\n".to_vec();
        for i in 0..24 {
            buf.extend_from_slice(format!("X-Extra-{i}: value-{i}\r\n").as_bytes());
        }
        buf.extend_from_slice(b"\r\n");
        buf
    };
    assert!(parse_head(&wide, &limits).is_ok());
    measure("request_parse_head_24", 40_000, || {
        black_box(parse_head(black_box(&wide), &limits).expect("head"));
    });

    let files = Files::closed();
    let probes = ProbeConfig::defaults();
    let get_root = request("GET", "/", b"");
    let page = reply(&get_root, &probes, &files).to_bytes(false);
    assert!(page.windows(12).any(|w| w == b"HTTP/1.1 200"));
    measure("request_reply_root", 4_000, || {
        black_box(reply(black_box(&get_root), &probes, &files).to_bytes(false));
    });

    let get_files = request("GET", "/files", b"");
    let listed = reply(&get_files, &probes, &files).to_bytes(false);
    assert!(listed.windows(12).any(|w| w == b"HTTP/1.1 200"));
    measure("request_reply_files", 40_000, || {
        black_box(reply(black_box(&get_files), &probes, &files).to_bytes(false));
    });

    let upload_body = br#"{"path":"notes/today.txt"}"#;
    let upload = request("POST", "/files", upload_body);
    let refused = reply(&upload, &probes, &files).to_bytes(false);
    assert!(
        refused.windows(12).any(|w| w == b"HTTP/1.1 403")
            || refused.windows(3).any(|w| w == b"403")
    );
    measure("request_reply_upload", 40_000, || {
        black_box(reply(black_box(&upload), &probes, &files).to_bytes(false));
    });

    let missing = request("GET", "/nope", b"");
    assert!(reply(&missing, &probes, &files)
        .to_bytes(false)
        .windows(12)
        .any(|w| w == b"HTTP/1.1 404"));
    measure("request_reply_404", 40_000, || {
        black_box(reply(black_box(&missing), &probes, &files).to_bytes(false));
    });

    let closed = Files::closed();
    assert_eq!(
        closed.read_path("notes/today.txt").unwrap_err().code(),
        "outside"
    );
    assert_eq!(
        closed.read_path("../secret.txt").unwrap_err().code(),
        "traversal"
    );
    measure("file_gate_parse_path", 100_000, || {
        black_box(closed.read_path(black_box("notes/today.txt")).unwrap_err());
    });
    measure("file_gate_parse_traversal", 100_000, || {
        black_box(closed.read_path(black_box("../secret.txt")).unwrap_err());
    });

    assert_eq!(classify("Notes.TXT", b"hello").unwrap(), "text");
    measure("file_gate_classify_small", 100_000, || {
        black_box(classify(black_box("Notes.TXT"), black_box(b"hello")).unwrap());
    });
    let text = vec![b'a'; 32 * 1024];
    assert_eq!(classify("notes.txt", &text).unwrap(), "text");
    measure("file_gate_classify_32kib", 2_000, || {
        black_box(classify(black_box("notes.txt"), black_box(&text)).unwrap());
    });
    let png = b"\x89PNG\r\n\x1a\nrest";
    assert_eq!(classify("shot.PNG", png).unwrap(), "image");
    measure("file_gate_classify_png", 100_000, || {
        black_box(classify(black_box("shot.PNG"), black_box(png.as_slice())).unwrap());
    });

    assert_eq!(sanitize_name("dir/Report.txt").unwrap(), "Report.txt");
    measure("file_gate_sanitize_name", 100_000, || {
        black_box(sanitize_name(black_box("dir/Report.txt")).unwrap());
    });
    let long_name = format!("{}.txt", "n".repeat(400));
    assert_eq!(sanitize_name(&long_name).unwrap().chars().count(), 80);
    measure("file_gate_sanitize_long", 40_000, || {
        black_box(sanitize_name(black_box(&long_name)).unwrap());
    });

    let small_json = br#"{"extra":false,"path":"notes/today.txt"}"#;
    assert_eq!(json_path(small_json).unwrap(), "notes/today.txt");
    measure("file_gate_json_path", 100_000, || {
        black_box(json_path(black_box(small_json)).unwrap());
    });
    let mut big = Vec::from(&br#"{"path":""#[..]);
    big.extend(std::iter::repeat_n(b'a', 2048));
    big.extend_from_slice(br#""}"#);
    assert_eq!(json_path(&big).unwrap().len(), 2048);
    measure("file_gate_json_path_2kib", 20_000, || {
        black_box(json_path(black_box(&big)).unwrap());
    });

    let spec = "timeout=200,nerve=localhost:8670/health,cortex=-,console=localhost:18080/api/v1/health,hermes=localhost:8642/health,voice=localhost:8002/health";
    let cfg = ProbeConfig::parse(spec).expect("spec");
    assert!(!cfg.endpoints[1].enabled);
    measure("probe_parse_spec", 40_000, || {
        black_box(ProbeConfig::parse(black_box(spec)).expect("spec"));
    });

    let set = probe_set();
    let json = set.to_json();
    assert!(json.contains("\"read_only\":true"));
    assert!(json.contains("\"writes\":0"));
    measure("probe_to_json", 40_000, || {
        black_box(set.to_json());
    });

    let mut body = String::from("{\"ok\":true,\"padding\":\"");
    body.push_str(&"a".repeat(3_000));
    body.push_str("\",\"mode\":\"dry-run\"}");
    assert_eq!(
        observed_mode(Service::Cortex, &body).as_deref(),
        Some("dry-run")
    );
    assert!(observed_mode(Service::Hermes, &body).is_none());
    measure("probe_observed_mode", 40_000, || {
        black_box(observed_mode(black_box(Service::Cortex), black_box(&body)));
    });

    let off = cfg.endpoints[1].clone();
    let report = probe_one(&off, Duration::from_millis(50));
    assert_eq!(report.state, State::Unknown);
    assert!(!report.enabled);
    measure("probe_one_disabled", 100_000, || {
        black_box(probe_one(black_box(&off), Duration::from_millis(50)));
    });
}
