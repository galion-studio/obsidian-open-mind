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
    for needle in [
        "--c-bg: #0B0F14",
        "--c-surface: #131A22",
        "--c-elevated: #1B2633",
        "--c-fg: #E6EDF3",
        "--c-muted: #A7B6C4",
        "--c-subtle: #A7B6C4",
        "--c-line: #24303C",
        "--c-accent: #58C4FF",
        "--c-accent-fg: #0B0F14",
        "--c-ice: #B8E8FF",
        "--c-winter: #05070A",
        "--c-bg: #F7F9FB",
        "--c-accent: #0B5FA5",
        "--display: \"Jost\", var(--sans)",
        "--sans: \"IBM Plex Sans\"",
        "--mono: \"IBM Plex Mono\"",
        "linear-gradient(135deg,#B8E8FF,#58C4FF 55%,#0B5FA5)",
        "box-shadow: 0 0 0 3px #F7F9FB",
        ".one-dot",
    ] {
        assert!(css.contains(needle), "stylesheet missing {needle}");
    }
    assert!(!css.contains("@import"));
    assert!(!css.contains("fonts.google"));
    assert!(!css.contains("#4A7C99"), "slate is not a text colour");
    let light = css
        .split(".galion-console[data-theme=\"light\"]")
        .nth(1)
        .unwrap();
    let light = light.split(".galion-console .life-stage").next().unwrap();
    assert!(
        !light.contains("#58C4FF"),
        "sky is not a light-theme colour"
    );
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
