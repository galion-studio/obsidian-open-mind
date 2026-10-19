// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

use openmind_tulu::{FixtureError, TuluProvider, LLAMA_SERVER_BIN, LLAMA_SERVER_PORT, USES_GPU};

fn key_prefix() -> String {
    ['s', 'k', '-'].into_iter().collect()
}

fn row_text(gpu_line: &str) -> String {
    format!(
        "id = \"dr-tulu\"\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"dr-tulu-8b\"\n{gpu_line}"
    )
}

#[test]
fn fixture_sets_gpu_false_and_uses_gpu_stays_false() {
    let text = include_str!("../fixtures/llama-server.toml");
    assert!(text.contains("gpu = false"));
    let row = TuluProvider::parse(text).unwrap();
    assert_eq!(row.id(), "dr-tulu");
    assert_eq!(row.endpoint(), "http://127.0.0.1:8095");
    assert!(row.endpoint().contains(&LLAMA_SERVER_PORT.to_string()));
    assert_eq!(row.model_id(), "dr-tulu-8b");
    assert!(!row.gpu_flag());
    assert!(!row.uses_gpu());
    assert_eq!(row.uses_gpu(), USES_GPU);
    assert!(!row.dials_network());
    assert!(!row.spawns_process());
    assert!(!row.vendors_upstream());
    assert!(row.speaks_openai());
    assert_eq!(LLAMA_SERVER_BIN, "llama-server");
    assert!(!LLAMA_SERVER_BIN.contains('/'));
}

#[test]
fn omitted_gpu_defaults_false() {
    let row = TuluProvider::parse(&row_text("")).unwrap();
    assert!(!row.gpu_flag());
    assert!(!row.uses_gpu());
    assert!(!row.spawns_process());
    assert!(!row.dials_network());
}

#[test]
fn explicit_gpu_true_is_recorded_and_still_unused() {
    let row = TuluProvider::parse(&row_text("gpu = true\n")).unwrap();
    assert!(row.gpu_flag());
    assert!(!row.uses_gpu());
    assert_eq!(row.uses_gpu(), USES_GPU);
    assert!(!row.spawns_process());
    assert!(!row.dials_network());
    assert!(!row.vendors_upstream());
}

#[test]
fn rejects_blank_shapes_and_key_shaped_tokens() {
    assert_eq!(TuluProvider::parse("   ").unwrap_err(), FixtureError::Empty);
    assert!(matches!(
        TuluProvider::parse("id = ").unwrap_err(),
        FixtureError::Toml(_)
    ));
    assert!(matches!(
        TuluProvider::parse("true").unwrap_err(),
        FixtureError::Toml(_)
    ));
    assert_eq!(
        TuluProvider::parse("# comment only\n").unwrap_err(),
        FixtureError::MissingField("id".to_owned())
    );
    assert_eq!(
        TuluProvider::parse("id = \"dr-tulu\"\n").unwrap_err(),
        FixtureError::MissingField("endpoint".to_owned())
    );
    assert_eq!(
        TuluProvider::parse("id = \"dr-tulu\"\nendpoint = \"http://127.0.0.1:8095\"\n")
            .unwrap_err(),
        FixtureError::MissingField("model_id".to_owned())
    );
    assert_eq!(
        TuluProvider::parse(&format!("{}\nport = 8095\n", row_text("gpu = false\n"))).unwrap_err(),
        FixtureError::UnknownField("port".to_owned())
    );
    assert!(matches!(
        TuluProvider::parse(&row_text("gpu = 1\n")).unwrap_err(),
        FixtureError::BadField { field, .. } if field == "gpu"
    ));
    assert!(matches!(
        TuluProvider::parse(&row_text("gpu = \"false\"\n")).unwrap_err(),
        FixtureError::BadField { field, .. } if field == "gpu"
    ));
    assert!(matches!(
        TuluProvider::parse("id = 1\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"dr-tulu-8b\"\n")
            .unwrap_err(),
        FixtureError::BadField { field, .. } if field == "id"
    ));
    assert!(matches!(
        TuluProvider::parse("id = \"\"\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"dr-tulu-8b\"\n")
            .unwrap_err(),
        FixtureError::BadField { field, .. } if field == "id"
    ));
    assert!(matches!(
        TuluProvider::parse("id = \"dr tulu\"\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"dr-tulu-8b\"\n")
            .unwrap_err(),
        FixtureError::BadField { field, .. } if field == "id"
    ));
    assert!(matches!(
        TuluProvider::parse("id = \"dr-tulu\"\nendpoint = \"\"\nmodel_id = \"dr-tulu-8b\"\n")
            .unwrap_err(),
        FixtureError::BadField { field, expected } if field == "endpoint" && expected == "a non-empty endpoint"
    ));
    assert!(matches!(
        TuluProvider::parse("id = \"dr-tulu\"\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"\"\n")
            .unwrap_err(),
        FixtureError::BadField { field, expected } if field == "model_id" && expected == "a non-empty model id"
    ));
    assert!(matches!(
        TuluProvider::parse("id = \"dr-tulu\"\nendpoint = \"http://127.0.0.1:8095/a b\"\nmodel_id = \"dr-tulu-8b\"\n")
            .unwrap_err(),
        FixtureError::BadField { field, .. } if field == "endpoint"
    ));

    let keyed = format!("{}live", key_prefix());
    assert_eq!(
        TuluProvider::parse(&format!(
            "id = \"{keyed}\"\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"dr-tulu-8b\"\n"
        ))
        .unwrap_err(),
        FixtureError::SecretShape("id")
    );
    assert_eq!(
        TuluProvider::parse(&format!(
            "id = \"dr-tulu\"\nendpoint = \"http://127.0.0.1:8095/{keyed}\"\nmodel_id = \"dr-tulu-8b\"\n"
        ))
        .unwrap_err(),
        FixtureError::SecretShape("endpoint")
    );
    assert_eq!(
        TuluProvider::parse(&format!(
            "id = \"dr-tulu\"\nendpoint = \"http://127.0.0.1:8095\"\nmodel_id = \"{keyed}\"\n"
        ))
        .unwrap_err(),
        FixtureError::SecretShape("model_id")
    );
    let shown = FixtureError::SecretShape("model_id").to_string();
    assert!(shown.contains("model_id"));
    assert!(!shown.contains(&keyed));
}

#[test]
fn crate_source_has_no_process_socket_or_vendor() {
    let src = include_str!("../src/lib.rs");
    for banned in [
        "std::process",
        "std::net",
        "std::fs",
        "TcpStream",
        "TcpListener",
        "UdpSocket",
        "reqwest",
        "hyper",
        "tokio",
        "ureq",
        "Command",
        ".spawn(",
        "libc",
        "llama.cpp",
        "/workspace/",
        "curl",
    ] {
        assert!(!src.contains(banned), "{banned}");
    }
    assert!(src.contains("fn uses_gpu"));
    assert!(src.contains("USES_GPU"));
    assert!(src.contains("always false"));
}
