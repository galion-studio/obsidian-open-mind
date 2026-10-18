# A — Foundation: the Rust core of the Obsidian / Open Mind stack

Read-only study of root@51.38.137.193 (ip50). Snapshot taken **2026-10-18 18:52 CEST (UTC+2)**. The VPS clock is UTC; every time below has been converted to Europe/Zagreb (CEST until 2026-10-25, then CET UTC+1). Published milestone: **2026-10-31**.
Load on the VPS: about 10 short ssh commands (cat/grep/ls/ps, plus `find -maxdepth 3/4` under nice/ionice). No writes, no Python run.

## 1. Component inventory

Average CPU is the unit's CPUUsageNSec divided by its uptime, as % of one core. RSS/cgroup figures are from the snapshot.

| Component | Lang | Size | Runs as / port | Avg CPU | RAM | Up since (CEST) |
|---|---|---|---|---|---|---|
| **openmind-schema** 0.1.0 | Rust (lib) | 1 file, 4 KB `lib.rs`; deps serde + serde_json | not a process. `/opt/galion/openmind-schema`, same copy in `/root/openmind-schema` (diff: identical) | – | – | src 2026-10-08 10:50 |
| **Galion One** `one` | Rust (std only, no Cargo) | `src/main.rs` 2,806 lines / 106 KB; binary 1.06 MB (not stripped; links libc, libm, libgcc_s, **libsqlite3** over raw FFI). 13 backup binaries of ~5 MB each sit next to it (`one.prev-*`, `one.bak-pyramid`) | nohup, **no systemd unit**, 127.0.0.1:8091 + 8002 | 0.2 % | 9.7 MB RSS, 5 threads | started 2026-10-13 12:57 (binary mtime 12:57) |
| **galion-desk** | Rust (std only, no Cargo) | `desk-rs/main.rs` 569 lines; binary 4.7 MB (with debug_info) | `galion-desk.service` (Restart=on-failure), 127.0.0.1:8646 | 0.003 % | 0.7 MB cgroup | 2026-10-12 07:01 (binary 2026-10-10 11:02) |
| **room** | Rust (std only) | `room/src/main.rs` 9.9 KB; binary 4.6 MB | **not running** (would bind 127.0.0.1:4020; a static "who's alive" page) | – | – | binary 2026-09-25 15:35 |
| **galion-rs** workspace | Rust (Cargo workspace, MIT) | 123 KB. **Only `Cargo.toml`, README, a migration plan, Dockerfile and `tests/integration_tests.rs`. The `crates/` directory does not exist.** | not built | – | – | – |
| Hermes gateway | **Python** 3.12 (hermes-agent 0.21.4, MIT) | venv `/opt/galion/venv` | `hermes.service`, 8642 | **2.8 %** | 354 MB RSS (367 MB cgroup), **701 threads** | 2026-10-16 09:09 |
| memory-gateway (TencentDB Agent Memory) | **Node/TypeScript** via tsx + esbuild | node_modules tree | `memory-gateway.service`, 8420 | 0.9 % | **614 MB RSS** (597 MB cgroup) + esbuild 11 MB | 2026-10-14 21:49 |
| console-ide | **Node/TS** (React 19, Vite **dev** server) | node_modules tree | `console.service`, 8080 | 0.14 % | **512 MB** + 32 MB wrapper | 2026-10-15 08:02 |
| link-watch | **Python** (94 lines) | – | `link-watch.service`, writes `state/link.json` | 0.03 % | 17 MB | 2026-10-12 07:01 |
| desk-hermes.py | **Python** (229 lines, sqlite3) | – | spawned by galion-desk on each snapshot | short-lived | – | – |
| Voice "ear/mouth/eye" workers | **Python** (`stt/stt_server.py`, `tts/tts_server.py` + `worker.py` 76 lines, `vision/*.py`) | models 1.6 + 3 + 2.3 GB | not running (TTS needs torch, which is missing) | – | – | – |
| Other Python in galion-light | guardian/desk_heal.py, mesh.py, loop.py, grade.py, blackbox/tick.py, origin.py, revive.py, mcp/stack-read.py, scripts/*.py (boss, earn, health-tick…) | – | not running in this snapshot | – | – | – |
| cloudflared | Go | – | `cloudflared.service` | 0.23 % | 37 MB | 2026-10-13 11:07 |

**Totals.** The Rust parts use about 10 MB of RAM and almost no CPU. The non-Rust parts use about 1.5 GB of RAM (memory-gateway 614 MB, console 544 MB, Hermes 354 MB) and roughly 4 % of one core on average. Hermes is the only piece with real CPU use. The stack is already CPU-light: in total it averages under 0.05 of the 8 vCPUs.

## 2. How the Rust foundation fits together (as built)

There is **no shared Rust foundation yet**. There are three separate single-file `std`-only programs, and each is built with bare `rustc`. The toolchain is `~/.cargo/bin/rustc 1.99.0` and is not on PATH. No build script references it. None of them uses Cargo, crates.io, tokio, hyper or serde.

- **Concurrency model.** Each one is `std::net::TcpListener` plus a thread per connection. HTTP/1 is parsed and written by hand. Outbound HTTP is also hand-written (`one`: `http_to()` with `TcpStream::connect_timeout`; desk: `http_get/http_post`). Some calls go out through `curl` in a subprocess (the edge probes in `one`). This is about the lightest design possible: around 10 MB of RAM, nothing to update, no async runtime.
- **openmind-schema is not depended on by anything.** Neither `one`, galion-desk nor room imports it (grep for `openmind` finds nothing), and none of them could, because none uses Cargo. It is a design document written as a crate:
  - `Machine` {studio, stack, stance, ram_mib, gpu, vram_mib, public_ip} with `weights()` (GPU and ≥8 GiB VRAM and ≥8 GiB RAM) and `mouth()` (≥8 GiB RAM).
  - `ProviderKind` {Remote, Local}, and `Provider` {id, kind, base_url, key_slot, default_model} ("adding a site is a new row, not a new program").
  - `KeySlot` {name, present, hint} (secrets never live in the schema).
  - `Surface` {console, desk, api, hermes}, and `Stack` with `Stack::cpu_desk()`, which has the laptop Ollama `huihui_ai/qwen3.5-abliterated:9b` and OpenRouter.
  - One test checks that a CPU box does not turn weights on and that the JSON contains no `sk-`.
  - Its stance: "The processor runs this. The card only runs the model."
- **Galion One → others.**
  - mind: Hermes at `127.0.0.1:8642/v1/chat/completions`, model id read from the file `one/model` (= `galion`).
  - ear: `8643/v1/audio/transcriptions`.
  - eye: 8645.
  - mouth: its own placeholder on 8002, plus a spawned `/workspace/tts/venv/bin/python` worker.
  - memory-gateway: checks `8420/health` and looks the process up through `ps`.
  - Reads `hermes/state.db` read-only through libsqlite3 FFI, and its "defence" table also through FFI.
  - Shells out to `python3 -c` to **delete rows from Hermes `state.db`** (session `one-warm`), and to `python3 one/models.py`, `pdftotext`, `nvidia-smi`, `sha256sum` and `curl`.
- **galion-desk** offers `GET/POST /desk` and `GET /health`.
  - It reads and writes JSONL/flag files under `galion-light/state/` (desk-notes, desk-focus, desk-hold, link.json, survival.json, machine.json) and also reads `hermes/gateway_state.json`, `/proc/meminfo|loadavg|cpuinfo` and Galion One's `voice`/`session.jsonl`.
  - It talks to laptop Ollama `127.0.0.1:11434` directly (`/api/generate`, `/api/ps`). That port is down.
  - For Hermes session data it shells out to `python3 /opt/galion/bin/desk-hermes.py`.
  - It never calls the Hermes API.
- **Integration bus = the filesystem.** `state/*.json(l)`, Hermes `state.db`, and `link.json` (written by the Python link-watch). There is no shared types crate on the wire. Each binary has its own ad-hoc JSON strings.
- **The aspirational design**, `galion-src/galion-rs`, is the opposite of CPU-light: tokio "full", axum, sqlx/sea-orm Postgres, Redis, ethers/siwe, Stripe, wgpu/winit, cranelift JIT, `ort` with CUDA/TensorRT, qdrant. It is unbuilt (no crates), and if built it would be a heavy server.

## 3. Licensing
- Declared MIT: `openmind-schema/Cargo.toml` (`license = "MIT"`) and the `galion-rs` workspace (`license = "MIT"`).
- **There is no LICENSE file anywhere in Slavik's own code** (openmind-schema, galion-light, one, desk-rs, room, galion-rs root). A Cargo `license` field alone is weak. The plan says MIT, so add `LICENSE` (MIT text plus copyright holder) to each repo before the 2026-10-31 milestone.
- `one`, desk-rs and room have no license declaration at all, and no git repo.
- Third-party code with LICENSE files: hermes-agent (MIT), node, node-memory, crew-skills (+loop-library). The TencentDB memory package license was not checked: it sits inside node_modules, which this study avoided.

## 4. Gaps vs "Rust-only, CPU-light"
1. **The foundation is not shared.** The OpenMind types are used by nobody. Three binaries copy-paste HTTP/JSON/sqlite code, and there is no Cargo workspace, lockfile, git or reproducible build.
2. **The Rust binaries still depend on Python:** `one` runs `python3 -c` to edit Hermes `state.db` and calls `models.py` and the TTS venv, and the desk calls `desk-hermes.py`. Writing to Hermes's DB from outside is also risky.
3. **The heavy parts are not Rust:**
   - memory-gateway (Node, 614 MB) is the biggest RAM user and runs TypeScript through tsx at runtime.
   - console runs `vite dev` in production (512 MB).
   - Hermes (Python, 354 MB, 701 threads) is upstream and should stay as-is.
   - link-watch is a 94-line Python loop that is easy to port.
   - The voice/vision workers are torch/Python and are not running.
4. **Ops:** `one` has no systemd unit. There are about 64 MB of stale backup binaries. Binaries are unstripped (galion-desk has debug info). The toolchain is not on PATH.
5. **Timestamps:** the box runs on UTC, and the state files and logs carry UTC/epoch times. **Rule for the foundation:** record every timestamp as RFC 3339 in Europe/Zagreb with an explicit offset (e.g. `2026-10-18T18:52:23+02:00`; `+01:00` after 2026-10-25). Use a shared `openmind::time` helper (e.g. `jiff` or `chrono-tz`) so that every service writes the same format.
6. galion-rs, the declared "Rust migration", is far from CPU-light (Postgres, Redis, wgpu, CUDA) and has no code. It should not be the base.

## 5. Exo and similar tools for GPU compute
The design rule is already written in openmind-schema: the CPU box owns the loop, memory and keys, and a GPU is just another `Provider` row. So any accelerator that exposes an **OpenAI-compatible HTTP endpoint** fits without new code. Add a `Provider{kind: Local, base_url}` row and a Hermes provider entry.

| Tool | License | Hardware | Fit |
|---|---|---|---|
| **exo** (exo-explore/exo, about 48k★) | Apache-2.0 | 1.0 rewrite: **Apple Silicon via MLX** (with RDMA over Thunderbolt 5, tensor parallelism, automatic discovery). Linux is CPU-only; GPU support is "in development". The tinygrad CUDA/AMD backend was removed in v1. The CUDA fork `Scottcjn/exo-cuda` is GPL-3.0 (license conflict with MIT, keep it out-of-process only) | Good for pooling **Macs** (Slavik's/Mate's) into one endpoint. It serves OpenAI Chat, Claude Messages, Responses and Ollama APIs, so Hermes can point at it the way it points at the laptop today. It is Python, so run it on the GPU hosts, **never on the VPS**. Weak for NVIDIA/AMD today. |
| **llama.cpp `rpc-server` / llama-server** | MIT | CUDA, ROCm/HIP, Vulkan (AMD/Intel), Metal, CPU | Best broad-hardware option. It is already on the box (`galion-light/tulu`, llama.cpp checkout). The RTX 4060 laptop runs llama-server (or rpc-server, to pool several GPUs, Macs included through Metal). It is a C++ single binary, with no Python. |
| Ollama | MIT | CUDA, ROCm, Metal | Already the "laptop" provider (11434). It is the simplest choice per machine, but it does not pool across machines. |
| GPUStack | Apache-2.0 | NVIDIA/AMD/Apple/Ascend, schedules llama.cpp/vLLM/MLX | A cluster manager if there are more than 3 heterogeneous GPU boxes. It is heavier. |
| mistral.rs / candle (Rust) | MIT/Apache | CUDA, Metal, CPU | Use this if a **Rust-native** inference worker is wanted later (`quant-worker` on RunPod). |
| vLLM | Apache-2.0 | NVIDIA, AMD ROCm | For RunPod/datacenter throughput, not for home devices. |

**Transport:** the GPU hosts should reach the VPS the same way the laptop does: a reverse SSH or Cloudflare tunnel to a 127.0.0.1 port. The VPS opens no inbound ports. A small Rust "link" probe (replacing link-watch) fills in `Machine.gpu/vram_mib` from each provider's `/v1/models` or `/api/ps` and flips `weights()`. (This last point is an **inference**: exo's model-listing endpoint was not tested.)

## 6. Top 5 recommendations (toward the 2026-10-31 milestone)
1. **Make `openmind` a real Cargo workspace** with git and MIT `LICENSE` files. Members: `openmind-schema` (types, plus a `time` module with Zagreb RFC 3339 timestamps), `openmind-http` (the existing std-thread HTTP/1 server and client, extracted from `one`; no tokio needed at this scale), `openmind-state` (typed state files and read-only rusqlite access to Hermes `state.db`). Then move `one`, `galion-desk` and `room` into it. Build them `--release`, stripped, with `panic=abort`, and expect binaries under about 1 MB.
2. **Remove Python from the Rust binaries.** Port `desk-hermes.py` and link-watch (≈320 lines together) to Rust. Replace `one`'s `python3 -c` DELETE on Hermes `state.db` with a call to the Hermes API (or drop the warm-session hack). Never write to Hermes's DB from outside.
3. **Fix ops for `one`.** Add a systemd unit (Restart=always), move the 13 backup binaries out (≈64 MB), and point its model file at a model that exists (the `galion` 404 is covered by the OpenRouter proposal).
4. **Cut the Node RAM first, Rust later.** Run the console as a production build (`vite build` + a static or small server) instead of `vite dev`, which saves about 400–500 MB. Precompile or bundle the memory-gateway instead of running tsx at runtime. Rewriting memory-gateway in Rust is a later stage that needs `embeddingService` fixed first. Keep Hermes upstream in Python; it is the "mind", not the foundation.
5. **Add GPU only as Provider rows.** Use llama.cpp server/rpc on the RTX 4060 and AMD machines and exo (MLX) on Macs, each reached through a tunnel to a 127.0.0.1 port and registered in openmind-schema and Hermes providers. Keep heavy runtimes (exo, torch voice workers) **off the VPS**. Stay with Apache/MIT tools and avoid the GPL exo-cuda fork inside MIT code. Shelve `galion-rs`'s heavy plan (Postgres, Redis, wgpu, CUDA in-process), because it contradicts the CPU-light stance.
