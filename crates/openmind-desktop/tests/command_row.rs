// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The command row's untested paths: Enter, a blank field, and the four
//! words that return before a kept command is marked.

use openmind_desktop::{document, LAYERS};

fn script() -> &'static str {
    let html = document();
    let start = html.find("<script>").expect("script") + "<script>".len();
    let end = start + html[start..].find("</script>").expect("script end");
    &html[start..end]
}

fn slice<'a>(src: &'a str, from: &str, to: &str) -> &'a str {
    let start = src.find(from).unwrap_or_else(|| panic!("missing {from}"));
    let after = start + from.len();
    let stop = src[after..]
        .find(to)
        .unwrap_or_else(|| panic!("missing {to} after {from}"));
    &src[start..after + stop]
}

fn before(hay: &str, left: &str, right: &str) {
    let l = hay.find(left).unwrap_or_else(|| panic!("missing {left}"));
    let r = hay.find(right).unwrap_or_else(|| panic!("missing {right}"));
    assert!(l < r, "{left} should come before {right}");
}

#[test]
fn the_command_row_stays_staged() {
    let layer = LAYERS
        .iter()
        .find(|layer| layer.id == "command")
        .expect("command row");
    assert_eq!(layer.name, "Command");
    assert_eq!(layer.state, "staged");
    assert!(layer.role.contains("Centre field"));
    let html = document();
    let at = html.find("data-layer=\"command\"").expect("row");
    let row = &html[at..at + 240];
    assert!(row.contains("<span class=\"state\">staged</span>"));
    assert!(row.contains("<span class=\"seen\"></span>"));
    assert!(row.contains("Centre field"));
    let field = html.find("id=\"command\"").expect("field");
    let kept = html.find("id=\"kept\"").expect("kept");
    assert!(field < kept);
    assert!(html[field..field + 120].contains("aria-label=\"Command\""));
}

#[test]
fn enter_keeps_the_typed_text_and_the_four_words_return() {
    let body = slice(
        script(),
        "command.addEventListener(\"keydown\"",
        "function probeWord",
    );
    before(body, "ev.key !== \"Enter\"", "ev.preventDefault()");
    before(body, "return;", "ev.preventDefault()");
    before(
        body,
        "command.value.replace(/^\\s+|\\s+$/g, \"\")",
        "if (!text)",
    );
    before(
        body,
        "if (!text) { play(\"deny\"); return; }",
        "createElement",
    );
    before(body, "row.textContent = text", "text.toLowerCase()");
    before(body, "kept.appendChild(row)", "command.value = \"\"");
    before(body, "command.value = \"\"", "text.toLowerCase()");
    before(body, "word === \"look\"", "word === \"files\"");
    before(body, "word === \"files\"", "word === \"voice\"");
    before(body, "word === \"voice\"", "word === \"mute\"");
    before(
        body,
        "runLook(); return;",
        "markSeen(\"command\", \"kept\")",
    );
    before(
        body,
        "files.click(); return;",
        "markSeen(\"command\", \"kept\")",
    );
    before(
        body,
        "toggleVoice(); return;",
        "markSeen(\"command\", \"kept\")",
    );
    before(
        body,
        "toggleMute(); return;",
        "markSeen(\"command\", \"kept\")",
    );
    before(body, "markSeen(\"command\", \"kept\")", "play(\"done\")");
    assert!(!body.contains("state.textContent"));
    assert!(!body.contains("innerHTML"));
}
