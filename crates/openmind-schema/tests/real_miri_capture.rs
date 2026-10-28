// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-data check for the Miri pass (slot 190): the live nerve `/events`
//! reply (captured from the VPS with a read-only `curl -s -i` GET) must
//! deserialize as `Vec<NerveEvent>` (Zagreb stamps enforced), and every JSON
//! body in the capture dir must parse with serde_json and re-serialize.
//!
//! Gated by `GALION_MIRI_CAPTURE=<dir of *.http files>`; skipped otherwise.
//! Under Miri it needs the FS pass (`MIRIFLAGS=-Zmiri-disable-isolation`).

use openmind_schema::NerveEvent;

fn body(raw: &[u8]) -> &[u8] {
    let i = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("head end");
    &raw[i + 4..]
}

#[test]
fn live_nerve_events_and_bodies_parse() {
    let Ok(dir) = std::env::var("GALION_MIRI_CAPTURE") else {
        eprintln!("skipped: set GALION_MIRI_CAPTURE=<dir>");
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "http"))
        .collect();
    files.sort();
    let mut events = 0;
    let mut json = 0;
    for f in &files {
        let raw = std::fs::read(f).unwrap();
        let b = body(&raw);
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        if name.contains("events") {
            let list: Vec<NerveEvent> =
                serde_json::from_slice(b).unwrap_or_else(|e| panic!("{name}: {e}"));
            for ev in &list {
                let back = serde_json::to_string(ev).unwrap();
                let again: NerveEvent = serde_json::from_str(&back).unwrap();
                assert_eq!(&again, ev);
            }
            events += list.len();
        }
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(b) {
            let text = serde_json::to_string(&v).unwrap();
            let again: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert_eq!(again, v, "{name}");
            json += 1;
        }
    }
    assert!(events > 0, "no nerve events in {dir}");
    eprintln!(
        "{} captures, {json} JSON bodies, {events} nerve events ok",
        files.len()
    );
}
