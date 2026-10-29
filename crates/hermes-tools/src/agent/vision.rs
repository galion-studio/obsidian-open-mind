// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `vision_analyze`: send one image from the workspace (or a read root) to
//! an image-capable model and return its answer. The prior stack used
//! `auxiliary.vision` = nvidia/nemotron-3.5-super-vl:free (fallback
//! google/gemma-4-31b-it:free). Here the first model is the Hermes default
//! (x-ai/grok-4.7 accepts text+image on OpenRouter), then the free prior
//! model; `HERMES_RS_VISION_MODEL` / `HERMES_RS_VISION_FALLBACK` change it.
//! The spend tracker is checked before and charged after every call.

use super::{cut, files, Shared};
use hermes_rs::config::Config;
use hermes_rs::provider::{resolve_target, ChatRequest, Provider};
use hermes_rs::tools::{Tool, ToolCtx, ToolOutput};
use serde_json::{json, Value};
use std::io::Read;
use std::sync::Arc;

/// Standard base64 with padding.
pub fn b64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// MIME type from the magic bytes; only formats vision models accept.
pub fn sniff(b: &[u8]) -> Option<&'static str> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some("image/webp")
    } else if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

pub struct Vision {
    pub sh: Arc<Shared>,
    pub provider: Arc<dyn Provider>,
    pub hermes: Arc<Config>,
}

impl Tool for Vision {
    fn name(&self) -> String {
        "vision_analyze".into()
    }
    fn description(&self) -> String {
        "Look at an image file in the workspace (PNG, JPEG, WebP or GIF, e.g. a browser screenshot) and answer a question about it with a vision model. Returns the answer and which model gave it.".into()
    }
    fn parameters(&self) -> Value {
        json!({"type": "object", "properties": {
            "path": {"type": "string", "description": "image path, relative to the workspace"},
            "question": {"type": "string", "default": "Describe this image in detail."}
        }, "required": ["path"]})
    }
    fn call(&self, args: &Value, ctx: &ToolCtx) -> ToolOutput {
        let sh = &self.sh;
        if let Err(e) = sh.gate(ctx) {
            return ToolOutput::err(format!("refused: {e}"));
        }
        if let Value::Object(o) = args {
            if let Some(k) = o
                .keys()
                .find(|k| !["path", "question"].contains(&k.as_str()))
            {
                return ToolOutput::err(format!("refused: unknown argument {k:?}"));
            }
        }
        let raw = args["path"].as_str().unwrap_or("");
        let p = match files::read_target(sh, raw) {
            Ok(p) => p,
            Err(e) => return ToolOutput::err(e),
        };
        let max = sh.cfg.vision_max_image_bytes;
        let mut buf = vec![];
        match std::fs::File::open(&p) {
            Ok(f) => {
                if let Err(e) = f.take(max + 1).read_to_end(&mut buf) {
                    return ToolOutput::err(format!("read {raw}: {e}"));
                }
            }
            Err(e) => return ToolOutput::err(format!("open {raw}: {e}")),
        }
        if buf.len() as u64 > max {
            return ToolOutput::err(format!("image is larger than {max} bytes"));
        }
        let Some(mime) = sniff(&buf) else {
            return ToolOutput::err("not a PNG, JPEG, WebP or GIF image");
        };
        let q = args["question"]
            .as_str()
            .filter(|q| !q.trim().is_empty())
            .unwrap_or("Describe this image in detail.");
        let q = cut(q, 2000);
        let data_url = format!("data:{mime};base64,{}", b64(&buf));
        let req = ChatRequest {
            messages: vec![json!({"role": "user", "content": [
                {"type": "text", "text": q},
                {"type": "image_url", "image_url": {"url": data_url}}
            ]})],
            tools: vec![],
            reasoning: None,
            max_tokens: Some(800),
        };
        let mut errors = vec![];
        for model in &sh.cfg.vision_models {
            if let Some(s) = &sh.spend {
                if let Err(b) = s.check(ctx.spend_session) {
                    return ToolOutput::err(format!("refused: {}", b.message()));
                }
            }
            let target = resolve_target(&self.hermes, Some(model), Some("openrouter"));
            sh.log(
                "vision_analyze",
                ctx,
                &format!("{} -> {}", raw, target.model),
            );
            match self.provider.chat(&target, &req, ctx.cancel, &mut |_| {}) {
                Ok(r) => {
                    if let Some(s) = &sh.spend {
                        s.record(ctx.spend_session, &target.model, r.usage.cost_usd);
                    }
                    if r.content.trim().is_empty() {
                        errors.push(format!("{}: empty answer", target.model));
                        continue;
                    }
                    return ToolOutput::ok(
                        json!({
                            "model": target.model,
                            "answer": hermes_rs::redact(&r.content),
                            "cost_usd": r.usage.cost_usd,
                            "image_bytes": buf.len(),
                        })
                        .to_string(),
                    );
                }
                Err(e) => errors.push(format!(
                    "{}: {}",
                    target.model,
                    cut(&hermes_rs::redact(&e), 300)
                )),
            }
        }
        ToolOutput::err(format!("no vision model answered: {}", errors.join(" | ")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        assert_eq!(b64(b""), "");
        assert_eq!(b64(b"f"), "Zg==");
        assert_eq!(b64(b"fo"), "Zm8=");
        assert_eq!(b64(b"foo"), "Zm9v");
        assert_eq!(b64(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn sniff_formats() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"GIF89a"), Some("image/gif"));
        assert_eq!(sniff(b"<svg"), None);
        assert_eq!(sniff(b"#!/bin/sh"), None);
    }
}
