// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Fuzz-style property tests for the HTTP parser and server: random bytes,
//! mutated valid requests, every truncation, and garbage over a real socket.
//! Deterministic (seeded xorshift) so failures reproduce. Set
//! `GALION_FUZZ_ITERS` to run longer, or `GALION_FUZZ_SECS` (+ optional
//! `GALION_FUZZ_CORPUS`, `GALION_FUZZ_SEED`) for the time-boxed,
//! corpus-backed `fuzz_http_timed` run that `.github/workflows/fuzz.yml` uses.

#[path = "fuzz_corpus/mod.rs"]
mod fuzz_corpus;

use galion_common::http::{self, parse_head, ClientRequest, Error, Limits, Response};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn byte(&mut self) -> u8 {
        // Bias towards the bytes that matter to the grammar.
        const HOT: &[u8] = b" \r\n:\t/HTP1.0GETPOST?%=;,\x00\x7f\x80\xff";
        if self.below(3) == 0 {
            HOT[self.below(HOT.len())]
        } else {
            self.next() as u8
        }
    }
}

fn iters(default: usize) -> usize {
    std::env::var("GALION_FUZZ_ITERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

const SEEDS: &[&[u8]] = &[
    b"GET / HTTP/1.1\r\nHost: a\r\n\r\n",
    b"POST /x?y=1 HTTP/1.0\r\nContent-Length: 3\r\nContent-Type: text/plain\r\n\r\nabc",
    b"HEAD /health HTTP/1.1\r\nHost: 127.0.0.1\r\nUser-Agent: x\r\nAccept: */*\r\n\r\n",
    b"OPTIONS * HTTP/1.1\nHost: b\n\n",
];

/// Invariants of a successful parse.
fn check(buf: &[u8], limits: &Limits) {
    match parse_head(buf, limits) {
        Ok(h) => {
            assert!(h.len <= buf.len());
            assert!(h.len <= limits.max_head_bytes);
            assert!(h.headers.len() <= limits.max_headers);
            assert!(!h.method.is_empty() && !h.target.is_empty());
            assert!(h.minor <= 1);
            for (k, v) in &h.headers {
                assert!(!k.is_empty());
                assert!(!k.contains([' ', ':', '\r', '\n']));
                assert!(!v.contains(['\r', '\n', '\0']));
                assert!(!k.eq_ignore_ascii_case("transfer-encoding"));
            }
            // Parsing a prefix that ends exactly at the head gives the same result.
            assert_eq!(parse_head(&buf[..h.len], limits).unwrap(), h);
        }
        Err(Error::Incomplete) => assert!(buf.len() <= limits.max_head_bytes),
        Err(e) => assert!(e.status() >= 400),
    }
}

#[test]
fn random_bytes_never_panic() {
    let limits = Limits::default();
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..iters(20_000) {
        let len = r.below(300);
        let buf: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        check(&buf, &limits);
    }
}

#[test]
fn mutated_requests_never_panic() {
    let limits = Limits {
        max_head_bytes: 512,
        max_headers: 8,
        ..Limits::default()
    };
    let mut r = Rng(0xdead_beef_cafe_f00d);
    for _ in 0..iters(30_000) {
        let mut buf = SEEDS[r.below(SEEDS.len())].to_vec();
        mutate(&mut r, &mut buf);
        check(&buf, &limits);
    }
}

/// One to six grammar-aware edits of a request head.
fn mutate(r: &mut Rng, buf: &mut Vec<u8>) {
    for _ in 0..1 + r.below(6) {
        match r.below(6) {
            0 if !buf.is_empty() => {
                let i = r.below(buf.len());
                buf[i] = r.byte();
            }
            1 => {
                let i = r.below(buf.len() + 1);
                let b = r.byte();
                buf.insert(i, b);
            }
            2 if !buf.is_empty() => {
                let i = r.below(buf.len());
                buf.remove(i);
            }
            3 => {
                let i = r.below(buf.len() + 1);
                let n = r.below(600);
                let b = r.byte();
                buf.splice(i..i, std::iter::repeat_n(b, n));
            }
            4 => {
                let i = r.below(buf.len() + 1);
                let extra: &[u8] = [
                    &b"\r\nContent-Length: 5"[..],
                    b"\r\nTransfer-Encoding: chunked",
                    b"\r\n folded",
                    b"\r\nContent-Length: 99999999999999999999999",
                    b"\r\n\r\n",
                    b"\n\n",
                ][r.below(6)];
                buf.splice(i..i, extra.iter().copied());
            }
            _ => buf.truncate(r.below(buf.len() + 1)),
        }
    }
}

#[test]
fn every_truncation_is_incomplete_or_ok() {
    let limits = Limits::default();
    for seed in SEEDS {
        let full = parse_head(seed, &limits).unwrap();
        for cut in 0..seed.len() {
            match parse_head(&seed[..cut], &limits) {
                Err(Error::Incomplete) => assert!(cut < full.len),
                Ok(h) => assert_eq!(h, full),
                Err(e) => panic!("cut {cut}: {e:?}"),
            }
        }
    }
}

#[test]
fn client_side_parsers_never_panic() {
    let mut r = Rng(0x0123_4567_89ab_cdef);
    for _ in 0..iters(20_000) {
        let len = r.below(200);
        let mut buf: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        if r.below(2) == 0 {
            buf.splice(0..0, b"HTTP/1.1 200 OK\r\n".iter().copied());
        }
        let _ = http::parse_response_head(&buf);
        let _ = http::dechunk(&buf);
        let _ = galion_common::base64::decode(&String::from_utf8_lossy(&buf));
    }
}

/// Garbage over a real socket: the server answers or closes, and still
/// serves a good request afterwards.
#[test]
fn live_server_survives_garbage() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let limits = Limits {
        head_timeout: Duration::from_millis(300),
        body_timeout: Duration::from_millis(300),
        max_body_bytes: 1024,
        ..Limits::default()
    };
    std::thread::spawn(move || {
        http::serve(
            l,
            limits,
            Arc::new(|req: &http::Request| {
                Response::text(
                    200,
                    format!("{} {} {}", req.method(), req.path(), req.body.len()),
                )
            }),
        )
    });
    let mut r = Rng(42);
    for i in 0..iters(80) {
        let mut s = TcpStream::connect(addr).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let mut buf = SEEDS[r.below(SEEDS.len())].to_vec();
        for _ in 0..r.below(4) {
            let j = r.below(buf.len() + 1);
            let b = r.byte();
            buf.insert(j, b);
        }
        if i % 5 == 0 {
            buf = (0..r.below(3000)).map(|_| r.byte()).collect();
        }
        let _ = s.write_all(&buf);
        let mut out = Vec::new();
        let _ = s.read_to_end(&mut out);
        if !out.is_empty() {
            assert!(
                out.starts_with(b"HTTP/1.1 "),
                "{:?}",
                String::from_utf8_lossy(&out)
            );
        }
    }
    let ok = http::send(
        addr,
        &ClientRequest::get("/still", "x"),
        Duration::from_secs(3),
        4096,
    )
    .unwrap();
    assert_eq!((ok.status, ok.text().as_str()), (200, "GET /still 0"));
    let big = vec![b'x'; 5000];
    let r = http::send(
        addr,
        &ClientRequest::post("/big", "x", "text/plain", &big),
        Duration::from_secs(3),
        4096,
    )
    .unwrap();
    assert_eq!(r.status, 413);
    let body = b"hello";
    let r = http::send(
        addr,
        &ClientRequest::post("/b", "x", "text/plain", body),
        Duration::from_secs(3),
        4096,
    )
    .unwrap();
    assert_eq!(r.text(), "POST /b 5");
}

/// A client that sends one byte every 100 ms is cut off at the head deadline.
#[test]
fn slowloris_is_cut_off() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let limits = Limits {
        head_timeout: Duration::from_millis(400),
        ..Limits::default()
    };
    std::thread::spawn(move || {
        http::serve(
            l,
            limits,
            Arc::new(|_: &http::Request| Response::text(200, "ok")),
        )
    });
    let mut s = TcpStream::connect(addr).unwrap();
    let start = std::time::Instant::now();
    let mut cut = false;
    for b in b"GET / HTTP/1.1\r\nX-Slow: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".iter() {
        if s.write_all(&[*b]).is_err() {
            cut = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut out = Vec::new();
    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out);
    assert!(cut || text.starts_with("HTTP/1.1 408"), "{text}");
    assert!(start.elapsed() < Duration::from_secs(6));
}

/// Outcome signature of one head parse: which branch the parser took and
/// the shape of the result. A new signature keeps the input in the corpus.
fn signature(buf: &[u8], limits: &Limits) -> u64 {
    let s = match parse_head(buf, limits) {
        Ok(h) => {
            // Coarse on purpose: known method or not, version, header-count
            // bucket, body framing. A fine signature (raw method, target
            // length) fills the 512-file cap with near-duplicates in seconds.
            let known = matches!(
                h.method.as_str(),
                "GET" | "POST" | "HEAD" | "PUT" | "DELETE" | "OPTIONS" | "PATCH"
            );
            let cl = h
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-length"));
            format!(
                "ok {known} {} {} {cl} {}",
                h.minor,
                h.headers.len().min(9),
                h.target.starts_with('/')
            )
        }
        Err(e) => {
            let d = format!("{e:?}");
            let kind = d.split(['(', ' ', '{']).next().unwrap_or("").to_string();
            format!("err {kind} {}", e.status())
        }
    };
    let r = http::parse_response_head(buf).is_some();
    let c = http::dechunk(buf).is_some();
    fuzz_corpus::fnv(format!("{s} {r} {c}").as_bytes())
}

/// Time-boxed, corpus-backed run for CI and the box (`GALION_FUZZ_SECS`;
/// returns at once when unset, so a plain `cargo test` is unchanged). Mixes
/// random bytes, mutated seeds/corpus entries and the client-side parsers;
/// a panic saves the input under `<corpus>/crashes/` and fails the test.
#[test]
fn fuzz_http_timed() {
    let Some(secs) = fuzz_corpus::secs() else {
        return;
    };
    let seed = fuzz_corpus::seed();
    let mut corpus = fuzz_corpus::Corpus::from_env();
    eprintln!(
        "fuzz seed {seed}, {secs} s, corpus {:?} ({} loaded)",
        corpus.dir(),
        corpus.loaded.len()
    );
    let limits = Limits::default();
    let small = Limits {
        max_head_bytes: 512,
        max_headers: 8,
        ..Limits::default()
    };
    let mut pool: Vec<Vec<u8>> = SEEDS.iter().map(|s| s.to_vec()).collect();
    pool.extend(corpus.loaded.iter().cloned());
    let mut sigs = std::collections::HashSet::new();
    // Replay the loaded corpus first: old finds are re-checked and their
    // shapes count as seen, so a warm run saves only new shapes.
    for buf in &corpus.loaded {
        for lim in [&limits, &small] {
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check(buf, lim);
                signature(buf, lim)
            }));
            match res {
                Ok(sig) => {
                    sigs.insert(sig);
                }
                Err(p) => {
                    let path = corpus.save_crash("http", buf);
                    eprintln!(
                        "fuzz-crash target=http seed={seed} input={} (corpus replay)",
                        path.display()
                    );
                    std::panic::resume_unwind(p);
                }
            }
        }
    }
    let mut r = Rng(seed | 1);
    let budget = Duration::from_secs(secs);
    let start = std::time::Instant::now();
    let mut n: u64 = 0;
    while start.elapsed() < budget {
        // Check the clock every 256 inputs, not every input.
        for _ in 0..256 {
            n += 1;
            let buf: Vec<u8> = match r.below(4) {
                0 => (0..r.below(300)).map(|_| r.byte()).collect(),
                _ => {
                    let mut b = pool[r.below(pool.len())].clone();
                    mutate(&mut r, &mut b);
                    b
                }
            };
            let lim = if r.below(2) == 0 { &limits } else { &small };
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                check(&buf, lim);
                let _ = http::parse_response_head(&buf);
                let _ = http::dechunk(&buf);
                let _ = galion_common::base64::decode(&String::from_utf8_lossy(&buf));
                signature(&buf, lim)
            }));
            match res {
                Ok(sig) => {
                    if sigs.insert(sig) {
                        corpus.save(&buf);
                        if pool.len() < 4 * fuzz_corpus::MAX_FILES {
                            pool.push(buf);
                        }
                    }
                }
                Err(p) => {
                    let path = corpus.save_crash("http", &buf);
                    eprintln!(
                        "fuzz-crash target=http seed={seed} input={}",
                        path.display()
                    );
                    std::panic::resume_unwind(p);
                }
            }
        }
    }
    let line = fuzz_corpus::summary(
        "http",
        n,
        sigs.len(),
        &corpus,
        start.elapsed().as_secs_f64(),
        seed,
    );
    eprintln!("{line}");
    assert!(n > 1000, "too few inputs in {secs} s: {n}");
}

/// The CI corpus helper: load order, de-duplication, size cap, crash path,
/// and no I/O at all without a directory.
#[test]
fn corpus_dir_roundtrip() {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("s202-corpus-roundtrip");
    let _ = std::fs::remove_dir_all(&d);
    let mut c = fuzz_corpus::Corpus::open(Some(d.clone()));
    assert_eq!((c.loaded.len(), c.len()), (0, 0));
    c.save(b"GET / HTTP/1.1\r\n\r\n");
    c.save(b"GET / HTTP/1.1\r\n\r\n"); // duplicate: same hash name
    c.save(&vec![b'x'; fuzz_corpus::MAX_BYTES + 1]); // oversize: skipped
    c.save(b"POST");
    assert_eq!((c.saved, c.len()), (2, 2));
    let crash = c.save_crash("http", b"\xff\x00");
    assert!(crash.starts_with(d.join("crashes")));
    assert_eq!(std::fs::read(&crash).unwrap(), b"\xff\x00");
    // Reopen: both inputs come back (sorted by name), the crashes/ subdir
    // is not loaded as a seed.
    let again = fuzz_corpus::Corpus::open(Some(d.clone()));
    assert_eq!(again.len(), 2);
    let mut got = again.loaded.clone();
    got.sort();
    assert_eq!(
        got,
        vec![b"GET / HTTP/1.1\r\n\r\n".to_vec(), b"POST".to_vec()]
    );
    // No dir: nothing is written anywhere.
    let mut none = fuzz_corpus::Corpus::open(None);
    none.save(b"x");
    assert_eq!((none.saved, none.len()), (0, 0));
    let _ = std::fs::remove_dir_all(&d);
}
