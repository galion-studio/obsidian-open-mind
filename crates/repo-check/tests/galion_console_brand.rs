// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The TypeScript console IDE shell stays on the Galion brand and inside the
//! existing 8 KiB overview stylesheet budget. Galion One is the product.
//! Galion Voice is the voice model, not a second app.

use std::path::PathBuf;

fn repo_file(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

#[test]
fn console_css_fits_the_overview_budget() {
    let css = repo_file("../../stack/research-agent/console-ide/src/components/galion-console.css");
    let bytes = css.len();
    assert!(
        bytes <= 8 * 1024,
        "galion-console.css is {bytes} bytes; the overview sheet budget is 8 KiB"
    );
    for needle in ["#0b0f14", "#58c4ff", "#0b5fa5", "Jost", "IBM Plex Sans", ".one-dot"] {
        assert!(css.contains(needle), "stylesheet missing {needle}");
    }
}

#[test]
fn console_panel_names_one_product_and_research() {
    let panel = repo_file("../../stack/research-agent/console-ide/src/components/hermes-panel.tsx");
    for needle in [
        "galion-console.css",
        "Galion Research",
        "Galion One",
        "Speak to Galion One",
        "Ask Galion Research",
        "className=\"one-dot\"",
        "Not a separate app",
    ] {
        assert!(panel.contains(needle), "panel missing {needle}");
    }
    assert!(!panel.contains("Hermes Research"));
    assert!(!panel.contains("Open Mind Voice"));
    assert!(!panel.contains(">Galion Voice<"));
}
