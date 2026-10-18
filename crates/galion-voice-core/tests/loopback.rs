// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Loopback: synthetic user audio up, mock model audio down, on every tick,
//! with a barge-in in the middle of the first reply.

use galion_voice_core::audio::{rms, synth, Frame, FrameSpec, Framer};
use galion_voice_core::backend::DuplexMode;
use galion_voice_core::mock::MockBackend;
use galion_voice_core::persona::StaticPersona;
use galion_voice_core::session::{Session, SessionConfig, SessionEvent, State};

const SPEC: FrameSpec = FrameSpec::MOSHI;

/// What the synthetic user does on a given tick.
#[derive(Clone, Copy, PartialEq)]
enum User {
    Quiet,
    Talk,
}

struct Rec {
    states: Vec<State>,
    playout_rms: Vec<Option<f32>>,
    barge_tick: Option<usize>,
    texts: Vec<String>,
}

fn run(
    duplex: DuplexMode,
    script: &dyn Fn(usize, &Rec) -> User,
    ticks: usize,
) -> (Session<MockBackend>, Rec) {
    let mut s = Session::new(MockBackend::new(SPEC, duplex, 20), SessionConfig::default());
    let persona = StaticPersona::new("You are Galion.", "NATF2.pt");
    let ev = s.start(&persona).unwrap();
    assert_eq!(ev.events, vec![SessionEvent::State(State::Listening)]);

    // Mic audio arrives in odd-sized chunks (like a sound card) and is framed.
    let mut framer = Framer::new(SPEC);
    let mut rec = Rec {
        states: vec![],
        playout_rms: vec![],
        barge_tick: None,
        texts: vec![],
    };
    let mut t = 0usize;
    let mut phase = 0usize;
    while t < ticks {
        let chunk = match script(t, &rec) {
            User::Talk => {
                let mut v = synth::tone(SPEC.sample_rate, 180.0, 0.3, 1000, phase);
                let n = synth::noise(t as u32 + 7, 0.002, 1000);
                for (a, b) in v.iter_mut().zip(n) {
                    *a = a.saturating_add(b);
                }
                v
            }
            User::Quiet => synth::noise(t as u32 + 7, 0.002, 1000),
        };
        phase += 1000;
        for frame in framer.push(&chunk) {
            if t >= ticks {
                break;
            }
            let out = s.tick(&frame).unwrap();
            for e in &out.events {
                match e {
                    SessionEvent::BargeIn => rec.barge_tick = Some(t),
                    SessionEvent::Text(x) => rec.texts.push(x.clone()),
                    _ => {}
                }
            }
            rec.playout_rms.push(out.playout.as_ref().map(Frame::rms));
            rec.states.push(s.state());
            t += 1;
        }
    }
    (s, rec)
}

/// Talk ticks 0..10, quiet; once 5 frames of the reply have played, talk 8
/// more ticks (barge-in), then quiet until the end.
fn barge_script(t: usize, rec: &Rec) -> User {
    if t < 10 {
        return User::Talk;
    }
    let played = rec.playout_rms.iter().filter(|p| p.is_some()).count();
    match rec.barge_tick {
        None if played >= 5 => User::Talk,
        None => User::Quiet,
        Some(b) if t < b + 7 => User::Talk,
        Some(_) => User::Quiet,
    }
}

#[test]
fn loopback_half_duplex_with_barge_in() {
    let (s, rec) = run(DuplexMode::HalfWithBargeIn, &barge_script, 120);
    let st = s.stats();
    let m = s.backend().stats();
    let b = rec.barge_tick.expect("barge-in happened");

    assert_eq!(st.barge_ins, 1);
    assert_eq!(m.interrupts, 1);
    assert_eq!(m.turns, 2, "two user turns -> two replies");
    assert_eq!(st.user_turns, 2);
    assert_eq!(
        rec.texts,
        vec!["reply 1".to_string(), "reply 2".to_string()]
    );
    assert_eq!(m.persona_voice, "NATF2.pt");

    // Barge-in silences playout on the very tick it is detected, and nothing
    // plays while the user holds the floor.
    assert_eq!(rec.playout_rms[b], None);
    for t in b..rec.states.len() {
        if rec.states[t] == State::UserSpeaking {
            assert_eq!(rec.playout_rms[t], None, "talked over at tick {t}");
        }
    }
    // Before the barge-in, the model was speaking and its audio was the 440 Hz
    // reply tone (rms of 0.25 amplitude sine ~ 0.177).
    assert_eq!(rec.states[b - 1], State::Speaking);
    let r = rec.playout_rms[b - 1].unwrap();
    assert!((r - 0.177).abs() < 0.01, "{r}");

    // Second reply plays in full (20 frames), then back to Listening.
    let after: usize = rec.playout_rms[b..].iter().filter(|p| p.is_some()).count();
    assert_eq!(after, 20);
    assert_eq!(*rec.states.last().unwrap(), State::Listening);
    let js = s.jitter_stats();
    assert_eq!(js.concealed, 0, "steady pacing must not underrun mid-reply");
    assert!(js.flushed >= 1);

    // Half duplex: only speech frames go up.
    assert!(st.frames_up < st.ticks);
    assert_eq!(m.frames_in, st.frames_up);
}

#[test]
fn loopback_full_duplex_streams_both_ways_every_tick() {
    let (s, rec) = run(DuplexMode::Full, &barge_script, 120);
    let st = s.stats();
    assert_eq!(st.frames_up, st.ticks, "full duplex: mic always flows up");
    assert_eq!(st.barge_ins, 1);
    assert!(st.frames_down >= 25);
    // During the model's first reply the mic was still being sent.
    let b = rec.barge_tick.unwrap();
    assert!(rec.states[..b].contains(&State::Speaking));
}

#[test]
fn backend_loss_goes_to_fallback_and_resumes() {
    let mut s = Session::new(
        MockBackend::new(SPEC, DuplexMode::Full, 5),
        SessionConfig::default(),
    );
    s.start(&StaticPersona::new("p", "v")).unwrap();
    let quiet = Frame {
        seq: 0,
        samples: synth::noise(3, 0.002, SPEC.frame_samples),
    };
    s.tick(&quiet).unwrap();
    s.backend_mut().fail_next_poll();
    let out = s.tick(&quiet).unwrap();
    assert!(out.events.contains(&SessionEvent::State(State::Fallback)));
    assert_eq!(s.tick(&quiet).unwrap(), Default::default());
    s.resume();
    assert_eq!(s.state(), State::Listening);
    s.close();
    assert_eq!(s.state(), State::Closed);
    assert!(s.tick(&quiet).is_err());
    assert!(rms(&quiet.samples) < 0.01);
}
