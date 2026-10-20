// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Human-readable plan of what the relay would do with a given config.
//! Never prints secret values: env vars are reported as set / not set.

use crate::config::{RelayConfig, MIMI_FRAME_MS};
use crate::protocol::{extract_role_prompt, upstream_url};

/// Facts about the environment, gathered by the caller (keeps this pure).
#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub now: String,
    pub auth_token_set: bool,
    pub store_key_set: bool,
    /// Contents of `upstream.role_prompt_file`, if readable.
    pub role_prompt_markdown: Option<String>,
    pub speaker_model_present: bool,
}

/// Host part of a ws/wss/http URL, for display.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    rest.split(['/', '?']).next().unwrap_or(rest)
}

fn set(b: bool) -> &'static str {
    if b {
        "set"
    } else {
        "NOT set"
    }
}

pub fn render(cfg: &RelayConfig, env: &Environment) -> Vec<String> {
    let mut l = Vec::new();
    l.push(format!("persona-plex-relay plan  ({})", env.now));
    l.push("mode: SCAFFOLD - loads config and prints this plan; no listener, no upstream connection, no model".into());
    l.push(String::new());
    l.push("[relay]".into());
    l.push(format!(
        "  would listen on     {} (not bound)",
        cfg.relay.bind
    ));
    l.push(format!(
        "  max sessions        {} (PersonaPlex server serves one conversation at a time)",
        cfg.relay.max_sessions
    ));
    l.push(format!(
        "  idle timeout        {} s",
        cfg.relay.idle_timeout_s
    ));
    l.push(format!(
        "  client token        env {} is {}",
        cfg.relay.auth_token_env,
        set(env.auth_token_set)
    ));
    l.push(String::new());
    l.push("[upstream / audio band]".into());
    l.push(format!(
        "  GPU upstream        {} -> host {}",
        if cfg.upstream.enabled {
            "ENABLED"
        } else {
            "disabled (no GPU provisioned)"
        },
        host_of(&cfg.upstream.url)
    ));
    l.push(format!(
        "  voice prompt        {}",
        cfg.upstream.voice_prompt
    ));
    match env.role_prompt_markdown.as_deref().map(extract_role_prompt) {
        Some(Some(p)) => {
            l.push(format!(
                "  role prompt         {} chars, {} lines, from {}",
                p.chars().count(),
                p.lines().count(),
                cfg.upstream.role_prompt_file
            ));
            let url = upstream_url(&cfg.upstream.url, &cfg.upstream.voice_prompt, &p);
            l.push(format!(
                "  upstream request    GET {} ... ({} bytes)",
                host_of(&url),
                url.len()
            ));
        }
        Some(None) => l.push(format!(
            "  role prompt         NOT FOUND in {}",
            cfg.upstream.role_prompt_file
        )),
        None => l.push(format!(
            "  role prompt         file unreadable: {}",
            cfg.upstream.role_prompt_file
        )),
    }
    l.push(format!(
        "  audio               {} Hz {} in Ogg, frames of {} ms ({} Mimi frames); user->model and model->user at once",
        cfg.audio.sample_rate_hz,
        cfg.audio.codec,
        cfg.audio.frame_ms,
        cfg.audio.frame_ms / MIMI_FRAME_MS
    ));
    l.push("  control band        text 0x02, control 0x03, metadata 0x04, error 0x05, ping 0x06; relay-private 0x40..0x42 (never sent upstream)".into());
    l.push(String::new());
    let s = &cfg.speaker;
    l.push("[speaker verification - soft identity signal]".into());
    l.push(format!("  enabled             {}", s.enabled));
    l.push(format!(
        "  model               {} ({}; dim {})",
        s.model_path,
        if env.speaker_model_present {
            "present"
        } else {
            "NOT present - not downloaded"
        },
        s.embedding_dim
    ));
    l.push(format!(
        "  thresholds          accept >= {:.2}, reject < {:.2}, between = uncertain  (PLACEHOLDERS - to be measured)",
        s.accept_threshold, s.reject_threshold
    ));
    l.push(format!(
        "  enrollment          >= {} phrases, each >= {:.2} cosine to centroid",
        s.enrollment_min_phrases, s.enrollment_min_consistency
    ));
    l.push(format!(
        "  continuous          window {} ms, min speech {} ms, identity lost after {} consecutive rejects",
        s.window_ms, s.min_speech_ms, s.max_consecutive_rejects
    ));
    l.push(format!("  liveness required   {}", s.require_liveness));
    l.push(format!(
        "  voiceprint store    {} (encrypted; key env {} is {})",
        s.store_dir,
        s.store_key_env,
        set(env.store_key_set)
    ));
    l.push("  gate                conversation: open | personal: voice | privileged: voice + 2nd factor | money: DENIED".into());
    l.push(String::new());
    l.push("[fallback]".into());
    l.push(format!(
        "  when GPU is off     {} via {}",
        cfg.fallback.mode, cfg.fallback.galion_one_url
    ));
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_never_prints_secret_values() {
        let cfg =
            RelayConfig::from_toml(include_str!("../persona-plex-relay.example.toml")).unwrap();
        let env = Environment {
            now: "2026-10-18T20:00:00+02:00".into(),
            auth_token_set: true,
            store_key_set: false,
            role_prompt_markdown: Some(
                "## Role prompt (text)\n```\nYou are Persona Plex.\n```\n".into(),
            ),
            speaker_model_present: false,
        };
        let text = render(&cfg, &env).join("\n");
        assert!(text.contains("PERSONA_PLEX_RELAY_TOKEN is set"));
        assert!(text.contains("PERSONA_PLEX_VOICEPRINT_KEY is NOT set"));
        assert!(text.contains("not bound"));
        assert!(text.contains("money: DENIED"));
        assert!(text.contains("role prompt         21 chars"));
        assert!(
            !text.contains("text_prompt="),
            "full upstream URL is not printed"
        );
    }

    #[test]
    fn host_extraction() {
        assert_eq!(
            host_of("wss://h.example:8998/api/chat?x=1"),
            "h.example:8998"
        );
        assert_eq!(host_of("http://127.0.0.1:8091"), "127.0.0.1:8091");
    }
}
