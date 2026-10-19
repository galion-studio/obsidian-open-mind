// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Hear, reply, and warm HTTP shapes for the voice helpers.
//!
//! `galion-light/one/talk.py` has no caller in `crates/`. This crate ports only
//! the hear/reply request bytes and the JSON those commands print. It does not
//! port pairing. It does not read `ONE_COOKIE`. It does not open
//! `admin-device.json`, the Hermes env file, or any other secret. It does not
//! append the channel log. It does not open Hermes `state.db`.
//!
//! `galion-light/one/warm.py` flocks `/tmp/one-talk.lock` and posts a synthesize
//! body for the text "Yes." with voice samantha. [`TALK_LOCK`] and
//! [`warm_request_body`] record that contract. galion-one already runs its own
//! `keep_warm` loop; this crate does not replace that loop and does not open
//! the live lock file.
//!
//! Dialing is limited to a loopback address that is not a live voice port.
//! Tests point at a spare listener. Nothing here has a default host or port.

use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

/// Lock path `warm.py` and `talk.py` flock (`LOCK_EX | LOCK_NB`).
/// Not opened by this crate.
pub const TALK_LOCK: &str = "/tmp/one-talk.lock";

/// Transcription route used by talk.py `hear`.
pub const HEAR_PATH: &str = "/v1/audio/transcriptions";

/// Chat route used by talk.py `reply`.
pub const REPLY_PATH: &str = "/v1/chat/completions";

/// Synthesize route used by talk.py `reply` and warm.py.
pub const SYNTH_PATH: &str = "/synthesize";

/// Multipart boundary shared with talk.py (`onebound`).
pub const BOUNDARY: &str = "onebound";

const WARM_TEXT: &str = "Yes.";
const WARM_VOICE: &str = "samantha";

const VOICE_BASE: &str = concat!(
    "Origin: galion-one. The admin is speaking, not typing in the desk. ",
    "You are the voice in the room. Samantha will read this aloud. ",
    "English. One short sentence. No lists. No markdown. No paths. No tools."
);

/// True when this crate must not open a TCP connection to `addr`.
///
/// Loopback only. Ports 8002, 8642, and 8643 are refused: they are live voice
/// services on this host, not destinations this helper may call.
pub fn dial_blocked(addr: SocketAddr) -> bool {
    !addr.ip().is_loopback() || matches!(addr.port(), 8002 | 8642 | 8643)
}

/// System text from talk.py `voice_setting`. `channel` is already-read channel
/// copy supplied by the caller. Empty or blank means the base sentence only.
/// This function does not open the channel file.
pub fn voice_setting(channel: &str) -> String {
    let channel = channel.trim();
    if channel.is_empty() {
        VOICE_BASE.to_string()
    } else {
        format!("{VOICE_BASE}\n{channel}")
    }
}

/// warm.py synthesize body: text "Yes.", voice samantha.
pub fn warm_request_body() -> Vec<u8> {
    to_body(&json!({"text": WARM_TEXT, "voice": WARM_VOICE}))
}

/// talk.py chat body: model hermes-agent, stream false, max_tokens 80.
pub fn chat_request(system: &str, heard: &str) -> Vec<u8> {
    to_body(&json!({
        "model": "hermes-agent",
        "stream": false,
        "max_tokens": 80,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": heard}
        ]
    }))
}

/// talk.py synthesize body for a reply line. Voice is always samantha.
pub fn synth_request(text: &str) -> Vec<u8> {
    to_body(&json!({"text": text, "voice": WARM_VOICE}))
}

/// Multipart body talk.py posts for `hear` (`file`, `language=en`, `fast=1`).
pub fn hear_multipart(wav: &[u8]) -> (String, Vec<u8>) {
    let mut payload = format!(
        "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"speech.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
    )
    .into_bytes();
    payload.extend_from_slice(wav);
    payload.extend_from_slice(
        format!(
            "\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nen\r\n--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"fast\"\r\n\r\n1\r\n--{BOUNDARY}--\r\n"
        )
        .as_bytes(),
    );
    (format!("multipart/form-data; boundary={BOUNDARY}"), payload)
}

/// `str(obj.get("text") or "").strip()[:400]` from talk.py `transcribe`.
pub fn transcript_text(body: &[u8]) -> String {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let Some(text) = value.get("text").and_then(Value::as_str) else {
        return String::new();
    };
    take_chars(text.trim(), 400)
}

/// First choice message content. `None` means the body is not a chat object
/// this helper can read (talk.py would fall through to the fault line).
pub fn chat_content(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    if !value.is_object() {
        return None;
    }
    match value.get("choices") {
        None | Some(Value::Null) => Some(String::new()),
        Some(Value::Array(choices)) if choices.is_empty() => Some(String::new()),
        Some(Value::Array(choices)) => {
            let message = choices.first().and_then(|row| row.get("message"));
            match message {
                None | Some(Value::Null) => Some(String::new()),
                Some(Value::Object(map)) => match map.get("content") {
                    Some(Value::String(text)) => Some(text.clone()),
                    None | Some(Value::Null) => Some(String::new()),
                    Some(_) => None,
                },
                Some(_) => None,
            }
        }
        Some(_) => None,
    }
}

/// talk.py `shorten`: collapse whitespace, keep the first sentence, cap at 140 chars.
pub fn shorten(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    take_chars(first_sentence(&collapsed), 140)
}

/// `shorten(text) or "Again"`.
pub fn spoken_line(content: &str) -> String {
    let line = shorten(content);
    if line.is_empty() {
        "Again".to_string()
    } else {
        line
    }
}

/// `{"say": ...}` printed by talk.py `hear`.
pub fn hear_reply(say: &str) -> Value {
    json!({"say": say})
}

/// Printed when reply's heard text is empty.
pub fn empty_reply() -> Value {
    json!({"say": "Again", "heard": "", "audio": ""})
}

/// Printed when the outer helper catches a failure. No `heard` key.
pub fn fault_reply() -> Value {
    json!({"say": "Again", "audio": ""})
}

/// Printed when the talk lock is already held. This crate does not take that lock.
pub fn wait_reply(heard: &str) -> Value {
    json!({"heard": heard, "say": "Wait", "audio": ""})
}

/// Standard base64, with padding, as talk.py uses for the `audio` field.
pub fn b64_encode(data: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= data.len() {
        let n = ((data[index] as u32) << 16)
            | ((data[index + 1] as u32) << 8)
            | (data[index + 2] as u32);
        push_quartet(&mut out, T, n);
        index += 3;
    }
    let rest = data.len() - index;
    if rest == 1 {
        let n = (data[index] as u32) << 16;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rest == 2 {
        let n = ((data[index] as u32) << 16) | ((data[index + 1] as u32) << 8);
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(T[((n >> 6) & 63) as usize] as char);
        out.push('=');
    }
    out
}

/// Post `wav` to `addr` `HEAR_PATH` and return the hear JSON object.
///
/// A blocked address is `PermissionDenied` and no socket is opened.
/// A transport or HTTP failure becomes [`fault_reply`], matching the helper's
/// outer catch. A 2xx body yields `{"say": <transcript>}`.
pub fn hear_exchange(addr: SocketAddr, wav: &[u8], timeout: Duration) -> io::Result<Value> {
    guard_addr(addr)?;
    let (content_type, body) = hear_multipart(wav);
    match post(
        addr,
        HEAR_PATH,
        &[("Content-Type", content_type.as_str())],
        &body,
        timeout,
    ) {
        Ok((code, resp)) if is_success(code) && serde_json::from_slice::<Value>(&resp).is_ok() => {
            Ok(hear_reply(&transcript_text(&resp)))
        }
        Ok(_) => Ok(fault_reply()),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => Err(err),
        Err(_) => Ok(fault_reply()),
    }
}

/// Run the reply shape against `addr`: chat, then synthesize.
///
/// Empty `heard` returns [`empty_reply`] and does not dial.
/// `bearer` is sent only as the caller supplied it. This function never loads a key.
/// `channel` is inlined into the system message. The channel file is not read.
/// Synthesize failure leaves `audio` empty and still returns the spoken line.
pub fn reply_exchange(
    addr: SocketAddr,
    heard: &str,
    channel: &str,
    bearer: &str,
    timeout: Duration,
) -> io::Result<Value> {
    if heard.is_empty() {
        return Ok(empty_reply());
    }
    guard_addr(addr)?;
    let system = voice_setting(channel);
    let chat = chat_request(&system, heard);
    let auth = format!("Bearer {bearer}");
    let (code, body) = match post(
        addr,
        REPLY_PATH,
        &[
            ("Content-Type", "application/json"),
            ("Authorization", auth.as_str()),
        ],
        &chat,
        timeout,
    ) {
        Ok(pair) => pair,
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => return Err(err),
        Err(_) => return Ok(fault_reply()),
    };
    if !is_success(code) {
        return Ok(fault_reply());
    }
    let Some(content) = chat_content(&body) else {
        return Ok(fault_reply());
    };
    let say = spoken_line(&content);
    let synth = synth_request(&say);
    let wav = match post(
        addr,
        SYNTH_PATH,
        &[("Content-Type", "application/json")],
        &synth,
        timeout,
    ) {
        Ok((synth_code, synth_body)) if is_success(synth_code) => synth_body,
        _ => Vec::new(),
    };
    let audio = if wav.is_empty() {
        String::new()
    } else {
        b64_encode(&wav)
    };
    Ok(json!({"heard": heard, "say": say, "audio": audio}))
}

/// Post the warm.py synthesize body. Returns the HTTP status.
/// Does not flock [`TALK_LOCK`] and does not retry.
pub fn warm_exchange(addr: SocketAddr, timeout: Duration) -> io::Result<u16> {
    guard_addr(addr)?;
    let body = warm_request_body();
    let (code, _) = post(
        addr,
        SYNTH_PATH,
        &[("Content-Type", "application/json")],
        &body,
        timeout,
    )?;
    Ok(code)
}

fn guard_addr(addr: SocketAddr) -> io::Result<()> {
    if !dial_blocked(addr) {
        return Ok(());
    }
    let message = if addr.ip().is_loopback() {
        "one-talk will not dial a live voice port"
    } else {
        "one-talk only dials loopback"
    };
    Err(io::Error::new(io::ErrorKind::PermissionDenied, message))
}

fn is_success(code: u16) -> bool {
    (200..300).contains(&code)
}

fn to_body(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec())
}

fn take_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

fn first_sentence(text: &str) -> &str {
    let mut end = text.len();
    for (idx, ch) in text.char_indices() {
        if matches!(ch, '.' | '!' | '?') {
            let after = idx + ch.len_utf8();
            if text[after..]
                .chars()
                .next()
                .is_some_and(|next| next.is_whitespace())
            {
                end = after;
                break;
            }
        }
    }
    &text[..end]
}

fn push_quartet(out: &mut String, alphabet: &[u8], n: u32) {
    out.push(alphabet[((n >> 18) & 63) as usize] as char);
    out.push(alphabet[((n >> 12) & 63) as usize] as char);
    out.push(alphabet[((n >> 6) & 63) as usize] as char);
    out.push(alphabet[(n & 63) as usize] as char);
}

fn post(
    addr: SocketAddr,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> io::Result<(u16, Vec<u8>)> {
    guard_addr(addr)?;
    for (name, value) in headers {
        if name.chars().any(|ch| ch == '\r' || ch == '\n' || ch == ':')
            || value.chars().any(|ch| ch == '\r' || ch == '\n')
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "header contains a line break",
            ));
        }
    }
    let mut stream = TcpStream::connect_timeout(&addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut head = format!(
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    read_response(&mut stream)
}

fn read_response(stream: &mut TcpStream) -> io::Result<(u16, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    loop {
        if buf.len() > 8_000_000 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response too large",
            ));
        }
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock
                    || err.kind() == io::ErrorKind::TimedOut =>
            {
                if buf.is_empty() {
                    return Err(err);
                }
                break;
            }
            Err(err) => return Err(err),
        }
        if let Some(pos) = header_end(&buf) {
            if let Some(len) = content_length(&buf[..pos]) {
                if buf.len() >= pos + 4 + len {
                    let code = status_code(&buf);
                    return Ok((code, buf[pos + 4..pos + 4 + len].to_vec()));
                }
            }
        }
    }
    let Some(pos) = header_end(&buf) else {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "no http header",
        ));
    };
    Ok((status_code(&buf), buf[pos + 4..].to_vec()))
}

fn header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(header: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(header);
    for line in text.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn status_code(buf: &[u8]) -> u16 {
    buf.split(|&byte| byte == b' ')
        .nth(1)
        .and_then(|token| std::str::from_utf8(token).ok())
        .and_then(|token| token.parse().ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn loopback(port: u16) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    }

    #[test]
    fn lock_path_matches_warm_py() {
        assert_eq!(TALK_LOCK, "/tmp/one-talk.lock");
        let body: Value = serde_json::from_slice(&warm_request_body()).unwrap();
        assert_eq!(body, json!({"text": "Yes.", "voice": "samantha"}));
        assert_eq!(body.as_object().unwrap().len(), 2);
    }

    #[test]
    fn live_voice_ports_are_blocked() {
        assert!(dial_blocked(loopback(8002)));
        assert!(dial_blocked(loopback(8642)));
        assert!(dial_blocked(loopback(8643)));
        assert!(dial_blocked("192.0.2.1:18002".parse().unwrap()));
        assert!(!dial_blocked(loopback(18002)));
        assert!(!dial_blocked(loopback(18643)));
    }

    #[test]
    fn exchange_refuses_non_loopback() {
        let addr: SocketAddr = "192.0.2.1:9".parse().unwrap();
        let timeout = Duration::from_millis(200);
        let err = hear_exchange(addr, b"x", timeout).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let err = reply_exchange(addr, "hi", "", "fixture", timeout).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let err = warm_exchange(addr, timeout).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn empty_heard_does_not_dial() {
        let addr: SocketAddr = "192.0.2.1:9".parse().unwrap();
        let value =
            reply_exchange(addr, "", "unused", "fixture", Duration::from_millis(50)).unwrap();
        assert_eq!(value, empty_reply());
    }

    #[test]
    fn voice_setting_matches_talk_py() {
        let base = voice_setting("  ");
        assert_eq!(
            base,
            "Origin: galion-one. The admin is speaking, not typing in the desk. You are the voice in the room. Samantha will read this aloud. English. One short sentence. No lists. No markdown. No paths. No tools."
        );
        assert_eq!(
            voice_setting("Stay brief.\n"),
            format!("{base}\nStay brief.")
        );
    }

    #[test]
    fn chat_body_shape_is_the_helper_not_the_live_stream() {
        let raw = chat_request(&voice_setting("Stay brief."), "say \"hi\"");
        let value: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["model"], "hermes-agent");
        assert_eq!(value["stream"], false);
        assert_eq!(value["max_tokens"], 80);
        assert_eq!(value["messages"].as_array().unwrap().len(), 2);
        assert_eq!(value["messages"][0]["role"], "system");
        assert!(value["messages"][0]["content"]
            .as_str()
            .unwrap()
            .ends_with("\nStay brief."));
        assert_eq!(value["messages"][1]["content"], "say \"hi\"");
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            ["max_tokens", "messages", "model", "stream"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>()
        );
        let text = String::from_utf8(raw).unwrap();
        assert!(!text.contains("admin-device"));
        assert!(!text.contains("state.db"));
    }

    #[test]
    fn shorten_and_transcript_follow_talk_py() {
        assert_eq!(shorten("  Hello   there.  Next sentence."), "Hello there.");
        assert_eq!(shorten("Hello.World stays"), "Hello.World stays");
        assert_eq!(shorten("Wait!"), "Wait!");
        assert_eq!(shorten("   "), "");
        assert_eq!(spoken_line("   "), "Again");
        let long = format!("{} next", "a".repeat(150));
        assert_eq!(shorten(&format!("{long}. tail")), "a".repeat(140));
        let padded = format!("  {}  ", "b".repeat(401));
        let body = serde_json::to_vec(&json!({"text": padded})).unwrap();
        assert_eq!(transcript_text(&body).chars().count(), 400);
        assert_eq!(transcript_text(br#"{"text":"  hi\nthere  "}"#), "hi\nthere");
        assert_eq!(transcript_text(br#"{"text":null}"#), "");
        assert_eq!(transcript_text(br"not-json"), "");
        assert_eq!(chat_content(br#"{"choices":[]}"#).unwrap(), "");
        assert_eq!(
            chat_content(br#"{"choices":[{"message":{"content":"Yes."}}]}"#).unwrap(),
            "Yes."
        );
        assert!(chat_content(br"not-json").is_none());
    }

    #[test]
    fn response_shapes_and_base64() {
        assert_eq!(hear_reply("hi"), json!({"say": "hi"}));
        assert_eq!(
            empty_reply(),
            json!({"say": "Again", "heard": "", "audio": ""})
        );
        assert_eq!(fault_reply(), json!({"say": "Again", "audio": ""}));
        assert_eq!(
            wait_reply("hi"),
            json!({"heard": "hi", "say": "Wait", "audio": ""})
        );
        assert_eq!(b64_encode(b""), "");
        assert_eq!(b64_encode(b"M"), "TQ==");
        assert_eq!(b64_encode(b"Ma"), "TWE=");
        assert_eq!(b64_encode(b"Man"), "TWFu");
        assert_eq!(b64_encode(b"FAKEWAV"), "RkFLRVdBVg==");
    }

    #[test]
    fn hear_multipart_matches_talk_py() {
        let (kind, body) = hear_multipart(b"RIFFtest");
        assert_eq!(kind, "multipart/form-data; boundary=onebound");
        let text_head = String::from_utf8_lossy(&body);
        assert!(text_head.contains("filename=\"speech.wav\""));
        assert!(text_head.contains("name=\"language\""));
        assert!(text_head.contains("name=\"fast\""));
        assert!(body.windows(8).any(|window| window == b"RIFFtest"));
        assert!(body.ends_with(b"--onebound--\r\n"));
    }
}
