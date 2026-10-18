// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Channel protocol types.
//!
//! Two bands ("two-bandwidth"):
//! * **Audio band** - two concurrent Opus streams, user->model and model->user,
//!   carried as moshi-server kind `0x01` messages. Full duplex: both flow at
//!   the same time; neither waits for a turn.
//! * **Control band** - low-rate text and control: the model's inner-monologue
//!   text (kind `0x02`), moshi control/metadata/error/ping, and the relay's own
//!   private messages (kinds `0x40..`), which never leave the VPS toward the GPU.
//!
//! Wire kinds `0x00..=0x07` follow the moshi web client
//! (`kyutai-labs/moshi` `client/src/protocol/encoder.ts`) and the PersonaPlex
//! server (`NVIDIA/personaplex` `moshi/moshi/server.py`, route `/api/chat`).

use std::fmt;

use crate::speaker::VerificationResult;

/// First byte of every WebSocket binary message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageKind {
    Handshake = 0x00,
    Audio = 0x01,
    Text = 0x02,
    Control = 0x03,
    Metadata = 0x04,
    Error = 0x05,
    Ping = 0x06,
    ColoredText = 0x07,
    /// Relay-private: speaker verification status for the client UI.
    RelaySpeaker = 0x40,
    /// Relay-private: switched to fallback (GPU off / lost).
    RelayFallback = 0x41,
    /// Relay-private: session closed by policy, with reason.
    RelayBye = 0x42,
}

/// Relay-private kinds start here and are never forwarded upstream.
pub const RELAY_PRIVATE_KIND_MIN: u8 = 0x40;

impl MessageKind {
    pub fn from_byte(b: u8) -> Option<Self> {
        use MessageKind::*;
        Some(match b {
            0x00 => Handshake,
            0x01 => Audio,
            0x02 => Text,
            0x03 => Control,
            0x04 => Metadata,
            0x05 => Error,
            0x06 => Ping,
            0x07 => ColoredText,
            0x40 => RelaySpeaker,
            0x41 => RelayFallback,
            0x42 => RelayBye,
            _ => return None,
        })
    }

    /// Which band the message travels on.
    pub fn band(self) -> Band {
        match self {
            MessageKind::Audio => Band::Audio,
            _ => Band::Control,
        }
    }
}

/// The two bands of the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Band {
    /// Opus audio, both directions at once.
    Audio,
    /// Text, control, metadata, relay status. Low rate.
    Control,
}

/// moshi control actions (`CONTROL_MESSAGES_MAP`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlAction {
    Start = 0,
    EndTurn = 1,
    Pause = 2,
    Restart = 3,
}

impl ControlAction {
    pub fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => ControlAction::Start,
            1 => ControlAction::EndTurn,
            2 => ControlAction::Pause,
            3 => ControlAction::Restart,
            _ => return None,
        })
    }
}

/// Why the relay went to fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FallbackReason {
    GpuDisabled = 0,
    GpuUnreachable = 1,
    GpuBusy = 2,
    UpstreamError = 3,
}

/// One decoded WebSocket message.
#[derive(Debug, Clone, PartialEq)]
pub enum WireMessage {
    Handshake {
        version: u8,
        model: u8,
    },
    /// Opus (Ogg pages) payload.
    Audio(Vec<u8>),
    Text(String),
    ColoredText {
        color: u8,
        text: String,
    },
    Control(ControlAction),
    /// JSON text, passed through opaquely.
    Metadata(String),
    Error(String),
    Ping,
    /// Relay-private: compact speaker status (score as cosine * 1000, decision code).
    RelaySpeaker {
        score_milli: Option<i16>,
        decision: u8,
    },
    RelayFallback(FallbackReason),
    RelayBye(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Empty,
    UnknownKind(u8),
    Truncated(MessageKind),
    BadUtf8(MessageKind),
    BadControl(u8),
    BadFallback(u8),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProtocolError::Empty => write!(f, "empty message"),
            ProtocolError::UnknownKind(k) => write!(f, "unknown message kind 0x{k:02x}"),
            ProtocolError::Truncated(k) => write!(f, "truncated {k:?} message"),
            ProtocolError::BadUtf8(k) => write!(f, "{k:?} payload is not UTF-8"),
            ProtocolError::BadControl(b) => write!(f, "unknown control action {b}"),
            ProtocolError::BadFallback(b) => write!(f, "unknown fallback reason {b}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

fn utf8(kind: MessageKind, b: &[u8]) -> Result<String, ProtocolError> {
    String::from_utf8(b.to_vec()).map_err(|_| ProtocolError::BadUtf8(kind))
}

impl WireMessage {
    pub fn kind(&self) -> MessageKind {
        match self {
            WireMessage::Handshake { .. } => MessageKind::Handshake,
            WireMessage::Audio(_) => MessageKind::Audio,
            WireMessage::Text(_) => MessageKind::Text,
            WireMessage::ColoredText { .. } => MessageKind::ColoredText,
            WireMessage::Control(_) => MessageKind::Control,
            WireMessage::Metadata(_) => MessageKind::Metadata,
            WireMessage::Error(_) => MessageKind::Error,
            WireMessage::Ping => MessageKind::Ping,
            WireMessage::RelaySpeaker { .. } => MessageKind::RelaySpeaker,
            WireMessage::RelayFallback(_) => MessageKind::RelayFallback,
            WireMessage::RelayBye(_) => MessageKind::RelayBye,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![self.kind() as u8];
        match self {
            WireMessage::Handshake { version, model } => out.extend([*version, *model]),
            WireMessage::Audio(b) => out.extend_from_slice(b),
            WireMessage::Text(s)
            | WireMessage::Metadata(s)
            | WireMessage::Error(s)
            | WireMessage::RelayBye(s) => out.extend_from_slice(s.as_bytes()),
            WireMessage::ColoredText { color, text } => {
                out.push(*color);
                out.extend_from_slice(text.as_bytes());
            }
            WireMessage::Control(a) => out.push(*a as u8),
            WireMessage::Ping => {}
            WireMessage::RelaySpeaker {
                score_milli,
                decision,
            } => {
                match score_milli {
                    Some(s) => {
                        out.push(1);
                        out.extend_from_slice(&s.to_be_bytes());
                    }
                    None => out.extend([0, 0, 0]),
                }
                out.push(*decision);
            }
            WireMessage::RelayFallback(r) => out.push(*r as u8),
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let (&first, payload) = bytes.split_first().ok_or(ProtocolError::Empty)?;
        let kind = MessageKind::from_byte(first).ok_or(ProtocolError::UnknownKind(first))?;
        Ok(match kind {
            // The PersonaPlex server sends a bare 0x00 as handshake.
            MessageKind::Handshake => WireMessage::Handshake {
                version: payload.first().copied().unwrap_or(0),
                model: payload.get(1).copied().unwrap_or(0),
            },
            MessageKind::Audio => WireMessage::Audio(payload.to_vec()),
            MessageKind::Text => WireMessage::Text(utf8(kind, payload)?),
            MessageKind::ColoredText => {
                let (&color, rest) = payload
                    .split_first()
                    .ok_or(ProtocolError::Truncated(kind))?;
                WireMessage::ColoredText {
                    color,
                    text: utf8(kind, rest)?,
                }
            }
            MessageKind::Control => {
                let b = *payload.first().ok_or(ProtocolError::Truncated(kind))?;
                WireMessage::Control(
                    ControlAction::from_byte(b).ok_or(ProtocolError::BadControl(b))?,
                )
            }
            MessageKind::Metadata => WireMessage::Metadata(utf8(kind, payload)?),
            MessageKind::Error => WireMessage::Error(utf8(kind, payload)?),
            MessageKind::Ping => WireMessage::Ping,
            MessageKind::RelaySpeaker => {
                if payload.len() < 4 {
                    return Err(ProtocolError::Truncated(kind));
                }
                let score_milli = if payload[0] == 1 {
                    Some(i16::from_be_bytes([payload[1], payload[2]]))
                } else {
                    None
                };
                WireMessage::RelaySpeaker {
                    score_milli,
                    decision: payload[3],
                }
            }
            MessageKind::RelayFallback => {
                let b = *payload.first().ok_or(ProtocolError::Truncated(kind))?;
                WireMessage::RelayFallback(match b {
                    0 => FallbackReason::GpuDisabled,
                    1 => FallbackReason::GpuUnreachable,
                    2 => FallbackReason::GpuBusy,
                    3 => FallbackReason::UpstreamError,
                    _ => return Err(ProtocolError::BadFallback(b)),
                })
            }
            MessageKind::RelayBye => WireMessage::RelayBye(utf8(kind, payload)?),
        })
    }

    /// Build the relay's speaker status message from a verification result.
    pub fn speaker_status(r: &VerificationResult) -> Self {
        WireMessage::RelaySpeaker {
            score_milli: r
                .score
                .map(|s| (s.clamp(-1.0, 1.0) * 1000.0).round() as i16),
            decision: r.decision as u8,
        }
    }
}

/// Client -> GPU filter. The PersonaPlex server only consumes kind 0x01 from
/// the client and warns on anything else, so the relay forwards audio only.
/// Relay-private kinds and anything unknown are never forwarded.
pub fn forwardable_upstream(bytes: &[u8]) -> bool {
    matches!(bytes.first(), Some(&k) if k == MessageKind::Audio as u8)
}

/// Direction of an audio stream in the full-duplex pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioStream {
    /// Microphone -> relay -> GPU. Also feeds speaker verification on the VPS.
    UserToModel,
    /// GPU -> relay -> speaker. Never fed to speaker verification.
    ModelToUser,
}

/// One chunk of Opus audio on the audio band, with relay-side bookkeeping.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioFrame {
    pub stream: AudioStream,
    /// Monotonic per-stream sequence number assigned by the relay.
    pub seq: u64,
    /// Milliseconds since session start when the relay received the frame.
    pub received_ms: u64,
    /// Opus (Ogg pages), exactly as on the wire after the kind byte.
    pub opus: Vec<u8>,
}

impl AudioFrame {
    pub fn to_wire(&self) -> WireMessage {
        WireMessage::Audio(self.opus.clone())
    }
}

/// Lifecycle of one voice session on the relay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Client connected; bearer token not yet checked.
    Authenticating,
    /// Authenticated; collecting enrollment phrases (first use only).
    Enrolling,
    /// Authenticated; scoring the first speech window against the voiceprint.
    Verifying,
    /// Audio flowing to and from the GPU.
    Active,
    /// GPU off or lost: Galion One text mode.
    Fallback,
    Closed,
}

/// Events that drive the session state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
    AuthOk {
        enrolled: bool,
    },
    AuthFailed,
    EnrollmentDone,
    VoiceAccepted,
    /// Rejected or uncertain: conversation may continue, but identity stays unverified.
    VoiceNotAccepted,
    GpuUnavailable,
    GpuBack,
    Hangup,
}

impl SessionState {
    /// Pure transition function. `None` = event not valid in this state.
    pub fn next(self, ev: SessionEvent) -> Option<SessionState> {
        use SessionEvent as E;
        use SessionState as S;
        Some(match (self, ev) {
            (_, E::Hangup) => S::Closed,
            (S::Closed, _) => return None,
            (S::Authenticating, E::AuthOk { enrolled: true }) => S::Verifying,
            (S::Authenticating, E::AuthOk { enrolled: false }) => S::Enrolling,
            (S::Authenticating, E::AuthFailed) => S::Closed,
            (S::Enrolling, E::EnrollmentDone) => S::Verifying,
            // Voice result never blocks plain conversation; it only sets the
            // identity level used by the action gate (speaker::authorize).
            (S::Verifying, E::VoiceAccepted | E::VoiceNotAccepted) => S::Active,
            (S::Active, E::VoiceAccepted | E::VoiceNotAccepted) => S::Active,
            (S::Verifying | S::Active | S::Enrolling, E::GpuUnavailable) => S::Fallback,
            (S::Fallback, E::GpuBack) => S::Verifying,
            (S::Fallback, E::VoiceAccepted | E::VoiceNotAccepted) => S::Fallback,
            _ => return None,
        })
    }
}

/// A relay session (no sockets: bookkeeping only).
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    /// Europe/Zagreb RFC 3339 with offset.
    pub started_at: String,
    pub state: SessionState,
    pub voice_prompt: String,
    pub frames_up: u64,
    pub frames_down: u64,
    pub last_verification: Option<VerificationResult>,
}

impl Session {
    pub fn new(
        id: impl Into<String>,
        started_at: impl Into<String>,
        voice_prompt: impl Into<String>,
    ) -> Self {
        Session {
            id: id.into(),
            started_at: started_at.into(),
            state: SessionState::Authenticating,
            voice_prompt: voice_prompt.into(),
            frames_up: 0,
            frames_down: 0,
            last_verification: None,
        }
    }

    /// Apply an event; returns false (and leaves state unchanged) if invalid.
    pub fn apply(&mut self, ev: SessionEvent) -> bool {
        match self.state.next(ev) {
            Some(s) => {
                self.state = s;
                true
            }
            None => false,
        }
    }
}

/// Percent-encode a query component (RFC 3986 unreserved kept).
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Upstream URL for PersonaPlex `/api/chat`: persona = text role prompt +
/// voice prompt, both passed as query parameters (`text_prompt`, `voice_prompt`).
pub fn upstream_url(base: &str, voice_prompt: &str, text_prompt: &str) -> String {
    let sep = if base.contains('?') { '&' } else { '?' };
    format!(
        "{base}{sep}voice_prompt={}&text_prompt={}",
        percent_encode(voice_prompt),
        percent_encode(text_prompt)
    )
}

/// Pull the role prompt out of the Persona Plex v1 markdown: the first fenced
/// code block after the `## Role prompt` heading.
pub fn extract_role_prompt(markdown: &str) -> Option<String> {
    let start = markdown.find("## Role prompt")?;
    let rest = &markdown[start..];
    let open = rest.find("```")?;
    let after_open = &rest[open + 3..];
    let body_start = after_open.find('\n')? + 1;
    let body = &after_open[body_start..];
    let close = body.find("```")?;
    let prompt = body[..close].trim();
    (!prompt.is_empty()).then(|| prompt.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speaker::{Decision, Liveness};

    #[test]
    fn roundtrip_all_kinds() {
        let msgs = vec![
            WireMessage::Handshake {
                version: 0,
                model: 0,
            },
            WireMessage::Audio(vec![0x4f, 0x67, 0x67, 0x53]),
            WireMessage::Text("hello".into()),
            WireMessage::ColoredText {
                color: 5,
                text: "hi".into(),
            },
            WireMessage::Control(ControlAction::EndTurn),
            WireMessage::Metadata("{\"a\":1}".into()),
            WireMessage::Error("boom".into()),
            WireMessage::Ping,
            WireMessage::RelaySpeaker {
                score_milli: Some(-123),
                decision: 2,
            },
            WireMessage::RelaySpeaker {
                score_milli: None,
                decision: 3,
            },
            WireMessage::RelayFallback(FallbackReason::GpuBusy),
            WireMessage::RelayBye("policy".into()),
        ];
        for m in msgs {
            assert_eq!(WireMessage::decode(&m.encode()).unwrap(), m);
        }
    }

    #[test]
    fn matches_moshi_wire_bytes() {
        assert_eq!(WireMessage::Audio(vec![9, 9]).encode(), vec![0x01, 9, 9]);
        assert_eq!(
            WireMessage::Control(ControlAction::Restart).encode(),
            vec![0x03, 3]
        );
        assert_eq!(WireMessage::Text("a".into()).encode(), vec![0x02, b'a']);
        // PersonaPlex server handshake is a bare 0x00.
        assert_eq!(
            WireMessage::decode(&[0x00]).unwrap(),
            WireMessage::Handshake {
                version: 0,
                model: 0
            }
        );
    }

    #[test]
    fn decode_errors() {
        assert_eq!(WireMessage::decode(&[]), Err(ProtocolError::Empty));
        assert_eq!(
            WireMessage::decode(&[0x99]),
            Err(ProtocolError::UnknownKind(0x99))
        );
        assert_eq!(
            WireMessage::decode(&[0x03, 9]),
            Err(ProtocolError::BadControl(9))
        );
        assert!(WireMessage::decode(&[0x02, 0xff, 0xfe]).is_err());
    }

    #[test]
    fn only_audio_goes_upstream() {
        assert!(forwardable_upstream(&[0x01, 1, 2]));
        for k in [0x00u8, 0x02, 0x03, 0x04, 0x05, 0x06, 0x40, 0x41, 0x42, 0xff] {
            assert!(
                !forwardable_upstream(&[k]),
                "kind {k:#x} must not be forwarded"
            );
        }
        assert!(!forwardable_upstream(&[]));
        assert_eq!(MessageKind::Audio.band(), Band::Audio);
        assert_eq!(MessageKind::Text.band(), Band::Control);
        assert!(MessageKind::RelaySpeaker as u8 >= RELAY_PRIVATE_KIND_MIN);
    }

    #[test]
    fn session_lifecycle() {
        let mut s = Session::new("s1", "2026-10-18T20:00:00+02:00", "NATF2.pt");
        assert!(
            !s.apply(SessionEvent::VoiceAccepted),
            "no voice before auth"
        );
        assert!(s.apply(SessionEvent::AuthOk { enrolled: false }));
        assert_eq!(s.state, SessionState::Enrolling);
        assert!(s.apply(SessionEvent::EnrollmentDone));
        assert!(s.apply(SessionEvent::VoiceNotAccepted));
        assert_eq!(s.state, SessionState::Active);
        assert!(s.apply(SessionEvent::GpuUnavailable));
        assert_eq!(s.state, SessionState::Fallback);
        assert!(s.apply(SessionEvent::GpuBack));
        assert!(s.apply(SessionEvent::Hangup));
        assert_eq!(s.state, SessionState::Closed);
        assert!(!s.apply(SessionEvent::GpuBack));
    }

    #[test]
    fn auth_failure_closes() {
        assert_eq!(
            SessionState::Authenticating.next(SessionEvent::AuthFailed),
            Some(SessionState::Closed)
        );
    }

    #[test]
    fn speaker_status_message() {
        let r = VerificationResult {
            score: Some(0.8123),
            decision: Decision::Accept,
            liveness: Liveness::NotChecked,
            speech_ms: 2000,
        };
        assert_eq!(
            WireMessage::speaker_status(&r),
            WireMessage::RelaySpeaker {
                score_milli: Some(812),
                decision: Decision::Accept as u8
            }
        );
    }

    #[test]
    fn url_and_prompt() {
        let u = upstream_url("wss://h:8998/api/chat", "NATF2.pt", "Hi, you.");
        assert_eq!(
            u,
            "wss://h:8998/api/chat?voice_prompt=NATF2.pt&text_prompt=Hi%2C%20you."
        );
        let md = "# x\n## Role prompt (text)\n\n```\nYou are Persona Plex.\nShort.\n```\n## Voice";
        assert_eq!(
            extract_role_prompt(md).unwrap(),
            "You are Persona Plex.\nShort."
        );
        assert!(extract_role_prompt("# nothing").is_none());
    }
}
