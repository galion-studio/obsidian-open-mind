// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Port of `scripts/real/room.sh` (deleted). Release `room` on the box:
//! session 1 on the host loopback with hostile reads, session 2 inside a
//! private user+net namespace (invisible from the host), session 3 the
//! no-flag page under `strace` (zero network syscalls) plus a `--listen`
//! positive control, then a leftover-process check. Never touches 4020,
//! the VPS or any live service. Ports 19840 (host) and 19841 (netns).
//!
//!   GALION_REAL=1 cargo test --release -p galion-realtest --test real_sh_room -- --nocapture
//!
//! The crate's own `room/tests/{real_room,no_bind}.rs` stay the unit-level
//! versions; this test runs the exact release binary end to end.

mod sh3;

use sh3::*;
use std::process::{Command, Stdio};
use std::time::Duration;

const HOST: u16 = PORT_BASE;
const NS: u16 = PORT_BASE + 1;

fn fixture() -> Scratch {
    let s = Scratch::new("room-real");
    s.put(
        "root/galion-light/room/index.html",
        "<!doctype html><title>room</title>\n",
    );
    s.put("root/galion-light/docs/guide.md", "# Operator guide\n");
    s.put("root/galion-light/docs/secret-keys.md", "# hidden\n");
    s.put(
        "root/galion-src/galion-app/package.json",
        "{\"name\": \"galion-app\"}\n",
    );
    s.put("root/outside/private.md", "# private\n");
    std::os::unix::fs::symlink(
        s.path("root/outside/private.md"),
        s.path("root/galion-src/escape.md"),
    )
    .unwrap();
    s
}

fn wait_listening(log: &std::path::Path) -> String {
    let mut line = String::new();
    wait_until(Duration::from_secs(10), || {
        line = std::fs::read_to_string(log).unwrap_or_default();
        line.contains("listening on")
    });
    line.replace('\n', " ")
}

fn enc(p: &str) -> String {
    p.bytes()
        .map(|b| match b {
            b'/' | b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'.' | b'_' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[test]
fn real_sh_room() {
    if !enabled("real_sh_room") {
        return;
    }
    let bin = need(&release_bins(None, &["-p", "room"], &[]).join("room"));
    let mut ck = Checks::default();
    let userns = Command::new("unshare")
        .args(["--user", "--map-root-user", "--net", "--", "true"])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    say(format!(
        "tools: unshare={} strace={} nsenter={} curl={}; user+net namespaces: {}",
        have("unshare"),
        have("strace"),
        have("nsenter"),
        have("curl"),
        if userns { "ok" } else { "refused" }
    ));
    let (machine, interp, _) = elf_facts(&bin);
    say(format!(
        "binary {}: {} bytes, {machine}, ELF interpreter: {}",
        bin.display(),
        std::fs::metadata(&bin).unwrap().len(),
        if interp {
            "yes (dynamic)"
        } else {
            "none (static)"
        }
    ));
    require_free(&[HOST, NS]);
    let fx = fixture();
    let root = fx.path("root");
    let rs = root.to_string_lossy().to_string();
    let mut procs = Procs::default();

    // Session 1: host loopback, hostile requests.
    say(format!("session 1: host loopback 127.0.0.1:{HOST}"));
    let log1 = fx.path("s1.err");
    let pid1 = procs.spawn(
        "room-host",
        Command::new(&bin)
            .arg("--listen")
            .env("ROOM_BIND", format!("127.0.0.1:{HOST}"))
            .env("ROOM_ROOT", &root)
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log1).unwrap()),
    );
    say(format!("  stderr: {}", wait_listening(&log1)));
    let h = get(HOST, "/health");
    ck.ok("health", status_of(&h) == 200 && body_of(&h) == "ok\n", "");
    let objs = body_of(&get(HOST, "/api/objects"));
    ck.ok(
        "objects listed, secret hidden",
        objs.contains("\"kind\":\"") && !objs.contains("secret-keys"),
        &format!(
            "{} bytes, {} objects",
            objs.len(),
            objs.matches("\"kind\":\"").count()
        ),
    );
    let guide = get(
        HOST,
        &format!(
            "/api/read?path={}",
            enc(&format!("{rs}/galion-light/docs/guide.md"))
        ),
    );
    ck.ok(
        "read guide",
        status_of(&guide) == 200 && body_of(&guide).starts_with("# Operator guide"),
        "",
    );
    for (what, p) in [
        ("secret", format!("{rs}/galion-light/docs/secret-keys.md")),
        ("symlink escape", format!("{rs}/galion-src/escape.md")),
        ("/etc/passwd", "/etc/passwd".to_string()),
    ] {
        let r = get(HOST, &format!("/api/read?path={}", enc(&p)));
        ck.ok(
            &format!("read {what} held"),
            status_of(&r) == 404 && body_of(&r) == "held\n",
            &format!("{}", status_of(&r)),
        );
    }
    let bad = get(HOST, "/api/read?path=%a%C3%A9%");
    ck.ok(
        "bad escape answered (old decoder panicked)",
        status_of(&bad) == 404,
        &format!("{}", status_of(&bad)),
    );
    let post = raw(
        HOST,
        b"POST /api/objects HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        Duration::from_secs(5),
    );
    ck.ok(
        "POST refused",
        status_of(&post) == 405,
        &format!("{}", status_of(&post)),
    );
    ck.ok(
        "health after hostile requests",
        status_of(&get(HOST, "/health")) == 200,
        "",
    );
    procs.stop("room-host");
    ck.ok(
        "session 1 teardown",
        !alive(pid1) && !port_open(HOST),
        &format!("pid {pid1} gone, :{HOST} closed"),
    );

    // Session 2: private network namespace. A holder process owns the
    // namespace; `nsenter` brings lo up and starts room inside it (no shell).
    if userns {
        say(format!(
            "session 2: private network namespace on 127.0.0.1:{NS}"
        ));
        let holder = procs.spawn(
            "netns-holder",
            Command::new("unshare")
                .args(["--user", "--map-root-user", "--net", "--", "sleep", "120"])
                .stdout(Stdio::null()),
        );
        let ns_args = |pid: u32| -> Vec<String> {
            [
                "--target",
                &pid.to_string(),
                "--user",
                "--net",
                "--preserve-credentials",
                "--",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect()
        };
        // Wait until the holder is inside its own user namespace.
        let me = std::fs::read_link("/proc/self/ns/user").ok();
        wait_until(Duration::from_secs(5), || {
            std::fs::read_link(format!("/proc/{holder}/ns/user")).ok() != me
        });
        let up = run(Command::new("nsenter")
            .args(ns_args(holder))
            .args(["ip", "link", "set", "lo", "up"]));
        ck.ok("lo up inside the namespace", up.status.success(), &err(&up));
        let log2 = fx.path("s2.err");
        let pid2 = procs.spawn(
            "room-ns",
            Command::new("nsenter")
                .args(ns_args(holder))
                .arg(&bin)
                .arg("--listen")
                .env("ROOM_BIND", format!("127.0.0.1:{NS}"))
                .env("ROOM_ROOT", &root)
                .stdout(Stdio::null())
                .stderr(std::fs::File::create(&log2).unwrap()),
        );
        let line = wait_listening(&log2);
        say(format!("  stderr: {line}"));
        ck.ok("netns room listening", line.contains("listening on"), "");
        ck.ok(
            "host cannot reach the netns listener",
            !port_open(NS),
            &format!(":{NS} refused on the host"),
        );
        if have("curl") {
            let inner = run(Command::new("nsenter").args(ns_args(holder)).args([
                "curl",
                "-s",
                "-m",
                "3",
                &format!("http://127.0.0.1:{NS}/health"),
            ]));
            ck.ok(
                "curl inside the namespace answers ok",
                out(&inner) == "ok\n",
                &out(&inner).replace('\n', ""),
            );
        }
        let links = run(Command::new("nsenter")
            .args(ns_args(holder))
            .args(["cat", "/proc/net/dev"]));
        let names: Vec<String> = out(&links)
            .lines()
            .skip(2)
            .filter_map(|l| l.split(':').next().map(|n| n.trim().to_string()))
            .collect();
        ck.ok(
            "only lo inside the namespace",
            names == ["lo"],
            &format!("{names:?}"),
        );
        procs.stop("room-ns");
        procs.stop("netns-holder");
        ck.ok(
            "session 2 teardown",
            !alive(pid2) && !alive(holder),
            &format!("pids {pid2},{holder} gone"),
        );
    } else {
        say("session 2: SKIP (unprivileged user+net namespaces unavailable)");
    }

    // Session 3: no-flag page under strace in an empty netns.
    if userns && have("strace") {
        say("session 3: no-flag page under strace in an empty netns");
        let trace = fx.path("trace");
        let o = run(Command::new("unshare")
            .args([
                "--user",
                "--map-root-user",
                "--net",
                "--",
                "strace",
                "-f",
                "-e",
                "trace=network",
                "-o",
            ])
            .arg(&trace)
            .arg(&bin)
            .env("ROOM_ROOT", &root));
        let t = std::fs::read_to_string(&trace).unwrap_or_default();
        let net = t
            .lines()
            .filter(|l| {
                ["socket(", "bind(", "listen(", "connect(", "accept"]
                    .iter()
                    .any(|k| l.contains(k))
            })
            .count();
        ck.ok(
            "page printed with zero network syscalls",
            o.status.success() && out(&o).len() > 10 && net == 0,
            &format!("page {} bytes, network syscalls {net}", out(&o).len()),
        );
        let trace2 = fx.path("trace2");
        let _ = run(Command::new("unshare")
            .args([
                "--user",
                "--map-root-user",
                "--net",
                "--",
                "strace",
                "-f",
                "-e",
                "trace=network",
                "-o",
            ])
            .arg(&trace2)
            .args(["timeout", "-s", "TERM", "1"])
            .arg(&bin)
            .arg("--listen")
            .env("ROOM_BIND", "127.0.0.1:0")
            .env("ROOM_ROOT", &root)
            .stderr(Stdio::null()));
        let t2 = std::fs::read_to_string(&trace2).unwrap_or_default();
        let seen: Vec<&str> = ["socket(", "bind(", "listen("]
            .into_iter()
            .filter(|k| t2.contains(k))
            .collect();
        ck.ok(
            "control: --listen shows socket/bind/listen",
            seen.len() == 3,
            &format!("{seen:?}"),
        );
    } else {
        say("session 3: SKIP (needs strace and user namespaces)");
    }

    let left = pids_running(&bin);
    ck.ok(
        "no leftover room processes",
        left.is_empty(),
        &format!("{left:?}"),
    );
    ck.finish("real_sh_room");
}
