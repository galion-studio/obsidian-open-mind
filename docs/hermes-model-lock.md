<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes model lock

Owner rule (2026-10-31): Hermes must not choose its own model. Grok 4.7
(`x-ai/grok-4.7` on OpenRouter) is the fixed choice.

Turn it on in the unit environment (never config.yaml, which Hermes's tools
cannot write anyway):

    Environment=HERMES_RS_MODEL_LOCK=x-ai/grok-4.7
    # optional, default openrouter
    Environment=HERMES_RS_MODEL_LOCK_PROVIDER=openrouter

With the lock on:

| Path | Before | With the lock |
|---|---|---|
| API `model` / `provider` (`/v1/chat/completions`, `/v1/runs`) | any alias, route or raw id | 400 `model_locked` unless generic (`hermes-agent`, `default`, empty) or the locked id; logged + nerve `model_lock_refused` |
| `POST /api/sessions/{id}/model` | pins any model | 403 `model_locked` for anything else |
| stored session pin (e.g. 50 Opus rows in rs-state.db) | reused on resume | ignored, rewritten to the lock (`model_lock_forced`) |
| `model_aliases`, api_server `model_routes` | boss->Opus, worker->Sonnet, river-*, galion->Haiku | dropped at load unless they point to the lock (logged) |
| `model.default` missing | code default Opus | the lock |
| delegate_task children (`delegation.model`, `HERMES_RS_DELEGATION_MODEL=river-flash`) | Sonnet / River DeepSeek | the lock |
| cron (`cron.model`, job `model`) | River DeepSeek / galion->Haiku | the lock |
| vision_analyze (`HERMES_RS_VISION_MODEL`, free Nemotron fallback) | default + free fallback | the lock only |
| `GET /v1/models` | every route | `hermes-agent` + the lock |
| backstop | none | `Agent::run` and `LockedProvider` refuse any target that is not the lock |
| self-modify | could propose code changing model resolution | `model_lock.rs` + its tests are protected; any added or removed patch line mentioning `model_lock` fails the gate check |

The agent has no tool that takes a model or provider argument (tested).
Without `HERMES_RS_MODEL_LOCK` nothing changes.

Tests: `cargo test -p hermes-rs model_lock` (unit) and
`cargo test -p hermes-rs --test model_lock` (loopback gateway, live config shape).
