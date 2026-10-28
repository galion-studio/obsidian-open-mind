// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The box-stack scenario suite. Every check talks to a real release binary
//! over real loopback HTTP (or runs a real CLI). Pairing tokens and the
//! fixture memory key are kept in memory and never written to a report.

use crate::http::{self, Resp, STUB_CTX, STUB_MODEL};
use crate::report::Suite;
use crate::stack::{off, signal_pid, Stack};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const RELAY_EXAMPLE: &str =
    include_str!("../../persona-plex-relay/persona-plex-relay.example.toml");

fn want_status(r: &Resp, code: u16) -> Result<(), String> {
    if r.status == code {
        Ok(())
    } else {
        Err(format!(
            "want HTTP {code}, got {} body {:?}",
            r.status,
            short(&r.text())
        ))
    }
}

fn short(s: &str) -> String {
    s.chars().take(160).collect()
}

fn zagreb_offset(ts: &str) -> Result<(), String> {
    if (ts.ends_with("+02:00") || ts.ends_with("+01:00")) && !ts.contains('Z') {
        Ok(())
    } else {
        Err(format!("not a Europe/Zagreb RFC 3339 stamp: {ts:?}"))
    }
}

/// Poll `f` until it returns Ok or `limit` passes.
fn eventually<T>(
    limit: Duration,
    mut f: impl FnMut() -> Result<T, String>,
) -> Result<(T, u128), String> {
    let t = Instant::now();
    loop {
        match f() {
            Ok(v) => return Ok((v, t.elapsed().as_millis())),
            Err(e) if t.elapsed() >= limit => {
                return Err(format!(
                    "{e} (gave up after {} ms)",
                    t.elapsed().as_millis()
                ))
            }
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
}

fn letters(n: usize) -> String {
    let mut raw = vec![0u8; n];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut raw);
    }
    raw.iter().map(|b| (b'a' + b % 26) as char).collect()
}

/// Stack boot: every supervised service reached health.
pub fn boot(st: &mut Stack) -> Suite {
    let mut s = Suite::new("stack-boot", "galion-boot");
    let alive = st.supervisor_alive();
    s.check("supervisor-running", || {
        if alive {
            Ok(format!(
                "galion-boot supervise up; boot to all-ready {} ms",
                st.boot_ms
            ))
        } else {
            Err("galion-boot supervise exited".into())
        }
    });
    for (name, ready) in st.ready.clone() {
        s.check(&format!("ready:{name}"), || match ready {
            Some(ms) => Ok(format!("healthy {ms} ms after supervisor start")),
            None => Err(format!(
                "never healthy in 30 s; log tail:\n{}",
                st.log_tail(&name, 8)
            )),
        });
    }
    let started = st
        .svcs
        .iter()
        .filter(|v| v.health.is_none())
        .map(|v| v.name.clone())
        .collect::<Vec<_>>();
    for name in started {
        s.check(&format!("running:{name}"), || match st.pid(&name) {
            Some(pid) => Ok(format!(
                "pid {pid} (no health endpoint; checked by its scenario)"
            )),
            None => Err("no pid file from the supervisor".into()),
        });
    }
    s.log_tail = crate::stack::tail_file(&st.path("logs/supervisor.log"), 25);
    s
}

fn cookie_of(r: &Resp) -> Option<String> {
    let v = r.header("set-cookie")?;
    let first = v.split(';').next()?.trim();
    first
        .strip_prefix("galion_admin=")
        .map(|t| t.to_string())
        .filter(|t| !t.is_empty())
}

/// Galion One with a real pairing store: unpaired is refused, the right code
/// pairs, a lockout refuses even the right code.
pub fn one_pairing(st: &Stack) -> Suite {
    let mut s = Suite::new("one-pairing", "galion-one");
    let a = st.addr(off::ONE_STORE);
    s.check("health-ok", || {
        let r = http::get(&a, "/health")?;
        want_status(&r, 200)?;
        (r.text().trim() == "ok")
            .then(|| "200 ok".to_string())
            .ok_or(format!("body {:?}", r.text()))
    });
    unpaired_checks(&mut s, &a);
    s.check("pair-wrong-code-refused", || {
        let r = http::post_json(&a, "/pair", &[], "{\"code\":\"WRONG0\"}")?;
        if cookie_of(&r).is_some() {
            return Err("wrong code was issued a cookie".into());
        }
        (r.text().contains("\"error\""))
            .then(|| format!("{} {}", r.status, r.text()))
            .ok_or(format!("body {:?}", r.text()))
    });
    let mut token = None;
    s.check("pair-right-code-issues-cookie", || {
        let body = format!("{{\"code\":\"{}\"}}", st.pair_code.to_ascii_lowercase());
        let r = http::post_json(&a, "/pair", &[], &body)?;
        let t = cookie_of(&r).ok_or(format!("no galion_admin cookie; body {:?}", r.text()))?;
        let flags = r.header("set-cookie").unwrap_or("").to_string();
        if !flags.contains("HttpOnly") || !flags.contains("SameSite=Strict") {
            return Err("cookie lacks HttpOnly/SameSite=Strict".into());
        }
        token = Some(t);
        Ok(format!(
            "{} {} (cookie issued, HttpOnly, SameSite=Strict; value not recorded)",
            r.status,
            r.text()
        ))
    });
    s.check("paired-cookie-accepted", || {
        let t = token.clone().ok_or("no token from the previous step")?;
        let c = format!("galion_admin={t}");
        let me = http::request(
            &a,
            "GET",
            "/me",
            &[("Cookie", &c)],
            b"",
            Duration::from_secs(5),
        )?;
        if !me.text().contains("\"you\":true") {
            return Err(format!("/me with cookie: {}", me.text()));
        }
        let an = http::request(
            &a,
            "GET",
            "/api/v1/analytics",
            &[("Cookie", &c)],
            b"",
            Duration::from_secs(8),
        )?;
        want_status(&an, 200)?;
        Ok("/me you:true, /api/v1/analytics 200".into())
    });
    s.check("lockout-after-5-wrong-codes", || {
        for i in 0..5 {
            http::post_json(&a, "/pair", &[], &format!("{{\"code\":\"BAD{i}\"}}"))?;
        }
        let body = format!("{{\"code\":\"{}\"}}", st.pair_code);
        let r = http::post_json(&a, "/pair", &[], &body)?;
        if cookie_of(&r).is_some() {
            return Err("right code paired during lockout".into());
        }
        r.text()
            .contains("later")
            .then(|| format!("right code during lockout: {}", r.text()))
            .ok_or(format!("body {:?}", r.text()))
    });
    s.check("store-holds-hashes-only", || {
        let text = fs::read_to_string(st.path("one-store/admin-device.json"))
            .map_err(|e| e.to_string())?;
        if text.contains(&st.pair_code) || token.as_ref().is_some_and(|t| text.contains(t.as_str()))
        {
            return Err("plaintext code or token in the device store".into());
        }
        Ok("device store has hashes only".into())
    });
    s.log_tail = st.log_tail("one-store", 10);
    s
}

fn unpaired_checks(s: &mut Suite, a: &str) {
    s.check("me-unpaired-false", || {
        let r = http::get(a, "/me")?;
        want_status(&r, 200)?;
        r.text()
            .contains("\"you\":false")
            .then(|| r.text())
            .ok_or(format!(
                "FAIL-OPEN: /me says {:?} without a cookie",
                r.text()
            ))
    });
    s.check("session-no-cookie-401", || {
        let r = http::get(a, "/session")?;
        want_status(&r, 401).map_err(|e| format!("FAIL-OPEN: {e}"))?;
        Ok(format!("401 {}", r.text()))
    });
    s.check("session-forged-cookie-401", || {
        let r = http::request(
            a,
            "GET",
            "/session",
            &[("Cookie", "galion_admin=forged-by-realtest")],
            b"",
            Duration::from_secs(5),
        )?;
        want_status(&r, 401).map_err(|e| format!("FAIL-OPEN: {e}"))?;
        Ok("401".into())
    });
    s.check("analytics-no-cookie-401", || {
        let r = http::request(
            a,
            "GET",
            "/api/v1/analytics",
            &[],
            b"",
            Duration::from_secs(8),
        )?;
        want_status(&r, 401).map_err(|e| format!("FAIL-OPEN: {e}"))?;
        Ok("401".into())
    });
    s.check("files-no-cookie-401", || {
        let r = http::get(a, "/files")?;
        want_status(&r, 401).map_err(|e| format!("FAIL-OPEN: {e}"))?;
        Ok("401".into())
    });
}

/// Galion One with a missing or empty pairing store must fail CLOSED.
pub fn one_fail_closed(st: &Stack, scenario: &str, svc: &str, offset: u16, what: &str) -> Suite {
    let mut s = Suite::new(scenario, "galion-one");
    let a = st.addr(offset);
    s.check("health-ok", || {
        let r = http::get(&a, "/health")?;
        want_status(&r, 200)?;
        Ok(format!("200 ({what})"))
    });
    unpaired_checks(&mut s, &a);
    s.check("pair-any-code-refused", || {
        let r = http::post_json(&a, "/pair", &[], "{\"code\":\"ANYTHING\"}")?;
        match cookie_of(&r) {
            Some(_) => Err("a cookie was issued with no pairing code configured".into()),
            None => Ok(format!("{} {}", r.status, r.text())),
        }
    });
    s.log_tail = st.log_tail(svc, 10);
    s
}

/// openmind-memory: auth, save (capture), search, data on disk.
pub fn memory(st: &Stack) -> Suite {
    let mut s = Suite::new("memory-save-search", "openmind-memory");
    let a = st.addr(off::MEMORY);
    let bearer = format!("Bearer {}", st.mem_key);
    let auth = [("Authorization", bearer.as_str())];
    let word = format!("realtest{}", letters(10));
    let session = format!("realtest-{}", letters(6));
    s.check("health-json", || {
        let j = http::get(&a, "/health")?.json()?;
        let ok =
            j["status"] == "ok" && j["implementation"] == "openmind-memory" && j["fts"] == true;
        ok.then(|| {
            format!(
                "status ok, fts true, version {}",
                j["implementation_version"]
            )
        })
        .ok_or(format!("health {j}"))
    });
    let body = format!(
        "{{\"user_content\":\"please remember the {word} fixture\",\"assistant_content\":\"noted: {word} is a realtest marker\",\"session_key\":\"{session}\"}}"
    );
    s.check("capture-no-bearer-401", || {
        let r = http::post_json(&a, "/capture", &[], &body)?;
        want_status(&r, 401).map(|_| "401".to_string())
    });
    s.check("capture-wrong-bearer-401", || {
        let r = http::post_json(
            &a,
            "/capture",
            &[("Authorization", "Bearer wrong-realtest")],
            &body,
        )?;
        want_status(&r, 401).map(|_| "401".to_string())
    });
    s.check("capture-saves-two-messages", || {
        let r = http::post_json(&a, "/capture", &auth, &body)?;
        want_status(&r, 200)?;
        let j = r.json()?;
        (j["l0_recorded"] == 2)
            .then(|| j.to_string())
            .ok_or(format!("capture {j}"))
    });
    s.check("search-conversations-finds-it", || {
        let q = format!("{{\"query\":\"{word}\",\"limit\":5}}");
        let (j, ms) = eventually(Duration::from_secs(3), || {
            let j = http::post_json(&a, "/search/conversations", &auth, &q)?.json()?;
            let hit = j["total"].as_u64().unwrap_or(0) >= 1
                && j["results"].as_str().unwrap_or("").contains(&word);
            hit.then_some(j.clone()).ok_or(format!("search {j}"))
        })?;
        Ok(format!("total {} after {ms} ms", j["total"]))
    });
    s.check("search-session-filter", || {
        let q = format!("{{\"query\":\"{word}\",\"session_key\":\"some-other-session\"}}");
        let j = http::post_json(&a, "/search/conversations", &auth, &q)?.json()?;
        (j["total"] == 0)
            .then(|| "other session sees 0".to_string())
            .ok_or(format!("leak across sessions: {j}"))
    });
    s.check("search-memories-answers", || {
        let q = format!("{{\"query\":\"{word}\"}}");
        let r = http::post_json(&a, "/search/memories", &auth, &q)?;
        want_status(&r, 200)?;
        let j = r.json()?;
        j.get("results")
            .is_some()
            .then(|| format!("strategy {}", j["strategy"]))
            .ok_or(format!("{j}"))
    });
    s.check("v2-needs-service-id", || {
        let r = http::post_json(&a, "/v2/conversation/add", &auth, "{}")?;
        want_status(&r, 401)?;
        Ok(short(&r.text()))
    });
    s.check("saved-on-disk", || {
        let dir = st.path("memory/conversations");
        let found = fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .flatten()
            .any(|e| fs::read_to_string(e.path()).is_ok_and(|t| t.contains(&word)));
        let db = st.path("memory/instances/realtest/vectors.db");
        (found && db.is_file())
            .then(|| "L0 JSONL shard + vectors.db present".to_string())
            .ok_or(format!("jsonl hit {found}, db {}", db.is_file()))
    });
    s.log_tail = st.log_tail("openmind-memory", 8);
    s
}

/// Nerve ingest. There is no nerve binary at 13a296c; the contract below is
/// the one on the swarm nerve work (`POST /signal` -> 202, `GET /signals`).
pub fn nerve(st: &Stack) -> Suite {
    let mut s = Suite::new("nerve-ingest", "openmind-nerve");
    if !st.nerve {
        let why = match st.bins.nerve() {
            Some(p) => format!(
                "{} present but not started (pass --with-nerve)",
                p.display()
            ),
            None => format!(
                "no openmind-nerve/galion-nerve binary in {} (none exists at 13a296c)",
                st.bins.dir.display()
            ),
        };
        s.skip("ingest-signal", &why);
        return s;
    }
    let a = st.addr(off::NERVE);
    s.check("healthz", || {
        want_status(&http::get(&a, "/healthz")?, 200).map(|_| "200".into())
    });
    s.check("ingest-signal-202", || {
        let r = http::post_json(
            &a,
            "/signal",
            &[],
            "{\"source\":\"realtest\",\"kind\":\"event\",\"name\":\"realtest-probe\",\"value\":1}",
        )?;
        want_status(&r, 202).map(|_| r.text())
    });
    s.check("signal-visible", || {
        let (_, ms) = eventually(Duration::from_secs(3), || {
            let t = http::get(&a, "/signals")?.text();
            t.contains("realtest")
                .then_some(())
                .ok_or(format!("/signals: {}", short(&t)))
        })?;
        Ok(format!("visible after {ms} ms"))
    });
    s.check("bad-signal-400", || {
        want_status(
            &http::post_json(&a, "/signal", &[], "{\"kind\":\"nope\"}")?,
            400,
        )
        .map(|_| "400".into())
    });
    s.log_tail = st.log_tail("nerve", 8);
    s
}

pub fn tuner(st: &Stack) -> Suite {
    let mut s = Suite::new("tuner", "tuner");
    let a = st.addr(off::TUNER);
    s.check("health-exact", || {
        let r = http::get(&a, "/health")?;
        want_status(&r, 200)?;
        let ok = r.text() == "{\"ok\":true}\n"
            && r.header("content-type") == Some("application/json")
            && r.header("cache-control") == Some("no-store");
        ok.then(|| "{\"ok\":true} json no-store".to_string())
            .ok_or(format!("{:?} {:?}", r.text(), r.headers))
    });
    s.check("head-health-no-body", || {
        let r = http::request(&a, "HEAD", "/health", &[], b"", Duration::from_secs(5))?;
        want_status(&r, 200)?;
        r.body
            .is_empty()
            .then(|| "200, empty body".to_string())
            .ok_or("HEAD returned a body".into())
    });
    s.check("page-served-query-stripped", || {
        let r = http::get(&a, "/?realtest=1")?;
        want_status(&r, 200)?;
        r.text()
            .contains("tuner fixture")
            .then(|| "fixture index.html".to_string())
            .ok_or(short(&r.text()))
    });
    s.check("assets-served", || {
        for p in ["/app.js", "/pitch.js", "/style.css", "/index.html"] {
            want_status(&http::get(&a, p)?, 200).map_err(|e| format!("{p}: {e}"))?;
        }
        Ok("app.js pitch.js style.css index.html".into())
    });
    s.check("post-405", || {
        want_status(&http::post_json(&a, "/", &[], "{}")?, 405).map(|_| "405".into())
    });
    s.check("unknown-404", || {
        want_status(&http::get(&a, "/realtest-nope")?, 404).map(|_| "404".into())
    });
    s.check("no-path-traversal", || {
        let r = http::get(&a, "/../../etc/passwd")?;
        (r.status == 404 && !r.text().contains("root:"))
            .then(|| "404".to_string())
            .ok_or(format!("{}", r.status))
    });
    s.log_tail = st.log_tail("tuner", 5);
    s
}

fn read_link(st: &Stack) -> Result<Value, String> {
    let t =
        fs::read_to_string(st.path("state/link.json")).map_err(|e| format!("link.json: {e}"))?;
    serde_json::from_str(&t).map_err(|e| format!("link.json not JSON: {e}"))
}

pub fn link_watch(st: &Stack) -> Suite {
    let mut s = Suite::new("link-watch", "link-watch");
    s.check("writes-link-json", || {
        let want = format!("{STUB_MODEL} ctx {STUB_CTX}");
        let (j, ms) = eventually(Duration::from_secs(10), || {
            let j = read_link(st)?;
            (j["laptop_port"] == true && j["hermes"] == true && j["model"] == want.as_str())
                .then_some(j.clone())
                .ok_or(format!("{j}"))
        })?;
        Ok(format!(
            "laptop_port true, model {}, hermes true ({ms} ms)",
            j["model"]
        ))
    });
    s.check("at-local-is-zagreb", || {
        let j = read_link(st)?;
        let ts = j["at_local"].as_str().unwrap_or("");
        zagreb_offset(ts).map(|_| ts.to_string())
    });
    s.check("detects-hermes-down", || {
        st.stub.hermes_up.store(false, Ordering::SeqCst);
        let r = eventually(Duration::from_secs(12), || {
            let j = read_link(st)?;
            (j["hermes"] == false)
                .then_some(())
                .ok_or("hermes still true".to_string())
        });
        st.stub.hermes_up.store(true, Ordering::SeqCst);
        let ((), ms) = r?;
        Ok(format!("hermes:false {ms} ms after the stub went 503"))
    });
    s.check("logs-change-event", || {
        let t = fs::read_to_string(st.path("state/link-log.json")).map_err(|e| e.to_string())?;
        t.contains("hermes down")
            .then(|| "event 'hermes down' in link-log.json".to_string())
            .ok_or(short(&t))
    });
    s.check("recovers-hermes-up", || {
        let ((), ms) = eventually(Duration::from_secs(12), || {
            let j = read_link(st)?;
            (j["hermes"] == true)
                .then_some(())
                .ok_or("hermes still false".to_string())
        })?;
        Ok(format!("hermes:true again after {ms} ms"))
    });
    s.log_tail = st.log_tail("link-watch", 5);
    s
}

pub fn desk(st: &Stack) -> Suite {
    let mut s = Suite::new("desk", "galion-desk");
    let a = st.addr(off::DESK);
    let marker = format!("realtest note {}", letters(8));
    s.check("health-json", || {
        let r = http::get(&a, "/health")?;
        want_status(&r, 200)?;
        (r.text() == "{\"ok\":true}")
            .then(|| r.text())
            .ok_or(r.text())
    });
    s.check("snapshot-json", || {
        let j = http::get(&a, "/desk")?.json()?;
        let ok = j["foundation"] == "rust"
            && j["line"] == true
            && j["model"].as_str().unwrap_or("").contains(STUB_MODEL);
        ok.then(|| format!("foundation rust, line true, model {}", j["model"]))
            .ok_or(short(&j.to_string()))
    });
    s.check("note-saved", || {
        let r = http::post_json(
            &a,
            "/desk",
            &[],
            &format!("{{\"act\":\"note\",\"text\":\"{marker}\"}}"),
        )?;
        let j = r.json()?;
        let has = j["notes"]
            .as_array()
            .is_some_and(|n| n.iter().any(|x| x["text"] == marker.as_str()));
        has.then(|| "note in snapshot and desk-notes.jsonl".to_string())
            .ok_or(short(&j["notes"].to_string()))
    });
    s.check("voice-switch", || {
        let j =
            http::post_json(&a, "/desk", &[], "{\"act\":\"voice\",\"name\":\"jarvis\"}")?.json()?;
        let file = fs::read_to_string(st.path("one-dir/voice")).unwrap_or_default();
        (j["voice"] == "jarvis" && file.trim() == "jarvis")
            .then(|| "voice jarvis".to_string())
            .ok_or(format!("voice {}", j["voice"]))
    });
    s.check("voice-rejects-unknown", || {
        let j = http::post_json(&a, "/desk", &[], "{\"act\":\"voice\",\"name\":\"../../x\"}")?
            .json()?;
        (j["voice"] == "jarvis")
            .then(|| "unknown voice ignored".to_string())
            .ok_or(format!("voice {}", j["voice"]))
    });
    s.check("focus-sanitised", || {
        let j = http::post_json(
            &a,
            "/desk",
            &[],
            "{\"act\":\"focus\",\"text\":\"rt-1;rm -rf /\"}",
        )?
        .json()?;
        (j["focus"] == "rt-1rm-rf")
            .then(|| "focus rt-1rm-rf".to_string())
            .ok_or(format!("focus {}", j["focus"]))
    });
    s.check("unknown-404", || {
        want_status(&http::get(&a, "/realtest-nope")?, 404).map(|_| "404".into())
    });
    s.log_tail = st.log_tail("galion-desk", 5);
    s
}

struct Ran {
    code: i32,
    out: String,
    err: String,
}

fn run(bin: &Path, args: &[&str], cwd: &Path, env: &[(&str, &str)]) -> Result<Ran, String> {
    let mut c = Command::new(bin);
    c.args(args)
        .current_dir(cwd)
        .env_clear()
        .env("TZ", "Europe/Zagreb")
        .env("PATH", "/usr/bin:/bin");
    for (k, v) in env {
        c.env(k, v);
    }
    let o = c
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("{}: {e}", bin.display()))?;
    Ok(Ran {
        code: o.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&o.stdout).into_owned(),
        err: String::from_utf8_lossy(&o.stderr).into_owned(),
    })
}

/// persona-plex-relay is a scaffold CLI at 13a296c: its contract is "print a
/// plan, open no socket, never print a secret".
pub fn voice_relay(st: &Stack) -> Suite {
    let mut s = Suite::new("voice-relay-contract", "persona-plex-relay");
    let bin = st.bins.path("persona-plex-relay");
    let dir = st.path("relay");
    s.check("no-config-prints-one-zagreb-stamp", || {
        let r = run(&bin, &[], &dir, &[])?;
        let line = r.out.trim();
        if r.code != 0 || r.out.lines().count() != 1 {
            return Err(format!("exit {} stdout {:?}", r.code, r.out));
        }
        zagreb_offset(line).map(|_| line.to_string())
    });
    let token = format!("rt-sentinel-{}", letters(12));
    let key = format!("rt-sentinel-{}", letters(12));
    let cfg = RELAY_EXAMPLE
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("role_prompt_file") {
                format!(
                    "role_prompt_file = \"{}\"",
                    dir.join("absent-prompt.md").display()
                )
            } else if l.trim_start().starts_with("model_path") {
                format!(
                    "model_path = \"{}\"",
                    dir.join("absent-model.onnx").display()
                )
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let cfg_path = dir.join("relay.toml");
    s.check("plan-never-prints-secrets", || {
        fs::write(&cfg_path, &cfg).map_err(|e| e.to_string())?;
        let p = cfg_path.to_string_lossy().into_owned();
        let r = run(
            &bin,
            &["--config", &p],
            &dir,
            &[
                ("PERSONA_PLEX_RELAY_TOKEN", &token),
                ("PERSONA_PLEX_VOICEPRINT_KEY", &key),
            ],
        )?;
        if r.code != 0 {
            return Err(format!("exit {} stderr {}", r.code, short(&r.err)));
        }
        if r.out.contains(&token)
            || r.out.contains(&key)
            || r.err.contains(&token)
            || r.err.contains(&key)
        {
            return Err("a secret value appeared in the output".into());
        }
        Ok(format!(
            "exit 0, {} plan lines, no secret in output",
            r.out.lines().count()
        ))
    });
    s.check("plan-has-zagreb-time", || {
        let p = cfg_path.to_string_lossy().into_owned();
        let r = run(&bin, &["--config", &p], &dir, &[])?;
        let has = r.out.contains("+02:00") || r.out.contains("+01:00");
        has.then(|| "plan stamped with a Zagreb offset".to_string())
            .ok_or(short(&r.out))
    });
    s.check("unknown-flag-exit-2", || {
        let r = run(&bin, &["--realtest-bogus"], &dir, &[])?;
        (r.code == 2)
            .then(|| "exit 2".to_string())
            .ok_or(format!("exit {}", r.code))
    });
    s.check("missing-explicit-config-exit-1", || {
        let r = run(&bin, &["--config", "/nonexistent/realtest.toml"], &dir, &[])?;
        (r.code == 1)
            .then(|| "exit 1".to_string())
            .ok_or(format!("exit {}", r.code))
    });
    s
}

/// guardian-tick sees a killed service, the stack's supervisor heals it, and
/// guardian-tick goes green again.
pub fn guardian_heal(st: &Stack) -> Suite {
    let mut s = Suite::new("guardian-tick-heal", "guardian-tick");
    let bin = st.bins.path("guardian-tick");
    let g = st.path("guardian");
    let probes = g.join("probes.toml");
    let body = format!(
        "[[probe]]\nkind = \"http\"\nname = \"tuner\"\nport = {}\npath = \"/health\"\n[[probe]]\nkind = \"http\"\nname = \"galion-desk\"\nport = {}\npath = \"/health\"\n[[probe]]\nkind = \"http\"\nname = \"openmind-memory\"\nport = {}\npath = \"/health\"\n[[probe]]\nkind = \"http\"\nname = \"galion-one\"\nport = {}\npath = \"/health\"\n",
        st.base + off::TUNER,
        st.base + off::DESK,
        st.base + off::MEMORY,
        st.base + off::ONE_STORE
    );
    let tick = |out: &str| -> Result<(Ran, Value), String> {
        let o = g.join(out).to_string_lossy().into_owned();
        let p = probes.to_string_lossy().into_owned();
        let j = g.join("events.jsonl").to_string_lossy().into_owned();
        let r = run(
            &bin,
            &["--probes", &p, "--out", &o, "--suggest", "--jsonl", &j],
            &g,
            &[],
        )?;
        if r.code != 0 {
            return Err(format!("exit {} {}", r.code, short(&r.err)));
        }
        let v: Value = serde_json::from_str(&fs::read_to_string(&o).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        Ok((r, v))
    };
    s.check("all-green-before", || {
        fs::write(&probes, &body).map_err(|e| e.to_string())?;
        let (r, v) = tick("before.json")?;
        zagreb_offset(v["ts"].as_str().unwrap_or(""))?;
        (v["host_ok"] == true)
            .then(|| r.out.trim().to_string())
            .ok_or(r.out)
    });
    let old = st.pid("tuner");
    let mut killed_at = None;
    s.check("kill-tuner", || {
        let pid = old.ok_or("no tuner pid from the supervisor")?;
        if !signal_pid("KILL", pid) {
            return Err(format!("kill -KILL {pid} failed"));
        }
        killed_at = Some(Instant::now());
        Ok(format!("SIGKILL sent to tuner pid {pid} (our own child)"))
    });
    s.check("detects-killed-tuner", || {
        std::thread::sleep(Duration::from_millis(200));
        let (r, v) = tick("during.json")?;
        let tuner_down = v["results"]
            .as_array()
            .is_some_and(|a| a.iter().any(|x| x["name"] == "tuner" && x["ok"] == false));
        let suggests = r
            .out
            .contains("suggest: tuner down: operator may restart tuner");
        (v["host_ok"] == false && tuner_down && suggests)
            .then(|| r.out.trim().replace('\n', " / "))
            .ok_or(r.out)
    });
    s.check("supervisor-heals-tuner", || {
        let a = st.addr(off::TUNER);
        let (_, _) = eventually(Duration::from_secs(15), || {
            want_status(&http::get(&a, "/health")?, 200)?;
            Ok(())
        })?;
        let heal = killed_at.map(|t| t.elapsed().as_millis()).unwrap_or(0);
        let new = st.pid("tuner");
        if new.is_none() || new == old {
            return Err(format!("pid did not change ({old:?} -> {new:?})"));
        }
        Ok(format!("healthy again {heal} ms after the kill, pid {old:?} -> {new:?} (galion-boot supervise, restart_sec 3)"))
    });
    s.check("all-green-after", || {
        let (r, v) = tick("after.json")?;
        (v["host_ok"] == true)
            .then(|| r.out.trim().to_string())
            .ok_or(r.out)
    });
    s.check("jsonl-zagreb-stamps", || {
        let t = fs::read_to_string(g.join("events.jsonl")).map_err(|e| e.to_string())?;
        let mut n = 0;
        for l in t.lines() {
            let v: Value = serde_json::from_str(l).map_err(|e| e.to_string())?;
            zagreb_offset(v["ts"].as_str().unwrap_or(""))?;
            n += 1;
        }
        (n == 12)
            .then(|| format!("{n} probe lines, all +02:00/+01:00"))
            .ok_or(format!("{n} lines"))
    });
    s.log_tail = crate::stack::tail_file(&st.path("logs/supervisor.log"), 8);
    s
}

/// The healer channel: a restart request file makes the supervisor restart
/// exactly that service.
pub fn heal_request(st: &Stack) -> Suite {
    let mut s = Suite::new("heal-request", "galion-boot");
    let old = st.pid("galion-desk");
    s.check("restart-request-honoured", || {
        let dir = st.path("sup/restart");
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        fs::write(dir.join("galion-desk"), "realtest\n").map_err(|e| e.to_string())?;
        let (new, ms) = eventually(Duration::from_secs(8), || {
            let new = st.pid("galion-desk");
            (new.is_some() && new != old)
                .then_some(new)
                .ok_or("pid unchanged".to_string())
        })?;
        Ok(format!("pid {old:?} -> {new:?} in {ms} ms"))
    });
    s.check("healthy-after-restart", || {
        let a = st.addr(off::DESK);
        let (_, ms) = eventually(Duration::from_secs(8), || {
            want_status(&http::get(&a, "/health")?, 200)
        })?;
        Ok(format!("200 after {ms} ms"))
    });
    s.check("other-services-untouched", || {
        let one = st.pid("one-store").ok_or("no one-store pid")?;
        let alive = Path::new(&format!("/proc/{one}")).exists();
        alive
            .then(|| format!("one-store pid {one} still alive"))
            .ok_or("one-store restarted too".into())
    });
    s
}

/// Run every box scenario in order.
pub fn all(st: &mut Stack) -> Vec<Suite> {
    vec![
        boot(st),
        one_pairing(st),
        one_fail_closed(
            st,
            "one-fail-closed-missing-store",
            "one-nostore",
            off::ONE_NOSTORE,
            "device store file absent",
        ),
        one_fail_closed(
            st,
            "one-fail-closed-empty-store",
            "one-empty",
            off::ONE_EMPTY,
            "device store is {}",
        ),
        memory(st),
        nerve(st),
        tuner(st),
        link_watch(st),
        desk(st),
        voice_relay(st),
        guardian_heal(st),
        heal_request(st),
    ]
}
