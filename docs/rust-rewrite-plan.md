<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Rust rewrite plan: Obsidian Open Mind, made by Galion. Written 2026-10-18T20:51:30+02:00 (Europe/Zagreb, CEST)

Read-only survey by Grok Bot of VPS <vps-ip> (Ubuntu 24.04 LXC, kernel 6.8.12-pve, 8 vCPU, 16 GB, UTC clock) and the
galion-studio GitHub account. Nothing was changed, restarted, installed or committed. Only this file was written.
Secret files were noted by name only, never opened. All times are Europe/Zagreb (+02:00 CEST). The VPS clock is UTC, so I converted.
LOC counts use find + wc (tokei is not installed). Vendored trees (node_modules, venv, target, .git, model weights) are left out.

## 0. Summary

- **13 Rust crates already exist** (12 in the `openmind` workspace plus `edge/openmind-node`, the `omnode` bin). Together they are about 13.6k lines.
  Another ~4.3k lines of Rust sit outside the repo: `galion-light/one/src` (2.8k, the older One), `desk-rs` (569), `room` (298), `tulu` (525), `/opt/galion/tuner/src` (72).
- **Running now** (checked with `ss -ltnp`, `systemctl` and `ps` at about 20:45 CEST): 8 stack processes. **3 are Rust**: `one` on 8091 and 8002, `galion-desk` on 8646, `tuner` on 8650.
  **5 are not Rust**: Hermes (Python, 8642), memory-gateway (Node/TS, 8420), console (Node/Vite **dev server**, 8080), link-watch (Python, no port), and cloudflared (Go, third-party, run twice).
- **The live binaries are not the openmind builds.** Live galion-desk is a 4.7 MB unstripped debug build from 10 Oct, built from `galion-light/desk-rs`; the openmind build is 403 KB.
  Live `one` is 1.06 MB from 13 Oct and still contains the `/workspace/.../tts/worker.py` and `models.py` strings; the openmind build is 643 KB.
  The Rust link-watch port is built but not switched (the Python one still runs). The switch is staged in `deploy/` and `TEACHER/STAGED.md` (T5).
- `one` is **not a systemd unit**. It was started with `nohup` from an ad-hoc operator `bash -c`, so it will not come back after a reboot (QUEUE B21/B22).
- The Python left on the box is about 11k first-party lines in galion-light: guardian/healer, scripts, origin, stt/tts/vision servers, mesh and loop. None of it runs today except link-watch.py.
- The legacy web apps behind galion.app, api, studio, docs, code, tulu and adriaticpadel.com all return **502**. Their ports (4000, 8000, 4001, 4042, 7681, 5530, 3010 and others) have no listener.
  Their sources are local clones in `/opt/galion/galion-src` (28 repos, last commits 2024-11 to 2026-01-21): about 0.5M lines of Python and about 1.6M lines of TS/TSX.
  Recommendation: **retire**. Do not port them.

## 1. Inventory of the old codebase

| Component | Where | Language | Size (files / lines) | What it does | Running now? | Depends on |
|---|---|---|---|---|---|---|
| Galion One (`one`) | live bin `galion-light/one/one`; src `galion-light/one/src/main.rs` (old) and `openmind/crates/galion-one` (current, 3 files) | **Rust** + HTML/JS front | old src 1 rs / 2,806; crate 4,096; front 4 html / 1,396; 6 py helpers / 442 (models, defence, talk, warm, set-model, trace) | The operator's ear, mind and mouth: the One page, health, nerve view, models cache, voice relay (mouth client on 8002, Whisper on 8643, Hermes on 8642) | **Yes**: pid 1013026, 8091 (200) and 8002 (503, voice worker missing torch). nohup, not systemd. RSS about 10 MB | Hermes API, guardian.db (read-only), models-cache.json, OpenRouter (refresh), `/workspace` paths |
| console (console-ide) | `/opt/galion/console-ide` (git, 1 commit) | TypeScript (TanStack Start, React, Vite, better-auth, pglite/pg, kysely) | 126 ts / 14k, 56 tsx / 8.2k, plus 75 mjs / 50k of build scripts; 340 MB with node_modules | The admin console and IDE at console.galion.app and obsidian.galion.app | **Yes**: console.service, but as **vite dev** on 8080 (RSS about 520 MB). The /health path returns 404 | Node in `tooling/node`, Postgres or pglite, auth |
| tuner | `/opt/galion/tuner` (not in a repo) | **Rust** (src/main.rs 72 lines) + JS www (4 files) | 72 rs; static-pie 509 KB | Static server for the ukulele tuner page | **Yes**: tuner.service on 8650 (RSS 264 KB). **tuner-quick.service** exposes it on a temporary trycloudflare URL | none |
| link-watch | `galion-light/scripts/link-watch.py` (live); `crates/link-watch` (port) | Python (live) / **Rust** (port) | py 94; rs 3 / 552 | Watches the laptop link: Ollama, Hermes and a France probe (the hard-coded IP was removed in B01) and writes a JSON status | **Yes, the Python one** (link-watch.service, RSS 17 MB). The Rust port (394 KB, 1.5 MB RSS) is staged in deploy/ | Ollama 11434, Hermes 8642 |
| galion-desk | live `/opt/galion/bin/galion-desk` (from `galion-light/desk-rs`); `crates/galion-desk` | **Rust** | desk-rs 569; crate 614 | The operator desk endpoint (`/health`, `/desk`). Reads the pod, never writes it | **Yes**: galion-desk.service on 8646. 4.7 MB debug build, RSS 1.7 MB | Ollama, state files; `desk-hermes.py` (now ported to `crates/desk-hermes`) |
| desk-hermes | `/opt/galion/bin/desk-hermes.py`; `crates/desk-hermes` | Python to **Rust** | py 229; rs 2 / 650 (rusqlite) | Desk "mind snap" of Hermes state | Only on demand (not resident) | Hermes state.db (read-only) |
| guardian / healer | `galion-light/guardian/{desk_heal.py, upstream/*.py, guardian.db}`, `scripts/guardian-tick.py, heal-*.py, health-tick.py, survival.py` | Python | guardian 8 py / 3,963 (the upstream Docker-era library is 3.7k of that); scripts about 650 | Probes on 8080, 8642, 8643, 8645, 8002, 80 and 8091; the healer restarts services; writes guardian.db | **No**. Stopped between 8 and 12 Oct (see docs/survival-rust-plan.md). Rust `guardian-tick` (report-only) is built | psutil, sqlite, systemctl, pgrep |
| keep.sh and boot scripts | `galion-light/{keep.sh 210, stack-boot.sh 180, install.sh, hardware.sh, persist-boot.sh, vps-boot.sh, start.sh, revive.py}` | bash with embedded python3 heredocs | about 0.6k sh plus inline Python | nohup supervisor loop (20 s) for ide, origin, one, tulu, room, hermes, vision, stt, tts and loop; writes hardware.json; RunPod volume restore | **No** process now. systemd units replaced the core ones; `one` was started by hand | python3, curl, `/workspace` paths |
| origin | `galion-light/origin.py` | Python | 684 | Splash and redirect on 80/3000/.../8200, and `/` redirects to /console | **No** (no listener on 80) | none |
| Hermes gateway | `/opt/galion/tooling/hermes-agent` (editable install v0.21.4 into `/opt/galion/venv`), home `/opt/galion/hermes` | Python (upstream Nous hermes-agent, third-party) | large upstream tree, not counted as ours | The agent gateway: API server on 8642 with the `galion` model route, kanban, cron (4 jobs: daily-digest, survival-cash, warn-sink, shadow-status), plugins action-timer and command-guard | **Yes**: hermes.service, RSS 300 MB, the top CPU user | OpenRouter, memory-gateway env, state.db (40 MB) |
| Hermes channels and integration | `hermes/channels/{one.jsonl, galion-one.md}`, `hermes/scripts/*.sh` (5, 98 lines), `galion-light/hermes/{setup,apply-host}.sh`, MCP `galion-stack` -> `galion-light/mcp/stack-read.py` (77) | jsonl, sh, py | small | One's chat channel; cron shell jobs; read-only stack MCP (lazy) | Channels are in use by one and Hermes. channel_directory has no platforms (no Telegram or Slack) | Hermes |
| memory-gateway (tdai) | `/opt/galion/memory-tencentdb` -> npm `@tencentdb-agent-memory/memory-tencentdb` 1.0.3 (MIT) | TypeScript via tsx (third-party) | 182 ts / 55.7k; 198 MB | Agent memory gateway (v2 router, pyramid, offload) used by Hermes | **Yes**: memory-gateway.service on 8420 (/health 200). RSS **615 MB**, plus an esbuild child | Node in `tooling/node-memory`, OpenTelemetry, jieba, ai-sdk; config in /root/.memory-tencentdb (not read) |
| node app on 8080 | same as console (vite dev) | TS | see console | see console | yes | see console |
| STT | `galion-light/stt/stt_server.py` + `/opt/galion/stt` (venv, faster-whisper-large-v3-turbo, 1.7 GB) | Python | 97 | Whisper HTTP on 8643 | **No** (stt.why: unstartable) | CUDA or CPU torch, ctranslate2 |
| TTS / voice | `galion-light/tts/{tts_server.py 178, worker.py 76}` + `/opt/galion/tts` (Chatterbox, 3.3 GB) | Python | 254 | Chatterbox voice (mouth) on 8002 | **No**. 8002 is answered by `one` with 503 (voice.why: no torch in the venv) | GPU torch |
| vision | `galion-light/vision/{vision_server.py, look.py}` + `/opt/galion/vision` (BLIP + Florence-2, 2.5 GB) | Python | 363 | Image captioning on 8645 | **No** | torch, transformers |
| tulu | `galion-light/tulu/{main.rs, gpu.rs}` + vendored llama.cpp (363 MB, build b11209) + model (3.9 GB) | **Rust** wrapper + C/C++ upstream | 525 rs | Local LLM server wrapper (CPU and GPU variants) for tulu.galion.app (5530) | **No** | llama.cpp, hardware.json |
| room | `galion-light/room`, `crates/room` | **Rust** | 298 / 352 | Read-only file view page | **No** (built, in dist/bin) | none |
| other galion-light Python | scripts: boss, earn, fit, flash_contest, skill-loop, shadow, self-eval, note, measure-volume, quota-probe, vidaa-bridge, znjan-net-watch, warm-voice; `mesh/mesh.py` 291; `loop/loop.py` 386; arena 409; grade 270; blackbox 124; guard 130 | Python | about 4.5k | Experiments: earning loops, contests, skill loop, LAN mesh, TV bridge, net watch | **No** | various |
| /opt/galion/bin helpers | `pw.py` 58, `replicate-picture.py` 92 | Python | 150 | Manual helpers | No | Replicate API |
| shadow | `galion-light/shadow/hermes-agent` (114 MB) | Python (copy of upstream Hermes) | about 2M py lines (vendored) | Shadow copy for experiments | No | none |
| crew-skills, arena, skills-packs | `/opt/galion/crew-skills` (third-party skill library), `/opt/galion/arena` (96 MB), `galion-light/skills*` | md + py | crew 745 py / 278k, 2.6k md; arena 200 py / 47k | Agent skills and gladiator arena material | No (content) | Hermes |
| phone-voice PWA | `openmind/apps/phone-voice` | JS/HTML | 198 js | Phone voice front end | Static | one / relay |
| legacy backends (502) | `/opt/galion/galion-src/*`, pm2 `galion-infrastructure/pm2/ecosystem.config.js` (uvicorn `/workspace/project-nexus/backend` :8000) | Python (FastAPI) + Next/Vite TS | galion-backend 378 py / 178k plus 47 sql; galion-platform 452 py / 200k, 1.7k tsx / 497k; galion-app 1.7k tsx / 554k; voice-backend 86 py / 30k; workspace3d 764 ts(x) / 225k | galion.app (4000), api (8000), studio (4001), docs (4042), code (ttyd 7681), tulu (5530), adriaticpadel (3010), plus 20 more ingress hosts | **No**: every ingress host except one, console and obsidian returns 502 | pm2, Postgres, GPU voice stack, Supabase |
| from-grok-build | `/opt/galion/from-grok-build` (661 MB, copied 8 Oct, 45,182 files) | mixed | workspace: `galion-light`, `galion-src` (same 28 repos), `hermes`, `hermes-research`; console app (`src` 155 ts(x) / 16k), `laptop-link` (2 PowerShell) | A snapshot of the Grok build machine | No | none |
| cloudflared | `/usr/local/bin/cloudflared`, config in `galion-src/galion-infrastructure/cloudflared/config.yml` | Go (third-party) | none | Named tunnel (36 ingress rules) plus the quick tuner tunnel | **Yes** (2 processes) | Cloudflare |

### GitHub (galion-studio)

The connector is signed in as `galion-studio` but **cannot see private repos**. `get_repository_tree` on galion-backend and voice-backend
returns 404, and `user:galion-studio is:private` returns 0. It lists 15 public repos (the profile says 17): hermes-research (MIT, almost empty),
community (HTML), galion-studio, galion-initiative-web (TS), galion-universal-downloader (JS), galion-agi-web, nexuslang-v2 (Python),
project-mc.galion.studio (Python), merlin-element-showcase, merlin, mmee (CSS), SentiaLink (Astro), xDrops, xDrop (Java) and TestDrop.
The private repos below were measured from their **local clones** in `/opt/galion/galion-src` instead (origin `git@github.com:galion-studio/<name>.git`).

| Repo (clone) | Last commit (CET/CEST) | Main language | Lines | Top level |
|---|---|---|---|---|
| galion-backend | 2026-01-20 22:12 +01:00 | Python FastAPI | 378 py / 178k, 47 sql / 21k | api, services, voice_ai, voice_ai_v2, database, migrations, security, video_to_vr_pipeline, many test_*.py and debug_*.py, main.py |
| galion-platform | 2026-01-21 13:25 +01:00 | Python + TSX (+ Swift, Kotlin, 21 rs / 2.4k) | about 900k | Documents/project-nexus |
| voice-backend | 2026-01-20 22:44 +01:00 | Python | 86 py / 30k | api, services, voice_ai, scripts, main.py |
| tactical-belts-pro | 2026-01-20 22:22 +01:00 | Next.js TS | 38 tsx / 5.4k | src, public, next.config.ts |
| magic-merlin-showcase | 2025-11-09 18:38 +01:00 | Vite React TS + Supabase | 185 tsx / 29k, 44 sql | src, supabase, public |
| merlin-element | 2026-01-20 22:11 +01:00 | Vite React TS + Supabase (fork of the above) | 187 tsx / 29.5k | same as magic-merlin-showcase |
| hermes-research | public, pushed 2026-10-07 13:58 | Markdown | 8 md / 187 | LICENSE, README, docs |
| community | public, pushed 2026-09-23 17:22 | HTML | none | 2026-09-23/, publish/, LICENSE, README |

## 2. What is already Rust vs not

| Rust crate (openmind) | Lines | Deps | Built bin (stripped) | Status |
|---|---|---|---|---|
| zagreb-time | 155 | chrono, chrono-tz | lib | done |
| openmind-schema | 153 | serde, serde_json | lib | done (B07 and B33 open) |
| openmind-config | 422 | serde, toml | lib (example 286 KB) | B02 done |
| openmind-survival | 479 | std + zagreb-time | lib | B18 done |
| guardian-tick | 342 | survival, toml, serde_json | 631 KB, RSS 4.3 MB | B19 done (report-only) |
| openmind-sandbox | 620 | libc | demo 372 KB | B27 done (no landlock in this kernel's LSM list) |
| link-watch | 552 | serde_json, zagreb-time | 395 KB, RSS 1.5 MB | B01 done; switch staged |
| galion-one | 4,096 | std only | 643 KB | B11 done; the live bin is an older build |
| galion-desk | 614 | std only | 403 KB | switch staged |
| desk-hermes | 650 | rusqlite | 1.43 MB | ported |
| room | 352 | std | 344 KB | built |
| persona-plex-relay | 1,998 | zagreb-time | not built | **untracked in git** |
| edge/openmind-node (`omnode`) | 3,196 | ed25519-dalek, sha2, mdns-sd, qrcode, landlock, socket2 | not built | standalone workspace |

**Rust outside the repo:** tuner (72, live), galion-light desk-rs (569, live source), one/src (2,806, older), room (298), tulu (525), publish/obsidian/one/main.rs (2,026, a published copy).

**Still not Rust (first-party):** about 11k lines of Python in galion-light and /opt/galion/bin (guardian 4.0k, scripts 3.1k, origin 0.7k, one helpers 0.4k, stt/tts/vision 0.7k, mesh/loop 0.7k, misc 1.1k), about 0.6k lines of shell boot scripts plus 5 Hermes cron .sh, about 22k lines of TS/TSX in the console, 0.2k JS in phone-voice, and the legacy repos (about 0.5M py, about 1.6M ts/tsx).
**Third-party, not ours to port:** Hermes agent (Python), memory-gateway (TS), cloudflared (Go), llama.cpp (C++), crew-skills.

Counts: 13 Rust crates plus 5 Rust programs outside the repo. Of the 8 live stack processes, 3 are Rust and 5 are not (2 Python, 2 Node, 1 Go).

## 3. Rewrite plan

Target: cheap CPU-only devices. Stripped bins under 2 MB, RSS under 10 MB idle, std or small crates, every host and port from `openmind-config` (no hard-coded hosts), Zagreb timestamps, systemd units staged and never auto-installed.
Default libraries: std, serde/serde_json/toml, chrono-tz (through zagreb-time), libc. Add these only where needed: `ureq` (blocking HTTPS client, rustls) for outbound calls, `tiny_http` or the existing hand-rolled std HTTP server,
`rusqlite` (bundled, FTS5) for SQLite, `landlock` and `rustix` for the sandbox, `mdns-sd` and `ed25519-dalek` for the edge, `image` for trace.
For ML, keep Rust as the orchestrator and use child processes: `whisper.cpp` or `whisper-rs` for CPU STT, `piper` or Kokoro through `ort` (ONNX Runtime) for CPU TTS, and `llama-server` (llama.cpp) for LLM and vision.
Tokio, axum and hyper only where a ticket says so (memory-gateway v1 may need them).
Build: `nice -n 19 cargo build -j 2 --release`; `x86_64-unknown-linux-musl` and `aarch64` static builds documented as options (B08).

### Per-component recommendation

| Component | Decision | Crate | Tickets (existing / new) | Effort |
|---|---|---|---|---|
| Galion One | **keep Rust; finish** (config paths, drop the legacy Python and TTS worker, switch the live bin to the openmind build, run it under systemd) | galion-one | B06, B12, B13, B14, B15, B21, B22; new **B51** (switch the live one, galion-desk and link-watch to the dist builds, staged) | 6 M + 3 S |
| link-watch | **switch** to Rust (done), then config | link-watch | B01 (done), B03, B51 | 1 S |
| galion-desk / desk-hermes | **keep Rust**; retire desk-rs and desk-hermes.py | galion-desk, desk-hermes | B05, B51; new **B52** (dedupe old Rust sources: desk-rs, one/src, room, publish copy, with openmind as the source of truth) | 1 M + 1 S |
| tuner | **keep Rust**; move into the workspace | tuner | new **B53** (import /opt/galion/tuner as `crates/tuner`, config-driven addr and root; retire tuner-quick) | 1 S |
| guardian / healer | **rewrite** (report-only first, actions later and only with approval) | openmind-survival, guardian-tick | B17-B25 (B17, B18, B19 done) | 4 S + 2 M left |
| keep.sh / stack-boot / install / hardware.sh | **retire** the nohup supervisor in favour of systemd units; **rewrite** hardware detection | new `openmind-boot` (hardware.json probe plus unit renderer, staged) | B21, B22, B24; new **B54** (hardware probe in Rust: CPU, RAM, GPU presence from /proc and /sys, writes hardware.json), **B55** (unit renderer from openmind.toml to deploy/*.service, no install) | 2 S + 1 M |
| origin.py | **retire** or fold into a tiny static redirect inside `one` (not running now) | none, or galion-one route | new **B56** (decide and implement the redirect; QUEUE lacks a ticket; legacy-python.md calls it B14, but B14 is the TTS spawn) | 1 S |
| Hermes gateway | **keep** (upstream Python, third-party). Integrate through HTTP 8642 and channels only | none | B13 (talk helpers never write state.db) | none |
| Hermes cron shell jobs | **keep** shell (tiny); fold health-type jobs into guardian probes | none | B20 | none |
| MCP galion-stack (stack-read.py) | **rewrite** as a small Rust stdio MCP server (read-only) | new `openmind-mcp-stack` | new **B57** | 1 M |
| memory-gateway (tdai, 615 MB RSS) | **keep now**, then **rewrite** the subset Hermes actually uses | new `openmind-memory` (rusqlite FTS5, std HTTP) | new **B58** (read-only API usage study), **B59** (Rust v0 for the used endpoints on a spare port), **B60** (shadow run and parity report; the switch is staged) | 1 S + 2 M |
| console (8080) | **keep the TS front end**; stop running vite dev in production (prod build T6 is staged) | none (later `openmind-console-api` if server functions need to leave Node) | T6 staged; new **B61** (console prod switch checklist and /health route, staged) | 1 S |
| STT / TTS / vision | **retire** the Python and torch servers on the VPS; replace them with Rust clients to providers chosen through openmind-provider (CPU whisper.cpp, piper, llama-server multimodal on LAN or GPU boxes) | `galleon-voice` (B47 scaffold), `openmind-provider` | B14, B34, B36, B38, B46, B47; new **B62** (CPU STT/TTS adapter: spawn whisper.cpp or piper as a sandboxed child, fixtures only) | 1 M |
| tulu | **keep Rust wrapper**; move into the workspace as a llama-server provider | tulu becomes a LlamaCppProvider | B36; new **B63** (import tulu/main.rs and gpu.rs, config-driven, no vendored llama.cpp in the repo) | 1 M |
| mesh.py / loop.py / znjan-net-watch / health-tick | **fold**: mesh goes into omnode discovery, net watch and health into guardian probes, loop is retired | omnode, openmind-discovery | B20, B35 | none extra |
| other galion-light scripts (earn, boss, contest, skill-loop, shadow, fit, note, vidaa, pw, replicate-picture) | **retire** (experiments) and archive | none | new **B64** (archive list and retire note; deletion only after Slavik's approval) | 1 S |
| set-model.py / trace.py / talk.py / warm.py / defence.py | port or delete (per B10) | galion-one modules, mark-trace | B12, B13, B15 | covered |
| omnode / edge | **keep Rust**; build and size-check | openmind-node | B08, B09, B32-B38 | covered |
| persona-plex-relay | **keep Rust**; commit decision (untracked) | persona-plex-relay | B46; new **B65** (track it in git, SPDX headers, tests, size and RSS) | 1 S |
| sandbox | **keep Rust** | openmind-sandbox | B28-B31 | covered |
| legacy web backends and sites (502) | **retire**; optionally serve static archive pages from one tiny Rust static server (tuner-style) | reuse `tuner` as a generic static server | new **B66** (per-host decision table: retire, static archive, or revive; staged ingress cleanup, no DNS change) | 1 S |
| GitHub private repos | **keep as archives**; no port | none | B66; owner action O1 (below) | none |

### Phases (order of work)

1. **Phase 0, stabilise what is already Rust (no new code):** B51 (switch one, galion-desk and link-watch to the dist builds), B21 and B22 (one under systemd, Restart=always), B61 (console prod), B53, B52. All staged for Slavik's approval.
2. **Phase 1, foundation:** B03-B09, B39, B42-B44 (config everywhere, portability inventory, size budget, RSS baseline, SPDX, self-check, SBOM).
3. **Phase 2, last Python out of the core:** B12-B16, B56, B57, B64 (+ B15). Gate with B16 no-python.sh.
4. **Phase 3, survival:** B20, B23, B24, B25, B54, B55, B40/B33 (nerve), B41.
5. **Phase 4, sidecars:** B58-B60 (memory-gateway in Rust), B62 and B63 (CPU STT/TTS/LLM providers), B34-B38 (provider registry and fallback), B46 and B47 (voice), B65.
6. **Phase 5, edge and sandbox:** B28-B32, B35, omnode build and size check.
7. **Phase 6, legacy retirement:** B66, archive the galion-src repos, remove dead tunnel ingress (staged), then B48-B50 (arena) last.

New tickets: B51-B66 (16 tickets: 12 S, 4 M). Existing open tickets: 41 of B01-B50 (9 are done: B01, B02, B10, B11, B17, B18, B19, B26, B27).

### Risks

- **Deploy drift:** the live binaries differ from the repo builds, and `one` runs under nohup. A reboot loses One. Switching may change behaviour (link-watch `france` becomes "unconfigured"; B11 needs the key and a retry backoff). Do the switch with rollback.sh, which is already staged.
- **Hermes is upstream Python.** Porting it is out of scope. The integration risk is state.db: the operator shell that started `one` also edited state.db through Python, and corruption artefacts exist (`state.db.malformed`, `recovered-state.db`). Rust code must open it read-only.
- **The memory-gateway rewrite** needs exact API parity with the Hermes plugin. Shadow-run it before any switch. The 615 MB RSS is the biggest saving on the box.
- **Voice on CPU-only devices:** Chatterbox and Florence need a GPU. CPU quality with whisper.cpp tiny/base and piper is lower, and latency will need measuring.
- **The kernel** has no Landlock in its LSM list (B26), so sandbox strength is rlimits plus a cleared env only on this LXC.
- **The box is busy** (load average about 7 on 8 vCPU while idle processes use little CPU, likely from the host). Keep builds at `nice -n 19 -j 2`.
- Concurrent agents are editing the openmind tree (uncommitted changes and untracked files exist). Coordinate waves per QUEUE.md.

### Owner actions (outside the queue: secrets, GitHub, DNS)

- **O1, secrets tracked in git (names only):** `galion-backend/.env`, `galion-backend/.env.backup`, `magic-merlin-showcase/.env`, `merlin-element/.env`, `tactical-belts-pro/.env.local`; `galion-backend/galion.db` is also tracked.
  Key-like patterns (file names only) appear in `galion-backend/ecosystem.config.js`, `voice-backend/scripts/ecosystem.config.js`, `galion-docs/DNS_CLOUDFLARE_SETUP.md`, and `galion-platform/Documents/project-nexus/docs/{deployment/PRODUCTION_STARTUP_GUIDE.md, DNS_CLOUDFLARE_SETUP.md}`.
  Rotate those keys and purge them from history before any repo becomes public.
- **O2:** `from-grok-build/workspace/.ssh/id_ed25519` (a private key copy, mode 600), `from-grok-build/root/galion-secrets/` and `from-grok-build/workspace/galion-light/secrets` all sit inside the 661 MB snapshot. Decide whether to keep them.
- **O3:** In the Hermes home, `config.yaml` and `SOUL.md` are mode 666 and `vault/` is mode 777 (world-writable).
- **O4:** `tuner-quick.service` publishes the tuner on a temporary trycloudflare URL. Keep it or stop it.
- **O5:** The GitHub connector token has no private-repo scope (404). Grant read access if you want a remote check of the private repos.
