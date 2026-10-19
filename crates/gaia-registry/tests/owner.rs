// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Acceptance: galion-one maps to one function, an unknown crate maps to none.

use gaia_registry::{owner, Function, TABLE};

#[test]
fn galion_one_maps_to_one_function() {
    assert_eq!(owner("galion-one"), Some(Function::Eleuthia));
}

#[test]
fn unknown_crate_maps_to_none() {
    assert!(owner("unknown-crate").is_none());
}

#[test]
fn doc_lists_each_crate_once_with_the_same_function() {
    let doc = include_str!("../../../docs/gaia-ownership.md");
    let mut rows = Vec::new();
    for line in doc.lines() {
        let Some(rest) = line.trim().strip_prefix("| `") else {
            continue;
        };
        let Some((name, after)) = rest.split_once("` | ") else {
            continue;
        };
        let function = after
            .split(" | ")
            .next()
            .expect("function column")
            .trim()
            .trim_end_matches('|')
            .trim();
        rows.push((name, function));
    }
    assert_eq!(rows.len(), TABLE.len());
    for (row, (name, function)) in TABLE.iter().zip(rows.iter()) {
        assert_eq!(row.crate_name, *name);
        assert_eq!(row.function.name(), *function);
        let token = format!("`{name}`");
        assert_eq!(doc.matches(token.as_str()).count(), 1, "{token}");
    }
}
