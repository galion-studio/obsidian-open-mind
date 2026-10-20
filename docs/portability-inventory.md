# Portability inventory

- DESK-ID: openmind.B04
- Generated: 2026-10-19T15:38:36+02:00 (Europe/Zagreb)
- Repo: /opt/galion/openmind
- Scope: `crates/**/*.rs` only. Read-only. No Rust sources were edited.
- Acceptance scan: `rg -n '127\.0\.0\.1|/opt/galion|/workspace/|:[0-9]{4}' crates/ --glob '*.rs'`
- Acceptance hits: 103
- Acceptance table rows: 103
- Counts equal: yes

One row per acceptance hit, in scan order. The value is the matched literal (not the whole source line). `n/a` means the hit should not become configuration: a test, a comment, a log string, a parse-failure sink, a security allowlist, or a regex false positive.

## Risk

- high: runtime host, port, or absolute path with no override. Moving the process off this host or this directory layout breaks the call.
- medium: runtime default that already has an env or config override, but the baked-in value is still this host, loopback, or a laptop path.
- low: test, comment, usage text, log text, parse-failure sink, intentional loopback allowlist, or a false positive. Two low rows are not sockets: CSS `z-index:100000` and JSON `tickMs` 1000. Both match `:[0-9]{4}` only.

Keys already present in `openmind-config` (`hosts.*`, `ports.*`, `paths.base`) or as env vars are reused. Other keys are proposals only.

## Acceptance table

| file:line | value | proposed config key | risk |
| --- | --- | --- | --- |
| crates/openmind-config/tests/config.rs:27 | 127.0.0.1 | hosts.ollama | low |
| crates/openmind-config/tests/config.rs:28 | 127.0.0.1 | hosts.hermes | low |
| crates/openmind-config/tests/config.rs:91 | 127.0.0.1 | hosts.ollama | low |
| crates/openmind-config/src/lib.rs:29 | 127.0.0.1 | hosts.ollama | low |
| crates/openmind-config/src/lib.rs:30 | 127.0.0.1 | hosts.hermes | low |
| crates/guardian-tick/tests/cli.rs:52 | 127.0.0.1:0 (ephemeral; was 18080) | n/a | low |
| crates/desk-hermes/src/main.rs:1 | /opt/galion/bin/desk-hermes.py | n/a | low |
| crates/desk-hermes/src/main.rs:13 | /opt/galion/hermes | HERMES_HOME | low |
| crates/desk-hermes/src/main.rs:181 | /opt/galion/hermes | HERMES_HOME | medium |
| crates/one-defence/src/main.rs:16 | /workspace/galion-light/guardian/guardian.db | ONE_DEFENCE_DB | low |
| crates/one-defence/src/main.rs:17 | /opt/galion/galion-light | ONE_DEFENCE_DB | low |
| crates/one-defence/src/main.rs:18 | /opt/galion/galion-light/guardian/guardian.db | ONE_DEFENCE_DB | medium |
| crates/one-defence/src/main.rs:30 | /opt/galion/galion-light/guardian/guardian.db | ONE_DEFENCE_DB | low |
| crates/link-watch/tests/golden_keys.rs:22 | http://127.0.0.1:1 | LINK_WATCH_OLLAMA_URL | low |
| crates/link-watch/tests/golden_keys.rs:23 | http://127.0.0.1:1 | LINK_WATCH_HERMES_URL | low |
| crates/link-watch/tests/golden_keys.rs:49 | http://127.0.0.1:1 | LINK_WATCH_FRANCE_URL | low |
| crates/link-watch/tests/config_fixture.rs:52 | 127.0.0.1:0 | n/a | low |
| crates/link-watch/tests/config_fixture.rs:109 | 127.0.0.1 | hosts.ollama | low |
| crates/link-watch/tests/config_fixture.rs:110 | 127.0.0.1 | hosts.hermes | low |
| crates/link-watch/tests/config_fixture.rs:111 | 127.0.0.1 | hosts.remote_probe | low |
| crates/link-watch/tests/config_fixture.rs:176 | http://127.0.0.1:1 | LINK_WATCH_OLLAMA_URL | low |
| crates/galion-desk/src/main.rs:17 | /opt/galion/galion-light/state/desk-notes.jsonl | paths.desk_notes | high |
| crates/galion-desk/src/main.rs:18 | /opt/galion/galion-light/state/desk-focus | paths.desk_focus | high |
| crates/galion-desk/src/main.rs:19 | /opt/galion/galion-light/state/link.json | paths.link | high |
| crates/galion-desk/src/main.rs:20 | /workspace/galion-light/one/voice | paths.desk_voice | high |
| crates/galion-desk/src/main.rs:21 | /workspace/galion-light/one/session.jsonl | paths.one_session | high |
| crates/galion-desk/src/main.rs:26 | 127.0.0.1:8646 | DESK_BIND | medium |
| crates/galion-desk/src/main.rs:29 | 127.0.0.1:8646 | DESK_BIND | low |
| crates/galion-desk/src/main.rs:96 | /opt/galion/galion-light/state/desk-hold | paths.desk_hold | high |
| crates/galion-desk/src/main.rs:117 | /opt/galion/galion-light/state | paths.state_dir | high |
| crates/galion-desk/src/main.rs:138 | /opt/galion/galion-light/state | paths.state_dir | high |
| crates/galion-desk/src/main.rs:167 | 127.0.0.1:11434 | ollama.url | high |
| crates/galion-desk/src/main.rs:184 | 127.0.0.1:11434 | ollama.url | high |
| crates/galion-desk/src/main.rs:225 | /opt/galion/galion-light/state/desk-hold | paths.desk_hold | high |
| crates/galion-desk/src/main.rs:238 | /opt/galion/openmind/dist/bin/desk-hermes | DESK_HERMES_BIN | medium |
| crates/galion-desk/src/main.rs:294 | /opt/galion/galion-light/state/survival.json | paths.survival | high |
| crates/galion-desk/src/main.rs:295 | /opt/galion/hermes/gateway_state.json | paths.hermes_gateway_state | high |
| crates/galion-desk/src/main.rs:297 | /opt/galion/galion-light/state/machine.json | paths.machine | high |
| crates/galion-desk/src/main.rs:489 | 127.0.0.1:11434 | ollama.url | high |
| crates/galion-desk/src/main.rs:503 | 127.0.0.1:1 | n/a | low |
| crates/galion-desk/src/main.rs:522 | 127.0.0.1:{port} | hosts.probe | high |
| crates/galion-desk/src/main.rs:532 | 127.0.0.1:1 | n/a | low |
| crates/openmind-survival/tests/survival.rs:28 | 127.0.0.1:0 (ephemeral; was 18080) | n/a | low |
| crates/openmind-survival/src/probes.rs:52 | 127.0.0.1:port | hosts.probe | low |
| crates/openmind-survival/src/probes.rs:66 | 127.0.0.1 | hosts.probe | high |
| crates/openmind-schema/src/lib.rs:101 | http://127.0.0.1:11434/v1 | providers.laptop.base_url | high |
| crates/galion-one/src/main.rs:79 | /workspace/galion-light | GALION_HOME | high |
| crates/galion-one/src/main.rs:93 | 127.0.0.1:{port} | ONE_BIND | medium |
| crates/galion-one/src/main.rs:100 | z-index:100000 | n/a | low |
| crates/galion-one/src/main.rs:108 | /workspace/hermes/channels/one.jsonl | paths.voice_log | high |
| crates/galion-one/src/main.rs:109 | /workspace/hermes/channels/galion-one.md | paths.hermes_channel | high |
| crates/galion-one/src/main.rs:813 | /workspace/hermes/channels | paths.hermes_channels_dir | high |
| crates/galion-one/src/main.rs:1223 | /workspace/galion-light/tts/worker.py | paths.tts_worker | high |
| crates/galion-one/src/main.rs:1459 | 127.0.0.1:8002 | ONE_MOUTH_BIND | high |
| crates/galion-one/src/main.rs:1824 | /workspace/hermes/modes.json | paths.hermes_modes | high |
| crates/galion-one/src/main.rs:2304 | tickMs:1000 | n/a | low |
| crates/galion-one/src/main.rs:2789 | /opt/galion/memory-tencentdb | paths.memory_tencentdb | high |
| crates/galion-one/src/main.rs:2822 | /opt/galion | paths.install_root | medium |
| crates/galion-one/src/main.rs:3087 | /workspace/hermes/modes.json | paths.hermes_modes | high |
| crates/galion-one/src/main.rs:3135 | /workspace/hermes/modes.json | paths.hermes_modes | high |
| crates/room/src/main.rs:13 | 127.0.0.1:{port} | ROOM_BIND | medium |
| crates/room/src/main.rs:70 | /workspace/galion-light/docs | paths.room_doc_roots | high |
| crates/room/src/main.rs:71 | /workspace/galion-src/galion-docs | paths.room_doc_roots | high |
| crates/room/src/main.rs:72 | /workspace/galion-src/galion-workspace3d/src | paths.room_doc_roots | high |
| crates/room/src/main.rs:73 | /workspace/galion-src/galion-app/docs | paths.room_doc_roots | high |
| crates/room/src/main.rs:74 | /workspace/galion-src/galion-app/src | paths.room_doc_roots | high |
| crates/room/src/main.rs:79 | /workspace/galion-src/galion-workspace3d/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:80 | /workspace/galion-src/galion-app/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:81 | /workspace/galion-src/galion-docs/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:82 | /workspace/galion-src/galion-rs/Cargo.toml | paths.room_manifests | high |
| crates/room/src/main.rs:83 | /workspace/galion-src/galion-console/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:84 | /workspace/galion-src/galion-api-docs/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:85 | /workspace/galion-src/galion-knowledge/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:86 | /workspace/galion-src/galion-studio-app/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:87 | /workspace/galion-src/merlin-element/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:88 | /workspace/galion-src/galion-shared/package.json | paths.room_manifests | high |
| crates/room/src/main.rs:273 | /workspace/galion-light/room/index.html | paths.room_index | high |
| crates/room/src/main.rs:298 | /workspace/galion-src/ | paths.room_read_prefix | high |
| crates/room/src/main.rs:299 | /workspace/galion-light/docs/ | paths.room_read_prefix | high |
| crates/persona-plex-relay/src/config.rs:294 | 127.0.0.1 | n/a | low |
| crates/persona-plex-relay/src/config.rs:454 | wss://GPU_HOST_NOT_PROVISIONED:8998/api/chat | upstream.url | low |
| crates/persona-plex-relay/src/config.rs:455 | ws://203.0.113.7:8998/api/chat | n/a | low |
| crates/persona-plex-relay/src/config.rs:459 | wss://GPU_HOST_NOT_PROVISIONED:8998/api/chat | upstream.url | low |
| crates/persona-plex-relay/src/config.rs:460 | ws://127.0.0.1:18998/api/chat | n/a | low |
| crates/persona-plex-relay/src/protocol.rs:591 | wss://h:8998/api/chat | n/a | low |
| crates/persona-plex-relay/src/protocol.rs:594 | wss://h:8998/api/chat | n/a | low |
| crates/link-watch/src/main.rs:1 | /opt/galion/galion-light/scripts/link-watch.py | n/a | low |
| crates/link-watch/src/main.rs:56 | /opt/galion/galion-light/state/link.json | LINK_WATCH_OUT | medium |
| crates/link-watch/src/main.rs:57 | /opt/galion/galion-light/state/laptop.json | LINK_WATCH_LAPTOP | medium |
| crates/link-watch/src/main.rs:58 | /opt/galion/galion-light/state/link-log.json | LINK_WATCH_LOG | medium |
| crates/link-watch/src/main.rs:59 | http://127.0.0.1:11434 | LINK_WATCH_OLLAMA_URL | medium |
| crates/link-watch/src/main.rs:60 | http://127.0.0.1:8642 | LINK_WATCH_HERMES_URL | medium |
| crates/link-watch/src/main.rs:571 | http://ollama.example.invalid:14001 | n/a | low |
| crates/link-watch/src/main.rs:572 | http://hermes.example.invalid:14002 | n/a | low |
| crates/link-watch/src/main.rs:575 | http://probe.example.invalid:14003 | n/a | low |
| crates/link-watch/src/main.rs:610 | http://probe.example.invalid:8080 | n/a | low |
| crates/link-watch/src/main.rs:637 | http://127.0.0.1:1 | LINK_WATCH_OLLAMA_URL | low |
| crates/link-watch/src/main.rs:638 | http://127.0.0.1:1 | LINK_WATCH_HERMES_URL | low |
| crates/link-watch/src/main.rs:648 | http://127.0.0.1:1 | LINK_WATCH_OLLAMA_URL | low |
| crates/link-watch/src/main.rs:649 | http://127.0.0.1:1 | LINK_WATCH_HERMES_URL | low |
| crates/persona-plex-relay/src/plan.rs:180 | wss://h.example:8998/api/chat?x=1 | n/a | low |
| crates/persona-plex-relay/src/plan.rs:181 | h.example:8998 | n/a | low |
| crates/persona-plex-relay/src/plan.rs:183 | 127.0.0.1:8091 | n/a | low |

Acceptance table risk totals: high 46, medium 12, low 45.

## Model and voice ids outside the acceptance regex

These lines do not match the acceptance regex, so they are not acceptance-table rows. Listed because the scan also looked for hard-coded model ids. Same columns. Not included in the 103.

| file:line | value | proposed config key | risk |
| --- | --- | --- | --- |
| crates/galion-desk/src/main.rs:166 | galion:latest | models.ollama_wake | high |
| crates/openmind-schema/src/lib.rs:103 | huihui_ai/qwen3.5-abliterated:9b | providers.laptop.default_model | high |
| crates/galion-one/src/main.rs:1461 | chatterbox | models.voice | medium |
| crates/galion-one/src/main.rs:1803 | galion | models.one | high |
| crates/persona-plex-relay/src/protocol.rs:546 | NATF2.pt | upstream.voice_prompt | low |

`chatterbox` is also embedded in the health body on acceptance row `crates/galion-one/src/main.rs:1459`. `NATF2.pt` is also on acceptance rows `protocol.rs:591` and `protocol.rs:594` (test URLs).
