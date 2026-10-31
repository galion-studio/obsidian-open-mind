// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Video posters and media facts through the host's `ffmpeg` / `ffprobe`
//! (a separate process, not linked; off unless a path is configured). Every
//! run has a wall timeout, one thread, `-nostdin`, and reads only the one
//! file it was given. Without ffmpeg the explorer still plays video: the
//! browser fetches the first frames itself (`preload=metadata`).

use crate::{Error, Result};
use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Wall time for one ffmpeg / ffprobe run.
pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Largest poster accepted from ffmpeg.
pub const MAX_POSTER: usize = 2 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    /// `ffprobe` next to `ffmpeg` when present.
    pub ffprobe: Option<PathBuf>,
}

impl Tools {
    pub fn new(ffmpeg: PathBuf) -> Tools {
        let probe = ffmpeg.with_file_name("ffprobe");
        Tools {
            ffprobe: probe.is_file().then_some(probe),
            ffmpeg,
        }
    }
}

fn run(cmd: &mut Command, max: usize) -> Result<Vec<u8>> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| Error::Io(format!("ffmpeg did not start: {}", e.kind())))?;
    let mut out = child
        .stdout
        .take()
        .ok_or_else(|| Error::Io("no stdout".into()))?;
    let reader = std::thread::spawn(move || {
        let mut v = Vec::new();
        let _ = (&mut out).take(max as u64 + 1).read_to_end(&mut v);
        v
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() > TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Io("ffmpeg timed out".into()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(25)),
            Err(e) => return Err(Error::Io(e.kind().to_string())),
        }
    }
    let v = reader.join().unwrap_or_default();
    if v.len() > max {
        return Err(Error::TooLarge("ffmpeg output too large".into()));
    }
    Ok(v)
}

/// One JPEG frame, `width` pixels wide, from about one second in (or the
/// first frame for short clips and still images).
pub fn poster(t: &Tools, file: &Path, width: u32, at_secs: f64) -> Result<Vec<u8>> {
    let width = width.clamp(64, 1280);
    let scale = format!("scale={width}:-2");
    let at = format!("{at_secs:.2}");
    let mut cmd = Command::new(&t.ffmpeg);
    cmd.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-threads",
        "1",
    ]);
    if at_secs > 0.0 {
        cmd.args(["-ss", &at]);
    }
    cmd.arg("-i").arg(file).args([
        "-frames:v",
        "1",
        "-vf",
        &scale,
        "-f",
        "image2",
        "-c:v",
        "mjpeg",
        "-q:v",
        "5",
        "pipe:1",
    ]);
    let v = run(&mut cmd, MAX_POSTER)?;
    if v.starts_with(b"\xff\xd8") {
        return Ok(v);
    }
    if at_secs > 0.0 {
        // Shorter than `at_secs`: take the first frame.
        return poster(t, file, width, 0.0);
    }
    Err(Error::Unsupported("ffmpeg could not read a frame".into()))
}

/// Duration, size and codecs from ffprobe (`None` when ffprobe is missing).
pub fn probe(t: &Tools, file: &Path) -> Option<Value> {
    let p = t.ffprobe.as_ref()?;
    let mut cmd = Command::new(p);
    cmd.args(["-v", "error", "-print_format", "json", "-show_entries",
        "format=duration,bit_rate,format_name:stream=codec_type,codec_name,width,height,r_frame_rate,sample_rate,channels"])
        .arg(file);
    let out = run(&mut cmd, 256 << 10).ok()?;
    let v: Value = serde_json::from_slice(&out).ok()?;
    let fmt = &v["format"];
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let video = streams.iter().find(|s| s["codec_type"] == "video");
    let audio = streams.iter().find(|s| s["codec_type"] == "audio");
    let num = |x: &Value| x.as_str().and_then(|s| s.parse::<f64>().ok());
    Some(serde_json::json!({
        "duration_s": num(&fmt["duration"]),
        "bit_rate": num(&fmt["bit_rate"]),
        "container": fmt["format_name"],
        "video": video.map(|s| serde_json::json!({"codec": s["codec_name"], "width": s["width"], "height": s["height"], "fps": s["r_frame_rate"]})),
        "audio": audio.map(|s| serde_json::json!({"codec": s["codec_name"], "sample_rate": s["sample_rate"], "channels": s["channels"]})),
    }))
}
