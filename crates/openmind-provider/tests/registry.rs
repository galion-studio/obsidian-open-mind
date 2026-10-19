// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use openmind_provider::{
    Approval, Capability, ComputeProvider, Health, ProviderError, ProviderKind, Registry,
    StaticProvider, REGISTRY_VERSION,
};
use std::path::PathBuf;

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "openmind-provider-b34-{}-{}",
        test,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Raw {
    id: &'static str,
    endpoint: &'static str,
    caps: Vec<Capability>,
    health: Health,
}

impl ComputeProvider for Raw {
    fn id(&self) -> &str {
        self.id
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::CpuLocal
    }

    fn endpoint(&self) -> &str {
        self.endpoint
    }

    fn health(&self) -> Health {
        self.health
    }

    fn capabilities(&self) -> &[Capability] {
        &self.caps
    }
}

#[test]
fn missing_file_is_empty_and_is_not_created() {
    let path = scratch("missing").join("registry.json");
    let registry = Registry::load(&path).unwrap();
    assert!(registry.is_empty());
    assert_eq!(registry.len(), 0);
    assert_eq!(registry.path(), path.as_path());
    assert!(!path.exists());
}

#[test]
fn save_writes_an_empty_version_1_document() {
    let path = scratch("empty-save").join("registry.json");
    let registry = Registry::load(&path).unwrap();
    registry.save().unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(text, "{\n  \"version\": 1,\n  \"providers\": []\n}\n");
    assert!(text.is_ascii());
    assert!(!text.contains("sk-"));
    let loaded = Registry::load(&path).unwrap();
    assert!(loaded.is_empty());
    assert_eq!(REGISTRY_VERSION, 1);
}

#[test]
fn register_persists_pending_and_copies_health_snapshot() {
    let path = scratch("register").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    let provider = StaticProvider::try_new(
        "laptop",
        ProviderKind::Lan,
        "http://192.0.2.10:11434/v1",
        vec![Capability::Llm, Capability::Vision],
    )
    .unwrap()
    .with_health(Health::Up);
    let as_dyn: &dyn ComputeProvider = &provider;
    registry.register(as_dyn).unwrap();

    let row = registry.get("laptop").unwrap();
    assert_eq!(row.id(), "laptop");
    assert_eq!(row.kind(), ProviderKind::Lan);
    assert_eq!(row.endpoint(), "http://192.0.2.10:11434/v1");
    assert_eq!(row.health(), Health::Up);
    assert_eq!(row.capabilities(), &[Capability::Llm, Capability::Vision]);
    assert_eq!(row.approval(), Approval::Pending);

    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"approval\": \"pending\""));
    assert!(text.contains("\"health\": \"up\""));
    assert!(text.contains("\"kind\": \"lan\""));
    assert!(!text.contains("sk-"));
    assert!(text.is_ascii());

    let loaded = Registry::load(&path).unwrap();
    assert_eq!(loaded.providers(), registry.providers());
}

#[test]
fn golden_file_matches_the_version_1_shape() {
    let path = scratch("golden").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    let provider = StaticProvider::try_new(
        "laptop",
        ProviderKind::Lan,
        "http://127.0.0.1:11434/v1",
        vec![Capability::Llm],
    )
    .unwrap();
    registry.register(&provider).unwrap();
    registry.set_approval("laptop", Approval::Approved).unwrap();
    registry.set_health("laptop", Health::Up).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text,
        "{\n  \"version\": 1,\n  \"providers\": [\n    {\n      \"id\": \"laptop\",\n      \"kind\": \"lan\",\n      \"endpoint\": \"http://127.0.0.1:11434/v1\",\n      \"capabilities\": [\n        \"llm\"\n      ],\n      \"approval\": \"approved\",\n      \"health\": \"up\"\n    }\n  ]\n}\n"
    );
}

#[test]
fn approval_transitions_round_trip() {
    let path = scratch("approval").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    registry
        .register(&cpu("cpu-local", "fixture://cpu-local"))
        .unwrap();
    registry
        .register(&lan("lan-box", "http://192.0.2.20:11434/v1"))
        .unwrap();
    registry
        .register(&gpu("gpu-box", "https://gpu.example.invalid/v1"))
        .unwrap();
    registry
        .register(&remote("remote-api", "https://api.example.invalid/v1"))
        .unwrap();

    registry
        .set_approval("lan-box", Approval::Approved)
        .unwrap();
    registry
        .set_approval("gpu-box", Approval::Rejected)
        .unwrap();
    registry
        .set_approval("lan-box", Approval::Approved)
        .unwrap();
    registry
        .set_approval("remote-api", Approval::Approved)
        .unwrap();
    registry
        .set_approval("remote-api", Approval::Pending)
        .unwrap();

    let loaded = Registry::load(&path).unwrap();
    assert_eq!(
        loaded.get("cpu-local").unwrap().approval(),
        Approval::Pending
    );
    assert_eq!(
        loaded.get("lan-box").unwrap().approval(),
        Approval::Approved
    );
    assert_eq!(
        loaded.get("gpu-box").unwrap().approval(),
        Approval::Rejected
    );
    assert_eq!(
        loaded.get("remote-api").unwrap().approval(),
        Approval::Pending
    );
    assert_eq!(
        loaded.get("cpu-local").unwrap().kind(),
        ProviderKind::CpuLocal
    );
    assert_eq!(loaded.get("gpu-box").unwrap().kind(), ProviderKind::Gpu);
    assert_eq!(
        loaded.get("remote-api").unwrap().kind(),
        ProviderKind::Remote
    );

    let pending: Vec<_> = loaded
        .with_approval(Approval::Pending)
        .map(|row| row.id())
        .collect();
    assert_eq!(pending, ["cpu-local", "remote-api"]);
    let approved: Vec<_> = loaded
        .with_approval(Approval::Approved)
        .map(|row| row.id())
        .collect();
    assert_eq!(approved, ["lan-box"]);
    let rejected: Vec<_> = loaded
        .with_approval(Approval::Rejected)
        .map(|row| row.id())
        .collect();
    assert_eq!(rejected, ["gpu-box"]);
    assert_eq!(
        loaded
            .providers()
            .iter()
            .map(|row| row.id())
            .collect::<Vec<_>>(),
        ["cpu-local", "lan-box", "gpu-box", "remote-api"]
    );
}

#[test]
fn capabilities_round_trip_in_order() {
    let path = scratch("caps").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    let provider = StaticProvider::try_new(
        "voice",
        ProviderKind::CpuLocal,
        "fixture://voice",
        vec![Capability::Stt, Capability::Tts, Capability::Llm],
    )
    .unwrap();
    registry.register(&provider).unwrap();
    let loaded = Registry::load(&path).unwrap();
    assert_eq!(
        loaded.get("voice").unwrap().capabilities(),
        &[Capability::Stt, Capability::Tts, Capability::Llm]
    );
    assert_eq!(Capability::Vision.as_str(), "vision");
}

#[test]
fn set_health_persists_without_using_the_endpoint() {
    let path = scratch("health").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    registry
        .register(
            &StaticProvider::try_new(
                "discard",
                ProviderKind::Lan,
                "http://192.0.2.55:9/v1",
                vec![Capability::Llm],
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(registry.get("discard").unwrap().health(), Health::Unknown);
    registry.set_health("discard", Health::Down).unwrap();
    let loaded = Registry::load(&path).unwrap();
    assert_eq!(loaded.get("discard").unwrap().health(), Health::Down);
    assert_eq!(
        loaded.get("discard").unwrap().endpoint(),
        "http://192.0.2.55:9/v1"
    );
}

#[test]
fn duplicate_id_leaves_the_file_unchanged() {
    let path = scratch("dup").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    registry
        .register(&cpu("cpu-local", "fixture://cpu"))
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = registry
        .register(&cpu("cpu-local", "fixture://other"))
        .unwrap_err();
    assert!(matches!(err, ProviderError::Duplicate { .. }), "{err}");
    assert!(err.to_string().contains("cpu-local"));
    assert_eq!(registry.len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

#[test]
fn unknown_id_does_not_rewrite_the_file() {
    let path = scratch("missing-id").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    registry
        .register(&cpu("cpu-local", "fixture://cpu"))
        .unwrap();
    let before = std::fs::read(&path).unwrap();
    let err = registry
        .set_approval("nope", Approval::Approved)
        .unwrap_err();
    assert!(matches!(err, ProviderError::NotFound { .. }), "{err}");
    assert!(err.to_string().contains("nope"));
    let err = registry.set_health("nope", Health::Up).unwrap_err();
    assert!(matches!(err, ProviderError::NotFound { .. }), "{err}");
    assert!(registry.get("nope").is_none());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(
        registry.get("cpu-local").unwrap().approval(),
        Approval::Pending
    );
}

#[test]
fn register_rejects_bad_fields_from_any_provider() {
    let path = scratch("bad-fields").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();

    let err = registry
        .register(&Raw {
            id: "",
            endpoint: "fixture://cpu",
            caps: vec![],
            health: Health::Unknown,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Id { .. }), "{err}");
    assert_eq!(err.to_string(), "provider id is empty");

    let err = registry
        .register(&Raw {
            id: "bad id",
            endpoint: "fixture://cpu",
            caps: vec![],
            health: Health::Unknown,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Id { .. }), "{err}");

    let err = registry
        .register(&Raw {
            id: "..",
            endpoint: "fixture://cpu",
            caps: vec![],
            health: Health::Unknown,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Id { .. }), "{err}");

    let err =
        StaticProvider::try_new(".", ProviderKind::CpuLocal, "fixture://cpu", vec![]).unwrap_err();
    assert!(matches!(err, ProviderError::Id { .. }), "{err}");

    let long = "a".repeat(65);
    let err =
        StaticProvider::try_new(long, ProviderKind::CpuLocal, "fixture://cpu", vec![]).unwrap_err();
    assert!(matches!(err, ProviderError::Id { .. }), "{err}");
    assert_eq!(
        err.to_string(),
        "provider id must be 1..=64 bytes from [A-Za-z0-9._-]"
    );

    let err = registry
        .register(&Raw {
            id: "ok-id",
            endpoint: "",
            caps: vec![],
            health: Health::Unknown,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Endpoint { .. }), "{err}");
    assert_eq!(err.to_string(), "endpoint is empty");

    let err = registry
        .register(&Raw {
            id: "ok-id",
            endpoint: "http://192.0.2.10/a b",
            caps: vec![],
            health: Health::Unknown,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Endpoint { .. }), "{err}");

    let err = registry
        .register(&Raw {
            id: "ok-id",
            endpoint: "fixture://cpu\n",
            caps: vec![],
            health: Health::Unknown,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Endpoint { .. }), "{err}");

    let err = registry
        .register(&Raw {
            id: "ok-id",
            endpoint: "fixture://cpu",
            caps: vec![Capability::Llm, Capability::Llm],
            health: Health::Up,
        })
        .unwrap_err();
    assert!(matches!(err, ProviderError::Capability { .. }), "{err}");
    assert!(err.to_string().contains("llm"));
    assert!(registry.is_empty());
    assert!(!path.exists());
}

#[test]
fn load_rejects_bad_documents() {
    let dir = scratch("bad-doc");
    let path = dir.join("registry.json");

    std::fs::write(&path, b"{").unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("registry.json"));

    std::fs::write(&path, [0xff, 0xfe]).unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");

    std::fs::write(&path, b"{\"version\": 2, \"providers\": []}\n").unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(
        matches!(err, ProviderError::Version { found: 2, .. }),
        "{err}"
    );
    assert!(err.to_string().contains("version 2"));

    std::fs::write(
        &path,
        b"{\"version\": 1, \"providers\": [], \"extra\": true}\n",
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");

    std::fs::write(
        &path,
        br#"{"version":1,"providers":[{"id":"laptop","kind":"Approved","endpoint":"fixture://x","capabilities":[],"approval":"pending","health":"unknown"}]}"#,
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");

    std::fs::write(
        &path,
        br#"{"version":1,"providers":[{"id":"laptop","kind":"lan","endpoint":"fixture://x","capabilities":[],"approval":"Approved","health":"unknown"}]}"#,
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");

    std::fs::write(
        &path,
        br#"{"version":1,"providers":[{"id":"laptop","kind":"lan","endpoint":"fixture://x","capabilities":[],"approval":"pending","health":"unknown","base_url":"nope"}]}"#,
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");

    std::fs::write(
        &path,
        br#"{"version":1,"providers":[{"id":"laptop","kind":"lan","endpoint":"fixture://x","capabilities":["llm","llm"],"approval":"pending","health":"unknown"}]}"#,
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("duplicate capability"));

    std::fs::write(
        &path,
        br#"{"version":1,"providers":[{"id":"laptop","kind":"lan","endpoint":"fixture://x","capabilities":[],"approval":"pending","health":"unknown"},{"id":"laptop","kind":"gpu","endpoint":"fixture://y","capabilities":[],"approval":"approved","health":"down"}]}"#,
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("duplicate provider id laptop"));

    std::fs::write(
        &path,
        br#"{"version":1,"providers":[{"id":"","kind":"lan","endpoint":"fixture://x","capabilities":[],"approval":"approved","health":"up"}]}"#,
    )
    .unwrap();
    let err = Registry::load(&path).unwrap_err();
    assert!(matches!(err, ProviderError::Parse { .. }), "{err}");
    assert!(err.to_string().contains("provider id is empty"));
}

#[test]
fn load_of_a_directory_is_an_io_error() {
    let dir = scratch("dir-path");
    let err = Registry::load(&dir).unwrap_err();
    assert!(matches!(err, ProviderError::Io { .. }), "{err}");
    assert!(err.to_string().contains("cannot read registry"));
}

#[test]
fn register_rolls_back_when_the_parent_cannot_be_created() {
    let dir = scratch("rollback");
    let path = dir.join("sub").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    assert!(registry.is_empty());
    std::fs::write(dir.join("sub"), b"not-a-directory").unwrap();
    let err = registry
        .register(&cpu("cpu-local", "fixture://cpu"))
        .unwrap_err();
    assert!(matches!(err, ProviderError::Io { .. }), "{err}");
    assert!(registry.is_empty());
    assert_eq!(registry.get("cpu-local"), None);
}

#[test]
fn library_source_has_no_socket_api() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .unwrap();
    for token in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
        "UnixStream",
        "reqwest",
        "ureq",
        "hyper",
    ] {
        assert!(!src.contains(token), "{token} found in lib.rs");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn register_or_list_does_not_open_a_socket_or_approve_pending() {
    let src = std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/lib.rs"))
        .unwrap();
    for token in [
        "std::net",
        "TcpStream",
        "UdpSocket",
        "TcpListener",
        "UnixStream",
        "reqwest",
        "ureq",
        "hyper",
    ] {
        assert!(!src.contains(token), "{token} found in lib.rs");
    }

    let path = scratch("list-pending").join("registry.json");
    let mut registry = Registry::load(&path).unwrap();
    let before = socket_fds();

    registry
        .register(&lan("pending-box", "http://192.0.2.10:9/v1"))
        .unwrap();
    assert_eq!(
        registry.get("pending-box").unwrap().approval(),
        Approval::Pending
    );
    assert_eq!(
        socket_fds(),
        before,
        "registering a provider opened a socket"
    );

    let listed: Vec<String> = registry
        .providers()
        .iter()
        .map(|row| row.id().to_string())
        .collect();
    assert_eq!(listed, ["pending-box"]);
    assert!(registry.with_approval(Approval::Approved).next().is_none());
    assert_eq!(
        registry.get("pending-box").unwrap().approval(),
        Approval::Pending
    );
    assert_eq!(socket_fds(), before, "listing providers opened a socket");

    registry
        .register(
            &StaticProvider::try_new(
                "up-box",
                ProviderKind::Remote,
                "http://192.0.2.20:9/v1",
                vec![Capability::Llm],
            )
            .unwrap()
            .with_health(Health::Up),
        )
        .unwrap();
    assert_eq!(registry.get("up-box").unwrap().health(), Health::Up);
    assert_eq!(
        registry.get("up-box").unwrap().approval(),
        Approval::Pending
    );
    assert_eq!(
        registry.get("pending-box").unwrap().approval(),
        Approval::Pending
    );
    assert!(registry.with_approval(Approval::Approved).next().is_none());

    let loaded = Registry::load(&path).unwrap();
    let pending: Vec<String> = loaded
        .with_approval(Approval::Pending)
        .map(|row| row.id().to_string())
        .collect();
    assert_eq!(pending, ["pending-box", "up-box"]);
    assert_eq!(loaded.with_approval(Approval::Approved).count(), 0);
    assert_eq!(
        socket_fds(),
        before,
        "registering providers opened a socket"
    );
}

#[cfg(target_os = "linux")]
fn socket_fds() -> Vec<String> {
    let mut names = Vec::new();
    let dir = std::fs::read_dir("/proc/self/fd").unwrap();
    for entry in dir.flatten() {
        if let Ok(target) = std::fs::read_link(entry.path()) {
            let text = target.to_string_lossy().into_owned();
            if text.starts_with("socket:") {
                names.push(text);
            }
        }
    }
    names.sort();
    names
}

fn cpu(id: &str, endpoint: &str) -> StaticProvider {
    StaticProvider::try_new(id, ProviderKind::CpuLocal, endpoint, vec![Capability::Llm]).unwrap()
}

fn lan(id: &str, endpoint: &str) -> StaticProvider {
    StaticProvider::try_new(id, ProviderKind::Lan, endpoint, vec![Capability::Llm]).unwrap()
}

fn gpu(id: &str, endpoint: &str) -> StaticProvider {
    StaticProvider::try_new(
        id,
        ProviderKind::Gpu,
        endpoint,
        vec![Capability::Llm, Capability::Vision],
    )
    .unwrap()
}

fn remote(id: &str, endpoint: &str) -> StaticProvider {
    StaticProvider::try_new(id, ProviderKind::Remote, endpoint, vec![Capability::Llm]).unwrap()
}
