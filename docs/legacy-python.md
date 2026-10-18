---
tags: [galion, openmind, python, audit, rust-port]
project: galion
status: draft
---
<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Legacy Python call-site audit (B10, read-only)

Generated: 2026-10-18T20:07:27+02:00 (Europe/Zagreb). Scope: crates/ in /opt/galion/openmind plus the helpers under /opt/galion/galion-light. Nothing was executed. Paths: GL = /opt/galion/galion-light, M = crates/galion-one/src/main.rs.

Rust launch sites are all in galion-one and all gated: ONE_LEGACY_PY=1 AND an explicit ONE_LEGACY_PY_BIN. With the default env no Python is spawned. Other crates (desk-hermes, link-watch, galion-desk) only mention .py in comments/Cargo descriptions (already ported).

| Script | Called from (file:line) | Inputs | Outputs | Side effects (DB writes flagged) | Shipped? | Proposed Rust replacement |
|---|---|---|---|---|---|---|
| GL/tts/worker.py (GPU Chatterbox voice worker) | M:231 (spawn boot_worker, if ONE_LEGACY_PY=1); M:1221 (existence check of the .py); M:1249 (Command::new(ONE_LEGACY_PY_BIN).arg(tts/worker.py)); M:1315 and M:1455 consume it | stdin framed phrases from Rust; HF_HOME /workspace/tts/models; voice wavs under /workspace/galion-src/voice-backend/assets/voices | stdout: byte R then length-prefixed PCM frames; stderr clip timings | GPU load; no DB writes. Venv currently broken (no torch, see GL/logs/voice.why), so the path is dead on this box | Yes, opt-in only (default off). Remote mouth (mouth_remote, port 8002) is the live path | Drop the spawn; keep the HTTP client path. galion-one: remove boot_worker/start_worker/Worker; voice_fold keeps only the :8002 /synthesize client. GPU side stays a separate service (fridge) |
| GL/one/models.py (OpenRouter model list refresh) | M:1839-1842 in models_json() (stale over 600 s cache, needs ONE_LEGACY_PY=1 + BIN) | Reads the Hermes env file for the OpenRouter key (secret, not copied here); GET openrouter.ai/api/v1/models | Writes GL/one/models-cache.json (id, name, sorted); prints count | Network call; file write; no DB. Reads a secret file | Yes, opt-in only; without it the cache is read as-is and goes stale | galion-one::models_refresh() (new fn): HTTP GET, sort by lowercased name, atomic write of models-cache.json; key read from env at call time, never logged. Ticket B11 |
| GL/origin.py (text check only) | M:2012 push("origin", text.contains("origin.py"), "light") in load_units(); ps source at M:1996 | ps -eo args output | Unit row ok/not ok in the nerve document | Read-only string match, no launch | Detector only. origin.py itself is launched by GL/keep.sh:97 and GL/stack-boot.sh:161 (python3, shell, not Rust) | Replace the match with a port/process probe for a Rust origin. Needs a new crate origin serving 80,3000,3100,3200,4000,8000,8100,8200 and 302 / to /console. Ticket B14 |
| GL/one/defence.py (guardian health read) | No caller found in crates/, GL scripts, /opt/galion/bin or /etc/systemd/system. Rust twin: M:1009 load_defence() (sqlite3 FFI; M:1065 opens guardian/guardian.db) | GL/guardian/guardian.db, table health_checks | Lines: service status ms | Read-only SQLite (no writes) | Dead script; already ported | None needed. Delete after B13 confirms nothing external runs it |
| GL/one/talk.py (voice turns: me, pair, hear, reply) | No caller found in crates/, GL shell, /opt/galion/bin or systemd. Only the file itself | argv me/pair/hear/reply; env ONE_COOKIE; admin device store (secret file, not read); audio file path; Whisper :8643; Hermes chat :8642 (uses Hermes env key); Chatterbox :8002 | JSON on stdout (pair token line is the only secret output) | WRITES admin-device.json (token hashes, chmod 600) on pair; APPENDS channels/one.jsonl; writes channels/galion-one.md; flock /tmp/one-talk.lock. Sends a chat turn to the Hermes API (Hermes may persist it itself); the script never opens Hermes state.db. No DB writes by this script | Unclear; appears superseded by Rust handlers in galion-one | New module galion-one::talk (pair, hear, reply): sha256 token store, HTTP to :8643/:8642/:8002, same flock path. Needs an owner decision first because it holds auth. Ticket B12 |
| GL/one/warm.py (keep Chatterbox resident) | No caller found. Rust twin: M:235 spawn keep_warm, M:1087 warm_due, M:1132 warm_pass (writes state/warm.ts) | flock /tmp/one-talk.lock; POST :8002/synthesize "Yes." voice samantha; sleep 20 s then every 300 s | none | Lock + HTTP POST; no DB | Dead script; already ported (off with ONE_NO_WARM=1) | Done: galion-one::warm_pass. Ticket B12: check lock-path parity with talk, then delete warm.py. Related GL/scripts/warm-voice.py (145 lines: Whisper + Chatterbox + Hermes ping, skip rules) overlaps warm_pass; fold in |
| GL/one/set-model.py (set plan/action lane model) | No caller found in crates/ or GL scripts | argv lane (plan or action), model (must contain a slash) | none | WRITES /workspace/hermes/modes.json (lane config file, not state.db, not a DB). Rust only reads it (M:1822, M:3086, M:3134) | Dead or manual operator tool | galion-one::set_model(lane, model): validate lane and slash, rewrite only plan/action keys, atomic write. Ticket B13 |
| GL/one/trace.py (black mark image to filled SVG) | No caller found in crates/, GL, or skill docs | argv src image, dest svg; Pillow; POST :8645/see for caption | Writes the dest SVG | File write only; no DB. Needs Pillow | Dead or manual; belongs to the mark skill | New crate mark-trace (bin): image crate for threshold + contour walk, HTTP to :8645/see. Low priority. Ticket B14 |

## Not Rust-called but still Python on the box (context)

- GL/keep.sh:97 and GL/stack-boot.sh:161 start origin.py with python3 (shell, not Rust).
- GL/scripts/*.py (health-tick, guardian-tick, earn, survival, loop/loop.py, etc.) are not referenced from crates/. link-watch.py and desk-hermes.py are already ported (crates/link-watch, crates/desk-hermes).
- guardian/*.py write guardian.db; Rust only reads it. No audited script opens Hermes state.db.

## Order of porting

1. B11 models refresh: models.py to galion-one::models_refresh (the only Rust-launched Python with a live purpose).
2. B12 talk/warm helpers: warm.py already in Rust (delete after parity check); talk.py port (auth-sensitive); fold warm-voice.py into warm_pass.
3. B13 set-model.py to set_model; confirm defence.py is dead and remove.
4. B14 origin.py port and the origin unit check; trace.py; then drop legacy_py/legacy_bin/boot_worker/Worker from galion-one so no ONE_LEGACY_PY path remains.
