// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
// DESK-ID: openmind.UCPU-1

//! Port of `scripts/real/ucpu.sh` (deleted). Universal-CPU real test: the
//! key services built as static aarch64 and armv7 musl release binaries run
//! under qemu-user on this x86_64 box (explicitly through qemu, no binfmt) and
//! answer real HTTP. Loopback only, temp data dirs, hermes-gateway `--mock`
//! (no provider, no network, no spend), galion-one with `ONE_NO_WARM=1`.
//!
//! Ports (slot 114), one arch at a time: 19845 galion-one, 19846 its mouth,
//! 19847 tuner, 19848 openmind-memory, 19849 hermes-gateway. Override the
//! first port with `REAL_UCPU_PORT_BASE` (slot 354; the 5 ports base..base+4).
//!
//! Build: bundled SQLite C code is compiled with `zig cc` through the Rust
//! wrapper `galion-ccwrap` (this crate's bin; replaces the shell wrappers).
//! Needs `ZIG=/path/to/zig` (or zig on PATH), a qemu-user emulator
//! (`qemu-aarch64-static` or `qemu-aarch64`, and the arm pair), rustup
//! targets aarch64-unknown-linux-musl and armv7-unknown-linux-musleabihf.
//! Ubuntu 26.04 dropped the `-static` names; the dynamic `qemu-user`
//! binaries run the same static musl ELFs. With `GALION_REAL_BIN_DIR` the
//! prebuilt `<dir>/../<triple>/release` binaries are used instead.
//!
//!   GALION_REAL=1 ZIG=... cargo test -p galion-realtest --test real_sh_ucpu -- --nocapture

mod sh3;

use galion_realtest::{http, ports};
use sh3::*;
use std::process::{Command, Stdio};
use std::time::Duration;

/// Default first port (slot 114 range); `REAL_UCPU_PORT_BASE` overrides it.
const DEFAULT_BASE: u16 = PORT_BASE + 5;
const BASE_ENV: &str = "REAL_UCPU_PORT_BASE";
const PKGS: [&str; 4] = ["galion-one", "tuner", "openmind-memory", "hermes-gateway"];
const ARCHES: [(&str, &str, &str, &str); 2] = [
    (
        "aarch64-unknown-linux-musl",
        "qemu-aarch64-static",
        "qemu-aarch64",
        "AArch64",
    ),
    (
        "armv7-unknown-linux-musleabihf",
        "qemu-arm-static",
        "qemu-arm",
        "ARM",
    ),
];
/// Not a secret: a throwaway key for the local mock gateway only.
const MOCK_KEY: &str = "ucpu-real-test-key-not-secret";

fn build_env(triple: &str) -> Vec<(String, String)> {
    let v = triple.replace('-', "_");
    let wrap = env!("CARGO_BIN_EXE_galion-ccwrap").to_string();
    let zig = std::env::var("ZIG").unwrap_or_else(|_| "zig".into());
    let cache = target_dir()
        .join("real-bins/zig-cache")
        .display()
        .to_string();
    vec![
        (format!("CC_{v}"), wrap.clone()),
        (format!("AR_{v}"), format!("{wrap} ar")),
        (
            format!("CARGO_TARGET_{}_LINKER", v.to_uppercase()),
            "rust-lld".into(),
        ),
        ("GALION_ZIG".into(), zig),
        ("ZIG_GLOBAL_CACHE_DIR".into(), cache.clone()),
        ("ZIG_LOCAL_CACHE_DIR".into(), cache),
    ]
}

#[allow(clippy::too_many_arguments)] // one line per probe mirrors the old script's table
fn probe(
    ck: &mut Checks,
    name: &str,
    port: u16,
    method: &str,
    path: &str,
    hdrs: &[(&str, &str)],
    body: &str,
    want: &str,
) {
    let r = http::request(
        &format!("127.0.0.1:{port}"),
        method,
        path,
        hdrs,
        body.as_bytes(),
        Duration::from_secs(20),
    );
    let (code, text) = match &r {
        Ok(r) => (r.status, r.text()),
        Err(e) => (0, e.clone()),
    };
    let short: String = text.replace('\n', " ").chars().take(160).collect();
    println!("    {method} :{port}{path} -> HTTP {code} {short}");
    ck.ok(
        name,
        code != 0 && text.contains(want),
        &format!("(wanted {want:?})"),
    );
}

#[test]
fn real_sh_ucpu() {
    if !enabled("real_sh_ucpu") {
        return;
    }
    let base = ports::base_from_env(BASE_ENV, DEFAULT_BASE, 5);
    let mut ck = Checks::default();
    for (triple, qemu_static, qemu_dyn, machine) in ARCHES {
        let qemu = if have(qemu_static) {
            qemu_static
        } else if have(qemu_dyn) {
            qemu_dyn
        } else {
            panic!(
                "missing {qemu_static} and {qemu_dyn} (apt: qemu-user, or qemu-user-static on older Ubuntu)"
            );
        };
        let mut args = Vec::new();
        for p in PKGS {
            args.extend(["-p", p]);
        }
        let bin = release_bins(Some(triple), &args, &build_env(triple));
        let ports: Vec<u16> = (base..base + 5).collect();
        say(format!(
            "=== {triple} under {qemu} (ports {}-{})",
            base,
            base + 4
        ));
        for p in PKGS {
            let f = need(&bin.join(p));
            let (m, interp, dynamic) = elf_facts(&f);
            println!(
                "    {p:<16} {:>8} bytes  machine={m}  interp={interp} dynamic={dynamic}",
                std::fs::metadata(&f).unwrap().len()
            );
            ck.ok(
                &format!("{triple} {p} static {machine}"),
                m == machine && !interp,
                "",
            );
        }
        require_free(&ports);
        let d = Scratch::new(&format!("ucpu-real-{triple}"));
        d.put(
            "www/index.html",
            "<!doctype html><title>tuner ucpu</title>\n",
        );
        for sub in ["one", "mem", "hermes"] {
            std::fs::create_dir_all(d.path(sub)).unwrap();
        }
        let log = |n: &str| std::fs::File::create(d.path(&format!("{n}.log"))).unwrap();
        let mut procs = Procs::default();
        let q = |b: &str| {
            let mut c = Command::new(qemu);
            c.arg(bin.join(b)).stdout(Stdio::null());
            c
        };
        procs.spawn(
            "galion-one",
            q("galion-one")
                .env("GALION_HOME", d.path("one"))
                .env("ONE_BIND", format!("127.0.0.1:{base}"))
                .env("ONE_MOUTH_BIND", format!("127.0.0.1:{}", base + 1))
                .env("ONE_NO_WARM", "1")
                .stderr(log("one")),
        );
        procs.spawn(
            "tuner",
            q("tuner")
                .env("TUNER_ADDR", format!("127.0.0.1:{}", base + 2))
                .env("TUNER_ROOT", d.path("www"))
                .stderr(log("tuner")),
        );
        procs.spawn(
            "openmind-memory",
            q("openmind-memory")
                .env("OPENMIND_MEMORY_HOST", "127.0.0.1")
                .env("OPENMIND_MEMORY_PORT", (base + 3).to_string())
                .env("OPENMIND_MEMORY_DATA_DIR", d.path("mem"))
                .stderr(log("mem")),
        );
        let h = d.path("hermes");
        procs.spawn(
            "hermes-gateway",
            q("hermes-gateway")
                .arg("--mock")
                .env("HERMES_HOME", &h)
                .env("HERMES_RS_CONFIG", h.join("none.yaml"))
                .env("HERMES_RS_HOST", "127.0.0.1")
                .env("HERMES_RS_PORT", (base + 4).to_string())
                .env("HERMES_RS_STATE_DB", h.join("state.db"))
                .env("HERMES_RS_SPEND_FILE", h.join("spend.json"))
                .env("HERMES_RS_CRON", "0")
                .env("HERMES_RS_MCP", "off")
                .env("API_SERVER_KEY", MOCK_KEY)
                .stderr(log("hermes")),
        );
        for p in [base, base + 2, base + 3, base + 4] {
            // emulated start-up is slower
            ck.ok(
                &format!("{triple}: listening on {p}"),
                wait_port(p, Duration::from_secs(60)),
                "",
            );
        }
        let one = base;
        probe(
            &mut ck,
            &format!("{triple} galion-one /health"),
            one,
            "GET",
            "/health",
            &[],
            "",
            "ok",
        );
        probe(
            &mut ck,
            &format!("{triple} galion-one /me"),
            one,
            "GET",
            "/me",
            &[],
            "",
            "{",
        );
        probe(
            &mut ck,
            &format!("{triple} tuner /health"),
            base + 2,
            "GET",
            "/health",
            &[],
            "",
            "\"ok\":true",
        );
        probe(
            &mut ck,
            &format!("{triple} tuner /"),
            base + 2,
            "GET",
            "/",
            &[],
            "",
            "tuner ucpu",
        );
        probe(
            &mut ck,
            &format!("{triple} memory /health"),
            base + 3,
            "GET",
            "/health",
            &[],
            "",
            "status",
        );
        let ct = [("Content-Type", "application/json")];
        probe(
            &mut ck,
            &format!("{triple} memory /capture (SQLite write)"),
            base + 3,
            "POST",
            "/capture",
            &ct,
            &format!("{{\"session_key\":\"ucpu-{triple}\",\"user_content\":\"ucpu emulated write {triple}\",\"assistant_content\":\"stored under qemu\"}}"),
            "",
        );
        probe(
            &mut ck,
            &format!("{triple} memory /search/conversations (SQLite read)"),
            base + 3,
            "POST",
            "/search/conversations",
            &ct,
            &format!("{{\"query\":\"emulated\",\"session_key\":\"ucpu-{triple}\"}}"),
            "emulated",
        );
        let bearer = format!("Bearer {MOCK_KEY}");
        probe(
            &mut ck,
            &format!("{triple} hermes-gateway /health"),
            base + 4,
            "GET",
            "/health",
            &[],
            "",
            "ok",
        );
        probe(
            &mut ck,
            &format!("{triple} hermes-gateway /v1/models"),
            base + 4,
            "GET",
            "/v1/models",
            &[("Authorization", &bearer)],
            "",
            "\"data\"",
        );
        probe(
            &mut ck,
            &format!("{triple} hermes-gateway /v1/chat/completions (mock, SQLite session)"),
            base + 4,
            "POST",
            "/v1/chat/completions",
            &[("Authorization", &bearer), ("Content-Type", "application/json")],
            &format!("{{\"model\":\"mock\",\"messages\":[{{\"role\":\"user\",\"content\":\"hello from {triple}\"}}]}}"),
            "\"choices\"",
        );
        for (name, c) in &procs.0 {
            println!(
                "    pid {} {name} VmRSS {}",
                c.id(),
                proc_field(c.id(), "VmRSS").unwrap_or_default()
            );
        }
        procs.stop_all();
        ck.ok(
            &format!("{triple}: all ports closed after stop"),
            wait_until(Duration::from_secs(5), || {
                ports.iter().all(|p| !port_open(*p))
            }),
            "",
        );
    }
    ck.finish("real_sh_ucpu");
}
