// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Key order regression (swarm/serde-order-pin): `render` keeps the
//! document's own key order, like `json.dumps(..., indent=2)` in
//! set-model.py. This must hold for `cargo test -p one-set-model` alone, not
//! only inside a `--workspace` run where another crate switched serde_json's
//! `preserve_order` on.

use one_set_model::render;

#[test]
fn render_keeps_document_key_order() {
    let doc: serde_json::Value =
        serde_json::from_str(r#"{"zeta":1,"alpha":{"y":true,"b":null},"mid":"x"}"#).unwrap();
    assert_eq!(
        render(&doc).unwrap(),
        "{\n  \"zeta\": 1,\n  \"alpha\": {\n    \"y\": true,\n    \"b\": null\n  },\n  \"mid\": \"x\"\n}\n"
    );
}
