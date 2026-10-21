// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// Real-life checks against the wired box stack started by
// crates/galion-realtest/tests/real_sh_desk_linkwatch.rs (release galion-desk on 19180, release
// desk-hermes on a fixture Hermes home, release link-watch writing link.json
// after fetching public pages, hermes-gateway --mock on 19182, galion-one on
// 19181/19183). Ignored by default; the script runs them with:
//
//   GALION_REAL=1 GALION_REAL_DESK=127.0.0.1:19180 GALION_REAL_STATE=<run>/state
//
// The response shape is compared with the shape of the LIVE desk's /desk
// (VPS, Oct 10 build) recorded read-only on 2026-10-20 in
// tests/data/live-desk-shape-20261020.json (types only, no values).

mod support;

use serde_json::Value;
use support::*;

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

fn stack() -> Option<(String, std::path::PathBuf)> {
    if env("GALION_REAL").as_deref() != Some("1") {
        println!("SKIP: GALION_REAL=1 not set");
        return None;
    }
    let desk = env("GALION_REAL_DESK").expect("GALION_REAL_DESK");
    let state = std::path::PathBuf::from(env("GALION_REAL_STATE").expect("GALION_REAL_STATE"));
    Some((desk, state))
}

/// Type shape like the recorded live file: objects by key, arrays by their
/// first element, scalars by type name.
fn shape(v: &Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(m.iter().map(|(k, v)| (k.clone(), shape(v))).collect()),
        Value::Array(a) => Value::Array(a.first().map(shape).into_iter().collect()),
        Value::String(_) => "string".into(),
        Value::Number(_) => "number".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Null => "null".into(),
    }
}

/// Paths present in `a` but missing in `b` (array element shapes compared
/// only when both sides have one).
fn missing(a: &Value, b: &Value, at: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, v) in x {
                let p = format!("{at}.{k}");
                match y.get(k) {
                    Some(w) => missing(v, w, &p, out),
                    None => out.push(p),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if let (Some(v), Some(w)) = (x.first(), y.first()) {
                missing(v, w, &format!("{at}[]"), out);
            }
        }
        (x, y) if x != y => out.push(format!("{at}: {x} vs {y}")),
        _ => {}
    }
}

#[test]
#[ignore = "needs the wired stack: crates/galion-realtest/tests/real_sh_desk_linkwatch.rs"]
fn real_desk_matches_the_live_shape() {
    let Some((desk, _)) = stack() else { return };
    let snap = json(&get(&desk, "/desk"));
    let live: Value =
        serde_json::from_str(include_str!("data/live-desk-shape-20261020.json")).unwrap();
    let ours = shape(&snap);
    let mut gone = Vec::new();
    missing(&live, &ours, "", &mut gone);
    // `events` and some mindpack arrays may be empty on either side; only
    // keys present on both sides are type-compared.
    assert!(
        gone.is_empty(),
        "live keys missing or retyped on the box desk: {gone:?}"
    );
    let mut extra = Vec::new();
    missing(&ours, &live, "", &mut extra);
    assert!(
        extra.is_empty(),
        "box desk has keys the live desk lacks: {extra:?}"
    );
}

#[test]
#[ignore = "needs the wired stack: crates/galion-realtest/tests/real_sh_desk_linkwatch.rs"]
fn real_desk_is_wired_to_link_watch_hermes_one_and_desk_hermes() {
    let Some((desk, state)) = stack() else { return };
    let snap = json(&get(&desk, "/desk"));
    assert_eq!(snap["foundation"], "rust");
    assert_eq!(snap["hermes"], true, "hermes-gateway on 19182");
    assert_eq!(snap["one"], true, "galion-one mouth on 19183");
    assert_eq!(snap["line"], false, "no Ollama on the box");
    assert_eq!(snap["tunnel"], false);
    let up: Vec<String> = snap["services"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["up"] == true)
        .map(|s| s["name"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(up, ["Hermes", "One"]);

    // desk-hermes ran on the fixture home (3 sessions, 5 messages).
    assert_eq!(snap["mindpack"]["mind"]["sessions"], 3);
    assert_eq!(snap["mindpack"]["mind"]["messages"], 5);
    assert_eq!(snap["mindpack"]["tasks"][0]["id"], "one-galion");

    // link-watch wrote link.json; the desk board reads it.
    let link: Value =
        serde_json::from_slice(&std::fs::read(state.join("link.json")).unwrap()).unwrap();
    assert_eq!(link["hermes"], true, "link-watch saw hermes-gateway");
    assert_eq!(link["laptop_port"], false);
    assert_eq!(snap["board"]["studio"], "Box test laptop");
    assert_eq!(snap["board"]["gpu"], "RTX 4060, 8188 MiB");
    assert_eq!(snap["board"]["laptop_ram"], "20 / 32 GB");
    assert_eq!(snap["board"]["agents"], 2);
    assert_eq!(snap["board"]["version"], "box-real");
    assert_eq!(snap["board"]["france"], false, "france is unconfigured");

    // The public pages were really fetched over HTTPS.
    let urls = link["urls"].as_array().expect("urls key");
    assert!(!urls.is_empty());
    for u in urls {
        let url = u["url"].as_str().unwrap();
        assert!(url.starts_with("https://"), "{url}");
        assert_eq!(u["ok"], true, "{u}");
        assert!((200..400).contains(&u["status"].as_u64().unwrap()), "{u}");
    }
}

#[test]
#[ignore = "needs the wired stack: crates/galion-realtest/tests/real_sh_desk_linkwatch.rs"]
fn real_desk_note_round_trip_and_routes() {
    let Some((desk, state)) = stack() else { return };
    let text = format!("real test note {}", std::process::id());
    let after = json(&post(
        &desk,
        &format!(r#"{{"act":"note","text":"{text}"}}"#),
    ));
    let notes = after["notes"].as_array().unwrap();
    assert_eq!(notes.last().unwrap()["text"], text.as_str());
    let file = std::fs::read_to_string(state.join("desk-notes.jsonl")).unwrap();
    assert!(file.contains(&text));
    let dropped = json(&post(&desk, r#"{"act":"drop"}"#));
    assert!(dropped["notes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|n| n["text"] != text.as_str()));
    // Same route matrix as the live desk.
    for (path, code) in [
        ("/", "404"),
        ("/health", "200"),
        ("/healthz", "200"),
        ("/desk/", "200"),
        ("/deskx", "200"),
        ("/DESK", "404"),
    ] {
        let resp = get(&desk, path);
        assert!(
            resp.starts_with(&format!("HTTP/1.1 {code}")),
            "{path}: {resp}"
        );
    }
}
