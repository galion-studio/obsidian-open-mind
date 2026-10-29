// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Thin run-time binding to the official Moonshine C API (`libmoonshine.so`,
//! moonshine-ai/moonshine `core/moonshine-c-api.h`, header version 30000,
//! release v0.1.5 @ 234f60f). Only the handful of transcription calls this
//! service needs are declared. The struct layouts below mirror the header
//! exactly; `layout_matches_header` in the tests pins their sizes.
//!
//! The library is opened with `dlopen` (libloading) from an explicit path,
//! never from the default search path, after its sha256 has been checked
//! (see `crate::models`).

use std::ffi::{c_char, c_void, CStr};
use std::path::Path;

/// `MOONSHINE_HEADER_VERSION` of the header these declarations mirror.
pub const HEADER_VERSION: i32 = 30000;
/// `MOONSHINE_MODEL_ARCH_TINY_STREAMING`.
pub const MODEL_ARCH_TINY_STREAMING: u32 = 2;
/// `MOONSHINE_FLAG_FORCE_UPDATE`.
pub const FLAG_FORCE_UPDATE: u32 = 1;

/// `struct moonshine_option_t`.
#[repr(C)]
pub struct MoonshineOption {
    pub name: *const c_char,
    pub value: *const c_char,
}

/// `struct transcript_line_t`.
#[repr(C)]
pub struct TranscriptLine {
    pub text: *const c_char,
    pub audio_data: *const f32,
    pub audio_data_count: usize,
    pub start_time: f32,
    pub duration: f32,
    pub id: u64,
    pub is_complete: i8,
    pub is_updated: i8,
    pub is_new: i8,
    pub has_text_changed: i8,
    pub have_speakers_changed: i8,
    pub speaker_spans: *const c_void,
    pub speaker_span_count: u64,
    pub last_transcription_latency_ms: u32,
    pub words: *const c_void,
    pub word_count: u64,
}

/// `struct transcript_t`.
#[repr(C)]
pub struct Transcript {
    pub lines: *mut TranscriptLine,
    pub line_count: u64,
}

type GetVersion = unsafe extern "C" fn() -> i32;
type ErrorToString = unsafe extern "C" fn(i32) -> *const c_char;
type LoadFromFiles =
    unsafe extern "C" fn(*const c_char, u32, *const MoonshineOption, u64, i32) -> i32;
type FreeTranscriber = unsafe extern "C" fn(i32);
type CreateStream = unsafe extern "C" fn(i32, u32) -> i32;
type StreamOp = unsafe extern "C" fn(i32, i32) -> i32;
type AddAudio = unsafe extern "C" fn(i32, i32, *const f32, u64, i32, u32) -> i32;
type TranscribeStream = unsafe extern "C" fn(i32, i32, u32, *mut *mut Transcript) -> i32;

/// Resolved function pointers. The `Library` is kept alive for as long as
/// any pointer may be called, and is never unloaded while the process runs.
pub struct Api {
    _lib: libloading::Library,
    pub get_version: GetVersion,
    pub error_to_string: ErrorToString,
    pub load_transcriber_from_files: LoadFromFiles,
    pub free_transcriber: FreeTranscriber,
    pub create_stream: CreateStream,
    pub free_stream: StreamOp,
    pub start_stream: StreamOp,
    pub stop_stream: StreamOp,
    pub add_audio: AddAudio,
    pub transcribe_stream: TranscribeStream,
}

impl Api {
    /// Open `libmoonshine.so` at exactly `path`. Its `$ORIGIN` RUNPATH finds
    /// `libonnxruntime.so.1` in the same directory.
    pub fn open(path: &Path) -> Result<Api, String> {
        // SAFETY: loading a shared library runs its initialisers. The file's
        // sha256 is verified against the pinned value before this call.
        let lib = unsafe { libloading::Library::new(path) }
            .map_err(|e| format!("dlopen {}: {e}", path.display()))?;
        // SAFETY: each symbol is looked up by its exported C name and cast to
        // the signature declared in moonshine-c-api.h (header 30000).
        unsafe {
            Ok(Api {
                get_version: sym(&lib, b"moonshine_get_version\0")?,
                error_to_string: sym(&lib, b"moonshine_error_to_string\0")?,
                load_transcriber_from_files: sym(&lib, b"moonshine_load_transcriber_from_files\0")?,
                free_transcriber: sym(&lib, b"moonshine_free_transcriber\0")?,
                create_stream: sym(&lib, b"moonshine_create_stream\0")?,
                free_stream: sym(&lib, b"moonshine_free_stream\0")?,
                start_stream: sym(&lib, b"moonshine_start_stream\0")?,
                stop_stream: sym(&lib, b"moonshine_stop_stream\0")?,
                add_audio: sym(&lib, b"moonshine_transcribe_add_audio_to_stream\0")?,
                transcribe_stream: sym(&lib, b"moonshine_transcribe_stream\0")?,
                _lib: lib,
            })
        }
    }

    /// Human-readable text for a Moonshine error code.
    pub fn error_text(&self, code: i32) -> String {
        // SAFETY: the library returns a static NUL-terminated string or NULL.
        let p = unsafe { (self.error_to_string)(code) };
        if p.is_null() {
            return format!("moonshine error {code}");
        }
        // SAFETY: non-null, NUL-terminated, static lifetime per the header.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy();
        format!("moonshine error {code}: {s}")
    }
}

unsafe fn sym<T: Copy>(lib: &libloading::Library, name: &[u8]) -> Result<T, String> {
    let s: libloading::Symbol<T> = lib.get(name).map_err(|e| {
        format!(
            "missing symbol {}: {e}",
            String::from_utf8_lossy(&name[..name.len() - 1])
        )
    })?;
    Ok(*s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_header() {
        // x86_64 / aarch64 LP64 sizes of the C structs in header 30000.
        if std::mem::size_of::<usize>() == 8 {
            assert_eq!(std::mem::size_of::<MoonshineOption>(), 16);
            assert_eq!(std::mem::size_of::<Transcript>(), 16);
            // text 8, audio 8, count 8, start 4, dur 4, id 8, 5 x i8 (+3 pad),
            // spans 8, span_count 8, latency 4 (+4 pad), words 8, word_count 8
            // = 88, checked against gcc offsetof on the v0.1.5 header.
            assert_eq!(std::mem::size_of::<TranscriptLine>(), 88);
            assert_eq!(std::mem::offset_of!(TranscriptLine, id), 32);
            assert_eq!(std::mem::offset_of!(TranscriptLine, is_complete), 40);
            assert_eq!(std::mem::offset_of!(TranscriptLine, speaker_spans), 48);
            assert_eq!(
                std::mem::offset_of!(TranscriptLine, last_transcription_latency_ms),
                64
            );
            assert_eq!(std::mem::offset_of!(TranscriptLine, words), 72);
        }
    }
}
