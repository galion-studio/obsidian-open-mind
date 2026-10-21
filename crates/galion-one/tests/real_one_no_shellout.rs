// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//
// Real-life test for swarm/one-no-shellout (wave 4 slot 148, ports
// 20180-20189). Starts the real galion-one binary three times and proves the
// pdftotext / curl shell-outs are gone where the branch says so:
//
//   A  127.0.0.1:20180 (mouth 20181)  default: built-in Rust PDF reader,
//      OPENROUTER_BASE_URL=http://127.0.0.1:20188/api/v1 (loopback stand-in)
//   B  127.0.0.1:20184 (mouth 20185)  ONE_PDF_TEXT=rust+pdftotext
//   C  127.0.0.1:20186 (mouth 20187)  ONE_PDF_TEXT=pdftotext (old path)
//   fake ear 20182, capturing fake mind (Hermes) 20183, OpenRouter
//   stand-in 20188.
//
// PATH holds only a fake `pdftotext` and a fake `curl` that leave a marker
// file when run, so "no subprocess" is observed, not assumed. PDFs go in
// through the real routes (POST /files, then POST /reply with X-Files) and
// the text is read back from what the mind received.
//
// A second test compares the real binary's `pdf-text` against the real
// poppler `pdftotext` on every PDF in GALION_PDF_CORPUS (if set) plus the
// committed fixtures.
//
//   GALION_REAL=1 [GALION_ONE_BIN=target/release/galion-one] \
//   [GALION_PDF_CORPUS=/dir/with/pdfs] \
//   cargo test -p galion-one --test real_one_no_shellout -- --nocapture --test-threads 1

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const A: &str = "127.0.0.1:20180";
const A_MOUTH: &str = "127.0.0.1:20181";
const EAR: &str = "127.0.0.1:20182";
const MIND: &str = "127.0.0.1:20183";
const B: &str = "127.0.0.1:20184";
const B_MOUTH: &str = "127.0.0.1:20185";
const C: &str = "127.0.0.1:20186";
const C_MOUTH: &str = "127.0.0.1:20187";
const ROUTER: &str = "127.0.0.1:20188";
// sha256("FIXTURE42"), the fixture pairing code (not a real secret).
const CODE_HASH: &str = "3315254ec1e783ba6819b1c99c44274501d057455eff3123d3da84d95b944e36";
// A made-up key for the stand-in; never a real credential.
const FAKE_KEY: &str = "sk-test-not-real-148";
const EXPECT: &str = "Galion One PDF test Dobar dan, Zagreb! first page text. Second line: 12345 hermes nerve cortex.";

struct Kill(Vec<Child>, PathBuf);
impl Drop for Kill {
    fn drop(&mut self) {
        for c in &mut self.0 {
            let _ = c.kill();
            let _ = c.wait();
        }
        let _ = std::fs::remove_dir_all(&self.1);
    }
}

fn request(
    addr: &str,
    method: &str,
    path: &str,
    extra: &str,
    body: &[u8],
) -> Option<(u16, String, Vec<u8>)> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(60))).ok()?;
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n{extra}\r\n",
        body.len()
    );
    s.write_all(req.as_bytes()).ok()?;
    s.write_all(body).ok()?;
    let mut raw = Vec::new();
    let _ = s.read_to_end(&mut raw);
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let code = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, head, raw[end + 4..].to_vec()))
}

/// HTTP stand-in that records every request (head + body) and answers with
/// `reply(request)`.
fn stand_in(addr: &str, reply: fn(&str) -> String) -> Arc<Mutex<Vec<String>>> {
    let listener = TcpListener::bind(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
            let mut buf = vec![0u8; 1 << 16];
            let mut got = Vec::new();
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
                if let Some(end) = got.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&got[..end]).to_ascii_lowercase();
                    let len = head
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if got.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let text = String::from_utf8_lossy(&got).into_owned();
            let resp = reply(&text);
            log.lock().unwrap().push(text);
            let _ = s.write_all(resp.as_bytes());
        }
    });
    seen
}

fn json_reply(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn ear_reply(_: &str) -> String {
    json_reply(r#"{"text":"please read my file"}"#)
}

fn mind_reply(_: &str) -> String {
    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n\
     data: {\"choices\":[{\"delta\":{\"content\":\"I read it.\"}}]}\n\n\
     data: [DONE]\n\n"
        .to_string()
}

fn router_reply(req: &str) -> String {
    if !req.starts_with("GET /api/v1/models ") {
        return "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".into();
    }
    // Chunked on purpose: the std client must decode it.
    let body = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/openrouter_models.json"
    ))
    .unwrap();
    let (a, b) = body.split_at(body.len() / 2);
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
        a.len(),
        b.len()
    )
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(secs), "timed out: {what}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn bin() -> String {
    std::env::var("GALION_ONE_BIN").unwrap_or_else(|_| env!("CARGO_BIN_EXE_galion-one").to_string())
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/tests/fixtures/pdf/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn start(work: &Path, home: &str, bind: &str, mouth: &str, extra: &[(&str, String)]) -> Child {
    let home = work.join(home);
    std::fs::create_dir_all(home.join("one")).unwrap();
    let store = work.join(format!("secrets-{}.json", bind.rsplit(':').next().unwrap()));
    std::fs::write(
        &store,
        format!(r#"{{"codeHash":"{CODE_HASH}","tokenHash":"","tokenHashes":[],"deviceHash":"","fails":0,"lockedUntil":0}}"#),
    )
    .unwrap();
    std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut c = Command::new(bin());
    c.env_clear()
        .env("PATH", work.join("bin"))
        .env("HOME", &home)
        .env("TZ", "Europe/Zagreb")
        .env("TMPDIR", work.join("tmp"))
        .env("ONE_NO_WARM", "1")
        .env("GALION_HOME", &home)
        .env("ONE_HERMES_DIR", work.join("hermes"))
        .env("ONE_HERMES_ENV", work.join("hermes.env"))
        .env("ONE_NVIDIA_SMI", "off")
        .env("ONE_CONSOLE", "off")
        .env("GALION_MIND", MIND)
        .env("GALION_EAR", EAR)
        .env("GALION_EYE", MIND)
        .env("GALION_MEMORY", MIND)
        .env("ONE_BIND", bind)
        .env("ONE_MOUTH_BIND", mouth)
        .env("ONE_DEVICE_STORE", &store)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in extra {
        c.env(k, v);
    }
    c.spawn().expect("start galion-one")
}

fn pair(addr: &str) -> String {
    let (c, head, _) = request(addr, "POST", "/pair", "", br#"{"code":"fixture42"}"#).unwrap();
    assert_eq!(c, 200);
    let token = head
        .lines()
        .find_map(|l| l.strip_prefix("Set-Cookie: galion_admin="))
        .and_then(|v| v.split(';').next())
        .expect("cookie");
    format!("Cookie: galion_admin={token}\r\n")
}

/// Upload `bytes` as `name`, ask a voice turn about it, return what the
/// mind was asked (the "They included a file" note) and the turn time.
fn ask_about(
    addr: &str,
    cookie: &str,
    name: &str,
    bytes: &[u8],
    mind: &Arc<Mutex<Vec<String>>>,
) -> (String, u128) {
    let (c, _, body) = request(
        addr,
        "POST",
        "/files",
        &format!("{cookie}X-File-Name: {name}\r\n"),
        bytes,
    )
    .unwrap();
    assert_eq!(c, 200, "upload {name}");
    let body = String::from_utf8_lossy(&body).into_owned();
    let id = body
        .split("\"id\":\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .expect("id")
        .to_string();
    let before = mind.lock().unwrap().len();
    let t = Instant::now();
    let _ = request(
        addr,
        "POST",
        "/reply",
        &format!("{cookie}X-Files: {id}\r\n"),
        b"RIFF0000WAVEfake",
    )
    .unwrap();
    let ms = t.elapsed().as_millis();
    // The mind also gets /health probes from the sampler; take the chat call.
    let chat = || {
        mind.lock().unwrap()[before..]
            .iter()
            .find(|r| r.starts_with("POST /v1/chat/completions "))
            .cloned()
    };
    wait_for("mind asked", 20, || chat().is_some());
    let asked = chat().unwrap();
    let note = asked
        .split("They included a file. ")
        .nth(1)
        .map(|r| {
            r.split("\\nplease read my file")
                .next()
                .unwrap_or(r)
                .to_string()
        })
        .unwrap_or_else(|| panic!("no file note in: {asked}"));
    (note, ms)
}

#[test]
fn real_pdf_and_models_without_subprocesses_on_slot_148_ports() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }
    let work = std::env::temp_dir().join(format!("one-no-shellout-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    for d in ["bin", "tmp", "hermes", "markers"] {
        std::fs::create_dir_all(work.join(d)).unwrap();
    }
    std::fs::write(work.join("hermes.env"), "").unwrap();
    let marker = |n: &str| work.join("markers").join(n);
    for (tool, out) in [("pdftotext", "FAKE PDFTOTEXT OUTPUT"), ("curl", "")] {
        let p = work.join("bin").join(tool);
        std::fs::write(
            &p,
            format!(
                "#!/bin/sh\n: > '{}'\necho '{out}'\n",
                marker(tool).display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let _ear = stand_in(EAR, ear_reply);
    let mind = stand_in(MIND, mind_reply);
    let router = stand_in(ROUTER, router_reply);
    let fake = work.join("bin/pdftotext").to_string_lossy().into_owned();
    let a = start(
        &work,
        "a",
        A,
        A_MOUTH,
        &[
            ("OPENROUTER_BASE_URL", format!("http://{ROUTER}/api/v1")),
            ("OPENROUTER_API_KEY", FAKE_KEY.to_string()),
        ],
    );
    let b = start(
        &work,
        "b",
        B,
        B_MOUTH,
        &[
            ("ONE_PDF_TEXT", "rust+pdftotext".into()),
            ("ONE_PDFTOTEXT", fake.clone()),
        ],
    );
    let c = start(
        &work,
        "c",
        C,
        C_MOUTH,
        &[
            ("ONE_PDF_TEXT", "pdftotext".into()),
            ("ONE_PDFTOTEXT", fake),
        ],
    );
    let _guard = Kill(vec![a, b, c], work.clone());
    for addr in [A, B, C] {
        wait_for("health", 15, || {
            request(addr, "GET", "/health", "", b"").is_some()
        });
    }
    let (ca, cb, cc) = (pair(A), pair(B), pair(C));

    // A: every fixture through the real routes, Rust reader, no subprocess.
    for name in [
        "gs-plain.pdf",
        "gs-objstm.pdf",
        "gs-rc4-r2.pdf",
        "gs-rc4-r3.pdf",
    ] {
        let (note, ms) = ask_about(A, &ca, name, &fixture(name), &mind);
        eprintln!("A {name}: {ms} ms: {note}");
        assert_eq!(note, format!("a pdf named {name}. First page: {EXPECT}"));
    }
    let locked = fixture("gs-userpw.pdf");
    let (note, _) = ask_about(A, &ca, "gs-userpw.pdf", &locked, &mind);
    eprintln!("A gs-userpw.pdf: {note}");
    assert_eq!(
        note,
        format!("a pdf named gs-userpw.pdf, {} bytes", locked.len())
    );
    if let Ok(dir) = std::env::var("GALION_PDF_CORPUS") {
        let mut n = 0;
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        for p in files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "pdf"))
        {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let (note, ms) = ask_about(A, &ca, &name, &std::fs::read(p).unwrap(), &mind);
            let shown: String = note.chars().take(110).collect();
            eprintln!("A corpus {name}: {ms} ms: {shown}");
            n += 1;
        }
        eprintln!("A corpus: {n} PDFs through /files + /reply");
    }
    assert!(!marker("pdftotext").exists(), "A ran pdftotext");

    // A: model list refresh over loopback http, no curl, key in a header.
    let (code, _, body) = request(A, "GET", "/models", &ca, b"").unwrap();
    let body = String::from_utf8_lossy(&body).into_owned();
    eprintln!("A /models: {code} {body}");
    assert_eq!(code, 200);
    assert_eq!(
        std::fs::read_to_string(work.join("a/one/models-cache.json")).unwrap(),
        std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/models_cache_expected.json"
        ))
        .unwrap()
    );
    let seen = router.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "{seen:?}");
    assert!(seen[0].contains(&format!("\r\nAuthorization: Bearer {FAKE_KEY}\r\n")));
    // curl is still used for the HTTPS edge probes (load_edge), by design;
    // it must never have been asked for the model list.
    let curl_calls = std::fs::read_to_string(marker("curl")).unwrap_or_default();
    assert!(
        !curl_calls.contains("models") && !curl_calls.contains("openrouter"),
        "{curl_calls}"
    );
    let edge = curl_calls.lines().count();
    eprintln!("A: models refreshed in-process (1 request to the stand-in); curl calls: {edge}, all edge probes: {:?}", curl_calls.lines().map(|l| l.rsplit(' ').next().unwrap_or("")).collect::<std::collections::BTreeSet<_>>());
    assert!(
        curl_calls.lines().all(|l| l.contains("-w %{http_code}")),
        "{curl_calls}"
    );

    // B: Rust first; the fallback only for the password-protected file.
    let (note, _) = ask_about(B, &cb, "gs-plain.pdf", &fixture("gs-plain.pdf"), &mind);
    assert_eq!(
        note,
        format!("a pdf named gs-plain.pdf. First page: {EXPECT}")
    );
    assert!(
        !marker("pdftotext").exists(),
        "B ran pdftotext although Rust had the text"
    );
    let (note, _) = ask_about(B, &cb, "gs-userpw.pdf", &locked, &mind);
    eprintln!("B gs-userpw.pdf: {note}");
    assert_eq!(
        note,
        "a pdf named gs-userpw.pdf. First page: FAKE PDFTOTEXT OUTPUT"
    );
    assert!(marker("pdftotext").exists());
    std::fs::remove_file(marker("pdftotext")).unwrap();

    // C: the old subprocess path still works when asked for.
    let (note, _) = ask_about(C, &cc, "gs-plain.pdf", &fixture("gs-plain.pdf"), &mind);
    eprintln!("C gs-plain.pdf: {note}");
    assert_eq!(
        note,
        "a pdf named gs-plain.pdf. First page: FAKE PDFTOTEXT OUTPUT"
    );
    assert!(marker("pdftotext").exists());

    // Private temp files: nothing left behind in TMPDIR.
    assert_eq!(std::fs::read_dir(work.join("tmp")).unwrap().count(), 0);
}

fn words(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| !matches!(c, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}'))
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    v.sort();
    v
}

/// Multiset intersection size of two sorted lists.
fn common(a: &[String], b: &[String]) -> usize {
    let (mut i, mut j, mut n) = (0, 0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                n += 1;
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    n
}

#[test]
fn real_pdf_corpus_matches_pdftotext() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real-life test");
        return;
    }
    if Command::new("pdftotext").arg("-v").output().is_err() {
        eprintln!("SKIP: poppler pdftotext not installed (the reference)");
        return;
    }
    let mut files: Vec<PathBuf> =
        std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pdf"))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
    if let Ok(dir) = std::env::var("GALION_PDF_CORPUS") {
        files.extend(std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()));
    }
    files.retain(|p| p.extension().is_some_and(|e| e == "pdf"));
    files.sort();
    let (mut ref_total, mut hit_total, mut checked) = (0usize, 0usize, 0usize);
    let mut worst = (100.0f64, String::new());
    let mut slowest = (0u128, String::new());
    for p in &files {
        let name = p.file_name().unwrap().to_string_lossy().into_owned();
        let reference = Command::new("pdftotext")
            .args(["-f", "1", "-l", "1", "-q"])
            .arg(p)
            .arg("-")
            .output()
            .unwrap();
        let t = Instant::now();
        let ours = Command::new(bin())
            .env_clear()
            .env("PATH", "/nonexistent")
            .arg("pdf-text")
            .arg(p)
            .output()
            .unwrap();
        let ms = t.elapsed().as_millis();
        let r = words(&String::from_utf8_lossy(&reference.stdout));
        let o = words(&String::from_utf8_lossy(&ours.stdout));
        let hit = common(&r, &o);
        let rtl = r
            .iter()
            .any(|w| w.chars().any(|c| ('\u{590}'..='\u{8ff}').contains(&c)));
        let recall = if r.is_empty() {
            100.0
        } else {
            100.0 * hit as f64 / r.len() as f64
        };
        let status = String::from_utf8_lossy(&ours.stderr).trim().to_string();
        eprintln!(
            "{name:44} pdftotext {:5} rust {:5} common {:5} recall {recall:5.1}% {ms:4} ms {status}{}",
            r.len(),
            o.len(),
            hit,
            if rtl { " (RTL: order differs, not scored)" } else { "" }
        );
        if ms > slowest.0 {
            slowest = (ms, name.clone());
        }
        assert!(ms < 5000, "{name} took {ms} ms");
        if rtl || r.len() < 3 {
            continue;
        }
        checked += 1;
        ref_total += r.len();
        hit_total += hit;
        if recall < worst.0 {
            worst = (recall, name.clone());
        }
        assert!(recall >= 85.0, "{name}: recall {recall:.1}%");
    }
    let overall = 100.0 * hit_total as f64 / ref_total.max(1) as f64;
    eprintln!(
        "corpus: {} files, {checked} scored, {hit_total}/{ref_total} words = {overall:.2}% recall; worst {:.1}% ({}), slowest {} ms ({})",
        files.len(),
        worst.0,
        worst.1,
        slowest.0,
        slowest.1
    );
    assert!(overall >= 95.0, "overall recall {overall:.2}%");
}
