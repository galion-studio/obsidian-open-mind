// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Test double for `galion-box` used by the box-side real test (tests/real_cli.rs). It creates
//! NO namespaces and runs NO sandbox (sandbox_vps_only): it only mimics the launcher's CLI
//! contract. `run ... -- MODE [ARG]` with MODE = sleep SECS | exit CODE | ignore-term SECS.
//! It writes its pid and argv into `<rootfs>/fake-box.{pid,args}` (rootfs is a scratch dir here).

use std::time::Duration;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("probe") => {
            println!("fake-box probe verdict=fake");
            return;
        }
        Some("run") => {}
        _ => std::process::exit(2),
    }
    let rootfs = a
        .iter()
        .position(|x| x == "--rootfs")
        .and_then(|i| a.get(i + 1))
        .cloned()
        .unwrap_or_default();
    let dash = a.iter().position(|x| x == "--").unwrap_or(a.len());
    let payload = &a[(dash + 1).min(a.len())..];
    let _ = std::fs::write(
        format!("{rootfs}/fake-box.pid"),
        std::process::id().to_string(),
    );
    let _ = std::fs::write(format!("{rootfs}/fake-box.args"), a.join("\n"));
    let arg = |i: usize| {
        payload
            .get(i)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    };
    match payload.first().map(String::as_str) {
        Some("sleep") => std::thread::sleep(Duration::from_secs(arg(1))),
        Some("exit") => std::process::exit(arg(1) as i32),
        Some("ignore-term") => {
            // SAFETY: ignoring SIGTERM in a test helper.
            unsafe { libc::signal(libc::SIGTERM, libc::SIG_IGN) };
            std::thread::sleep(Duration::from_secs(arg(1)));
        }
        _ => std::process::exit(127),
    }
}
