// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Size/RSS probe: 60 s of synthetic full-duplex loopback (750 ticks of 80 ms,
//! run as fast as possible) against the mock backend, with periodic barge-ins.

use galion_voice_core::audio::{synth, Frame, FrameSpec};
use galion_voice_core::backend::DuplexMode;
use galion_voice_core::mock::MockBackend;
use galion_voice_core::persona::StaticPersona;
use galion_voice_core::session::{Session, SessionConfig};

fn main() {
    let spec = FrameSpec::MOSHI;
    let mut s = Session::new(
        MockBackend::new(spec, DuplexMode::Full, 30),
        SessionConfig::default(),
    );
    s.start(&StaticPersona::new("probe", "probe"))
        .expect("start");
    for t in 0..750u64 {
        // Talk 1 s every 4 s; replies get cut by the next talk spurt.
        let talking = (t % 50) < 12;
        let samples = if talking {
            synth::tone(spec.sample_rate, 180.0, 0.3, spec.frame_samples, 0)
        } else {
            synth::noise(t as u32 + 1, 0.002, spec.frame_samples)
        };
        s.tick(&Frame { seq: t, samples }).expect("tick");
    }
    let st = s.stats();
    println!(
        "ticks={} up={} down={} turns={} barge_ins={} jitter={:?}",
        st.ticks,
        st.frames_up,
        st.frames_down,
        st.user_turns,
        st.barge_ins,
        s.jitter_stats()
    );
}
