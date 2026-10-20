// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Approved discovery rows feed the fallback; pending and rejected never do.

use openmind_provider::route::{self, ProviderRow};
use openmind_provider::{
    fallback, Approval, Capability, ComputeProvider, Health, ProviderKind, Registry, StaticProvider,
};
use std::path::PathBuf;

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openmind-provider-lanroute-{}-{}",
        test,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn lan(id: &str, endpoint: &str) -> StaticProvider {
    StaticProvider::try_new(id, ProviderKind::Lan, endpoint, vec![Capability::Llm]).unwrap()
}

fn cpu() -> ProviderRow {
    ProviderRow {
        id: "cpu-local".into(),
        kind: fallback::Kind::CpuLocal,
        endpoint: "http://127.0.0.1:19311".into(),
        approval: fallback::Approval::Approved,
        health: fallback::Health::Unknown,
        capabilities: vec![fallback::Capability::Llm],
    }
}

fn ids(rows: &[ProviderRow]) -> Vec<&str> {
    rows.iter().map(|r| r.id.as_str()).collect()
}

#[test]
fn approved_rows_only_and_lan_needs_join_up() {
    let dir = scratch("approved");
    let mut reg = Registry::load(dir.join("p.json")).unwrap();
    reg.register(&lan("lan-a", "http://127.0.0.1:19310").with_health(Health::Up))
        .unwrap();
    reg.register(&lan("lan-b", "http://127.0.0.1:19313").with_health(Health::Up))
        .unwrap();
    reg.register(&lan("lan-c", "http://127.0.0.1:19314").with_health(Health::Up))
        .unwrap();
    let need = fallback::Capability::Llm;

    // All pending: the chain is the CPU fallback only.
    assert!(reg.approved_rows().is_empty());
    assert_eq!(ids(&route::chain(need, &reg, &[cpu()])), ["cpu-local"]);

    reg.set_approval("lan-a", Approval::Approved).unwrap();
    reg.set_approval("lan-b", Approval::Rejected).unwrap();
    assert_eq!(ids(&reg.approved_rows()), ["lan-a"]);
    assert_eq!(
        ids(&route::chain(need, &reg, &[cpu()])),
        ["lan-a", "cpu-local"]
    );

    // join said down: skipped, CPU takes over.
    reg.set_health("lan-a", Health::Down).unwrap();
    assert_eq!(ids(&route::chain(need, &reg, &[cpu()])), ["cpu-local"]);
    // never joined counts as not up for a LAN row.
    reg.set_health("lan-a", Health::Unknown).unwrap();
    assert_eq!(ids(&route::chain(need, &reg, &[cpu()])), ["cpu-local"]);
    reg.set_health("lan-a", Health::Up).unwrap();

    // Denied: gone from the chain even though health is up.
    reg.set_approval("lan-a", Approval::Rejected).unwrap();
    assert_eq!(ids(&route::chain(need, &reg, &[cpu()])), ["cpu-local"]);
    assert!(reg.approved_rows().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn gpu_before_lan_before_cpu() {
    let dir = scratch("order");
    let mut reg = Registry::load(dir.join("p.json")).unwrap();
    reg.register(&lan("lan-a", "http://127.0.0.1:19310").with_health(Health::Up))
        .unwrap();
    reg.set_approval("lan-a", Approval::Approved).unwrap();
    let mut gpu = cpu();
    gpu.id = "gpu-0".into();
    gpu.kind = fallback::Kind::Gpu;
    let chain = route::chain(fallback::Capability::Llm, &reg, &[cpu(), gpu]);
    assert_eq!(ids(&chain), ["gpu-0", "lan-a", "cpu-local"]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_moved_device_updates_only_after_re_approval() {
    let dir = scratch("moved");
    let path = dir.join("p.json");
    let mut reg = Registry::load(&path).unwrap();
    reg.register(&lan("lan-a", "http://127.0.0.1:19310").with_health(Health::Up))
        .unwrap();
    reg.set_approval("lan-a", Approval::Approved).unwrap();

    assert!(reg
        .note_endpoint("lan-a", "http://127.0.0.1:19315")
        .unwrap());
    assert!(!reg
        .note_endpoint("lan-a", "http://127.0.0.1:19315")
        .unwrap());
    let row = reg.get("lan-a").unwrap();
    assert_eq!(row.endpoint(), "http://127.0.0.1:19310");
    assert_eq!(row.pending_endpoint(), Some("http://127.0.0.1:19315"));
    // Still routed at the approved endpoint, never at the new one.
    let rows = reg.approved_rows();
    assert_eq!(rows[0].endpoint, "http://127.0.0.1:19310");

    // Persisted and reloadable.
    let mut reg = Registry::load(&path).unwrap();
    assert_eq!(
        reg.get("lan-a").unwrap().pending_endpoint(),
        Some("http://127.0.0.1:19315")
    );

    // A deny does not apply the move.
    reg.set_approval("lan-a", Approval::Rejected).unwrap();
    assert_eq!(
        reg.get("lan-a").unwrap().endpoint(),
        "http://127.0.0.1:19310"
    );

    // Re-approval applies it and asks for a new join.
    reg.set_approval("lan-a", Approval::Approved).unwrap();
    let row = reg.get("lan-a").unwrap();
    assert_eq!(row.endpoint(), "http://127.0.0.1:19315");
    assert_eq!(row.pending_endpoint(), None);
    assert_eq!(row.health(), Health::Unknown);
    assert!(route::chain(fallback::Capability::Llm, &reg, &[]).is_empty());

    // Seen at its current endpoint again clears a stale pending move.
    reg.note_endpoint("lan-a", "http://127.0.0.1:19316")
        .unwrap();
    assert!(reg
        .note_endpoint("lan-a", "http://127.0.0.1:19315")
        .unwrap());
    assert_eq!(reg.get("lan-a").unwrap().pending_endpoint(), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn vram_is_kept_and_old_files_still_load() {
    let dir = scratch("vram");
    let path = dir.join("p.json");
    let old = r#"{"version":1,"providers":[{"id":"a","kind":"lan","endpoint":"http://127.0.0.1:1","capabilities":["llm"],"approval":"approved","health":"up"}]}"#;
    std::fs::write(&path, old).unwrap();
    let mut reg = Registry::load(&path).unwrap();
    assert_eq!(reg.get("a").unwrap().vram_gb(), None);
    reg.save().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("vram_gb") && !text.contains("pending_endpoint"));

    reg.register(&lan("b", "http://127.0.0.1:2").with_vram_gb(Some(24)))
        .unwrap();
    let reg = Registry::load(&path).unwrap();
    assert_eq!(reg.get("b").unwrap().vram_gb(), Some(24));

    let mut reg = reg;
    let err = reg
        .register(&lan("c", "http://127.0.0.1:3").with_vram_gb(Some(0)))
        .unwrap_err();
    assert!(err.to_string().contains("vram_gb"));
    let bad = old.replace(r#""health":"up""#, r#""health":"up","vram_gb":9999"#);
    std::fs::write(&path, bad).unwrap();
    assert!(Registry::load(&path).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}
