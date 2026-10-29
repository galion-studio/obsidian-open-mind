// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Safe wrapper over the Moonshine streaming transcriber, the per-session
//! line tracker that turns Moonshine's line list into partial/final events,
//! and a fixed pool of preloaded transcribers (one per allowed session).

use crate::ffi::{self, Api, MoonshineOption, Transcript};
use crate::protocol::ServerMsg;
use std::ffi::{CStr, CString};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Engine settings passed to `moonshine_load_transcriber_from_files`.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Minimum new audio (seconds) before Moonshine re-analyses the stream.
    pub transcription_interval: f32,
    /// VAD threshold (Moonshine default 0.5).
    pub vad_threshold: f32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig {
            transcription_interval: 0.2,
            vad_threshold: 0.5,
        }
    }
}

/// A copy of one Moonshine line, taken right after `transcribe_stream`.
#[derive(Debug, Clone, PartialEq)]
pub struct LineSnap {
    pub text: String,
    pub start_time: f32,
    pub duration: f32,
    pub is_complete: bool,
    pub has_text_changed: bool,
    pub latency_ms: u32,
}

/// One loaded transcriber (model sessions live inside libmoonshine).
pub struct Transcriber {
    api: Arc<Api>,
    handle: i32,
}

// SAFETY: the transcriber is an integer handle into libmoonshine's own
// handle map (guarded by its `transcriber_map_mutex`). We only ever use one
// `Transcriber` from one thread at a time (it is moved, never shared), which
// also keeps the "transcript memory valid until the next call" rule intact.
unsafe impl Send for Transcriber {}

impl Transcriber {
    /// Load Tiny Streaming from `model_dir`. Privacy-relevant options are
    /// passed explicitly OFF: no input WAV dumps, no text logging, no
    /// speaker ID (which could trigger model downloads in bindings), and no
    /// audio copies in results.
    pub fn load(api: Arc<Api>, model_dir: &Path, cfg: &EngineConfig) -> Result<Self, String> {
        let path = CString::new(model_dir.to_string_lossy().as_bytes())
            .map_err(|_| "model dir contains NUL".to_string())?;
        let pairs: Vec<(CString, CString)> = [
            (
                "transcription_interval",
                format!("{}", cfg.transcription_interval),
            ),
            ("vad_threshold", format!("{}", cfg.vad_threshold)),
            ("return_audio_data", "false".into()),
            ("log_output_text", "false".into()),
            ("log_ort_run", "false".into()),
            ("identify_speakers", "false".into()),
            ("word_timestamps", "false".into()),
        ]
        .into_iter()
        .map(|(k, v)| (CString::new(k).unwrap(), CString::new(v).unwrap()))
        .collect();
        let opts: Vec<MoonshineOption> = pairs
            .iter()
            .map(|(k, v)| MoonshineOption {
                name: k.as_ptr(),
                value: v.as_ptr(),
            })
            .collect();
        // SAFETY: all pointers are valid NUL-terminated strings that outlive
        // the call; `opts` has `opts.len()` entries.
        let h = unsafe {
            (api.load_transcriber_from_files)(
                path.as_ptr(),
                ffi::MODEL_ARCH_TINY_STREAMING,
                opts.as_ptr(),
                opts.len() as u64,
                ffi::HEADER_VERSION,
            )
        };
        if h < 0 {
            return Err(format!("load transcriber: {}", api.error_text(h)));
        }
        Ok(Transcriber { api, handle: h })
    }

    /// Create and start a stream on this transcriber.
    pub fn open_stream(&mut self) -> Result<Stream<'_>, String> {
        // SAFETY: valid transcriber handle.
        let s = unsafe { (self.api.create_stream)(self.handle, 0) };
        if s < 0 {
            return Err(format!("create stream: {}", self.api.error_text(s)));
        }
        // SAFETY: valid handles.
        let r = unsafe { (self.api.start_stream)(self.handle, s) };
        let st = Stream { t: self, handle: s };
        if r != 0 {
            return Err(format!("start stream: {}", st.t.api.error_text(r)));
        }
        Ok(st)
    }
}

impl Drop for Transcriber {
    fn drop(&mut self) {
        // SAFETY: handle came from load and is freed exactly once.
        unsafe { (self.api.free_transcriber)(self.handle) };
    }
}

/// A started stream; freed on drop.
pub struct Stream<'a> {
    t: &'a mut Transcriber,
    handle: i32,
}

impl Stream<'_> {
    /// Buffer audio (cheap; no inference).
    pub fn add_audio(&mut self, samples: &[f32], sample_rate: u32) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        // SAFETY: pointer and length describe a live slice.
        let r = unsafe {
            (self.t.api.add_audio)(
                self.t.handle,
                self.handle,
                samples.as_ptr(),
                samples.len() as u64,
                sample_rate as i32,
                0,
            )
        };
        if r != 0 {
            return Err(format!("add audio: {}", self.t.api.error_text(r)));
        }
        Ok(())
    }

    /// Run the heavy analysis and copy the line list out.
    pub fn transcribe(&mut self, force: bool) -> Result<Vec<LineSnap>, String> {
        let mut out: *mut Transcript = std::ptr::null_mut();
        let flags = if force { ffi::FLAG_FORCE_UPDATE } else { 0 };
        // SAFETY: valid handles and out-pointer.
        let r =
            unsafe { (self.t.api.transcribe_stream)(self.t.handle, self.handle, flags, &mut out) };
        if r != 0 {
            return Err(format!("transcribe: {}", self.t.api.error_text(r)));
        }
        Ok(copy_lines(out))
    }

    /// Stop the stream (no more audio) and drain the rest: every line
    /// in the result is complete.
    ///
    /// libmoonshine only runs its end-of-segment flush (a full re-decode of
    /// the open line) when there is unread audio at stop time; if the last
    /// partial tick already consumed everything, the final would just be the
    /// last partial. So 100 ms of silence goes in first: it costs one extra
    /// decode and always yields the flushed, better final.
    pub fn finish(&mut self) -> Result<Vec<LineSnap>, String> {
        self.add_audio(&[0.0f32; 1600], crate::audio::SAMPLE_RATE)?;
        // SAFETY: valid handles.
        let r = unsafe { (self.t.api.stop_stream)(self.t.handle, self.handle) };
        if r != 0 {
            return Err(format!("stop stream: {}", self.t.api.error_text(r)));
        }
        self.transcribe(false)
    }
}

impl Drop for Stream<'_> {
    fn drop(&mut self) {
        // SAFETY: handle came from create_stream and is freed exactly once.
        unsafe { (self.t.api.free_stream)(self.t.handle, self.handle) };
    }
}

fn copy_lines(t: *mut Transcript) -> Vec<LineSnap> {
    if t.is_null() {
        return Vec::new();
    }
    // SAFETY: libmoonshine owns `t` until the next call on this transcriber;
    // we copy everything out before returning and keep no pointers.
    unsafe {
        let t = &*t;
        if t.lines.is_null() || t.line_count == 0 {
            return Vec::new();
        }
        std::slice::from_raw_parts(t.lines, t.line_count as usize)
            .iter()
            .map(|l| LineSnap {
                text: if l.text.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(l.text).to_string_lossy().into_owned()
                },
                start_time: l.start_time,
                duration: l.duration,
                is_complete: l.is_complete != 0,
                has_text_changed: l.has_text_changed != 0,
                latency_ms: l.last_transcription_latency_ms,
            })
            .collect()
    }
}

/// Turns successive line lists into partial/final events. Pure logic.
#[derive(Debug, Default)]
pub struct LineTracker {
    /// Lines below this index are final (sent or skipped as empty).
    finalized: usize,
    /// Last partial text sent for line `finalized`.
    last_partial: Option<String>,
    pub partials: u64,
    pub finals: u64,
}

impl LineTracker {
    /// Diff `lines` against what was sent. With `all_final` (after stop),
    /// every remaining line is treated as complete.
    pub fn update(&mut self, lines: &[LineSnap], all_final: bool) -> Vec<ServerMsg> {
        let mut out = Vec::new();
        let mut i = self.finalized;
        while i < lines.len() {
            let l = &lines[i];
            let text = l.text.trim().to_string();
            if l.is_complete || all_final {
                // Skip VAD noise that never produced text and never showed a
                // partial; otherwise always close the line for the client.
                if !text.is_empty() || self.last_partial.is_some() {
                    out.push(ServerMsg::Final {
                        line: i as u64,
                        text,
                    });
                    self.finals += 1;
                }
                self.finalized = i + 1;
                self.last_partial = None;
            } else {
                if !text.is_empty() && self.last_partial.as_deref() != Some(text.as_str()) {
                    out.push(ServerMsg::Partial {
                        line: i as u64,
                        text: text.clone(),
                    });
                    self.partials += 1;
                    self.last_partial = Some(text);
                }
                // Only the last line can be incomplete.
                break;
            }
            i += 1;
        }
        out
    }
}

/// Preloaded transcribers; taking one is how the session cap is enforced.
pub struct EnginePool {
    free: Mutex<Vec<Transcriber>>,
    size: usize,
}

impl EnginePool {
    pub fn new(ts: Vec<Transcriber>) -> Arc<Self> {
        let size = ts.len();
        Arc::new(EnginePool {
            free: Mutex::new(ts),
            size,
        })
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn available(&self) -> usize {
        self.free.lock().map(|v| v.len()).unwrap_or(0)
    }

    /// Take a transcriber if one is free.
    pub fn try_take(self: &Arc<Self>) -> Option<PoolGuard> {
        let t = self.free.lock().ok()?.pop()?;
        Some(PoolGuard {
            pool: Arc::clone(self),
            t: Some(t),
        })
    }
}

/// A checked-out transcriber, returned to the pool on drop.
pub struct PoolGuard {
    pool: Arc<EnginePool>,
    t: Option<Transcriber>,
}

impl PoolGuard {
    pub fn transcriber(&mut self) -> &mut Transcriber {
        self.t.as_mut().expect("present until drop")
    }
}

impl Drop for PoolGuard {
    fn drop(&mut self) {
        if let Some(t) = self.t.take() {
            if let Ok(mut v) = self.pool.free.lock() {
                v.push(t);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(text: &str, complete: bool) -> LineSnap {
        LineSnap {
            text: text.into(),
            start_time: 0.0,
            duration: 0.0,
            is_complete: complete,
            has_text_changed: true,
            latency_ms: 0,
        }
    }

    #[test]
    fn partial_then_final() {
        let mut t = LineTracker::default();
        assert_eq!(
            t.update(&[l(" hello", false)], false),
            vec![ServerMsg::Partial {
                line: 0,
                text: "hello".into()
            }]
        );
        // Same text again: nothing new.
        assert!(t.update(&[l("hello", false)], false).is_empty());
        assert_eq!(
            t.update(&[l("hello world", false)], false),
            vec![ServerMsg::Partial {
                line: 0,
                text: "hello world".into()
            }]
        );
        assert_eq!(
            t.update(&[l("Hello world.", true), l("next", false)], false),
            vec![
                ServerMsg::Final {
                    line: 0,
                    text: "Hello world.".into()
                },
                ServerMsg::Partial {
                    line: 1,
                    text: "next".into()
                }
            ]
        );
        // Old complete lines are never re-sent.
        assert!(t
            .update(&[l("Hello world.", true), l("next", false)], false)
            .is_empty());
        assert_eq!((t.partials, t.finals), (3, 1));
    }

    #[test]
    fn stop_finalizes_everything() {
        let mut t = LineTracker::default();
        t.update(&[l("one", false)], false);
        let ev = t.update(&[l("One.", true), l("two", false)], true);
        assert_eq!(
            ev,
            vec![
                ServerMsg::Final {
                    line: 0,
                    text: "One.".into()
                },
                ServerMsg::Final {
                    line: 1,
                    text: "two".into()
                }
            ]
        );
    }

    #[test]
    fn empty_noise_lines_are_skipped() {
        let mut t = LineTracker::default();
        assert!(t.update(&[l("", false)], false).is_empty());
        assert!(t.update(&[l("", true)], false).is_empty());
        let ev = t.update(&[l("", true), l("hi", true)], false);
        assert_eq!(
            ev,
            vec![ServerMsg::Final {
                line: 1,
                text: "hi".into()
            }]
        );
    }

    #[test]
    fn line_whose_partial_vanished_still_gets_a_final() {
        let mut t = LineTracker::default();
        t.update(&[l("uh", false)], false);
        assert_eq!(
            t.update(&[l("", true)], false),
            vec![ServerMsg::Final {
                line: 0,
                text: String::new()
            }]
        );
    }

    #[test]
    fn several_lines_completed_at_once() {
        let mut t = LineTracker::default();
        let ev = t.update(&[l("a", true), l("b", true), l("c", false)], false);
        assert_eq!(ev.len(), 3);
        assert_eq!(t.finals, 2);
    }
}
