// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the first galion-common migration (tuner and
//! galion-origin). This is the Rust port of the former `scripts/real/common.sh`
//! from slot 16; its output format is unchanged. It is gated by
//! `GALION_REAL=1` and by `#[ignore]`:
//!
//! ```text
//! GALION_REAL=1 cargo test --release -p galion-common --test real_common \
//!     -- --ignored --nocapture
//! ```
//!
//! The test builds the migrated tuner and galion-origin (release) and starts
//! them on base+0 and base+1. With `GALION_REAL_OLD=1` it also builds the
//! pre-migration binaries from `GALION_REAL_OLD_REV` (default 13a296ce) and
//! runs them on base+3 (old tuner) and base+2 (old origin). Then it hammers
//! all of them over real HTTP (latency, malformed, oversized, slowloris),
//! samples memory and threads, and tears down. All processes must still be
//! alive at the end.
//!
//! `GALION_REAL_PORT_BASE` defaults to 19160, slot 16's range as before.
//! Other slots must set their own range.

#[path = "real_hammer.rs"]
mod real_hammer;
mod realsupport;

use realsupport::{addr, now, Proc};
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

#[test]
#[ignore = "real-life test: GALION_REAL=1, builds and runs tuner/galion-origin"]
fn real_common() {
    if !realsupport::enabled("real_common") {
        return;
    }
    let base = realsupport::port_base(19160);
    let with_old = std::env::var("GALION_REAL_OLD").ok().as_deref() == Some("1");
    let rev = std::env::var("GALION_REAL_OLD_REV")
        .unwrap_or_else(|_| "13a296ce15b5bbd66adc26f6bfae787b2c1b5190".into());
    let packages = ["tuner", "galion-origin"];
    let scratch = realsupport::Scratch::new("galion-common-real");
    let ws = realsupport::workspace();
    let bin = realsupport::build(&ws, &realsupport::target_dir("new"), &packages);

    // origin fixture root
    let root = scratch.join("root");
    std::fs::create_dir_all(root.join("docs/build-session")).unwrap();
    std::fs::create_dir_all(root.join("state")).unwrap();
    std::fs::create_dir_all(scratch.join("grinder")).unwrap();
    std::fs::write(root.join("docs/build-session/a.md"), "# note\n").unwrap();
    std::fs::write(scratch.join("grinder/index.html"), "<h1>grinder</h1>\n").unwrap();
    let www = ws.join("crates/tuner/www").display().to_string();
    let tuner = |name: &str, dir: &Path, port: u16| {
        realsupport::start(
            name,
            &dir.join("tuner"),
            &[],
            &[
                ("TUNER_ADDR", format!("127.0.0.1:{port}")),
                ("TUNER_ROOT", www.clone()),
            ],
            port,
            &scratch.join(&format!("{name}.log")),
        )
    };
    let origin = |name: &str, dir: &Path, port: u16| {
        realsupport::start(
            name,
            &dir.join("galion-origin"),
            &[],
            &[
                ("ORIGIN_ADDR", format!("127.0.0.1:{port}")),
                ("ORIGIN_ROOT", root.display().to_string()),
                (
                    "ORIGIN_GRINDER_ROOT",
                    scratch.join("grinder").display().to_string(),
                ),
            ],
            port,
            &scratch.join(&format!("{name}.log")),
        )
    };

    let mut procs: Vec<Proc> = vec![tuner("tuner", &bin, base), origin("origin", &bin, base + 1)];
    let mut targets: Vec<(String, SocketAddr)> = vec![
        ("tuner".into(), addr(base)),
        ("origin".into(), addr(base + 1)),
    ];
    if with_old {
        let old = realsupport::build_old(&rev, &scratch, &packages, &packages);
        procs.push(tuner("old-tuner", &old, base + 3));
        procs.push(origin("old-origin", &old, base + 2));
        targets.push(("old-tuner".into(), addr(base + 3)));
        targets.push(("old-origin".into(), addr(base + 2)));
    }

    println!("== {} start", now());
    realsupport::mem("before", &procs);
    let sampler = realsupport::Sampler::start(&procs);
    for (name, a) in &targets {
        real_hammer::hammer(name, *a);
    }
    std::thread::sleep(Duration::from_secs(1));
    println!("== {} done", now());
    realsupport::mem("after", &procs);
    sampler.finish();
    let dead = realsupport::alive_report(&mut procs);
    assert!(dead.is_empty(), "died under abuse: {dead:?}");
}
