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
