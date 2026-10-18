// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `CpuPipelineBackend`: STT -> text model -> TTS behind the voice-core
//! `VoiceBackend` trait (turn-based, barge-in cancels the reply).
//!
//! The heavy work runs on one worker thread (`std::thread` + `std::sync::mpsc`,
//! no async runtime): `end_of_user_turn()` only hands the utterance over, so the
//! session tick that ends a turn returns at once and audio keeps flowing.
//! Results come back through `poll()`.
//!
//! Cancellation: every turn carries a generation number. `interrupt()` (barge-in)
//! and a new `end_of_user_turn()` bump the generation; the worker checks it
//! before each stage (STT, model, TTS) and drops stale work, and `poll()`
//! discards anything that still arrives from an older generation. A stage that
//! is already running (e.g. one Whisper call) finishes, but its result is never
//! played.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

use galion_voice_core::audio::{Frame, FrameSpec};
use galion_voice_core::backend::{
    BackendError, BackendEvent, Capabilities, DuplexMode, VoiceBackend,
};
use galion_voice_core::persona::PersonaSource;

use crate::resample::{f32_to_i16, i16_to_f32, linear};
use crate::stt::{SpeechToText, TextModel, TextToSpeech};

/// Timings of the last turn, in milliseconds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TurnTiming {
    pub utterance_ms: u64,
    pub stt_ms: u64,
    pub model_ms: u64,
    pub tts_ms: u64,
}

/// One utterance for the worker.
struct Job {
    gen: u64,
    pcm: Vec<i16>,
    system: Arc<str>,
    voice: Arc<str>,
}

enum Msg {
    Event(BackendEvent),
    Transcript(String),
    Timing(TurnTiming),
    Failed(String),
}

struct Worker<S, M, T> {
    stt: S,
    model: M,
    tts: T,
    spec: FrameSpec,
    current: Arc<AtomicU64>,
    tx: Sender<(u64, Msg)>,
}

impl<S: SpeechToText, M: TextModel, T: TextToSpeech> Worker<S, M, T> {
    fn stale(&self, gen: u64) -> bool {
        self.current.load(Ordering::Acquire) != gen
    }

    /// Returns false when the backend side is gone.
    fn send(&self, gen: u64, m: Msg) -> bool {
        self.tx.send((gen, m)).is_ok()
    }

    fn run(mut self, jobs: Receiver<Job>) {
        let mut out_seq = 0u64;
        while let Ok(job) = jobs.recv() {
            if self.stale(job.gen) {
                continue;
            }
            let gen = job.gen;
            let msgs = match self.turn(job, &mut out_seq) {
                Ok(m) => m,
                Err(e) => vec![Msg::Failed(e)],
            };
            for m in msgs {
                if !self.send(gen, m) {
                    return;
                }
            }
        }
    }

    /// One turn. Messages are collected and sent at the end of each stage so a
    /// cancelled turn stops early.
    fn turn(&mut self, job: Job, out_seq: &mut u64) -> Result<Vec<Msg>, String> {
        let ms = |t: std::time::Instant| t.elapsed().as_millis() as u64;
        let gen = job.gen;
        let mut timing = TurnTiming {
            utterance_ms: job.pcm.len() as u64 * 1000 / self.spec.sample_rate.max(1) as u64,
            ..TurnTiming::default()
        };
        let t = std::time::Instant::now();
        let input = linear(
            &i16_to_f32(&job.pcm),
            self.spec.sample_rate,
            self.stt.sample_rate(),
        );
        let text = self.stt.transcribe(&input)?;
        timing.stt_ms = ms(t);
        if self.stale(gen) {
            return Ok(Vec::new());
        }
        if text.trim().is_empty() {
            return Ok(vec![
                Msg::Transcript(text),
                Msg::Timing(timing),
                Msg::Event(BackendEvent::TurnDone),
            ]);
        }
        // The transcript goes out before the model runs, so a UI can show it.
        let user = format!("user: {text}");
        if !self.send(gen, Msg::Transcript(text.clone()))
            || !self.send(gen, Msg::Event(BackendEvent::Text(user)))
        {
            return Ok(Vec::new());
        }
        let t = std::time::Instant::now();
        let reply = self.model.reply(&job.system, &text)?;
        timing.model_ms = ms(t);
        if self.stale(gen) {
            return Ok(Vec::new());
        }
        if !self.send(gen, Msg::Event(BackendEvent::Text(reply.clone()))) {
            return Ok(Vec::new());
        }
        let t = std::time::Instant::now();
        let speech = self.tts.synthesize(&reply, &job.voice)?;
        let speech = f32_to_i16(&linear(
            &i16_to_f32(&speech),
            self.tts.sample_rate(),
            self.spec.sample_rate,
        ));
        timing.tts_ms = ms(t);
        if self.stale(gen) {
            return Ok(Vec::new());
        }
        let n = self.spec.frame_samples.max(1);
        let mut msgs = Vec::with_capacity(speech.len() / n + 3);
        for chunk in speech.chunks(n) {
            let mut samples = chunk.to_vec();
            samples.resize(n, 0);
            msgs.push(Msg::Event(BackendEvent::Audio(Frame {
                seq: *out_seq,
                samples,
            })));
            *out_seq += 1;
        }
        msgs.push(Msg::Timing(timing));
        msgs.push(Msg::Event(BackendEvent::TurnDone));
        Ok(msgs)
    }
}

/// Engines waiting for `start()`, then the running worker.
enum Engine<S, M, T> {
    Parts(S, M, T),
    Running {
        jobs: Sender<Job>,
        results: Receiver<(u64, Msg)>,
        handle: Option<JoinHandle<()>>,
    },
    Gone,
}

pub struct CpuPipelineBackend<S, M, T> {
    engine: Engine<S, M, T>,
    spec: FrameSpec,
    system_prompt: Arc<str>,
    voice: Arc<str>,
    started: bool,
    closed: bool,
    utterance: Vec<i16>,
    queue: VecDeque<BackendEvent>,
    gen: Arc<AtomicU64>,
    pending: bool,
    pub last_transcript: String,
    pub last_timing: TurnTiming,
}

impl<S, M, T> CpuPipelineBackend<S, M, T>
where
    S: SpeechToText + Send + 'static,
    M: TextModel + Send + 'static,
    T: TextToSpeech + Send + 'static,
{
    /// The engines move to the worker thread on `start()`.
    pub fn new(stt: S, model: M, tts: T, spec: FrameSpec) -> Self {
        CpuPipelineBackend {
            engine: Engine::Parts(stt, model, tts),
            spec,
            system_prompt: Arc::from(""),
            voice: Arc::from(""),
            started: false,
            closed: false,
            utterance: Vec::new(),
            queue: VecDeque::new(),
            gen: Arc::new(AtomicU64::new(0)),
            pending: false,
            last_transcript: String::new(),
            last_timing: TurnTiming::default(),
        }
    }

    /// True while a turn is with the worker and its reply has not fully come
    /// back yet.
    pub fn busy(&self) -> bool {
        self.pending
    }

    fn ready(&self) -> Result<(), BackendError> {
        if self.closed {
            Err(BackendError::Closed)
        } else if !self.started {
            Err(BackendError::NotStarted)
        } else {
            Ok(())
        }
    }

    /// Invalidate in-flight work and queued output.
    fn cancel(&mut self) -> u64 {
        self.queue.clear();
        self.pending = false;
        self.gen.fetch_add(1, Ordering::AcqRel) + 1
    }

    fn spawn(&mut self) -> Result<(), BackendError> {
        let Engine::Parts(stt, model, tts) = std::mem::replace(&mut self.engine, Engine::Gone)
        else {
            return Ok(());
        };
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (res_tx, results) = mpsc::channel();
        let worker = Worker {
            stt,
            model,
            tts,
            spec: self.spec,
            current: Arc::clone(&self.gen),
            tx: res_tx,
        };
        let handle = std::thread::Builder::new()
            .name("galion-voice-cpu".into())
            // Model code may recurse a little deeper than the 2 MiB default.
            .stack_size(8 << 20)
            .spawn(move || worker.run(job_rx))
            .map_err(|e| BackendError::Transport(format!("spawn worker: {e}")))?;
        self.engine = Engine::Running {
            jobs,
            results,
            handle: Some(handle),
        };
        Ok(())
    }

    /// Move finished worker output into the queue. Err = worker failed a turn.
    fn drain(&mut self) -> Result<(), BackendError> {
        let Engine::Running { results, .. } = &self.engine else {
            return Ok(());
        };
        let current = self.gen.load(Ordering::Acquire);
        let mut failed = None;
        loop {
            match results.try_recv() {
                Ok((gen, msg)) => {
                    if gen != current {
                        continue;
                    }
                    match msg {
                        Msg::Event(ev) => {
                            if ev == BackendEvent::TurnDone {
                                self.pending = false;
                            }
                            self.queue.push_back(ev);
                        }
                        Msg::Transcript(t) => self.last_transcript = t,
                        Msg::Timing(t) => self.last_timing = t,
                        Msg::Failed(e) => {
                            self.pending = false;
                            failed = Some(e);
                        }
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Worker died (panic in a model): the session falls back.
                    self.pending = false;
                    self.queue
                        .push_back(BackendEvent::Unavailable("cpu worker stopped".into()));
                    self.engine = Engine::Gone;
                    break;
                }
            }
        }
        match failed {
            Some(e) => Err(BackendError::Protocol(e)),
            None => Ok(()),
        }
    }
}

impl<S, M, T> VoiceBackend for CpuPipelineBackend<S, M, T>
where
    S: SpeechToText + Send + 'static,
    M: TextModel + Send + 'static,
    T: TextToSpeech + Send + 'static,
{
    fn name(&self) -> &str {
        "cpu-pipeline"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            duplex: DuplexMode::HalfWithBargeIn,
            spec: self.spec,
            emits_text: true,
        }
    }

    fn start(&mut self, persona: &dyn PersonaSource) -> Result<(), BackendError> {
        if self.closed {
            return Err(BackendError::Closed);
        }
        self.system_prompt = Arc::from(persona.text_prompt());
        self.voice = Arc::from(persona.voice_prompt());
        self.spawn()?;
        self.started = true;
        Ok(())
    }

    fn send_audio(&mut self, frame: &Frame) -> Result<(), BackendError> {
        self.ready()?;
        // Cap one utterance at 30 s (the Whisper window).
        if self.utterance.len() < self.spec.sample_rate as usize * 30 {
            self.utterance.extend_from_slice(&frame.samples);
        }
        Ok(())
    }

    /// Hands the utterance to the worker and returns at once.
    fn end_of_user_turn(&mut self) -> Result<(), BackendError> {
        self.ready()?;
        // A new turn supersedes whatever the worker was still doing.
        let gen = self.cancel();
        let job = Job {
            gen,
            pcm: std::mem::take(&mut self.utterance),
            system: Arc::clone(&self.system_prompt),
            voice: Arc::clone(&self.voice),
        };
        match &self.engine {
            Engine::Running { jobs, .. } if jobs.send(job).is_ok() => {
                self.pending = true;
                Ok(())
            }
            _ => Err(BackendError::Transport("cpu worker stopped".into())),
        }
    }

    fn interrupt(&mut self) -> Result<(), BackendError> {
        self.ready()?;
        self.cancel();
        self.utterance.clear();
        Ok(())
    }

    fn poll(&mut self) -> Result<Vec<BackendEvent>, BackendError> {
        self.ready()?;
        self.drain()?;
        let mut out = Vec::new();
        while let Some(ev) = self.queue.pop_front() {
            let audio = matches!(ev, BackendEvent::Audio(_));
            out.push(ev);
            if audio {
                break;
            }
        }
        Ok(out)
    }

    /// Non-blocking: the worker exits after its current stage.
    fn close(&mut self) {
        self.closed = true;
        self.cancel();
        // Dropping the job sender ends the worker loop; it is detached so a
        // running Whisper call never blocks close().
        if let Engine::Running { handle, .. } = &mut self.engine {
            drop(handle.take());
        }
        self.engine = Engine::Gone;
    }
}

impl<S, M, T> Drop for CpuPipelineBackend<S, M, T> {
    fn drop(&mut self) {
        // Stale generation: the worker drops anything still queued.
        self.gen.fetch_add(1, Ordering::AcqRel);
    }
}

/// Test / offline stand-ins. No network, no paid API.
pub mod mocks {
    use super::*;
    use galion_voice_core::audio::synth;

    /// Reports how much audio it heard (proves the audio reached it).
    pub struct MockStt;
    impl SpeechToText for MockStt {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
            Ok(format!("heard {} ms", pcm.len() / 16))
        }
    }

    /// `MockStt` that takes a fixed wall time, like a real model.
    pub struct SlowStt(pub std::time::Duration);
    impl SpeechToText for SlowStt {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn transcribe(&mut self, pcm: &[f32]) -> Result<String, String> {
            std::thread::sleep(self.0);
            MockStt.transcribe(pcm)
        }
    }

    /// Echo "brain": replies with the persona's first word and the user text.
    pub struct EchoModel;
    impl TextModel for EchoModel {
        fn reply(&mut self, system: &str, user: &str) -> Result<String, String> {
            let who = system.split_whitespace().next().unwrap_or("assistant");
            Ok(format!("{who} heard: {user}"))
        }
    }

    /// One 120 ms beep per word at 16 kHz. Licence-clean placeholder TTS.
    pub struct BeepTts;
    impl TextToSpeech for BeepTts {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn synthesize(&mut self, text: &str, _voice: &str) -> Result<Vec<i16>, String> {
            let mut out = Vec::new();
            for (i, _) in text.split_whitespace().enumerate() {
                out.extend(synth::tone(16_000, 520.0, 0.25, 1920, i * 1920));
                out.extend(std::iter::repeat_n(0i16, 640));
            }
            Ok(out)
        }
    }

    /// Always fails (error path).
    pub struct FailingTts;
    impl TextToSpeech for FailingTts {
        fn sample_rate(&self) -> u32 {
            16_000
        }
        fn synthesize(&mut self, _: &str, _: &str) -> Result<Vec<i16>, String> {
            Err("tts broke".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::mocks::*;
    use super::*;
    use galion_voice_core::audio::synth;
    use galion_voice_core::persona::StaticPersona;
    use galion_voice_core::session::{Session, SessionConfig, SessionEvent, State};
    use std::time::{Duration, Instant};

    fn talk(t: usize) -> Frame {
        Frame {
            seq: t as u64,
            samples: synth::tone(24_000, 180.0, 0.3, 1920, t * 1920),
        }
    }
    fn quiet(t: usize) -> Frame {
        Frame {
            seq: t as u64,
            samples: synth::noise(t as u32 + 1, 0.002, 1920),
        }
    }
    fn persona() -> StaticPersona {
        StaticPersona::new("Galion is a calm guide.", "v1")
    }

    #[test]
    fn pipeline_turn_then_barge_in_through_a_session() {
        let spec = FrameSpec::MOSHI;
        let be = CpuPipelineBackend::new(MockStt, EchoModel, BeepTts, spec);
        let mut s = Session::new(be, SessionConfig::default());
        s.start(&persona()).unwrap();
        let mut texts = Vec::new();
        let mut played = 0;
        let mut barge_at = None;
        // 1 s of speech; silence until 3 reply frames played; talk again for
        // 1 s (barge-in); then silence until the second reply is in.
        for t in 0..5000 {
            let speak = t < 12 || (played >= 3 && barge_at.is_none_or(|b| t < b + 12));
            let f = if speak { talk(t) } else { quiet(t) };
            let out = s.tick(&f).unwrap();
            for e in out.events {
                match e {
                    SessionEvent::Text(x) => texts.push(x),
                    SessionEvent::BargeIn => barge_at = Some(t),
                    _ => {}
                }
            }
            if out.playout.is_some() {
                played += 1;
            }
            if texts.len() >= 4 && !s.backend().busy() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(barge_at.is_some(), "barge-in happened");
        assert!(texts[1].starts_with("Galion heard: heard "), "{texts:?}");
        // Speech 12 frames + VAD hangover 6 = 18 frames * 80 ms reached STT.
        assert_eq!(texts[0], "user: heard 1440 ms");
        assert_eq!(texts.len(), 4, "{texts:?}");
        // Last timing is the barge-in turn (onset preroll 1 + 12 + hangover 6).
        let tm = s.backend().last_timing;
        assert!(tm.utterance_ms > 0, "{tm:?}");
        assert_eq!(s.stats().barge_ins, 1);
        assert!(s.stats().user_turns >= 2);
        assert_ne!(s.state(), State::Closed);
    }

    /// The tick that ends the turn must not wait for a 400 ms STT.
    #[test]
    fn end_of_turn_tick_does_not_block_on_stt() {
        let stt = SlowStt(Duration::from_millis(400));
        let be = CpuPipelineBackend::new(stt, EchoModel, BeepTts, FrameSpec::MOSHI);
        let mut s = Session::new(be, SessionConfig::default());
        s.start(&persona()).unwrap();
        let mut worst = Duration::ZERO;
        let mut end_tick = None;
        let mut first_audio = None;
        let began = Instant::now();
        for t in 0..2000 {
            let f = if t < 12 { talk(t) } else { quiet(t) };
            let t0 = Instant::now();
            let out = s.tick(&f).unwrap();
            worst = worst.max(t0.elapsed());
            if out.events.contains(&SessionEvent::UserSpeechEnd) {
                end_tick = Some(began.elapsed());
            }
            if out.playout.is_some() && first_audio.is_none() {
                first_audio = Some(began.elapsed());
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        let (end, audio) = (end_tick.unwrap(), first_audio.unwrap());
        assert!(
            worst < Duration::from_millis(100),
            "a tick blocked {worst:?}"
        );
        // The reply really came from the slow worker, later.
        assert!(
            audio >= end + Duration::from_millis(390),
            "{end:?} {audio:?}"
        );
        assert!(s.backend().last_timing.stt_ms >= 400);
    }

    /// Barge-in while the worker is still inside STT: that reply never plays.
    #[test]
    fn interrupt_during_stt_drops_the_reply() {
        let stt = SlowStt(Duration::from_millis(150));
        let mut be = CpuPipelineBackend::new(stt, EchoModel, BeepTts, FrameSpec::MOSHI);
        be.start(&persona()).unwrap();
        be.send_audio(&talk(0)).unwrap();
        be.end_of_user_turn().unwrap();
        assert!(be.busy());
        std::thread::sleep(Duration::from_millis(20));
        be.interrupt().unwrap();
        assert!(!be.busy());
        std::thread::sleep(Duration::from_millis(300));
        for _ in 0..50 {
            assert!(be.poll().unwrap().is_empty(), "stale reply leaked");
        }
        // The next turn still works.
        be.send_audio(&talk(1)).unwrap();
        be.end_of_user_turn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got = Vec::new();
        while !got.contains(&BackendEvent::TurnDone) && Instant::now() < deadline {
            got.extend(be.poll().unwrap());
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            got.contains(&BackendEvent::Text("user: heard 80 ms".into())),
            "{got:?}"
        );
        assert!(got.iter().any(|e| matches!(e, BackendEvent::Audio(_))));
    }

    /// A newer turn supersedes an older one still in flight.
    #[test]
    fn new_turn_supersedes_pending_turn() {
        let stt = SlowStt(Duration::from_millis(100));
        let mut be = CpuPipelineBackend::new(stt, EchoModel, BeepTts, FrameSpec::MOSHI);
        be.start(&persona()).unwrap();
        be.send_audio(&talk(0)).unwrap();
        be.end_of_user_turn().unwrap();
        for t in 1..3 {
            be.send_audio(&talk(t)).unwrap();
        }
        be.end_of_user_turn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut texts = Vec::new();
        while be.busy() && Instant::now() < deadline {
            for e in be.poll().unwrap() {
                if let BackendEvent::Text(t) = e {
                    texts.push(t);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            texts.first().map(String::as_str),
            Some("user: heard 160 ms")
        );
        assert!(
            !texts.iter().any(|t| t.contains("heard 80 ms")),
            "{texts:?}"
        );
    }

    #[test]
    fn worker_error_surfaces_on_poll_and_close_does_not_block() {
        let stt = SlowStt(Duration::from_millis(10));
        let mut be = CpuPipelineBackend::new(stt, EchoModel, FailingTts, FrameSpec::MOSHI);
        be.start(&persona()).unwrap();
        be.send_audio(&talk(0)).unwrap();
        be.end_of_user_turn().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let err = loop {
            match be.poll() {
                Err(e) => break e,
                Ok(_) if Instant::now() > deadline => panic!("no error"),
                Ok(_) => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        assert_eq!(err, BackendError::Protocol("tts broke".into()));
        // Start a slow turn and close while it runs.
        be.send_audio(&talk(1)).unwrap();
        be.end_of_user_turn().unwrap();
        let t0 = Instant::now();
        be.close();
        assert!(t0.elapsed() < Duration::from_millis(5));
        assert_eq!(be.poll(), Err(BackendError::Closed));
    }
}
