// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Print the warm lock path and the hear/reply/synthesize shapes.
//! Dials nothing. Opens no files. Reads no secrets.

use one_talk::{
    chat_request, synth_request, voice_setting, warm_request_body, HEAR_PATH, REPLY_PATH,
    SYNTH_PATH, TALK_LOCK,
};
use serde_json::{json, Value};
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    if env::args().len() != 1 {
        eprintln!("usage: one-talk");
        eprintln!("prints the warm lock path and helper JSON shapes; dials nothing");
        return ExitCode::from(2);
    }
    let doc = json!({
        "lock": TALK_LOCK,
        "hear": HEAR_PATH,
        "reply": REPLY_PATH,
        "synthesize": SYNTH_PATH,
        "warm": parse_own(&warm_request_body()),
        "sample_chat": parse_own(&chat_request(&voice_setting(""), ".")),
        "sample_synth": parse_own(&synth_request("Yes.")),
    });
    let mut stdout = io::stdout().lock();
    if serde_json::to_writer(&mut stdout, &doc).is_err() {
        return ExitCode::from(1);
    }
    if stdout.write_all(b"\n").is_err() {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn parse_own(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}
