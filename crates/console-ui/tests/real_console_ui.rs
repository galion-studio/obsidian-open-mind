// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test of the Rust console pages against the live console API.
//! Replaces `scripts/real/console-ui-rust.sh` (Rust only, no shell).
//!
//! ```text
//! GALION_REAL=1 cargo test -p console-ui --release --test real_console_ui -- --ignored --nocapture
//! ```
//!
//! What it does, in order:
//! 1. `cargo build --release -p console-static` (same target dir);
//! 2. unless `CONSOLE_UI_UPSTREAM` already answers, opens a read-only SSH
//!    forward 127.0.0.1:19379 -> VPS 127.0.0.1:8080 (console API) and
//!    19378 -> 8091 (desk pulse); `ssh -N`, nothing runs or is written on
//!    the VPS;
//! 3. starts `galion-console-static --ui rust` on 127.0.0.1:19375;
//! 4. GETs every Rust page and every unported room through the front, reads
//!    the same live API payloads directly, and asserts each page shows
//!    exactly what the API says;
//! 5. with `CONSOLE_UI_SHOTS=DIR`, takes headless Chrome screenshots.
//!
//! Only GET requests are sent. It never POSTs, never opens
//! `/api/v1/markers/stream` (its handler bumps a live counter) and never
//! calls `/api/v1/ping` (it probes the public hosts).
//!
//! env: `CONSOLE_UI_PORT` (19375), `CONSOLE_UI_UPSTREAM` (127.0.0.1:19379),
//! `CONSOLE_UI_PULSE` (127.0.0.1:19378), `CONSOLE_UI_VPS` (ssh target `user@host`,
//! required unless the upstream already answers; no address is kept in the
//! source), `CONSOLE_UI_SSH_KEY`
//! (~/.ssh/id_ed25519_vps), `CONSOLE_UI_SHOTS` (screenshot dir).

use console_ui::client::{self, Forward};
use console_ui::html::esc;
use console_ui::json::{self, Value};
use console_ui::{pages, rooms, PORTED};
use galion_realtest::console_routes as routes;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

fn env_or(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_string())
}

fn addr(name: &str, default: &str) -> SocketAddr {
    env_or(name, default).parse().expect(name)
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"))
}

fn now() -> String {
    // Box clock is Europe/Zagreb; `date` prints the offset.
    Command::new("date")
        .arg("-Iseconds")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

fn get(a: SocketAddr, path: &str, host: &str) -> Option<(u16, Vec<u8>, Duration)> {
    let fwd = Forward {
        host: host.into(),
        ..Forward::default()
    };
    let t = Instant::now();
    let r = client::call(a, "GET", path, &fwd, None).ok()?;
    Some((r.status, r.body, t.elapsed()))
}

fn page(front: SocketAddr, path: &str, host: &str) -> (u16, String) {
    let (c, b, _) = get(front, path, host).unwrap_or_else(|| panic!("front GET {path}"));
    (c, String::from_utf8_lossy(&b).into_owned())
}

/// Live JSON read; `None` when `console_routes::decide` says the Node-era
/// route is gone on live (option A) or retired (option B).
fn api(up: SocketAddr, path: &str) -> Option<Value> {
    assert!(
        !path.contains("markers/stream") && !path.contains("/ping"),
        "{path}"
    );
    let (c, b, _) = get(up, path, "127.0.0.1").unwrap_or_else(|| panic!("live GET {path}"));
    if routes::decide(path, c) == routes::Live::Gone {
        return None;
    }
    assert_eq!(c, 200, "live {path}");
    Some(json::parse(&b).unwrap_or_else(|e| panic!("live {path}: {e:?}")))
}

/// A room whose live read is gone must say so, not show data.
fn gone_room(html: &str, p: &str) {
    assert!(
        routes::page_says_unavailable(html),
        "{p}: read gone on live but the room does not say so"
    );
    eprintln!("real: {p}: live read gone -> room states the miss");
}

fn answers(a: SocketAddr) -> bool {
    matches!(get(a, "/api/v1/health", "127.0.0.1"), Some((200, _, _)))
}

fn wait_for(what: &str, secs: u64, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(secs), "{what} timed out");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).as_str().unwrap_or("")
}

fn has(html: &str, text: &str, what: &str) {
    assert!(html.contains(&esc(text)), "{what}: missing {text:?}");
}

fn rss_kb(pid: u32) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .ok()
        .and_then(|t| {
            t.lines()
                .find(|l| l.starts_with("VmRSS"))
                .map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string())
        })
        .unwrap_or_else(|| "?".into())
}

/// Headless Chrome screenshot, killed after 60 s.
fn shot(dir: &Path, name: &str, url: &str, extra: &[&str]) {
    let file = dir.join(format!("{name}.png"));
    let mut cmd = Command::new("google-chrome");
    cmd.args([
        "--headless=new",
        "--no-sandbox",
        "--disable-gpu",
        "--hide-scrollbars",
        "--window-size=412,915",
    ])
    .args(extra)
    .arg(format!("--screenshot={}", file.display()))
    .arg(url)
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else {
        eprintln!("real: no google-chrome, screenshots skipped");
        return;
    };
    let t = Instant::now();
    loop {
        if let Ok(Some(_)) = child.try_wait() {
            break;
        }
        if t.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let len = std::fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    eprintln!("real: shot {} {len} B", file.display());
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 19375/19378/19379"]
fn rust_pages_match_live_api() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1");
        return;
    }
    eprintln!("real: start {}", now());
    let port = env_or("CONSOLE_UI_PORT", "19375");
    let front: SocketAddr = format!("127.0.0.1:{port}").parse().unwrap();
    let up = addr("CONSOLE_UI_UPSTREAM", "127.0.0.1:19379");
    let pulse = addr("CONSOLE_UI_PULSE", "127.0.0.1:19378");

    // 1. Build the release front.
    let st = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(root())
        .env("CARGO_INCREMENTAL", "0")
        .args([
            "build",
            "--release",
            "-j",
            "3",
            "-q",
            "-p",
            "console-static",
        ])
        .status()
        .expect("cargo build");
    assert!(st.success(), "build failed");
    let bin = target_dir().join("release/galion-console-static");
    eprintln!(
        "real: binary bytes {}",
        std::fs::metadata(&bin).expect("binary").len()
    );

    // 2. Read-only forward unless one already answers (always for
    // CONSOLE_REAL_API=local, slot 395: a box-local console build answers
    // the API port, galion-one and hermes-rs stay live via the forward).
    let api_target = routes::api_target(bin.to_str());
    let hermes = addr("CONSOLE_UI_HERMES", "127.0.0.1:19377");
    let _tunnel = if api_target == routes::ApiTarget::Vps && answers(up) {
        eprintln!("real: upstream {up} already answers, no new forward");
        None
    } else {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        let key = env_or("CONSOLE_UI_SSH_KEY", &format!("{home}/.ssh/id_ed25519_vps"));
        let child = Command::new("ssh")
            .args(["-i", &key])
            .args(["-o", "IdentitiesOnly=yes", "-o", "BatchMode=yes"])
            .args([
                "-o",
                "ExitOnForwardFailure=yes",
                "-o",
                "ServerAliveInterval=15",
            ])
            .arg("-N")
            .args(routes::api_forward_args(&api_target, &up.to_string()))
            .args(routes::owner_forward_args(
                &api_target,
                None,
                Some(&hermes.to_string()),
            ))
            .arg("-L")
            .arg(format!("{pulse}:127.0.0.1:8091"))
            .arg(std::env::var("CONSOLE_UI_VPS").expect(
                "set CONSOLE_UI_VPS=user@host (the VPS) or start a forward on CONSOLE_UI_UPSTREAM",
            ))
            .stdin(Stdio::null())
            .spawn()
            .expect("ssh");
        let k = Kill(child, "ssh forward");
        let local = routes::start_local_api(
            &api_target,
            &up.to_string(),
            &pulse.to_string(),
            Some(&pulse.to_string()),
            Some(&hermes.to_string()),
            None,
        );
        wait_for("ssh forward", 20, || answers(up));
        eprintln!("real: forward {up} -> VPS 8080, {pulse} -> VPS 8091 (ssh -N)");
        Some((k, local))
    };

    // 3. The front.
    let log = std::env::temp_dir().join("console-ui-rust-real.log");
    let child = Command::new(&bin)
        .args(["--bind", &front.to_string(), "--ui", "rust"])
        .args(["--upstream", &up.to_string(), "--pulse", &pulse.to_string()])
        .stdout(Stdio::null())
        .stderr(std::fs::File::create(&log).unwrap())
        .spawn()
        .expect("front");
    let pid = child.id();
    let _front = Kill(child, "front");
    wait_for("front", 10, || TcpStream::connect(front).is_ok());
    let first = std::fs::read_to_string(&log).unwrap_or_default();
    eprintln!("{}", first.lines().next().unwrap_or(""));

    // 4a. Every Rust page: 200, no script, timing.
    let mut htmls = Vec::new();
    for p in PORTED {
        let (code, body, t) = get(front, p, "127.0.0.1").expect(p);
        let html = String::from_utf8_lossy(&body).into_owned();
        let scripts = html.matches("<script").count();
        eprintln!(
            "real: GET {p} -> {code} {} B {:.3} s, <script> tags: {scripts}",
            body.len(),
            t.as_secs_f64()
        );
        if p.starts_with("/md/") && code == 404 {
            // File rooms: this front gets no --user-md/--memory-root, so 404
            // "not configured" is the designed answer (live :8080 answers the
            // same, route map row `/md/user`).
            assert!(html.contains("not configured"), "{p}: 404 without the note");
        } else {
            assert_eq!(code, 200, "{p}");
        }
        assert_eq!(scripts, 0, "{p}");
        if html.contains("did not load") {
            // Allowed only while a board read is gone on live (option A) or
            // retired (B); `routes::decide` below checks each read.
            assert!(
                routes::mode() != routes::Mode::Node,
                "{p}: an API read failed"
            );
            eprintln!("real: {p}: states a read that did not load (Node-era route gone on live)");
        }
        assert!(!html.contains("Not in Rust yet"), "{p}");
        htmls.push((*p, html));
    }
    let html_of = |p: &str| htmls.iter().find(|(q, _)| *q == p).unwrap().1.clone();
    let css = html_of("/agents")
        .split("rel=\"stylesheet\" href=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap()
        .to_string();
    let (c, b, _) = get(front, &css, "127.0.0.1").unwrap();
    eprintln!("real: GET {css} -> {c} {} B", b.len());
    assert_eq!(c, 200);

    // 4b. Unported rooms.
    let not_ported: Vec<&str> = rooms::ROOMS
        .iter()
        .filter_map(|r| r.to)
        .filter(|p| !PORTED.contains(p))
        .collect();
    for p in &not_ported {
        let (c, h) = page(front, p, "127.0.0.1");
        assert_eq!(c, 200, "{p}");
        assert!(h.contains("Not in Rust yet"), "{p}");
    }
    eprintln!(
        "real: {} Rust pages, {} rooms answer \"Not in Rust yet\": {}",
        PORTED.len(),
        not_ported.len(),
        not_ported.join(" ")
    );

    // 4c. Live comparison, page by page (same minute, GET only). Slot 384
    // (D-375-1): each read goes through `console_routes::decide`; a read that
    // is gone on the live Rust console (option A) or retired (option B) is
    // skipped and its room must state the miss (checked in 4a).
    let health = api(up, "/api/v1/health").expect("live Rust console /api/v1/health");
    let services_v = api(up, "/api/v1/services");
    let domains = api(up, "/api/v1/domains");
    let pulses = pages::pulses_of(&api(pulse, "/api/v1/awake").unwrap_or(Value::Null));
    let version = s(&health, "version").to_string();

    if let (Some(services_v), Some(domains)) = (&services_v, &domains) {
        let services = pages::services_of(services_v);
        assert!(!services.is_empty());
        let h = html_of("/status");
        let c = pages::status::counts(&services, &pulses);
        let line = format!("{} subsystems online · {} held", c.online, c.held);
        assert!(h.contains(&line), "status counts {line}");
        let healthy = s(&health, "status") == "healthy";
        assert!(h.contains(if healthy {
            "Light stack is up."
        } else {
            "Origins are dark."
        }));
        for d in domains.get("items").as_array() {
            has(&h, s(d, "host"), "/status host");
        }
        eprintln!(
            "real: /status {line}, {} hosts, healthy={healthy}",
            domains.get("items").as_array().len()
        );

        let h = html_of("/services");
        let (upg, downg, extras) = pages::services::groups(&services, &pulses);
        let up_at = h.find("<h2>Up</h2>").unwrap();
        let down_at = h.find("<h2>Down</h2>").unwrap();
        for (group, inside) in [(&upg, true), (&downg, false)] {
            for sv in group.iter() {
                let at = h.find(&format!(">{}<", esc(&sv.name))).unwrap();
                assert_eq!(at > up_at && at < down_at, inside, "{} group", sv.id);
            }
        }
        eprintln!(
            "real: /services up {} down {} pulse-only {} (pulse rows {})",
            upg.len(),
            downg.len(),
            extras.len(),
            pulses.len()
        );

        let h = html_of("/domains");
        let mut n = 0;
        for d in domains.get("items").as_array() {
            let sid = s(d, "serviceId");
            let name = services
                .iter()
                .find(|x| x.id == sid)
                .map(|x| x.name.as_str())
                .filter(|n| !n.is_empty())
                .unwrap_or(s(d, "role"));
            let row = format!(
                "<span>{}</span><span class=\"muted\">{} · {}</span>",
                esc(s(d, "host")),
                esc(s(d, "brand")),
                esc(name)
            );
            assert!(h.contains(&row), "/domains row {row}");
            n += 1;
        }
        eprintln!(
            "real: /domains {n} rows = /api/v1/domains count {}",
            domains.get("count").as_f64().unwrap_or(-1.0)
        );
    } else {
        for p in ["/status", "/services", "/domains"] {
            assert!(
                routes::page_says_unavailable(&html_of(p)),
                "{p}: board reads gone but the room does not say so"
            );
        }
        eprintln!(
            "real: /status /services /domains: board reads gone on live -> rooms state the miss"
        );
    }

    let h = html_of("/agents");
    assert!(h.contains(&format!(">{}<", esc(&version))), "version");
    let pod = s(health.get("target"), "pod");
    if !pod.is_empty() {
        assert!(h.contains(&format!(">{}<", esc(pod))), "pod");
    }
    eprintln!(
        "real: /agents version {version} pod {:?} (Rust health has no target: {})",
        pod,
        health.get("target").is_null()
    );

    if let Some(repos) = api(up, "/api/v1/repos") {
        let h = html_of("/repos");
        let items = repos.get("items").as_array();
        for r in items {
            assert!(
                h.contains(&format!(
                    "<span class=\"mono\">{}</span>",
                    esc(s(r, "name"))
                )),
                "repo {}",
                s(r, "name")
            );
            has(&h, s(r, "role"), "repo role");
        }
        let summary = format!(
            "{} repositories indexed: {} public, {} private visible",
            items.len(),
            repos.get("public").as_f64().unwrap_or(0.0) as u64,
            repos.get("privateVisible").as_f64().unwrap_or(0.0) as u64
        );
        assert!(h.contains(&summary), "{summary}");
        eprintln!("real: /repos {summary}");
    } else {
        gone_room(&html_of("/repos"), "/repos");
    }

    if let Some(catalog) = api(up, "/api/v1/catalog") {
        let h = html_of("/docs");
        let all = catalog.get("items").as_array();
        let live = all.iter().filter(|e| s(e, "zone") == "live").count();
        let held = all.iter().filter(|e| s(e, "zone") == "held").count();
        for want in [
            format!("Ledger · {} routes", all.len()),
            format!("Live · {live}"),
            format!("Fridge · {held}"),
        ] {
            assert!(h.contains(&want), "/docs {want}");
        }
        for e in all {
            has(&h, s(e, "path"), "/docs path");
        }
        let (_, hq) = page(front, "/docs?q=voice", "127.0.0.1");
        let count = |zone: &str| {
            all.iter()
                .filter(|e| {
                    s(e, "zone") == zone
                        && ["path", "title", "source"]
                            .iter()
                            .any(|k| s(e, k).to_lowercase().contains("voice"))
                })
                .count()
        };
        let (fl, fh) = (count("live"), count("held"));
        assert!(
            hq.contains(&format!("Live · {fl}")) && hq.contains(&format!("Fridge · {fh}")),
            "/docs?q=voice"
        );
        eprintln!(
            "real: /docs {} routes (live {live}, fridge {held}; API says live {} held {}); ?q=voice -> {fl}+{fh}",
            all.len(),
            catalog.get("live").as_f64().unwrap_or(-1.0),
            catalog.get("held").as_f64().unwrap_or(-1.0)
        );
    } else {
        gone_room(&html_of("/docs"), "/docs");
    }

    if let Some(cf) = api(up, "/api/v1/cloudflare") {
        let h = html_of("/cloudflare");
        let steps = cf.get("steps").as_array();
        for st in steps {
            has(&h, st.as_str().unwrap_or(""), "cloudflare step");
        }
        let mut recs = 0;
        for z in cf.get("zones").as_array() {
            has(&h, s(z, "zone"), "zone");
            for r in z.get("records").as_array() {
                has(&h, s(r, "host"), "record");
                recs += 1;
            }
        }
        has(&h, s(cf.get("tunnel"), "cname"), "cname");
        eprintln!(
            "real: /cloudflare {} steps, {} zones, {recs} records, cname matches",
            steps.len(),
            cf.get("zones").as_array().len()
        );
    } else {
        gone_room(&html_of("/cloudflare"), "/cloudflare");
    }

    if let Some(pack) = api(up, "/api/v1/pack") {
        let h = html_of("/pack");
        has(&h, s(&pack, "compose"), "compose");
        has(&h, s(&pack, "caddy"), "caddy");
        eprintln!(
            "real: /pack compose {} B + Caddyfile {} B shown verbatim",
            s(&pack, "compose").len(),
            s(&pack, "caddy").len()
        );
    } else {
        gone_room(&html_of("/pack"), "/pack");
    }

    if let Some(ledger) = api(up, "/api/v1/ledger") {
        let h = html_of("/chill");
        let heavy: Vec<&Value> = ledger
            .get("services")
            .as_array()
            .iter()
            .filter(|x| s(x, "weight") == "heavy")
            .collect();
        let fridge = ledger.get("fridge").as_array();
        for x in heavy.iter().copied().chain(fridge.iter()) {
            has(&h, s(x, "name"), "chill name");
            has(&h, s(x, "summary"), "chill summary");
        }
        let asleep = h.matches("badge chill\">asleep").count();
        assert_eq!(asleep, heavy.len() + fridge.len());
        eprintln!(
            "real: /chill {} asleep = {} heavy + {} fridge (services payload says chill {})",
            asleep,
            heavy.len(),
            fridge.len(),
            services_v
                .as_ref()
                .and_then(|v| v.get("chill").as_f64())
                .unwrap_or(-1.0)
        );
    } else {
        gone_room(&html_of("/chill"), "/chill");
    }

    let h = html_of("/jupyter");
    let pod_id = s(health.get("target"), "podId");
    if pod_id.is_empty() {
        // Rust /api/v1/health carries no `target` (route map row): no link to
        // invent; the room must not show a broken `https://-8888` link.
        assert!(
            !h.contains("https://-8888"),
            "/jupyter: link without a pod id"
        );
        eprintln!("real: /jupyter: live health has no target.podId (Rust console) -> no pod link");
    } else {
        let href = format!("https://{pod_id}-8888.proxy.runpod.net/lab");
        assert!(h.contains(&href), "{href}");
        eprintln!("real: /jupyter {href}");
    }
    assert!(html_of("/data").contains("no database server"));
    assert!(html_of("/soon").contains("Hibernating"));

    // 4d. The gate on remote hosts, against the live admin check. While
    // `/api/v1/admin` is gone on the live Rust console the console host
    // fails closed (503, no data) instead of asking to pair.
    let (adc, _, _) = get(up, "/api/v1/admin", "127.0.0.1").expect("live admin");
    let admin_gone = routes::decide("/api/v1/admin", adc) == routes::Live::Gone;
    let not = version.as_str();
    for (host, want) in [
        ("console.galion.app", "This phone"),
        ("galion.app", "Soon online."),
        ("developer.galion.app", "Discussions"),
    ] {
        for p in ["/agents", "/repos", "/pack"] {
            let (c, h) = page(front, p, host);
            if host == "console.galion.app" {
                assert!(routes::gate_ok(c, &h, admin_gone), "{host}{p} gate {c}");
            } else {
                assert_eq!(c, 200, "{host}{p}");
                assert!(h.contains(want), "{host}{p}");
            }
            assert!(
                !h.contains(&format!(">{not}<")),
                "{host}{p} leaked the version"
            );
            assert!(!h.contains("reverse_proxy"), "{host}{p} leaked the pack");
        }
        eprintln!(
            "real: Host {host} /agents /repos /pack -> \"{}\", no page data",
            if host == "console.galion.app" && admin_gone {
                routes::GATE_CLOSED
            } else {
                want
            }
        );
    }
    eprintln!("real: front RSS {} kB", rss_kb(pid));

    // 5. Screenshots.
    if let Ok(dir) = std::env::var("CONSOLE_UI_SHOTS") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        let base = format!("http://127.0.0.1:{port}");
        let only: Vec<String> = std::env::var("CONSOLE_UI_SHOTS_ONLY")
            .map(|v| v.split(',').map(str::to_string).collect())
            .unwrap_or_default();
        for p in PORTED {
            let name = p.trim_start_matches('/');
            if !only.is_empty() && !only.iter().any(|o| o == name) {
                continue;
            }
            shot(
                &dir,
                &format!("after-{name}-dark"),
                &format!("{base}{p}"),
                &[],
            );
            shot(
                &dir,
                &format!("after-{name}-light"),
                &format!("{base}/_ui/theme?to=light&back={p}"),
                &[],
            );
        }
        shot(
            &dir,
            "after-not-ported-hermes",
            &format!("{base}/hermes"),
            &[],
        );
        // .test, not .app: .app is HSTS-preloaded and Chrome would go https.
        shot(
            &dir,
            "after-pair-console-host",
            &format!("http://console.galion.test:{port}/repos"),
            &["--host-resolver-rules=MAP console.galion.test 127.0.0.1"],
        );
    }
    eprintln!("real: end {}", now());
}
