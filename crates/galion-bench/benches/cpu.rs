// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Timed runs for the loopback mock hit path and the HTTP read/write it uses.
//! `cargo bench` ignores `#[test]`, so the timed command is
//! `cargo test --profile bench --bench cpu`.

use galion_bench::client::get;
use galion_bench::http::{read_request, write_response};
use galion_bench::{parse_jsonl, MockServer};
use std::hint::black_box;
use std::io::Cursor;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Instant;

const SAMPLES: usize = 5;

fn report(label: &str, iters: u64, mut op: impl FnMut()) {
    for _ in 0..8 {
        op();
    }
    let mut samples = [0u64; SAMPLES];
    for sample in &mut samples {
        let start = Instant::now();
        for _ in 0..iters {
            op();
        }
        *sample = (start.elapsed().as_nanos() as u64) / iters;
    }
    samples.sort_unstable();
    println!(
        "{label} median_ns {} samples {samples:?} iters {iters}",
        samples[SAMPLES / 2]
    );
}

#[test]
fn cpu_hot_paths() {
    let raw = b"GET /health HTTP/1.1\r\nHost: localhost\r\nAccept: application/json\r\nConnection: close\r\n\r\n";
    report("request_read", 20_000, || {
        let req = read_request(Cursor::new(black_box(raw))).unwrap();
        black_box(req.method);
        black_box(req.path);
    });

    let headers = [("Content-Type".to_string(), "application/json".to_string())];
    let body = r#"{"status":"ok"}"#;
    report("response_write", 20_000, || {
        let mut out = Vec::with_capacity(128);
        write_response(&mut out, black_box(200), &headers, body).unwrap();
        black_box(out);
    });

    let fixtures = parse_jsonl(
        r#"{"id":"ping","method":"GET","path":"/health","status":200,"body":{"status":"ok"}}"#,
    )
    .expect("fixture");
    let server = MockServer::start(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), fixtures)
        .expect("loopback mock");
    let addr = server.addr();
    report("loopback_mock_hit", 200, || {
        let response = get(black_box(addr), "/health").expect("hit");
        assert_eq!(response.status, 200);
        black_box(response.body);
    });
}
