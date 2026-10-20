// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Parse talk fixtures in memory. No socket. No audio playback.

#![forbid(unsafe_code)]

use one_talk::{
    b64_encode, chat_content, chat_request, empty_reply, fault_reply, hear_multipart, hear_reply,
    spoken_line, synth_request, transcript_text, voice_setting, wait_reply, warm_request_body,
    TALK_LOCK,
};
use serde_json::{json, Value};
use std::fs;
use std::time::SystemTime;

#[test]
fn talk_fixtures_parse_without_socket_or_playback() {
    let before_fd = fd_targets();
    let before_sockets = socket_fds(&before_fd);
    assert!(!before_fd.is_empty(), "proc fd table was not readable");
    let before_children = child_pids();
    let before_sound = sound_fds(&before_fd);
    let lock_before = lock_stamp();

    let doc: Value = serde_json::from_str(include_str!("../fixtures/talk.json"))
        .expect("talk fixture must be json");

    let hear_bytes = serde_json::to_vec(&doc["hear_transcript"]).unwrap();
    let heard = transcript_text(&hear_bytes);
    assert_eq!(heard, "Hello there. Next bit.");
    assert_eq!(hear_reply(&heard), json!({"say": "Hello there. Next bit."}));

    let chat_bytes = serde_json::to_vec(&doc["chat_completion"]).unwrap();
    let content = chat_content(&chat_bytes).expect("chat fixture");
    assert_eq!(content, "Yes, I am here. And more follows.");
    assert_eq!(spoken_line(&content), "Yes, I am here.");

    let reply = &doc["reply_turn"];
    assert_eq!(reply["heard"], "good evening");
    assert_eq!(reply["say"], "Yes, I am here.");
    let audio = reply["audio"].as_str().expect("audio text");
    assert_eq!(audio, b64_encode(b"FAKEWAV"));
    assert!(audio.is_ascii());
    assert!(!audio.contains('/'));
    assert!(!audio.contains(".wav"));

    assert_eq!(doc["empty_reply"], empty_reply());
    assert_eq!(doc["fault_reply"], fault_reply());
    assert_eq!(doc["wait_reply"], wait_reply("good evening"));

    let warm: Value = serde_json::from_slice(&warm_request_body()).unwrap();
    assert_eq!(doc["warm_body"], warm);
    assert_eq!(warm["voice"], "samantha");
    assert_eq!(warm["text"], "Yes.");

    let channel = doc["sample"]["channel"].as_str().unwrap();
    let sample_heard = doc["sample"]["heard"].as_str().unwrap();
    let built = chat_request(&voice_setting(channel), sample_heard);
    let built_value: Value = serde_json::from_slice(&built).unwrap();
    assert_eq!(built_value["model"], doc["sample"]["model"]);
    assert_eq!(built_value["stream"], false);
    assert_eq!(built_value["max_tokens"], 80);
    assert_eq!(built_value["messages"][1]["content"], sample_heard);
    assert_eq!(
        built_value["messages"][0]["content"],
        voice_setting(channel)
    );

    let say = doc["reply_turn"]["say"].as_str().unwrap();
    let synth: Value = serde_json::from_slice(&synth_request(say)).unwrap();
    assert_eq!(synth["text"], say);
    assert_eq!(synth["voice"], "samantha");

    let stub = doc["speech_stub"].as_str().unwrap().as_bytes();
    let (kind, body) = hear_multipart(stub);
    assert_eq!(kind, "multipart/form-data; boundary=onebound");
    assert!(body.windows(stub.len()).any(|window| window == stub));
    assert!(!body.windows(4).any(|window| window == b"WAVE"));

    let after_fd = fd_targets();
    assert_eq!(before_sockets, socket_fds(&after_fd));
    assert_eq!(before_sound, sound_fds(&after_fd));
    assert_eq!(before_fd, after_fd);
    assert_eq!(before_children, child_pids());
    assert_eq!(lock_before, lock_stamp());
}

fn lock_stamp() -> Option<(u64, SystemTime)> {
    let meta = fs::metadata(TALK_LOCK).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

fn socket_fds(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| target.starts_with("socket:"))
        .cloned()
        .collect()
}

fn sound_fds(targets: &[String]) -> Vec<String> {
    targets
        .iter()
        .filter(|target| {
            target.contains("/dev/snd")
                || target.contains("/dev/dsp")
                || target.contains("/dev/audio")
        })
        .cloned()
        .collect()
}

fn fd_targets() -> Vec<String> {
    let mut names = Vec::new();
    let dir = fs::read_dir("/proc/self/fd").expect("fd");
    for entry in dir.flatten() {
        if let Ok(target) = fs::read_link(entry.path()) {
            names.push(target.to_string_lossy().into_owned());
        }
    }
    names.sort();
    names
}

fn child_pids() -> Vec<String> {
    let mut kids = Vec::new();
    let dir = fs::read_dir("/proc/self/task").expect("task");
    for entry in dir.flatten() {
        let Ok(text) = fs::read_to_string(entry.path().join("children")) else {
            continue;
        };
        kids.extend(text.split_whitespace().map(str::to_owned));
    }
    kids.sort();
    kids
}
