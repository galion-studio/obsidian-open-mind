// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use openmind_llama::{
    FixtureError, HealthState, LlamaCppProvider, ModelCard, ModelList, ModelMeta, ModelStatus,
    ParsedBody, ServerKind, LLAMA_SERVER_PORT, RPC_SERVER_PORT,
};

fn row(kind: ServerKind) -> LlamaCppProvider {
    let endpoint = match kind {
        ServerKind::LlamaServer => "http://127.0.0.1:8080",
        ServerKind::RpcServer => "127.0.0.1:50052",
    };
    LlamaCppProvider::new("laptop-4060", endpoint, kind).unwrap()
}

fn key_prefix() -> String {
    ['s', 'k', '-'].into_iter().collect()
}

#[test]
fn provider_row_does_not_dial_or_need_a_gpu() {
    let llama = row(ServerKind::LlamaServer);
    assert_eq!(llama.id(), "laptop-4060");
    assert_eq!(llama.endpoint(), "http://127.0.0.1:8080");
    assert_eq!(llama.kind(), ServerKind::LlamaServer);
    assert!(llama.speaks_openai());
    assert!(!llama.is_rpc_device());
    assert!(!llama.dials_network());
    assert!(!llama.requires_gpu());
    assert!(!llama.spawns_process());
    assert_eq!(LLAMA_SERVER_PORT, 8080);

    let rpc = row(ServerKind::RpcServer);
    assert_eq!(rpc.endpoint(), "127.0.0.1:50052");
    assert!(rpc.is_rpc_device());
    assert!(!rpc.speaks_openai());
    assert!(!rpc.dials_network());
    assert!(!rpc.requires_gpu());
    assert!(!rpc.spawns_process());
    assert_eq!(RPC_SERVER_PORT, 50052);
}

#[test]
fn gpu_requirement_is_false_and_no_process_is_spawned() {
    let src = include_str!("../src/lib.rs");
    for banned in ["std::process", "Command::new", "process::Command"] {
        assert!(!src.contains(banned), "{banned}");
    }
    for kind in [ServerKind::LlamaServer, ServerKind::RpcServer] {
        let provider = row(kind);
        assert!(!provider.requires_gpu());
        assert!(!provider.spawns_process());
        let health = provider.parse_health(r#"{"status":"ok"}"#).unwrap();
        assert!(health.is_ready());
        let models = provider
            .parse_models(r#"{"object":"list","data":[]}"#)
            .unwrap();
        assert!(models.data.is_empty());
    }
}

#[test]
fn rejects_blank_and_key_shaped_rows() {
    let blank = LlamaCppProvider::new("  ", "http://127.0.0.1:8080", ServerKind::LlamaServer);
    assert!(matches!(
        blank,
        Err(FixtureError::BadField { field, .. }) if field == "id"
    ));
    let spaced = LlamaCppProvider::new("lap top", "http://127.0.0.1:8080", ServerKind::LlamaServer);
    assert!(matches!(spaced, Err(FixtureError::BadField { .. })));
    let keyed = format!("{}live", key_prefix());
    let err = LlamaCppProvider::new(keyed, "http://127.0.0.1:8080", ServerKind::LlamaServer);
    assert_eq!(err, Err(FixtureError::SecretShape("id")));
    let err = LlamaCppProvider::new(
        "laptop-4060",
        format!("http://127.0.0.1:8080/{}", key_prefix()),
        ServerKind::LlamaServer,
    );
    assert_eq!(err, Err(FixtureError::SecretShape("endpoint")));
}

#[test]
fn health_ok_fixture() {
    let body = include_str!("../fixtures/health_ok.json");
    let health = row(ServerKind::LlamaServer).parse_health(body).unwrap();
    assert!(health.is_ready());
    assert_eq!(health.state, HealthState::Ready);
    assert_eq!(health.status.as_deref(), Some("ok"));
    assert_eq!(health.slots_idle, None);
    assert_eq!(health.slots_processing, None);
    assert!(health.error.is_none());
}

#[test]
fn health_loading_fixture() {
    let body = include_str!("../fixtures/health_loading.json");
    let health = row(ServerKind::RpcServer).parse_health(body).unwrap();
    assert!(health.is_loading());
    assert!(health.status.is_none());
    let err = health.error.unwrap();
    assert_eq!(err.code, Some(503));
    assert_eq!(err.message.as_deref(), Some("Loading model"));
    assert_eq!(err.kind.as_deref(), Some("unavailable_error"));
    assert!(!row(ServerKind::RpcServer).dials_network());
}

#[test]
fn health_no_slot_fixture() {
    let body = include_str!("../fixtures/health_no_slot.json");
    let health = row(ServerKind::LlamaServer).parse_health(body).unwrap();
    assert!(health.is_no_slot());
    assert_eq!(health.state, HealthState::NoSlot);
    let err = health.error.unwrap();
    assert_eq!(err.message.as_deref(), Some("no slot available"));
    assert_eq!(err.code, Some(503));
    assert_eq!(err.kind.as_deref(), Some("unavailable_error"));
}

#[test]
fn health_legacy_status_and_slots() {
    let provider = row(ServerKind::LlamaServer);
    let ready = provider
        .parse_health(r#"{"status":"ok","slots_idle":3,"slots_processing":1,"extra":true}"#)
        .unwrap();
    assert!(ready.is_ready());
    assert_eq!(ready.slots_idle, Some(3));
    assert_eq!(ready.slots_processing, Some(1));

    let legacy = provider
        .parse_health(r#"{"status":"no slot available"}"#)
        .unwrap();
    assert!(legacy.is_no_slot());

    let loading = provider
        .parse_health(r#"{"status":"loading model"}"#)
        .unwrap();
    assert!(loading.is_loading());
}

#[test]
fn health_other_errors_and_rejects() {
    let provider = row(ServerKind::LlamaServer);
    let unavailable = provider
        .parse_health(r#"{"error":{"code":500,"message":"server blew up","type":"server_error"}}"#)
        .unwrap();
    assert_eq!(unavailable.state, HealthState::Unavailable);

    let stream = provider
        .parse_health(
            r#"{"error":{"code":503,"message":"Stream owner model is loading, retry later","type":"unavailable_error"}}"#,
        )
        .unwrap();
    assert!(stream.is_loading());

    assert_eq!(
        provider.parse_health("   ").unwrap_err(),
        FixtureError::Empty
    );
    assert!(matches!(
        provider.parse_health("{").unwrap_err(),
        FixtureError::Json(_)
    ));
    assert_eq!(
        provider.parse_health("[]").unwrap_err(),
        FixtureError::NotAnObject
    );
    assert_eq!(
        provider.parse_health(r#"{"ok":true}"#).unwrap_err(),
        FixtureError::HealthShape
    );
    assert_eq!(
        provider.parse_health(r#"{"slots_idle":1}"#).unwrap_err(),
        FixtureError::HealthShape
    );
    assert!(matches!(
        provider.parse_health(r#"{"status":""}"#).unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_health(r#"{"status":"ok","slots_idle":-1}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_health(r#"{"status":"ok","slots_idle":1.5}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
}

#[test]
fn models_fixture_keeps_meta_and_ignores_ollama_key() {
    let body = include_str!("../fixtures/v1_models.json");
    let list = row(ServerKind::LlamaServer).parse_models(body).unwrap();
    assert_eq!(
        list,
        ModelList {
            object: "list".to_owned(),
            data: vec![ModelCard {
                id: "llama-3.1-8b-instruct-q4".to_owned(),
                object: "model".to_owned(),
                created: Some(1_735_142_223),
                owned_by: Some("llamacpp".to_owned()),
                aliases: vec!["main".to_owned()],
                tags: vec!["local".to_owned()],
                meta: Some(ModelMeta {
                    vocab_type: Some(2),
                    n_vocab: Some(128_256),
                    n_ctx: Some(4096),
                    n_ctx_train: Some(131_072),
                    n_embd: Some(4096),
                    n_params: Some(8_030_261_312),
                    size: Some(4_912_898_304),
                    ftype: Some("Q4_K_M".to_owned()),
                }),
                input_modalities: vec![],
                output_modalities: vec![],
                status: None,
            }],
        }
    );
}

#[test]
fn models_meta_null_and_router_status() {
    let null_meta = include_str!("../fixtures/v1_models_meta_null.json");
    let list = row(ServerKind::LlamaServer)
        .parse_models(null_meta)
        .unwrap();
    assert_eq!(list.data.len(), 1);
    assert!(list.data[0].meta.is_none());
    assert_eq!(list.data[0].id, "loading-alias");

    let router = include_str!("../fixtures/v1_models_router.json");
    let list = row(ServerKind::LlamaServer).parse_models(router).unwrap();
    assert_eq!(list.data.len(), 3);

    let loaded = &list.data[0];
    assert_eq!(loaded.id, "box-text");
    assert_eq!(loaded.input_modalities, vec!["text", "image"]);
    assert_eq!(loaded.output_modalities, vec!["text"]);
    let status = loaded.status.clone().unwrap();
    assert!(status.is_loaded());
    assert_eq!(
        status,
        ModelStatus {
            value: "loaded".to_owned(),
            args: vec![
                "llama-server".to_owned(),
                "-ctx".to_owned(),
                "4096".to_owned()
            ],
            failed: false,
            exit_code: None,
        }
    );

    let failed = &list.data[1];
    let status = failed.status.clone().unwrap();
    assert!(status.is_failed());
    assert_eq!(status.exit_code, Some(1));
    assert_eq!(status.value, "unloaded");

    let sleeping = &list.data[2];
    assert!(sleeping.status.as_ref().unwrap().is_sleeping());
}

#[test]
fn models_rejects_bad_shapes() {
    let provider = row(ServerKind::LlamaServer);
    assert_eq!(
        provider
            .parse_models(r#"{"models":[],"object":"list"}"#)
            .unwrap_err(),
        FixtureError::MissingField("data".to_owned())
    );
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"dict","data":[]}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"list","data":"nope"}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"list","data":[1]}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert_eq!(
        provider
            .parse_models(r#"{"object":"list","data":[{"object":"model"}]}"#)
            .unwrap_err(),
        FixtureError::MissingField("data[0].id".to_owned())
    );
    assert_eq!(
        provider
            .parse_models(r#"{"object":"list","data":[]}"#)
            .unwrap(),
        ModelList {
            object: "list".to_owned(),
            data: vec![],
        }
    );
}

#[test]
fn parse_dispatches_paths_and_does_not_fetch() {
    let provider = row(ServerKind::LlamaServer);
    let ok = include_str!("../fixtures/health_ok.json");
    let models = include_str!("../fixtures/v1_models.json");

    assert!(matches!(
        provider.parse("/health", ok).unwrap(),
        ParsedBody::Health(_)
    ));
    assert!(matches!(
        provider.parse("/v1/health?x=1", ok).unwrap(),
        ParsedBody::Health(_)
    ));
    assert!(matches!(
        provider.parse("/health/", ok).unwrap(),
        ParsedBody::Health(_)
    ));
    assert!(matches!(
        provider.parse("/v1/models?reload=1", models).unwrap(),
        ParsedBody::Models(_)
    ));
    assert!(matches!(
        provider
            .parse("http://127.0.0.1:8080/health", ok)
            .unwrap_err(),
        FixtureError::UnknownPath(_)
    ));
    assert!(matches!(
        provider.parse("/completion", ok).unwrap_err(),
        FixtureError::UnknownPath(_)
    ));
    assert!(!provider.dials_network());
    assert!(!provider.requires_gpu());
}

#[test]
fn crate_source_has_no_client() {
    let src = include_str!("../src/lib.rs");
    for banned in [
        "TcpStream",
        "UdpSocket",
        "std::net",
        "reqwest",
        "hyper",
        "tokio",
        "std::process",
    ] {
        assert!(!src.contains(banned), "{banned}");
    }
}
