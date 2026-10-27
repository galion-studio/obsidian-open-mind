<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# no-openrouter-mergefix (wave-4 slot 241): real tests

All times Europe/Zagreb (CEST, +02:00). Ports 21110-21119.

## Why
The main-merge (23:32 +02:00) skipped `swarm/no-openrouter-nerve-cortex`
49163e0: its `tests/no_openrouter.rs` source scan failed on the event-mode
`crates/openmind-nerve/src/evt/escalate.rs` on main-staging, because that file
names OpenRouter (to refuse it) and holds an optional `openai` escalator. The
merge also had a one-hunk conflict in `crates/openmind-nerve/src/lib.rs`.
Reproduced 01:09 +02:00: `source_has_no_llm_path` FAILED with
`evt/escalate.rs: openrouter.ai / openrouter / chat/completions`.

## What changed (branch swarm/no-openrouter-mergefix, base 49163e0)
- Merged swarm/main-staging 7e2a12e into the branch; lib.rs resolved as
  staging's version (`deny(unsafe_code)`, `evt`, `feed`, `reflex_audit`) plus
  the `egress` module line and its doc line.
- `egress.rs`: honest docs (the built-in client has no LLM path; the only
  direct LLM client is the optional event-mode `openai` escalator, off by
  default, which refuses OpenRouter and the Hermes gateway); new
  `GUARD_FILES` (`egress.rs`, `evt/escalate.rs`) and `HANDOFF_PATHS`
  (`/event`, `/signal`, `/events`, `/escalations`, `/api/v1/cortex/events`),
  which `check_path` never refuses.
- `evt/escalate.rs::endpoint_allowed` now also uses
  `egress::mentions_forbidden` (one FORBIDDEN list for the crate).
- `sensors/surface.rs::check`: a surface target that names OpenRouter or an
  LLM path is refused before DNS / connect (it was the only unguarded raw
  TCP connect).
- `tests/no_openrouter.rs` (6 tests): source scan (guard files may name
  OpenRouter only on refusal lines and carry no LLM path in production code;
  every other file: no OpenRouter, no LLM path, no OPENROUTER env read);
  egress-site scan (every `curl` and raw `TcpStream::connect` is behind
  `check_url` / `endpoint_allowed` / `check_addr`+`check_path` /
  `mentions_forbidden`; no other network client); loopback client refusal;
  escalator refuses 8 OpenRouter/Hermes URLs at runtime and at config;
  default escalation = `delegate` (zero spend, `Outcome::Delegated`);
  surface probe never resolves an LLM target.
- `tests/real_no_openrouter.rs` (GALION_REAL=1, release binary).
- Test fixtures use TEST-NET (203.0.113.15, 192.0.2.1) instead of public
  IPv4 (repo-check `hosts` 2 errors -> 0).
- Two main-staging lint fixes I made were dropped again because staging
  7c95348 fixed the same lines (duplicate `#[test]` in cortex_replay.rs:
  same fix, merges clean; journal.rs `Msg` lint allow: reverted, staging's
  wins).

## Negative controls (01:15 +02:00): every one made the guard fail
| change | failing test |
|---|---|
| `// call openrouter.ai here` appended to cortex.rs | source_has_no_llm_path |
| alerts.rs curl without `check_url` | every_egress_site_is_guarded |
| escalate.rs without `egress::mentions_forbidden` | every_egress_site_is_guarded |
| `BUILTIN_ENDPOINT = Some(".../v1/chat/completions")` | source_has_no_llm_path |
| surface.rs without the host guard | every_egress_site_is_guarded |

## Real test (release, box), 01:40:43-01:41:07 +02:00 on the throwaway merge
`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_no_openrouter -- --nocapture --test-threads 1`
- `delegate_handoff_with_openrouter_env_present`: event-mode nerve on
  127.0.0.1:21110 with `OPENROUTER_API_KEY` (dummy) and `OPENROUTER_BASE_URL`
  pointing at a decoy listener on 21119; 5 journal failure lines through a
  FIFO -> `escalations=1 outcomes=["delegated"] llm_calls=0 handoff_ms=1005
  decoy_connections=0`; stderr names both env vars, never the value; value
  absent from /stats, /escalations, /inventory and the event log.
- `llm_endpoints_refused_at_start`: `NERVE_EVT_ESCALATE=openai` with
  `https://openrouter.ai/...`, `https://openrouter%2Eai/...`,
  `http://127.0.0.1:8642/...`, `http://hermes.local:21117/...` -> exit 2,
  "refused: OpenRouter endpoints are forbidden" / "refused: escalation via
  Hermes is forbidden"; port 21111 never listened, decoy 21117 0 connections.
- After: no listener on 21110-21119.
- Binary (x86_64 musl): the strings `openrouter` (3) and `chat/completions`
  (1) appear only in the deny lists and refusal messages.

## VPS, read-only (01:19:37 +02:00)
- nerve :8670 /health 200; /stats escalate mode `delegate`,
  `paid_enabled false`, `endpoint_host null`, `escalation_refused 0`,
  escalations_total 3; /escalations: all 3 `outcome: delegated`.
- gaia-cortex :8674 /health: `mode live`, `hermes_handoff true`,
  `handoffs 3`, `llm_enabled true`, `incidents_open 1`; :8672 200.
- `/var/lib/gaia-cortex/hermes-inbox/done/`: 3 handoff files; the Hermes
  spend file holds matching `cortex-hoff-*` sessions (Hermes did the paid
  call, not the cortex). Hermes day total (all callers) $0.188.
- `ss -tnp`: 0 non-loopback sockets of openmind-nerve / gaia-cortex.
- journal 24 h (galion-nerve, gaia-cortex): 0 lines naming openrouter.

So live matches the guard: the nerve escalates by delegating, the cortex
hands off to Hermes through its inbox, only Hermes spends.
