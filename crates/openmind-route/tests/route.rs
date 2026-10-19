// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Route order: GPU, else an approved LAN row, else a CPU local row.
//! Pending rows are not chosen. Fixture rows only.

use openmind_route::{route, Approval, Capability, Health, Kind, ProviderRow};
use std::path::PathBuf;

fn base(id: &str, kind: Kind) -> ProviderRow {
    ProviderRow {
        id: id.to_owned(),
        kind,
        endpoint: format!("fixture://{id}"),
        approval: Approval::Approved,
        health: Health::Up,
        capabilities: vec![Capability::Llm],
    }
}

#[test]
fn gpu_wins_even_when_listed_after_lan_and_cpu() {
    let rows = [
        base("cpu-local", Kind::CpuLocal),
        base("lan-box", Kind::Lan),
        base("gpu-box", Kind::Gpu),
    ];
    let picked = route(Capability::Llm, &rows).expect("gpu");
    assert_eq!(picked.id, "gpu-box");
    assert_eq!(picked.kind, Kind::Gpu);
    assert_eq!(picked, rows[2]);
}

#[test]
fn approved_lan_when_gpu_is_down() {
    let mut gpu = base("gpu-box", Kind::Gpu);
    gpu.health = Health::Down;
    let rows = [
        gpu,
        base("lan-box", Kind::Lan),
        base("cpu-local", Kind::CpuLocal),
    ];
    assert_eq!(route(Capability::Llm, &rows).expect("lan").id, "lan-box");
}

#[test]
fn pending_lan_falls_through_to_cpu() {
    let mut lan = base("lan-box", Kind::Lan);
    lan.approval = Approval::Pending;
    let rows = [lan, base("cpu-local", Kind::CpuLocal)];
    assert_eq!(route(Capability::Llm, &rows).expect("cpu").id, "cpu-local");
}

#[test]
fn rejected_lan_is_not_approved() {
    let mut lan = base("lan-box", Kind::Lan);
    lan.approval = Approval::Rejected;
    let rows = [lan, base("cpu-local", Kind::CpuLocal)];
    assert_eq!(route(Capability::Llm, &rows).expect("cpu").id, "cpu-local");
}

#[test]
fn pending_or_rejected_gpu_falls_through_to_approved_lan() {
    let mut pending = base("gpu-pending", Kind::Gpu);
    pending.approval = Approval::Pending;
    let mut rejected = base("gpu-rejected", Kind::Gpu);
    rejected.approval = Approval::Rejected;
    let rows = [pending, rejected, base("lan-box", Kind::Lan)];
    assert_eq!(route(Capability::Llm, &rows).expect("lan").id, "lan-box");
}

#[test]
fn empty_endpoint_is_invisible() {
    let mut gpu = base("gpu-box", Kind::Gpu);
    gpu.endpoint.clear();
    let mut lan = base("lan-box", Kind::Lan);
    lan.endpoint.clear();
    let rows = [gpu, lan, base("cpu-local", Kind::CpuLocal)];
    assert_eq!(route(Capability::Llm, &rows).expect("cpu").id, "cpu-local");
}

#[test]
fn need_must_be_covered_before_a_tier_wins() {
    let mut gpu = base("gpu-box", Kind::Gpu);
    gpu.capabilities = vec![Capability::Vision];
    let mut lan = base("lan-box", Kind::Lan);
    lan.capabilities = vec![Capability::Stt, Capability::Llm];
    let rows = [gpu, lan, base("cpu-local", Kind::CpuLocal)];
    assert_eq!(route(Capability::Llm, &rows).expect("lan").id, "lan-box");
    assert_eq!(route(Capability::Vision, &rows).expect("gpu").id, "gpu-box");
    assert!(route(Capability::Tts, &rows).is_none());
}

#[test]
fn first_usable_row_inside_a_tier_wins() {
    let mut later = base("gpu-b", Kind::Gpu);
    later.health = Health::Up;
    let mut earlier = base("gpu-a", Kind::Gpu);
    earlier.health = Health::Unknown;
    let rows = [earlier, later];
    assert_eq!(route(Capability::Llm, &rows).expect("first").id, "gpu-a");
}

#[test]
fn unknown_health_gpu_still_beats_an_up_lan() {
    let mut gpu = base("gpu-box", Kind::Gpu);
    gpu.health = Health::Unknown;
    let rows = [base("lan-box", Kind::Lan), gpu];
    assert_eq!(route(Capability::Llm, &rows).expect("gpu").id, "gpu-box");
}

#[test]
fn down_lan_falls_through_to_cpu() {
    let mut lan = base("lan-box", Kind::Lan);
    lan.health = Health::Down;
    let rows = [lan, base("cpu-local", Kind::CpuLocal)];
    assert_eq!(route(Capability::Llm, &rows).expect("cpu").id, "cpu-local");
}

#[test]
fn remote_is_never_chosen() {
    let rows = [base("api", Kind::Remote), base("api-2", Kind::Remote)];
    assert!(route(Capability::Llm, &rows).is_none());
}

#[test]
fn unapproved_cpu_is_not_a_fallback() {
    let mut cpu = base("cpu-local", Kind::CpuLocal);
    cpu.approval = Approval::Pending;
    assert!(route(Capability::Llm, &[cpu]).is_none());
}

#[test]
fn empty_input_does_not_choose_a_pending_seed() {
    assert!(route(Capability::Llm, &[]).is_none());
    assert!(route(Capability::Stt, &[]).is_none());
}

#[test]
fn built_in_pending_seeds_do_not_shadow_an_approved_lan() {
    let rows = [base("lan-box", Kind::Lan)];
    let picked = route(Capability::Llm, &rows).expect("lan");
    assert_eq!(picked.id, "lan-box");
    assert_eq!(picked.approval, Approval::Approved);
    assert_eq!(picked.endpoint, "fixture://lan-box");
}

#[test]
fn approved_fixture_with_a_seed_id_is_not_the_pending_copy() {
    let rows = [base("seed-openmind", Kind::Gpu)];
    let picked = route(Capability::Llm, &rows).expect("gpu");
    assert_eq!(picked, rows[0]);
    assert_eq!(picked.approval, Approval::Approved);
    assert_eq!(picked.endpoint, "fixture://seed-openmind");
}

#[test]
fn sources_do_not_name_sockets_or_http_clients() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("manifest");
    for token in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
        "UnixStream",
        "reqwest",
        "ureq",
        "hyper",
        "tokio",
    ] {
        assert!(!lib.contains(token), "{token} in lib.rs");
        assert!(!manifest.contains(token), "{token} in Cargo.toml");
    }
}

const COVERAGE: [Capability; 4] = [
    Capability::Llm,
    Capability::Stt,
    Capability::Tts,
    Capability::Vision,
];

const PENDING_SEEDS: &[(&str, &str)] = &[
    ("seed-openmind", "127.0.0.1:7787"),
    ("seed-ollama", "127.0.0.1:11434"),
    ("seed-llama-rpc", "127.0.0.1:50052"),
    ("seed-exo", "127.0.0.1:52415"),
];

#[test]
fn route_never_returns_a_pending_seed_row_or_remote_and_opens_no_socket() {
    let seeds = pending_seed_rows(Health::Unknown, vec![Capability::Llm]);
    let ripe_seeds = pending_seed_rows(Health::Up, COVERAGE.to_vec());
    let remote = covered("hosted-api", Kind::Remote);
    let remote_b = covered("hosted-api-b", Kind::Remote);
    let gpu = covered("gpu-box", Kind::Gpu);
    let lan = covered("lan-box", Kind::Lan);
    let cpu = covered("cpu-local", Kind::CpuLocal);
    let named = covered("seed-openmind", Kind::Gpu);

    let only_remote = [remote.clone(), remote_b.clone()];
    let cpu_case = [remote.clone(), ripe_seeds[0].clone(), cpu.clone()];
    let gpu_case = [
        remote.clone(),
        seeds[0].clone(),
        lan.clone(),
        cpu.clone(),
        gpu.clone(),
    ];
    let named_case = [named.clone(), remote];

    let before = fd_targets();
    assert!(!before.is_empty(), "proc fd table was not readable");

    for need in COVERAGE {
        assert!(route(need, &[]).is_none());
        assert!(route(need, &seeds).is_none());
        assert!(route(need, &ripe_seeds).is_none());
        assert!(route(need, &only_remote).is_none());

        let cpu_pick = route(need, &cpu_case).expect("cpu");
        assert_eq!(cpu_pick.id, "cpu-local");
        assert_not_seed_or_remote(&cpu_case, Some(cpu_pick));

        let gpu_pick = route(need, &gpu_case).expect("gpu");
        assert_eq!(gpu_pick.id, "gpu-box");
        assert_eq!(gpu_pick.kind, Kind::Gpu);
        assert_not_seed_or_remote(&gpu_case, Some(gpu_pick));

        let named_pick = route(need, &named_case).expect("named");
        assert_eq!(named_pick.id, "seed-openmind");
        assert_eq!(named_pick.endpoint, "fixture://seed-openmind");
        assert_ne!(named_pick.kind, Kind::Remote);
        assert!(!is_pending_seed_row(&named_pick));
        assert_not_seed_or_remote(&named_case, Some(named_pick));
    }

    let after = fd_targets();
    assert_eq!(socket_fds(&before), socket_fds(&after));

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lib = std::fs::read_to_string(root.join("src/lib.rs")).expect("lib");
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("manifest");
    for token in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
        "UnixStream",
        "reqwest",
        "ureq",
        "hyper",
        "tokio",
    ] {
        assert!(!lib.contains(token), "{token} in lib.rs");
        assert!(!manifest.contains(token), "{token} in Cargo.toml");
    }
}

fn covered(id: &str, kind: Kind) -> ProviderRow {
    let mut row = base(id, kind);
    row.capabilities = COVERAGE.to_vec();
    row
}

fn pending_seed_rows(health: Health, capabilities: Vec<Capability>) -> Vec<ProviderRow> {
    PENDING_SEEDS
        .iter()
        .map(|&(id, endpoint)| ProviderRow {
            id: id.to_owned(),
            kind: Kind::Lan,
            endpoint: endpoint.to_owned(),
            approval: Approval::Pending,
            health,
            capabilities: capabilities.clone(),
        })
        .collect()
}

fn is_pending_seed_row(row: &ProviderRow) -> bool {
    row.approval == Approval::Pending
        && PENDING_SEEDS
            .iter()
            .any(|&(id, endpoint)| row.id == id && row.endpoint == endpoint)
}

fn assert_not_seed_or_remote(rows: &[ProviderRow], picked: Option<ProviderRow>) {
    let Some(chosen) = picked else {
        return;
    };
    assert_ne!(chosen.kind, Kind::Remote);
    assert!(!is_pending_seed_row(&chosen), "pending seed {}", chosen.id);
    assert!(
        rows.iter().any(|row| row == &chosen),
        "not a caller row: {}",
        chosen.id
    );
}

fn socket_fds(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| target.starts_with("socket:"))
        .cloned()
        .collect()
}

fn fd_targets() -> Vec<String> {
    let mut names = Vec::new();
    let dir = std::fs::read_dir("/proc/self/fd").expect("fd");
    for entry in dir.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            names.push(target.to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}
