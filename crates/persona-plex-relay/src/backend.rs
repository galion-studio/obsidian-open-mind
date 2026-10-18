// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Adapter: the PersonaPlex relay protocol as a `galion-voice-core` backend.
//!
//! [`RelayBackend`] speaks the moshi/PersonaPlex two-stream protocol
//! ([`crate::protocol::WireMessage`]) over any [`Transport`] (a WebSocket in
//! production, an in-memory queue in tests) and turns it into
//! [`VoiceBackend`] calls, so the same voice-core session drives either the
//! GPU model or the mock/CPU backends.
//!
//! Audio payload: the wire carries Opus in Ogg pages. Opus is not in M1 (see
//! `docs/ws-voice.md`, codec section), so the codec is pluggable through
//! [`AudioCodec`]; [`PcmPassthrough`] (raw s16le) exists for tests and is NOT
//! understood by a real PersonaPlex server.

use galion_voice_core::audio::{Frame, FrameSpec};
use galion_voice_core::backend::{
    BackendError, BackendEvent, Capabilities, DuplexMode, VoiceBackend,
};
use galion_voice_core::persona::PersonaSource;

use crate::protocol::{upstream_url, WireMessage};

/// Non-blocking message pipe to the GPU side (one WebSocket binary message each).
pub trait Transport {
    fn send(&mut self, msg: Vec<u8>) -> Result<(), String>;
    /// Next received message, if any, without blocking.
    fn try_recv(&mut self) -> Result<Option<Vec<u8>>, String>;
}

/// PCM frame <-> wire audio payload.
pub trait AudioCodec {
    fn encode(&mut self, frame: &Frame) -> Vec<u8>;
    fn decode(&mut self, payload: &[u8]) -> Result<Vec<i16>, String>;
}

/// Raw little-endian i16 PCM. Test-only stand-in for Opus/Ogg.
#[derive(Debug, Default, Clone, Copy)]
pub struct PcmPassthrough;

impl AudioCodec for PcmPassthrough {
    fn encode(&mut self, frame: &Frame) -> Vec<u8> {
        frame.samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }
    fn decode(&mut self, payload: &[u8]) -> Result<Vec<i16>, String> {
        if !payload.len().is_multiple_of(2) {
            return Err("odd PCM payload length".into());
        }
        Ok(payload
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| i16::from_le_bytes(*b))
            .collect())
    }
}

pub struct RelayBackend<T: Transport, C: AudioCodec> {
    transport: T,
    codec: C,
    base_url: String,
    url: Option<String>,
    handshake: bool,
    closed: bool,
    seq_out: u64,
    dropping: bool,
}

impl<T: Transport, C: AudioCodec> RelayBackend<T, C> {
    /// `base_url` comes from config (`[upstream]`), never hard-coded.
    pub fn new(transport: T, codec: C, base_url: impl Into<String>) -> Self {
        RelayBackend {
            transport,
            codec,
            base_url: base_url.into(),
            url: None,
            handshake: false,
            closed: false,
            seq_out: 0,
            dropping: false,
        }
    }

    /// Upstream URL with the persona query, set by `start()`. The caller opens
    /// the WebSocket to it and hands the socket over as the transport.
    pub fn upstream_url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn handshake_seen(&self) -> bool {
        self.handshake
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn transport_mut(&mut self) -> &mut T {
        &mut self.transport
    }

    fn ready(&self) -> Result<(), BackendError> {
        if self.closed {
            Err(BackendError::Closed)
        } else if self.url.is_none() {
            Err(BackendError::NotStarted)
        } else {
            Ok(())
        }
    }
}

impl<T: Transport, C: AudioCodec> VoiceBackend for RelayBackend<T, C> {
    fn name(&self) -> &str {
        "personaplex-relay"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            duplex: DuplexMode::Full,
            spec: FrameSpec::MOSHI,
            emits_text: true,
        }
    }

    fn start(&mut self, persona: &dyn PersonaSource) -> Result<(), BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        self.url = Some(upstream_url(
            &self.base_url,
            &persona.voice_prompt(),
            &persona.text_prompt(),
        ));
        Ok(())
    }

    fn send_audio(&mut self, frame: &Frame) -> Result<(), BackendError> {
        self.ready()?;
        // Mic audio flows from the first tick; the server buffers it until its
        // handshake. Upstream is audio-only (kind 0x01), as forwardable_upstream().
        let msg = WireMessage::Audio(self.codec.encode(frame)).encode();
        self.transport.send(msg).map_err(BackendError::Transport)
    }

    fn end_of_user_turn(&mut self) -> Result<(), BackendError> {
        // Full duplex: the model hears the silence itself. Nothing to send.
        self.ready()
    }

    fn interrupt(&mut self) -> Result<(), BackendError> {
        // The PersonaPlex server accepts only audio from the client, so there
        // is no cancel message: the model notices the user talking. Locally,
        // drop model audio already received in the same poll batch.
        self.ready()?;
        self.dropping = true;
        Ok(())
    }

    fn poll(&mut self) -> Result<Vec<BackendEvent>, BackendError> {
        self.ready()?;
        self.dropping = false;
        let mut out = Vec::new();
        while let Some(bytes) = self.transport.try_recv().map_err(BackendError::Transport)? {
            let msg =
                WireMessage::decode(&bytes).map_err(|e| BackendError::Protocol(e.to_string()))?;
            match msg {
                WireMessage::Handshake { .. } => self.handshake = true,
                WireMessage::Audio(p) => {
                    let samples = self.codec.decode(&p).map_err(BackendError::Protocol)?;
                    out.push(BackendEvent::Audio(Frame {
                        seq: self.seq_out,
                        samples,
                    }));
                    self.seq_out += 1;
                }
                WireMessage::Text(t) | WireMessage::ColoredText { text: t, .. } => {
                    out.push(BackendEvent::Text(t))
                }
                WireMessage::Error(e) => out.push(BackendEvent::Unavailable(e)),
                WireMessage::RelayFallback(r) => {
                    out.push(BackendEvent::Unavailable(format!("fallback: {r:?}")))
                }
                WireMessage::RelayBye(r) => out.push(BackendEvent::Unavailable(r)),
                WireMessage::Control(_)
                | WireMessage::Metadata(_)
                | WireMessage::Ping
                | WireMessage::RelaySpeaker { .. } => {}
            }
        }
        Ok(out)
    }

    fn close(&mut self) {
        self.closed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::forwardable_upstream;
    use galion_voice_core::audio::synth;
    use galion_voice_core::persona::StaticPersona;
    use galion_voice_core::session::{Session, SessionConfig, SessionEvent, State};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Queue {
        sent: Vec<Vec<u8>>,
        inbox: VecDeque<Vec<u8>>,
    }

    impl Transport for Queue {
        fn send(&mut self, msg: Vec<u8>) -> Result<(), String> {
            self.sent.push(msg);
            Ok(())
        }
        fn try_recv(&mut self) -> Result<Option<Vec<u8>>, String> {
            Ok(self.inbox.pop_front())
        }
    }

    fn model_frame(i: usize) -> Vec<u8> {
        let n = FrameSpec::MOSHI.frame_samples;
        let f = Frame {
            seq: 0,
            samples: synth::tone(24_000, 440.0, 0.25, n, i * n),
        };
        WireMessage::Audio(PcmPassthrough.encode(&f)).encode()
    }

    #[test]
    fn relay_backend_drives_a_voice_core_session() {
        let rb = RelayBackend::new(
            Queue::default(),
            PcmPassthrough,
            "ws://gpu.invalid/api/chat",
        );
        let mut s = Session::new(rb, SessionConfig::default());
        s.start(&StaticPersona::new("Be Galion", "NATF2.pt"))
            .unwrap();
        let url = s.backend().upstream_url().unwrap().to_string();
        assert!(url.contains("voice_prompt=NATF2.pt"), "{url}");
        assert!(url.contains("text_prompt=Be%20Galion"), "{url}");

        // GPU side: handshake, text, three audio frames.
        let inbox = &mut s.backend_mut().transport_mut().inbox;
        inbox.push_back(
            WireMessage::Handshake {
                version: 0,
                model: 0,
            }
            .encode(),
        );
        inbox.push_back(WireMessage::Text("Hi".into()).encode());
        for i in 0..3 {
            inbox.push_back(model_frame(i));
        }
        let quiet = Frame {
            seq: 0,
            samples: synth::noise(5, 0.002, FrameSpec::MOSHI.frame_samples),
        };
        let out = s.tick(&quiet).unwrap();
        assert!(s.backend().handshake_seen());
        assert!(out.events.contains(&SessionEvent::Text("Hi".into())));
        assert!(out.playout.is_some(), "prebuffer of 2 met by 3 frames");
        assert_eq!(s.state(), State::Speaking);

        // Full duplex: mic frame went up on the same tick, audio-only.
        let sent = &s.backend().transport().sent;
        assert_eq!(sent.len(), 1);
        assert!(sent.iter().all(|m| forwardable_upstream(m)));
        assert_eq!(sent[0].len(), 1 + 2 * FrameSpec::MOSHI.frame_samples);

        // Upstream error -> session fallback.
        s.backend_mut()
            .transport_mut()
            .inbox
            .push_back(WireMessage::Error("gpu oom".into()).encode());
        let out = s.tick(&quiet).unwrap();
        assert!(out.events.contains(&SessionEvent::State(State::Fallback)));
    }

    #[test]
    fn not_started_and_bad_payloads_are_errors() {
        let mut rb = RelayBackend::new(Queue::default(), PcmPassthrough, "ws://x.invalid/api/chat");
        let f = Frame::silence(0, 4);
        assert_eq!(rb.send_audio(&f), Err(BackendError::NotStarted));
        rb.start(&StaticPersona::new("t", "v")).unwrap();
        rb.transport_mut().inbox.push_back(vec![0x01, 0xAA]); // 1 byte PCM
        assert!(matches!(rb.poll(), Err(BackendError::Protocol(_))));
        rb.transport_mut().inbox.push_back(vec![0x99]);
        assert!(matches!(rb.poll(), Err(BackendError::Protocol(_))));
        assert_eq!(
            PcmPassthrough.decode(&[1, 0, 255, 255]).unwrap(),
            vec![1, -1]
        );
        rb.close();
        assert_eq!(rb.poll(), Err(BackendError::Closed));
    }
}
