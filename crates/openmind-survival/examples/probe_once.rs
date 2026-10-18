// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use openmind_survival::*;

fn show(r: &ProbeResult) {
    println!(
        "{{\"name\":\"{}\",\"ok\":{},\"detail\":\"{}\",\"ts\":\"{}\"}}",
        r.name, r.ok, r.detail, r.ts
    );
}

fn main() {
    show(&check_http("closed-port", 1, "/"));
    let mem = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    show(&check_meminfo(&mem));
    let la = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    show(&check_loadavg(&la, cores));
}
