// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// The local mouth (text to speech) as a small Rust interface.
//
// Galion One used to spawn the Python Chatterbox worker (tts/worker.py) and
// talk to it over stdin/stdout. That spawn is gone (B14). What is left is
// this seam: `Mouth` is what the :8002 server and the /voice route call.
// Today `local()` is the `Unavailable` stub. With no GALION_MOUTH set, :8002
// /health answers 503 `"status":"unavailable"` with reason "voice not deployed" (it used to
// claim a Chatterbox/CUDA model that was "loading" forever) and /synthesize
// returns {"error":"voice"} (500) at once.
//
// The CPU speech backend lives out of process: `galion-voice-serve`
// (crates/galion-voice-cpu, Whisper tiny.en + KittenTTS nano, licence-gated,
// no downloads) speaks both the ear contract (/v1/audio/transcriptions) and
// the mouth contract (/synthesize). Point GALION_MOUTH (and GALION_EAR) at it;
// :8002 /health then passes that service's health through. It stays out of
// this binary on purpose: it needs ~450 MB RSS and the galion-one unit is
// capped at MemoryMax=128M.

use std::io::{self, Read};

/// PCM format every backend returns: 16-bit little-endian mono at this rate.
pub const SAMPLE_RATE: u32 = 24_000;

/// Longest text a single synth call accepts (chars), as before.
pub const MAX_CHARS: usize = 300;

#[derive(Debug)]
pub enum SpeechError {
    /// No backend on this machine (the stub).
    Unavailable,
    /// A backend exists but this call failed.
    Failed(String),
}

pub trait Mouth: Send + Sync {
    /// Short backend name for logs and /health.
    fn name(&self) -> &'static str;
    /// True once the backend can take a synth call.
    fn ready(&self) -> bool;
    /// Speak `text` with voice `voice` (0 samantha, 1 jarvis). PCM chunks go to
    /// `sink` as they are produced; Ok(()) after the last one.
    fn synth(&self, voice: u8, text: &str, sink: &mut dyn FnMut(&[u8])) -> Result<(), SpeechError>;
}

/// The stub: no speech models on this box.
pub struct Unavailable;

impl Mouth for Unavailable {
    fn name(&self) -> &'static str {
        "unavailable"
    }
    fn ready(&self) -> bool {
        false
    }
    fn synth(&self, _: u8, _: &str, _: &mut dyn FnMut(&[u8])) -> Result<(), SpeechError> {
        Err(SpeechError::Unavailable)
    }
}

/// The backend Galion One uses for its local mouth.
pub fn local() -> &'static dyn Mouth {
    static STUB: Unavailable = Unavailable;
    &STUB
}

/// Collapse whitespace and cap the length, as the old worker path did.
pub fn normalize(text: &str) -> String {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    text.chars().take(MAX_CHARS).collect()
}

// Framed child protocol (the one tts/worker.py spoke), kept as pure functions
// so a future child-process backend can reuse it without Python:
//   ready:    the child writes one byte b'R'
//   request:  u32 LE length, then [voice byte][utf-8 text]
//   reply:    repeated u32 LE length + PCM bytes; length 0 ends the reply

/// Largest PCM frame accepted from a backend.
pub const MAX_FRAME: usize = 8_000_000;

pub fn frame_request(voice: u8, text: &str) -> Vec<u8> {
    let mut body = vec![voice];
    body.extend_from_slice(text.as_bytes());
    let mut msg = (body.len() as u32).to_le_bytes().to_vec();
    msg.extend_from_slice(&body);
    msg
}

pub fn read_ready(r: &mut impl Read) -> io::Result<()> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    if b[0] == b'R' {
        Ok(())
    } else {
        Err(io::Error::other("voice backend did not ready"))
    }
}

pub fn read_frames(r: &mut impl Read, sink: &mut dyn FnMut(&[u8])) -> io::Result<()> {
    loop {
        let mut len = [0u8; 4];
        r.read_exact(&mut len)?;
        let len = u32::from_le_bytes(len) as usize;
        if len == 0 {
            return Ok(());
        }
        if len > MAX_FRAME {
            return Err(io::Error::other("voice frame too large"));
        }
        let mut pcm = vec![0u8; len];
        r.read_exact(&mut pcm)?;
        sink(&pcm);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn stub_is_unavailable_and_fast() {
        let m = local();
        assert!(!m.ready());
        assert_eq!(m.name(), "unavailable");
        let t = std::time::Instant::now();
        let mut n = 0;
        assert!(matches!(
            m.synth(0, "hi", &mut |_| n += 1),
            Err(SpeechError::Unavailable)
        ));
        assert_eq!(n, 0);
        assert!(t.elapsed().as_millis() < 50);
    }

    #[test]
    fn normalize_caps_and_collapses() {
        assert_eq!(normalize("  a \n b\tc "), "a b c");
        assert_eq!(normalize(&"x".repeat(500)).len(), MAX_CHARS);
    }

    #[test]
    fn framed_protocol_round_trip() {
        assert_eq!(
            frame_request(1, "Yes."),
            vec![5, 0, 0, 0, 1, b'Y', b'e', b's', b'.']
        );
        let mut wire = vec![b'R'];
        for chunk in [&[1u8, 2][..], &[3, 4, 5, 6][..]] {
            wire.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            wire.extend_from_slice(chunk);
        }
        wire.extend_from_slice(&0u32.to_le_bytes());
        let mut c = Cursor::new(wire);
        read_ready(&mut c).unwrap();
        let mut got = Vec::new();
        read_frames(&mut c, &mut |p| got.extend_from_slice(p)).unwrap();
        assert_eq!(got, [1, 2, 3, 4, 5, 6]);
        let mut bad = Cursor::new((MAX_FRAME as u32 + 1).to_le_bytes().to_vec());
        assert!(read_frames(&mut bad, &mut |_| {}).is_err());
        assert!(read_ready(&mut Cursor::new(vec![b'x'])).is_err());
    }
}
