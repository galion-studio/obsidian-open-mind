// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! One dictation session: a bounded drop-oldest audio queue between the
//! WebSocket receive task and a dedicated engine thread.
//!
//! The receive path only converts a frame and pushes it (a short mutex hold,
//! never during inference). The engine thread drains everything queued
//! (coalescing), feeds it to Moonshine, and every `partial_interval` runs the
//! heavy `transcribe` call and emits partial/final events. If the engine
//! falls behind and the queue fills, the oldest chunks are dropped and
//! counted, and the client gets one non-fatal warning (rate-limited).
//!
//! Nothing here writes audio or text anywhere except the outbound channel.

use crate::audio::SAMPLE_RATE;
use crate::engine::{LineSnap, LineTracker, Stream};
use crate::protocol::ServerMsg;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// What the engine thread needs from a stream (real or fake in tests).
pub trait StreamEngine {
    fn add_audio(&mut self, samples: &[f32], sample_rate: u32) -> Result<(), String>;
    fn transcribe(&mut self, force: bool) -> Result<Vec<LineSnap>, String>;
    fn finish(&mut self) -> Result<Vec<LineSnap>, String>;
}

impl StreamEngine for Stream<'_> {
    fn add_audio(&mut self, samples: &[f32], sample_rate: u32) -> Result<(), String> {
        Stream::add_audio(self, samples, sample_rate)
    }
    fn transcribe(&mut self, force: bool) -> Result<Vec<LineSnap>, String> {
        Stream::transcribe(self, force)
    }
    fn finish(&mut self) -> Result<Vec<LineSnap>, String> {
        Stream::finish(self)
    }
}

#[derive(Default)]
struct QState {
    chunks: VecDeque<Vec<f32>>,
    stop: bool,
    abort: bool,
    dropped_chunks: u64,
    dropped_samples: u64,
}

/// Bounded queue of audio chunks; full means drop the oldest.
pub struct AudioQueue {
    cap: usize,
    st: Mutex<QState>,
    cv: Condvar,
}

/// What the engine thread took out of the queue in one go.
#[derive(Default)]
pub struct Drained {
    pub chunks: Vec<Vec<f32>>,
    pub stop: bool,
    pub abort: bool,
}

impl AudioQueue {
    pub fn new(cap: usize) -> Self {
        AudioQueue {
            cap: cap.max(1),
            st: Mutex::new(QState::default()),
            cv: Condvar::new(),
        }
    }

    /// Push a chunk. Returns the number of samples dropped to make room (0
    /// normally). Never blocks on inference.
    pub fn push(&self, chunk: Vec<f32>) -> u64 {
        let mut dropped = 0;
        if let Ok(mut s) = self.st.lock() {
            if s.stop || s.abort {
                return 0;
            }
            while s.chunks.len() >= self.cap {
                if let Some(old) = s.chunks.pop_front() {
                    dropped += old.len() as u64;
                    s.dropped_chunks += 1;
                }
            }
            s.dropped_samples += dropped;
            s.chunks.push_back(chunk);
        }
        self.cv.notify_one();
        dropped
    }

    /// End of audio: flush and send finals.
    pub fn stop(&self) {
        if let Ok(mut s) = self.st.lock() {
            s.stop = true;
        }
        self.cv.notify_one();
    }

    /// Client went away: stop without finals.
    pub fn abort(&self) {
        if let Ok(mut s) = self.st.lock() {
            s.abort = true;
        }
        self.cv.notify_one();
    }

    pub fn dropped(&self) -> (u64, u64) {
        self.st
            .lock()
            .map(|s| (s.dropped_chunks, s.dropped_samples))
            .unwrap_or((0, 0))
    }

    /// Wait until there is something to do or `deadline` passes, then take
    /// everything queued.
    pub fn wait_drain(&self, deadline: Option<Instant>) -> Drained {
        let Ok(mut s) = self.st.lock() else {
            return Drained {
                abort: true,
                ..Drained::default()
            };
        };
        loop {
            if !s.chunks.is_empty() || s.stop || s.abort {
                break;
            }
            match deadline {
                None => match self.cv.wait(s) {
                    Ok(g) => s = g,
                    Err(_) => {
                        return Drained {
                            abort: true,
                            ..Drained::default()
                        }
                    }
                },
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        break;
                    }
                    match self.cv.wait_timeout(s, d - now) {
                        Ok((g, _)) => s = g,
                        Err(_) => {
                            return Drained {
                                abort: true,
                                ..Drained::default()
                            }
                        }
                    }
                }
            }
        }
        Drained {
            chunks: s.chunks.drain(..).collect(),
            stop: s.stop,
            abort: s.abort,
        }
    }
}

/// Per-session counters and timings (no audio, no text).
#[derive(Debug, Default, Clone)]
pub struct SessionStats {
    pub chunks: u64,
    pub samples: u64,
    pub transcribe_calls: u64,
    pub transcribe_ms_total: u64,
    pub transcribe_ms_max: u64,
    pub partials: u64,
    pub finals: u64,
    /// Wall time from the first audio chunk reaching the engine thread to
    /// the first partial (or final) event.
    pub first_text_ms: Option<u64>,
    /// Wall time from stop to the last final having been emitted.
    pub stop_to_final_ms: Option<u64>,
    pub aborted: bool,
    pub error: bool,
}

/// Engine-thread body. `emit` returns false when the client is gone.
pub fn run_engine<E: StreamEngine>(
    engine: &mut E,
    queue: &AudioQueue,
    partial_interval: Duration,
    mut emit: impl FnMut(ServerMsg) -> bool,
) -> SessionStats {
    let mut st = SessionStats::default();
    let mut tracker = LineTracker::default();
    let mut first_audio: Option<Instant> = None;
    let mut next_tick: Option<Instant> = None;
    let mut pending_audio = false;
    let mut send = |msgs: Vec<ServerMsg>, st: &mut SessionStats, first: Option<Instant>| -> bool {
        for m in msgs {
            if st.first_text_ms.is_none() && !matches!(m, ServerMsg::Error { .. }) {
                if let Some(f) = first {
                    st.first_text_ms = Some(f.elapsed().as_millis() as u64);
                }
            }
            if !emit(m) {
                return false;
            }
        }
        true
    };
    loop {
        let d = queue.wait_drain(next_tick);
        if d.abort {
            st.aborted = true;
            break;
        }
        for c in &d.chunks {
            st.chunks += 1;
            st.samples += c.len() as u64;
            if let Err(e) = engine.add_audio(c, SAMPLE_RATE) {
                st.error = true;
                send(vec![ServerMsg::Error { message: e }], &mut st, first_audio);
                return st;
            }
        }
        if !d.chunks.is_empty() {
            pending_audio = true;
            if first_audio.is_none() {
                let now = Instant::now();
                first_audio = Some(now);
                next_tick = Some(now + partial_interval);
            }
        }
        if d.stop {
            let t = Instant::now();
            let r = engine.finish();
            let ms = t.elapsed().as_millis() as u64;
            st.transcribe_calls += 1;
            st.transcribe_ms_total += ms;
            st.transcribe_ms_max = st.transcribe_ms_max.max(ms);
            match r {
                Ok(lines) => {
                    let ev = tracker.update(&lines, true);
                    send(ev, &mut st, first_audio);
                }
                Err(e) => {
                    st.error = true;
                    send(vec![ServerMsg::Error { message: e }], &mut st, first_audio);
                }
            }
            st.stop_to_final_ms = Some(t.elapsed().as_millis() as u64);
            break;
        }
        if let Some(tick) = next_tick {
            if Instant::now() >= tick {
                if pending_audio {
                    let t = Instant::now();
                    let r = engine.transcribe(false);
                    let ms = t.elapsed().as_millis() as u64;
                    st.transcribe_calls += 1;
                    st.transcribe_ms_total += ms;
                    st.transcribe_ms_max = st.transcribe_ms_max.max(ms);
                    match r {
                        Ok(lines) => {
                            let ev = tracker.update(&lines, false);
                            if !send(ev, &mut st, first_audio) {
                                st.aborted = true;
                                break;
                            }
                        }
                        Err(e) => {
                            st.error = true;
                            send(vec![ServerMsg::Error { message: e }], &mut st, first_audio);
                            break;
                        }
                    }
                    pending_audio = false;
                }
                // Schedule from now: if inference overran, do not try to
                // catch up with back-to-back calls; the queue coalesces.
                next_tick = Some(Instant::now() + partial_interval);
            }
        }
    }
    st.partials = tracker.partials;
    st.finals = tracker.finals;
    st
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Fake engine: text grows by one word per transcribe call; a line
    /// completes after `words_per_line` words.
    struct Fake {
        fed: u64,
        calls: u64,
        words_per_line: u64,
        delay: Duration,
    }

    impl Fake {
        fn lines(&self, all_final: bool) -> Vec<LineSnap> {
            let words = self.calls;
            let mut v = Vec::new();
            let full = words / self.words_per_line;
            for i in 0..full {
                v.push(snap(&format!("line{i}"), true));
            }
            let rem = words % self.words_per_line;
            if rem > 0 {
                v.push(snap(&format!("part{rem}"), all_final));
            }
            v
        }
    }

    fn snap(t: &str, c: bool) -> LineSnap {
        LineSnap {
            text: t.into(),
            start_time: 0.0,
            duration: 0.0,
            is_complete: c,
            has_text_changed: true,
            latency_ms: 0,
        }
    }

    impl StreamEngine for Fake {
        fn add_audio(&mut self, s: &[f32], _r: u32) -> Result<(), String> {
            self.fed += s.len() as u64;
            Ok(())
        }
        fn transcribe(&mut self, _f: bool) -> Result<Vec<LineSnap>, String> {
            std::thread::sleep(self.delay);
            self.calls += 1;
            Ok(self.lines(false))
        }
        fn finish(&mut self) -> Result<Vec<LineSnap>, String> {
            Ok(self.lines(true))
        }
    }

    #[test]
    fn queue_drops_oldest_when_full() {
        let q = AudioQueue::new(3);
        for i in 0..5 {
            q.push(vec![i as f32; 10]);
        }
        assert_eq!(q.dropped(), (2, 20));
        let d = q.wait_drain(Some(Instant::now()));
        assert_eq!(d.chunks.len(), 3);
        assert_eq!(d.chunks[0][0], 2.0);
        assert_eq!(d.chunks[2][0], 4.0);
    }

    #[test]
    fn push_after_stop_is_ignored() {
        let q = AudioQueue::new(3);
        q.stop();
        assert_eq!(q.push(vec![0.0; 10]), 0);
        let d = q.wait_drain(None);
        assert!(d.stop && d.chunks.is_empty());
    }

    #[test]
    fn partials_then_finals_on_stop() {
        let q = Arc::new(AudioQueue::new(64));
        let q2 = Arc::clone(&q);
        let feeder = std::thread::spawn(move || {
            for _ in 0..25 {
                q2.push(vec![0.0; 320]);
                std::thread::sleep(Duration::from_millis(20));
            }
            q2.stop();
        });
        let mut fake = Fake {
            fed: 0,
            calls: 0,
            words_per_line: 2,
            delay: Duration::ZERO,
        };
        let mut got = Vec::new();
        let st = run_engine(&mut fake, &q, Duration::from_millis(100), |m| {
            got.push(m);
            true
        });
        feeder.join().unwrap();
        assert_eq!(fake.fed, 25 * 320);
        assert!(st.transcribe_calls >= 3, "{st:?}");
        assert!(got.iter().any(|m| matches!(m, ServerMsg::Partial { .. })));
        let finals: Vec<_> = got
            .iter()
            .filter(|m| matches!(m, ServerMsg::Final { .. }))
            .collect();
        assert!(!finals.is_empty());
        // Line numbers of finals strictly increase.
        let mut last = None;
        for m in &got {
            if let ServerMsg::Final { line, .. } = m {
                assert!(last.is_none_or(|l| *line > l));
                last = Some(*line);
            }
        }
        assert!(st.stop_to_final_ms.is_some());
        assert!(st.first_text_ms.is_some());
        assert!(!st.aborted);
    }

    #[test]
    fn abort_sends_nothing_more() {
        let q = AudioQueue::new(8);
        q.push(vec![0.0; 320]);
        q.abort();
        let mut fake = Fake {
            fed: 0,
            calls: 0,
            words_per_line: 2,
            delay: Duration::ZERO,
        };
        let mut n = 0;
        let st = run_engine(&mut fake, &q, Duration::from_millis(50), |_| {
            n += 1;
            true
        });
        assert!(st.aborted);
        assert_eq!(n, 0);
    }

    #[test]
    fn slow_engine_never_blocks_the_pusher() {
        // Engine takes 200 ms per call; the pusher must still push 50 x 20 ms
        // frames in about real time, with the overflow dropped oldest-first.
        let q = Arc::new(AudioQueue::new(5));
        let q2 = Arc::clone(&q);
        let eng = std::thread::spawn(move || {
            let mut fake = Fake {
                fed: 0,
                calls: 0,
                words_per_line: 100,
                delay: Duration::from_millis(200),
            };
            run_engine(&mut fake, &q2, Duration::from_millis(10), |_| true)
        });
        let t = Instant::now();
        let mut max_push = Duration::ZERO;
        for _ in 0..50 {
            let p = Instant::now();
            q.push(vec![0.0; 320]);
            max_push = max_push.max(p.elapsed());
            std::thread::sleep(Duration::from_millis(2));
        }
        let push_total = t.elapsed();
        q.stop();
        let st = eng.join().unwrap();
        assert!(
            max_push < Duration::from_millis(50),
            "push blocked {max_push:?}"
        );
        assert!(push_total < Duration::from_millis(1000), "{push_total:?}");
        let (dc, _) = q.dropped();
        assert!(dc > 0, "expected drops, stats {st:?}");
        assert_eq!(st.samples + q.dropped().1, 50 * 320);
    }
}
