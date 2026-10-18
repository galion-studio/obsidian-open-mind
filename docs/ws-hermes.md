# Workstream: Hermes (upgrade plan + Rust port)

Branch `ws/hermes`. Written 2026-10-18 ~21:30 +02:00 (Europe/Zagreb).
Production was not touched. Hermes on the VPS is still upstream Python `hermes-agent` 0.21.4 (`hermes.service`, 127.0.0.1:8642). Nothing was upgraded, restarted, installed or switched. No OpenRouter or River calls were made; all tests use the mock provider.

## Summary

| Item | State |
|---|---|
| Upgrade | Plan only (below). Latest upstream is **v0.22.2 (2026-10-15)**. Needs Slavik's approval. |
| Rust port | `crates/hermes-rs` (library) + `crates/hermes-gateway` (binary). v1 covers what Galion uses. |
| Persona Plex | `crates/openmind-persona` M1 done, wired into the galion-one channel. Opt-in. |
| Staged unit | `deploy/staged/hermes-rs.service`. Shadow on :8652 with `--mock`. Not installed. |
| Size / RSS | 942 KB static (x86_64-musl). RSS 1.2 MB after 20 streamed chats with `--mock`. Python Hermes RSS is 303 MB. |

## Upgrade

### Where things stand

- On the VPS, `/opt/galion/tooling/hermes-agent` is an editable git install at `fde4997` (2026-09-22) reporting 0.21.4.
- It carries about 609 lines of local Galion patches (`git diff`), and `tests/` and `evals/` were deleted.
- Runtime home is `/opt/galion/hermes`. `config.yaml` is `_config_version: 45`.

### Releases since 0.21.4

| Tag | Date | Notes |
|---|---|---|
| v2026.9.24 / v0.21.5 | 2026-09-24 | Desktop plugin SDK wave. No server-side change that affects Galion. |
| v0.21.6 | 2026-10-08 | **Security fixes.** Dashboard auth (spoofed X-Forwarded-For, body caps, loopback-only login codes); repo git filter programs no longer run on automatic git calls; email allowlist display-name spoofing. Galion uses neither the dashboard nor email, but the git-filter fix applies to subagent worktrees. |
| v0.21.7 | 2026-10-11 | `hermes update` on a source install now follows **tagged releases**, not `main`. An install newer than the latest tag stays put. Reasoning survives resume. |
| v0.22.0 "Threshold" | 2026-10-14 | Big release. Breaking items are listed below. |
| v0.22.1 | 2026-10-14 | **Built from the wrong commit.** Do not install. |
| v0.22.2 | 2026-10-15 | What 0.22.1 should have been. The gateway starts after a reboot again (PID-reuse boot guard), and long local-model turns no longer break. **Known issue:** sessions that ran under the 0.22.1 gateway can't be deleted. Galion never ran 0.22.1, so this doesn't apply. |

### Breaking changes vs Galion's config

| Area | 0.22 change | Effect on Galion | Action |
|---|---|---|---|
| Runtime | Managed installs move to Python 3.14 (`hermes pm`). | The editable git venv could be replaced or diverge. KittenTTS isn't available (unused). | Stage in a separate clone/venv. Don't let `hermes update` manage the prod venv. |
| Multiplex | One gateway per machine is the default. `hermes update` folds per-profile gateways into one, and profiles are served at `/p/<profile>/`. | Galion has one default profile and one `hermes.service`. hermes.ts calls the root paths. The fold should be a no-op, but the boot guard and host lock are new. | Check `hermes gateway status` in staging. Keep `hermes.service` unchanged. hermes-rs already mirrors `/p/default/*`. |
| Sessions | `session_reset` idle/daily is retired. **`sessions.auto_prune` now defaults to ON** (deletes ended sessions inactive >90 days at startup). | Old Galion sessions in `state.db` would be deleted on first start. | **Set `sessions.auto_prune: false` before upgrading.** Back up state.db. |
| Model pinning | Unchanged upstream. Galion's "stored session model applies only when the turn named none" is a **local patch**. | `hermes update` / `git pull` would conflict with or drop the patch. | Re-apply `galion-patches.diff` on top of the tag, then run the session-pin test (hermes-rs `session_history_and_model_pinning` describes the expected behaviour). |
| delegate_task | Live subagents with steer/stop; `delegation.fallback_providers`; `child_timeout_seconds` (inactivity); failed children surface at once. | Caps (`max_concurrent_children` 3, depth 1) are kept. Galion's "only when the user asked for a helper" gate is a **local patch**. | Re-apply the patch. Consider `child_timeout_seconds`. |
| Cron | Unpinned jobs follow the main model at fire time; `pinned` locks one; `hermes cron resnap` removed; self-healing re-runs (5/15/30 min) and catch-up fire after a restart; `--interpreter`. | All 4 Galion jobs are `no_agent` scripts, so the model change doesn't matter. Self-healing could **re-fire** `survival-cash`/`warn-sink` after a restart. | Check that the scripts are idempotent. `cron.model` (River DeepSeek) only matters for agent jobs. |
| Context cap | No change to `context_length`. Leaner prompts (~2.1k fewer tokens). Rare tools deferred behind `tool_search`. | The 65,536 cap still applies. Deferred tools change the tool list the boss sees. | Check that `delegate_task` stays loaded (it's core, not deferred). |
| Tools | `todo`→`todo_list`, `cronjob`→`cronjob_manage`, `process`→`process_manage` (aliases kept). Plugin compat import layer removed. | `tools/galion_privacy.py` and `plugins/memory/memory_tencentdb` are local and may use the removed compat imports. Memory providers moved to catalog plugins. | Import-test both local plugins in staging before any switch. |
| API server | Reasoning streams to OpenAI-compatible clients; stream approvals answerable; `/v1/runs` on a natively owned session gets a bounded 409. | hermes.ts would start receiving `reasoning_content` deltas (it ignores unknown fields). | Run the console smoke test in staging. |
| Files | Owner-only perms on state.db, credentials and caches. | Anything reading state.db as another user (the console?) could lose access. | Check which user console.service runs as. |

### Upgrade plan (for Slavik's approval; nothing done)

1. Back up `/opt/galion/hermes/{config.yaml,state.db,cron/,SOUL.md,channels/}` and `/root/galion-secrets/hermes.env` (the file, not printed) to a dated dir. Save the patch: `git -C /opt/galion/tooling/hermes-agent diff > galion-patches-fde4997.diff` plus untracked files (`tools/galion_privacy.py`, `plugins/memory/memory_tencentdb`).
2. Staging: a new clone at tag `v0.22.2` in a separate dir and venv, with its own `HERMES_HOME` holding a copy of config and state, on another loopback port. Re-apply the Galion patches and resolve conflicts.
3. In the staging config, set `sessions.auto_prune: false`. Check the multiplex boot guard with `hermes gateway status`.
4. Smoke tests against staging: the hermes-rs golden test shapes (health, models, galion-one HTTP/1.0 stream, console River stream, runs lifecycle, model pinning, delegate gate). Run each cron script once by hand in staging and check that it's idempotent.
5. Switch (Slavik decides): stop hermes.service, point the unit at the new venv, start it, then watch the console and the Galion One keep-warm.
6. **Rollback**: stop, point the unit back at `/opt/galion/tooling/hermes-agent` (fde4997, untouched), restore the backed-up `state.db`/config if a migration ran, then start. State.db migrations are forward-only, so restoring the backup is required if v30 → newer migrations ran.

## Rust port (hermes-rs)

Layout:
- `crates/hermes-rs` is the library. `crates/hermes-gateway` is the binary (`hermes-gateway [--mock] [--check-config]`).
- Config: `HERMES_HOME/config.yaml` (YAML subset parser, so Galion's config reads as-is) plus env. Loopback by default.
- The key is `API_SERVER_KEY` from env only. The gateway refuses to start if the key is missing or under 16 chars.

Design (Universal CPU):
- std threads, a blocking HTTP/1.x server, one request per connection, `Connection: close`, close-delimited SSE. This works for Galion One's HTTP/1.0 client and for node fetch.
- No async runtime and no TLS crate. HTTPS goes through the system `curl` with config on stdin, so the key and body never appear in argv (same pattern as galion-one models_refresh).

### Done

- **API.** Root and `/p/default/` mirror:
  - `GET /health`, `/health/detailed`, `/v1/models` (hermes-agent + route aliases), `/v1/capabilities`.
  - `POST /v1/chat/completions`, stream and non-stream:
    - upstream chunk shapes, `hermes.tool.progress` events, final chunk with usage;
    - `X-Hermes-Session-Id` / session key headers;
    - error envelopes: 401 gateway_auth, 429 concurrency, 402 spend cap, 400 bad request.
  - `/v1/runs` (start/get/events/stop/steer; approval → 409 approval_not_active, as with `approvals.mode: off`).
  - `/api/sessions`, `/api/sessions/{id}`, `/api/sessions/{id}/messages`, `POST /api/sessions/{id}/model`, `/api/jobs` (read-only), `/api/spend`.
- **Sessions.** Model pinning (Galion patch semantics). Stored history wins when it is at least as long as the body history. Session key resume.
- **Providers.**
  - OpenRouter: model mapping (`model_aliases`, `model_routes.galion`) and reasoning (`reasoning:{enabled,effort}`, omitted when off for anthropic/); `usage.include`.
  - River and custom providers: top-level `reasoning_effort` (medium/minimal → low; off → "none", omitted for GLM). Ollama off → `think:false`.
- **Agent loop.**
  - Tool calls, streaming deltas and reasoning, steer injection, cancel.
  - Context fit to `context_length` (65,536), max turns, tool-loop guardrail hard stop.
- **delegate_task:**
  - at most 3 children per call;
  - depth 1 (children get no delegate tool);
  - node-wide concurrent child gate;
  - children share the parent's per-session spend cap;
  - Galion "user asked for a helper" gate;
  - `delegation.reasoning_effort`.
- **Spend tracker.** Per-session USD, per-day USD and per-day call caps, persisted, day rolls over in Zagreb time. `:free` models cost 0.
- **SOUL.md / channels.** `channels/galion-one.md` for session keys `galion-one`, `one-warm`, `galion-one:*`. Persona Plex is opt-in (see below).
- **SQLite store (`sqlite` feature, default on host).**
  - Column-compatible with upstream state.db schema v30 (`sessions`, `messages`).
  - hermes-rs writes its own DB (`rs-state.db`).
  - The Python `state.db` is opened **read-only** (`SQLITE_OPEN_READ_ONLY`) as a history fallback. It is never copied or written.
  - TEXT dates in REAL columns are coerced; this is hermes.ts's `datetime('now')` quirk.
  - Without the feature (cross builds) a pure-Rust JSONL store is used.
- **Cron.**
  - Reads `cron/jobs.json` read-only and keeps its own state file, so a shadow never disturbs Python's bookkeeping.
  - Schedules: `interval` (anchored on start, so it never fires everything at once) and `once`.
  - `no_agent` bash scripts are confined to `HERMES_HOME/scripts` and run with:
    - a cleared env and `workdir`;
    - a timeout (120 s) with process-group kill;
    - redacted output written in upstream `cron/output/<id>/<ts>.md` format.
  - Agent jobs run through the agent, using `cron.model`/provider.
  - Off unless `HERMES_RS_CRON=1`.
- **MCP client (stdio).** `initialize` → `notifications/initialized` → paged `tools/list` → `tools/call`. Upstream names `mcp__<server>__<tool>`, with a 64-char clamp and sha256 suffix. Per-call timeout, one reconnect. `HERMES_RS_MCP=0` disables it.
- **Mock provider** for all tests (`--mock` on the binary).

### Stubbed / not ported (v1)

- `cron` expression schedules: listed with `supported:false`, never fired. Self-healing re-runs and chat delivery aren't ported (Galion has no platforms).
- Python cron scripts are **refused** (hermes-rs never runs Python). All 4 Galion jobs are `.sh`.
- MCP: HTTP/SSE transports, resources/prompts utility tools, sampling, hot reload, lazy cached tool lists (discovery runs at startup instead).
- `/v1/responses`, `/v1/artifacts/*`, `/v1/skills`, `/v1/toolsets`, `/api/sessions/{id}/chat[/stream]`, `/fork`, `/api/jobs/*` writes, `/api/cron/fire`, `/api/model/options`, `/api/platforms/*`. hermes.ts and Galion One use none of these.
- Approvals (Galion runs `approvals.mode: off`), `galion_privacy` tool, context compression (it truncates instead), skills, toolsets beyond clock/delegate/MCP, `streaming.enabled` gating, keep-alive connections.
- No money or payment tools, by design.

### Parity checklist (golden tests)

`crates/hermes-rs/tests/golden.rs` drives the real HTTP server with the request shapes hermes.ts and Galion One send:

- [x] `/health` shape (`status, platform: hermes-agent, version`) — `health_is_open_and_shaped`
- [x] 401 envelope `gateway_auth_error/gateway_auth_failed` — `auth_is_required_with_upstream_envelope`
- [x] `/v1/models` with the `galion` route — `models_list_includes_galion_route`
- [x] Galion One HTTP/1.0 voice stream (`session_key: galion-one`) — `galion_one_voice_stream_http10`
- [x] Galion One non-stream JSON — `galion_one_non_stream_json`
- [x] Console River stream (hermes.ts body, `X-Hermes-Session-Id`) — `console_chat_stream_river`
- [x] Model pinning + stored history — `session_history_and_model_pinning`, `session_key_resumes_latest_session`
- [x] Console `/v1/runs` lifecycle + events + stop/steer/approval — `console_runs_lifecycle`
- [x] `max_concurrent_runs` 3 → 429 — `concurrency_cap_returns_429`
- [x] Spend cap → 402 — `spend_cap_blocks_with_402`
- [x] Tool calls + `hermes.tool.progress` — `tool_calls_stream_progress_events`
- [x] Bad JSON / missing messages — `bad_requests`
- [x] delegate_task caps end to end — `delegate_task_end_to_end_with_caps`
- [x] Persona Plex on galion-one — `persona_plex_on_galion_one_when_configured`
- [x] Cron agent job + `/api/jobs` — `cron_agent_job_runs_through_mock_and_api_lists_jobs`
- [x] MCP stdio (fake bash server) — `tests/mcp.rs`
- [x] state.db v30 read-only + TEXT-in-REAL — `session_sqlite::tests`
- [ ] Byte-for-byte diff against a live Python response. This needs a shadow run (staged unit) and Slavik's OK.
- [ ] Real OpenRouter/River wire. Only `build_body` is unit-tested; no paid calls were made.

Totals: hermes-rs has 30 unit + 16 golden + 1 MCP tests; openmind-persona has 5. `cargo fmt --check` and `clippy --all-targets -D warnings` pass with and without the `sqlite` feature.

### Size and memory

All builds use the release profile (opt-level z, LTO, strip).

| Build | Binary | RSS |
|---|---|---|
| x86_64-unknown-linux-musl, static, no sqlite | 942,088 B | VmRSS 1,160 KB after 20 streamed `--mock` chats |
| aarch64-unknown-linux-musl, static | 815,512 B | — |
| x86_64-linux-gnu with bundled sqlite | 1,845,704 B | VmRSS 4,392 KB |
| Python Hermes (VPS, PID 1217064, measured ~20:57 +02:00) | — | VmRSS 310,192 KB (~303 MB), VSZ 4.88 GB |

The mock RSS is a floor. Real provider calls add one short-lived `curl` per LLM request.

### Shadow run

`deploy/staged/hermes-rs.service` is **not installed**. It has:
- port 8652 and `--mock`;
- its own state under `/var/lib/hermes-rs`;
- Python state.db read-only;
- cron, MCP and persona off;
- `MemoryMax=64M` and systemd sandboxing;
- no `[Install]`.

Turning off `--mock` would spend money, which needs Slavik. With cron enabled in the shadow, jobs would double-fire alongside Python Hermes, so it must stay off until Python cron is stopped.

## Universal CPU

The steer at 21:02 changed the plan. Original intent was tokio + a TLS client + rusqlite everywhere. The changes:

- No async runtime. It uses std threads with a blocking server, which suits a Pi or phone node.
- No TLS crate (no ring/openssl C cross-compile). HTTPS goes through the system `curl`, which exists on Linux, Termux and Pi OS.
- `rusqlite` (`bundled`, `default-features=false`) is an **optional** default feature. The box has no C cross compilers, so cross builds use `--no-default-features` and get the JSONL store.
- Deps: `serde_json` only (plus optional rusqlite). openmind-persona is std-only. Europe/Zagreb DST is computed in code (no chrono-tz). The YAML subset parser is in-tree.
- Linking: Tier 1 musl targets link with the toolchain's `rust-lld`, using `-j 1` for cross builds.

Per-target results (2026-10-18 ~21:27 +02:00):

| Target | Tier | Command | Result |
|---|---|---|---|
| x86_64-unknown-linux-musl | 1 | `cargo build --release --no-default-features -p hermes-gateway` | builds, static, 942 KB, runs (RSS above) |
| aarch64-unknown-linux-musl | 1 | same | builds, static AArch64 ELF, 815 KB (not run: no device) |
| aarch64-linux-android | 2 | `cargo check --release --no-default-features -p hermes-gateway -p openmind-persona` | OK |
| armv7-unknown-linux-musleabihf | 2 | same | OK |
| x86_64-unknown-linux-gnu | host | default features (sqlite) | builds, tests pass |

Commands used:
```
export CARGO_TARGET_DIR=/workspace/oom-target/hermes
LLD=$(ls ~/.rustup/toolchains/*/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-lld)
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER=$LLD CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=$LLD \
  nice -n 19 cargo build -j 1 --release --no-default-features -p hermes-gateway --target aarch64-unknown-linux-musl
```

SQLite on a phone or Pi needs a target C compiler (for example `aarch64-linux-musl-gcc`, or the NDK for Android). Until then those nodes use the JSONL store.

## Persona Plex

The `crates/openmind-persona` crate is std-only and has no deps. Source: `/opt/galion/openmind/personas/persona-plex-v1.md` on the VPS (read-only); a copy is in `crates/openmind-persona/personas/`.

Stable API (v1): this is the surface Galion One (ws/one-desk-tuner) and voice (ws/voice, galion-voice-core) should consume.

- `Persona::parse(&str)` / `Persona::from_file(&Path)`: load the persona file.
- `validate() -> Vec<Issue>` / `is_valid()`. Errors mean "don't use":
  - missing name or version;
  - role prompt over 2000 chars;
  - missing the official length rule `"one or two short sentences"` (`LENGTH_RULE`), or a Length trait that disagrees with it;
  - missing the "never move money" rule or the identity rule;
  - voice reference given as a URL.

  Warnings: no voice reference, no changelog, changelog entries without an offset or newer than the file version.
- `role_prompt()`: the v1 Samantha role prompt, verbatim (the test pins it).
- `voice_prompt()`: the audio voice prompt reference (a path on the GPU host; never bundled).
- `style_rules(Channel::Text | Channel::Voice)`: per-channel rules.
- `prompt(soul: Option<&str>, channel)`: SOUL.md + role prompt + channel rules.
- `version()`, `changelog()`.

Gateway wiring is opt-in with `HERMES_RS_PERSONA=/path/persona-plex-v1.md`:
- It applies only on the `galion-one` channel. Session `one-admin` gets text rules; every other session gets voice rules.
- On a persona channel the global `agent.system_prompt` ("Reply in exactly four sentences…") is dropped, because it conflicts with the persona length rule.
- An invalid persona file is ignored, and the plain channel prompt is used.
- The file's status is still "Proposed", so the staged unit leaves it off.

Milestones: M1 (loader, validator, prompt builder, v1 role prompt test, gateway wiring) is **done**. Next: Galion One and voice consume `style_rules(Voice)` and `voice_prompt()`.

## What needs Slavik

1. Approve or decline the 0.22.2 upgrade plan above, especially `sessions.auto_prune: false` and re-applying the Galion patches.
2. Install the staged shadow unit (mock, :8652). Later, a paid shadow (no `--mock`) with the spend caps above.
3. Persona Plex: mark v1 approved before `HERMES_RS_PERSONA` is set anywhere.
4. Decide whether Galion's MCP server `galion-stack` (a Python script) gets a Rust port. hermes-rs can spawn it but should not depend on Python.
5. A cross C toolchain on the build box, if phone/Pi nodes need SQLite.

## Process note

On this box I ran `python3 --version` once by mistake earlier in this session (2026-10-18 evening, +02:00), inside a combined shell command. That broke the "never run Python" rule. Nothing else Python was run, and nothing ran on the VPS.

### Memory provider (memory_tencentdb port, 2026-10-18)
`src/memory.rs` ports the Python `memory_tencentdb` plugin's lifecycle: `/recall` before a turn (non-empty
`context` goes into the system prompt as `## memory-tencentdb Memory`), `/capture` after a completed turn
(background thread, max 4 in flight), `/session/end` when a session is idle for `HERMES_RS_MEMORY_IDLE_END_SECS`
(default 1800; 0 = never) or at shutdown. session_key = Hermes session id, user_id = `default` (as the plugin).
On when config.yaml has `memory.provider: memory_tencentdb` or `HERMES_RS_MEMORY=1`; `HERMES_RS_MEMORY=0` forces off.
Env names as the plugin: `MEMORY_TENCENTDB_GATEWAY_HOST`/`_PORT` (127.0.0.1:8420), bearer from
`MEMORY_TENCENTDB_GATEWAY_API_KEY` then `TDAI_GATEWAY_API_KEY`. Timeouts: recall 2000 ms, capture 5000 ms
(`HERMES_RS_MEMORY_RECALL_TIMEOUT_MS`, `HERMES_RS_MEMORY_CAPTURE_TIMEOUT_MS`). All failures are non-fatal; 5
consecutive failures pause calls for 60 s. Not ported: the memory_search / conversation_search / read_node
tools, `/v2/offload` (query_mmd, tool-pair ingest), the gateway supervisor/auto-start (systemd owns it).
