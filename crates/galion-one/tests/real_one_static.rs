// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the STATIC x86_64 musl galion-one (Rust port of the old
//! `scripts/real/one-probe.sh`, slug one-probe). Runs only with
//! `GALION_REAL=1`. Starts the binary on spare ports with a FIXTURE home
//! (repo fixtures only, never live data), exercises every HTTP route, the
//! ONE-PROBE self probe, fail-closed pairing and `/api/v1/services`, then
//! kills everything it started.
//!
//! Ports (slot 05 range by default, `GALION_REAL_PORT_BASE` moves them):
//! base+0 galion-one, +1 its mouth, +2 memory-gateway stand-in (a second
//! galion-one with a fixture `TDAI_DATA_DIR`), +3 the stand-in's mouth,
//! +4/+5/+6 mind/ear/eye (nothing listens), +9 the stand-in's own memory
//! target (nothing listens).
//!
//! ```text
//! cargo build --release -p galion-one --target x86_64-unknown-linux-musl
//! GALION_REAL=1 cargo test --release -p galion-one --test real_one_static -- --nocapture
//! ```

use galion_realtest::http::{self, Resp};
use galion_realtest::realrun::{self, Checks, Proc, Scratch};
use galion_realtest::sha256;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const TRIPLE: &str = "x86_64-unknown-linux-musl";
const CODE: &str = "FIXTURE42";

fn call(addr: &str, method: &str, path: &str, cookie: Option<&str>, body: &[u8]) -> Resp {
    let ck = cookie.map(|c| format!("galion_admin={c}"));
    let mut h: Vec<(&str, &str)> = vec![("Content-Type", "application/json")];
    if let Some(c) = ck.as_deref() {
        h.push(("Cookie", c));
    }
    http::request(addr, method, path, &h, body, Duration::from_secs(30))
        .unwrap_or_else(|e| panic!("{method} {path}: {e}"))
}

/// The old script's `show`: one line per request, bodies of JSON/text only.
fn show(addr: &str, method: &str, path: &str, cookie: Option<&str>, body: &[u8]) -> Resp {
    let r = call(addr, method, path, cookie, body);
    let ct = r.header("content-type").unwrap_or("").to_string();
    let shown = if ct.starts_with("application/json") || ct.starts_with("text/plain") {
        let t = r.text().replace('\n', " ");
        t.chars().take(300).collect::<String>()
    } else {
        format!("sha={}", &sha256::hex(&r.body)[..12])
    };
    println!(
        "{method} {path} -> {} {ct} {} | {shown}",
        r.status,
        r.body.len()
    );
    r
}

fn json(r: &Resp) -> Value {
    r.json().unwrap_or(Value::Null)
}

fn readelf(bin: &Path, flag: &str) -> String {
    String::from_utf8_lossy(&realrun::run(Command::new("readelf").arg(flag).arg(bin)).stdout)
        .into_owned()
}

fn write(p: PathBuf, s: &str) {
    std::fs::write(p, s).unwrap();
}

#[test]
fn static_one_routes_pairing_lockout_fail_closed() {
    if !realrun::enabled("real_one_static") {
        return;
    }
    let base = realrun::port_base(19050);
    let p = |o: u16| format!("127.0.0.1:{}", base + o);
    realrun::require_free(&[base, base + 1, base + 2, base + 3]);
    let bins = realrun::release_bins_for(Some(TRIPLE), &["-p", "galion-one"]);
    let bin = bins.join("galion-one");
    let mut c = Checks::new();

    realrun::say(format!("binary {}", bin.display()));
    let h = readelf(&bin, "-h");
    for l in h
        .lines()
        .filter(|l| l.contains("Type:") || l.contains("Machine:"))
    {
        println!("  {}", l.trim());
    }
    c.that("x86-64 ELF", h.contains("X86-64"));
    let needed = readelf(&bin, "-d").matches("(NEEDED)").count();
    c.eq("static: 0 NEEDED entries", needed, 0);
    c.that(
        "static: no program interpreter",
        !readelf(&bin, "-l").contains("program interpreter"),
    );
    let bytes = std::fs::read(&bin).unwrap();
    println!(
        "  sha256: {}  size: {}",
        &sha256::hex(&bytes)[..16],
        bytes.len()
    );

    // ---- fixture home (generated here; no live files)
    let w = Scratch::new("one-probe");
    let fix = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let home = w.path("home");
    for d in [
        "home/one/font",
        "home/guardian",
        "hermes/channels",
        "mem/data",
        "memhome",
    ] {
        std::fs::create_dir_all(w.path(d)).unwrap();
    }
    for page in ["index", "nerve", "admin", "space"] {
        write(
            home.join(format!("one/{page}.html")),
            &format!("<!doctype html><html><head><title>fixture {page}</title></head><body><p>fixture {page}</p></body></html>\n"),
        );
    }
    write(
        home.join("one/mark.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 1 1\"/>\n",
    );
    write(home.join("one/font/jost.woff2"), "wOF2fixture");
    write(
        home.join("one/catalog.json"),
        "[{\"kind\":\"file\",\"place\":\"box\",\"name\":\"fixture.txt\"}]\n",
    );
    write(
        home.join("one/models-cache.json"),
        "[{\"id\": \"fixture/model\", \"name\": \"Fixture model\"}]\n",
    );
    std::fs::copy(
        fix.join("guardian-fixture.db"),
        home.join("guardian/guardian.db"),
    )
    .unwrap();
    std::fs::copy(
        fix.join("memory-wal-fixture.db"),
        w.path("mem/data/vectors.db"),
    )
    .unwrap();
    std::fs::copy(
        fix.join("memory-wal-fixture.db-wal"),
        w.path("mem/data/vectors.db-wal"),
    )
    .unwrap();
    write(w.path("hermes/modes.json"), "{\"saved\":[]}\n");
    write(w.path("hermes.env"), "");
    let store = w.path("admin-device.json");
    write(
        store.clone(),
        &format!(
            "{{\"codeHash\":\"{}\",\"tokenHash\":\"\",\"tokenHashes\":[],\"deviceHash\":\"\",\"fails\":0,\"lockedUntil\":0}}\n",
            sha256::hex(CODE.as_bytes())
        ),
    );
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&store, std::fs::Permissions::from_mode(0o600)).unwrap();
    }

    let common = |cmd: &mut Command| {
        cmd.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("ONE_NO_WARM", "1")
            .env("ONE_HERMES_ENV", w.path("hermes.env"))
            .env("GALION_EAR", p(5))
            .env("GALION_EYE", p(6))
            .env("GALION_MIND", p(4));
    };
    let mut mcmd = Command::new(&bin);
    common(&mut mcmd);
    mcmd.env("HOME", w.path("memhome"))
        .env("TDAI_DATA_DIR", w.path("mem/data"))
        .env("ONE_BIND", p(2))
        .env("ONE_MOUTH_BIND", p(3))
        .env("GALION_MEMORY", p(9))
        .env("GALION_HOME", w.path("memhome"))
        .env("ONE_HERMES_DIR", w.path("memhome/hermes"))
        .env("ONE_DEVICE_STORE", w.path("none.json"));
    let mem = Proc::spawn("memory-stand-in", mcmd, &w.path("mem.log"));
    let mut ocmd = Command::new(&bin);
    common(&mut ocmd);
    ocmd.env("HOME", w.path("home"))
        .env("GALION_HOME", &home)
        .env("ONE_HERMES_DIR", w.path("hermes"))
        .env("ONE_DEVICE_STORE", &store)
        .env("ONE_BIND", p(0))
        .env("ONE_MOUTH_BIND", p(1))
        .env("GALION_MEMORY", p(2));
    let one = Proc::spawn("galion-one", ocmd, &w.path("one.log"));
    let b = p(0);
    assert!(
        realrun::wait_http(&b, "/health", Duration::from_secs(10)),
        "one did not start"
    );
    std::thread::sleep(Duration::from_secs(6)); // let sample() run a few ticks

    realrun::say("unpaired (store present, no cookie)");
    for path in [
        "/",
        "/index.html",
        "/nerve",
        "/admin",
        "/space",
        "/mark.svg",
        "/font/jost.woff2",
        "/link",
        "/api/v1/awake",
        "/api/v1/catalog",
        "/pair",
    ] {
        show(&b, "GET", path, None, b"");
    }
    let r = show(&b, "GET", "/health", None, b"");
    c.eq(
        "health 200 ok",
        (r.status, r.text().trim().to_string()),
        (200, "ok".into()),
    );
    let r = show(&b, "GET", "/me", None, b"");
    c.eq("unpaired /me", json(&r)["you"].as_bool(), Some(false));
    let r = show(&b, "GET", "/api/v1/services", None, b"");
    let mem_ok = json(&r)["services"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["id"] == "openmind-memory").cloned())
        .map(|s| (s["port"].as_u64(), s["ok"].as_bool()));
    c.eq(
        "services: memory up through the stand-in",
        mem_ok,
        Some((Some(u64::from(base + 2)), Some(true))),
    );
    for path in [
        "/api/v1/analytics",
        "/api/v1/analytics/services",
        "/session",
        "/files",
        "/files/abc",
        "/model",
        "/models",
    ] {
        let r = show(&b, "GET", path, None, b"");
        c.eq(&format!("unpaired GET {path} refused"), r.status, 401);
    }
    c.eq(
        "GET /nope 404",
        show(&b, "GET", "/nope", None, b"").status,
        404,
    );
    for (path, body) in [
        ("/admin/say", &br#"{"text":"hi"}"#[..]),
        ("/files", b"x"),
        ("/model", br#"{"model":"x"}"#),
        ("/voice", br#"{"voice":"jarvis"}"#),
    ] {
        let r = show(&b, "POST", path, None, body);
        c.eq(&format!("unpaired POST {path} refused"), r.status, 401);
    }
    for path in ["/hear", "/reply"] {
        let r = show(&b, "POST", path, None, b"RIFF");
        c.eq(
            &format!("unpaired POST {path} says pair"),
            json(&r)["error"].as_str(),
            Some("pair"),
        );
    }

    realrun::say("pairing");
    let r = show(&b, "POST", "/pair", None, br#"{"code":"WRONG1"}"#);
    c.eq("wrong code", json(&r)["error"].as_str(), Some("no"));
    let r = call(
        &b,
        "POST",
        "/pair",
        None,
        format!("{{\"code\":\"{}\"}}", CODE.to_lowercase()).as_bytes(),
    );
    let cookie = r
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .find_map(|(_, v)| {
            v.strip_prefix("galion_admin=")
                .and_then(|v| v.split(';').next())
        })
        .map(str::to_string);
    c.that("right code (lower-case) issues a cookie", cookie.is_some());
    let cookie = cookie.unwrap_or_default();
    let s = std::fs::read_to_string(&store).unwrap_or_default();
    c.that(
        "store holds hashes only (no code, no cookie)",
        !s.contains(CODE) && !s.contains(&cookie),
    );
    let ck = Some(cookie.as_str());
    let r = show(&b, "GET", "/me", ck, b"");
    c.eq("paired /me", json(&r)["you"].as_bool(), Some(true));

    realrun::say("paired");
    for path in [
        "/api/v1/analytics/health",
        "/api/v1/analytics/infrastructure",
        "/api/v1/analytics/services",
        "/api/v1/analytics/agents",
        "/api/v1/analytics/users",
        "/api/v1/analytics/workforce",
        "/api/v1/analytics/overview",
        "/session",
        "/files",
        "/model",
        "/models",
    ] {
        let r = show(&b, "GET", path, ck, b"");
        c.eq(&format!("paired GET {path}"), r.status, 200);
    }
    let full = json(&call(&b, "GET", "/api/v1/analytics", ck, b""));
    let mut keys: Vec<&String> = full
        .as_object()
        .map(|o| o.keys().collect())
        .unwrap_or_default();
    keys.sort();
    println!("GET /api/v1/analytics -> keys: {keys:?}");
    let counts = &full["memory"]["counts"];
    println!("  memory counts: {counts}");
    c.eq(
        "memory counts from the WAL fixture",
        (
            counts["l0_conversations"].as_u64(),
            counts["l1_records"].as_u64(),
        ),
        (Some(600), Some(333)),
    );
    let r = show(&b, "POST", "/files?name=note.txt", ck, b"fixture file body");
    let id = json(&r)["id"].as_str().unwrap_or("").to_string();
    c.that("upload returns an id", !id.is_empty());
    let r = show(&b, "GET", &format!("/files/{id}"), ck, b"");
    c.eq(
        "download returns the bytes",
        r.body,
        b"fixture file body".to_vec(),
    );
    c.eq(
        "GET /files/zz 404",
        show(&b, "GET", "/files/zz", ck, b"").status,
        404,
    );
    show(&b, "POST", "/model", ck, br#"{"model":"anything"}"#);
    let r = show(&b, "POST", "/voice", ck, br#"{"voice":"jarvis"}"#);
    c.eq("voice switch", json(&r)["voice"].as_str(), Some("jarvis"));
    show(&b, "POST", "/hear", ck, b"RIFF");
    show(&b, "POST", "/reply", ck, b"RIFF");
    let r = show(&b, "POST", "/admin/say", ck, br#"{"text":"hello"}"#);
    c.eq("admin/say paired", r.status, 200);

    realrun::say("lockout: 5 wrong codes, then the right one");
    for _ in 0..5 {
        show(&b, "POST", "/pair", None, br#"{"code":"WRONG"}"#);
    }
    let r = show(
        &b,
        "POST",
        "/pair",
        None,
        format!("{{\"code\":\"{CODE}\"}}").as_bytes(),
    );
    c.eq(
        "locked out even with the right code",
        json(&r)["error"].as_str(),
        Some("later"),
    );
    let st: Value = serde_json::from_str(&std::fs::read_to_string(&store).unwrap_or_default())
        .unwrap_or(Value::Null);
    c.eq("store counts 5 fails", st["fails"].as_u64(), Some(5));
    c.that(
        "store has a lock time",
        st["lockedUntil"].as_u64().unwrap_or(0) > 0,
    );

    realrun::say("fail closed: device store removed, old cookie");
    std::fs::rename(&store, w.path("admin-device.json.gone")).unwrap();
    let r = show(&b, "GET", "/me", ck, b"");
    c.eq(
        "no store: /me you:false",
        json(&r)["you"].as_bool(),
        Some(false),
    );
    c.eq(
        "no store: analytics 401",
        show(&b, "GET", "/api/v1/analytics", ck, b"").status,
        401,
    );
    c.eq(
        "no store: session 401",
        show(&b, "GET", "/session", ck, b"").status,
        401,
    );
    let r = show(
        &b,
        "POST",
        "/pair",
        None,
        format!("{{\"code\":\"{CODE}\"}}").as_bytes(),
    );
    c.eq(
        "no store: right code refused",
        json(&r)["error"].as_str(),
        Some("no"),
    );

    realrun::say("mouth");
    let m = p(1);
    let r = show(&m, "GET", "/health", None, b"");
    c.eq("mouth /health 503 loading", r.status, 503);
    show(&m, "POST", "/synthesize", None, br#"{"text":"hi"}"#);
    c.eq(
        "mouth /nope 404",
        show(&m, "GET", "/nope", None, b"").status,
        404,
    );

    realrun::say("ONE-PROBE: stderr of both instances");
    println!("  stderr one: {}", one.log_text().replace('\n', " "));
    println!("  stderr mem: {}", mem.log_text().replace('\n', " "));
    drop(one);
    drop(mem);
    let open = realrun::ports_closed(
        &[base, base + 1, base + 2, base + 3],
        Duration::from_secs(3),
    );
    c.eq("ports closed after teardown", open, vec![]);
    c.finish("real_one_static");
}
