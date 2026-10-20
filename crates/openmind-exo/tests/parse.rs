// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use openmind_exo::{
    ExoProvider, FixtureError, HealthError, HealthState, ModelCard, ModelList, ParsedBody,
    EXO_PORT, LINUX_GPU_USED,
};

fn row() -> ExoProvider {
    ExoProvider::new("mac-studio", "http://127.0.0.1:52415").unwrap()
}

fn key_prefix() -> String {
    ['s', 'k', '-'].into_iter().collect()
}

#[test]
fn provider_row_does_not_dial_or_use_linux_gpu() {
    let provider = row();
    assert_eq!(provider.id(), "mac-studio");
    assert_eq!(provider.endpoint(), "http://127.0.0.1:52415");
    assert!(provider.speaks_openai());
    assert!(!provider.dials_network());
    assert!(!provider.requires_gpu());
    assert!(!provider.uses_linux_gpu());
    assert_eq!(LINUX_GPU_USED, provider.uses_linux_gpu());
    assert_eq!(EXO_PORT, 52415);
    assert_eq!(ExoProvider::HEALTH_PATH, "/health");
    assert_eq!(ExoProvider::MODELS_PATH, "/v1/models");
}

#[test]
fn rejects_blank_and_key_shaped_rows() {
    let blank = ExoProvider::new("  ", "http://127.0.0.1:52415");
    assert!(matches!(
        blank,
        Err(FixtureError::BadField { field, .. }) if field == "id"
    ));
    let spaced = ExoProvider::new("mac studio", "http://127.0.0.1:52415");
    assert!(matches!(spaced, Err(FixtureError::BadField { .. })));
    let keyed = format!("{}live", key_prefix());
    let err = ExoProvider::new(keyed, "http://127.0.0.1:52415");
    assert_eq!(err, Err(FixtureError::SecretShape("id")));
    let err = ExoProvider::new(
        "mac-studio",
        format!("http://127.0.0.1:52415/{}", key_prefix()),
    );
    assert_eq!(err, Err(FixtureError::SecretShape("endpoint")));
    let empty_endpoint = ExoProvider::new("mac-studio", "");
    assert!(matches!(
        empty_endpoint,
        Err(FixtureError::BadField { field, .. }) if field == "endpoint"
    ));
}

#[test]
fn health_ok_fixture() {
    let body = include_str!("../fixtures/health_ok.json");
    let health = row().parse_health(body).unwrap();
    assert!(health.is_ready());
    assert_eq!(health.state, HealthState::Ready);
    assert_eq!(health.status.as_deref(), Some("ok"));
    assert_eq!(health.node_id.as_deref(), Some("node-1234"));
    assert!(health.error.is_none());
}

#[test]
fn health_unavailable_fixture() {
    let body = include_str!("../fixtures/health_unavailable.json");
    let health = row().parse_health(body).unwrap();
    assert!(!health.is_ready());
    assert_eq!(health.state, HealthState::Unavailable);
    assert_eq!(health.status.as_deref(), Some("unavailable"));
    assert!(health.node_id.is_none());
    assert_eq!(
        health.error,
        Some(HealthError {
            code: Some(503),
            message: Some("cluster has no ready instance".to_owned()),
            kind: Some("server_error".to_owned()),
        })
    );
}

#[test]
fn health_ready_aliases_and_error_only() {
    let provider = row();
    let healthy = provider
        .parse_health(r#"{"status":"Healthy","extra":true}"#)
        .unwrap();
    assert!(healthy.is_ready());
    assert_eq!(healthy.status.as_deref(), Some("Healthy"));

    let down = provider
        .parse_health(r#"{"error":{"code":500,"message":"not ready","type":"server_error"}}"#)
        .unwrap();
    assert_eq!(down.state, HealthState::Unavailable);
    assert!(down.status.is_none());
    assert_eq!(down.error.unwrap().code, Some(500));

    let both = provider
        .parse_health(r#"{"status":"ok","error":{"message":"stale"}}"#)
        .unwrap();
    assert!(both.is_ready());
    assert_eq!(both.error.unwrap().message.as_deref(), Some("stale"));
}

#[test]
fn health_rejects_bad_shapes() {
    let provider = row();
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
        provider
            .parse_health(r#"{"node_id":"node-1"}"#)
            .unwrap_err(),
        FixtureError::HealthShape
    );
    assert!(matches!(
        provider.parse_health(r#"{"status":""}"#).unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider.parse_health(r#"{"status":1}"#).unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_health(r#"{"status":"ok","error":"nope"}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_health(r#"{"status":"ok","error":{"code":1.5}}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_health(r#"{"status":"ok","node_id":7}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
}

#[test]
fn models_fixture_keeps_exo_card_fields() {
    let body = include_str!("../fixtures/v1_models.json");
    let list = row().parse_models(body).unwrap();
    assert_eq!(list.data.len(), 2);
    assert_eq!(
        list.data[0],
        ModelCard {
            id: "mlx-community/Llama-3.2-1B-Instruct-4bit".to_owned(),
            object: "model".to_owned(),
            created: Some(1_735_689_600),
            owned_by: Some("exo".to_owned()),
            hugging_face_id: Some("mlx-community/Llama-3.2-1B-Instruct-4bit".to_owned()),
            name: Some("Llama-3.2-1B-Instruct-4bit".to_owned()),
            description: Some("Small MLX instruct model".to_owned()),
            context_length: Some(131_072),
            tags: vec!["local".to_owned()],
            storage_size_megabytes: Some(700),
            supports_tensor: Some(true),
            tasks: vec!["TextGeneration".to_owned()],
            is_custom: Some(false),
            family: Some("llama".to_owned()),
            quantization: Some("4bit".to_owned()),
            base_model: Some("Llama 3.2 1B Instruct".to_owned()),
            capabilities: vec!["text".to_owned()],
            reasoning_dialect: Some("none".to_owned()),
        }
    );
    let custom = &list.data[1];
    assert_eq!(custom.is_custom, Some(true));
    assert_eq!(custom.context_length, Some(0));
    assert_eq!(custom.description.as_deref(), Some(""));
    assert_eq!(custom.supports_tensor, Some(false));
    assert!(custom.tags.is_empty());
    assert!(custom.capabilities.is_empty());
}

#[test]
fn models_minimal_openai_object_leaves_extras_absent() {
    let body = include_str!("../fixtures/v1_models_minimal.json");
    let list = row().parse_models(body).unwrap();
    assert_eq!(
        list,
        ModelList {
            object: "list".to_owned(),
            data: vec![ModelCard {
                id: "mlx-community/Llama-3.2-1B-Instruct-4bit".to_owned(),
                object: "model".to_owned(),
                created: None,
                owned_by: None,
                hugging_face_id: None,
                name: None,
                description: None,
                context_length: None,
                tags: vec![],
                storage_size_megabytes: None,
                supports_tensor: None,
                tasks: vec![],
                is_custom: None,
                family: None,
                quantization: None,
                base_model: None,
                capabilities: vec![],
                reasoning_dialect: None,
            }],
        }
    );
}

#[test]
fn models_rejects_bad_shapes() {
    let provider = row();
    assert_eq!(provider.parse_models("").unwrap_err(), FixtureError::Empty);
    assert_eq!(
        provider.parse_models("[]").unwrap_err(),
        FixtureError::NotAnObject
    );
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
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"list","data":[{"id":"x","object":"list"}]}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"list","data":[{"id":"","object":"model"}]}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"list","data":[{"id":"x","object":"model","created":-1}]}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(
                r#"{"object":"list","data":[{"id":"x","object":"model","context_length":-1}]}"#
            )
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(r#"{"object":"list","data":[{"id":"x","object":"model","tags":[1]}]}"#)
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
    assert!(matches!(
        provider
            .parse_models(
                r#"{"object":"list","data":[{"id":"x","object":"model","is_custom":"yes"}]}"#
            )
            .unwrap_err(),
        FixtureError::BadField { .. }
    ));
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
    let provider = row();
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
        provider
            .parse("/v1/models?status=downloaded", models)
            .unwrap(),
        ParsedBody::Models(_)
    ));
    assert!(matches!(
        provider.parse("/models", models).unwrap(),
        ParsedBody::Models(_)
    ));
    assert!(matches!(
        provider
            .parse("http://127.0.0.1:52415/health", ok)
            .unwrap_err(),
        FixtureError::UnknownPath(_)
    ));
    assert!(matches!(
        provider.parse("/v1/chat/completions", ok).unwrap_err(),
        FixtureError::UnknownPath(_)
    ));
    assert!(!provider.dials_network());
    assert!(!provider.requires_gpu());
    assert!(!provider.uses_linux_gpu());
}

#[test]
fn crate_source_has_no_client_and_names_the_linux_rule() {
    let src = include_str!("../src/lib.rs");
    for banned in [
        "TcpStream",
        "UdpSocket",
        "std::net",
        "reqwest",
        "hyper",
        "tokio",
        "std::process",
        "ureq",
    ] {
        assert!(!src.contains(banned), "{banned}");
    }
    assert!(src.contains("Exo GPU on Linux is not used here"));
    let doc = include_str!("../../../docs/exo-adapter.md");
    assert!(doc.contains("Exo GPU on Linux is not used here."));
    assert!(doc.contains("openmind.B37"));
}
