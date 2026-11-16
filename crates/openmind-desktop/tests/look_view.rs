// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Look's untested branches: the word for a probe, the early returns, and
//! which stack row receives the word. The page script is the program.

use openmind_desktop::{document, LAYERS, SERVICES};

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
fn probe_word_maps_enabled_and_state() {
    let body = slice(script(), "function probeWord(", "function clearSeen");
    before(body, "!item || !item.enabled", "return \"off\"");
    before(body, "return \"off\"", "item.state || \"unknown\"");
    before(
        body,
        "state !== \"up\" && state !== \"down\" && state !== \"unknown\"",
        "return \"unknown\"",
    );
    before(body, "return \"unknown\"", "return state");
}

#[test]
fn look_clears_seen_then_stops_or_reads_probes() {
    let body = slice(script(), "function runLook()", "look.addEventListener");
    before(body, "lookRows.textContent = \"\"", "clearSeen()");
    before(
        body,
        "clearSeen()",
        "location.protocol === \"file:\" || !loopbackPage()",
    );
    before(
        body,
        "Look only runs on the loopback listener.",
        "fetch(\"/probes\"",
    );
    before(body, "play(\"deny\")", "fetch(\"/probes\"");
    before(body, "markHere()", "fetch(\"/probes\"");
    assert!(body.contains("{ method: \"GET\", credentials: \"omit\" }"));
    assert!(!body.contains("POST"));
    assert!(!body.contains("innerHTML"));
    assert!(!body.contains("state.textContent"));

    before(body, "The local stack did not answer.", "JSON.parse(text)");
    before(
        body,
        "The probe report was not readable.",
        "data && data.probes",
    );
    before(
        body,
        "No probes were enabled. Rows stay staged or planned.",
        "var wants = false",
    );
    before(body, "play(\"notice\")", "var wants = false");
    before(body, "row.className = \"chip\"", "probeWord(item)");
    before(
        body,
        "String(item.service || \"service\") + \" — \" + word",
        "markSeen(String(item.service || \"\"), word)",
    );
    before(body, "item.enabled && word !== \"up\"", "wants = true");
    before(
        body,
        "The local stack wants a look. Rows stay staged or planned.",
        "The local stack answered. Rows stay staged or planned.",
    );
    assert!(body.contains("if (wants) play(\"notice\")"));
    assert!(body.contains(".catch(function ()"));
}

#[test]
fn look_writes_the_seen_span_of_the_matching_row() {
    let html = document();
    let mark = slice(script(), "function markSeen(", "function clearSeenWord");
    before(mark, "data-layer", ".seen");
    assert!(mark.contains("mark.textContent = word"));
    assert!(!mark.contains("state"));
    let clear = slice(script(), "function clearSeen()", "function markSeen");
    assert!(clear.contains("mark.textContent = \"\""));
    assert!(!clear.contains("class=\"state\""));

    for service in SERVICES {
        let id = service.as_str();
        let has_row = LAYERS.iter().any(|layer| layer.id == id);
        let marker = format!("data-layer=\"{id}\"");
        assert_eq!(html.contains(&marker), has_row, "{id}");
    }
    assert!(!html.contains("data-layer=\"cortex\""));
    let look = html.find("id=\"look\"").expect("look");
    let rows = html.find("id=\"look-rows\"").expect("rows");
    assert!(look < rows);
    assert!(html[rows..rows + 80].contains("aria-label=\"Local stack probes\""));
}
