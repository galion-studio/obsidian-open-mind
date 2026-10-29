# VPS check: could Moonshine / sherpa-onnx STT leak data off the server? (29 Oct 2026)

Read-only check over SSH (host alias `slavik-vps`), run 08:24–08:35 CET (Europe/Zagreb). No installs, downloads, config edits, restarts or deletes. No git was used on /opt/galion/openmind. Env vars are listed by NAME only. IPs are masked.
One slip: a redirect in one of my scan commands created an empty-ish scratch file, `/tmp/.x_none` (143 bytes, a list of Cargo.lock paths). I left it in place because deleting is out of scope. It is safe to remove.

## Short answers
- **Is it Linux?** Yes. Ubuntu 24.04 LTS, x86_64, Proxmox kernel 6.8.12-pve. It is an **unprivileged LXC container**, not a full VM (root is mapped to uid 100000). CPU is an AMD Ryzen 7 3800X with 8 vCPUs visible, AVX2/FMA/F16C, no AVX-512. RAM is 16 GiB with no swap. Disk is 100 GB ZFS, 71 GB free. systemd 255, cgroup v2.
- **Is Moonshine or sherpa-onnx installed?** No.
  - There is no `moonshine`, `useful-moonshine*`, `moonshine-voice` or `sherpa_onnx` package in any venv.
  - No sherpa or onnx libraries are in `ldconfig`.
  - No Moonshine or sherpa model files exist anywhere.
  - The only "moonshine" hits are the model *source files* that ship inside HF `transformers` 5.2.0 in the TTS venv (`transformers/models/moonshine*`). They are not installed models.
  - The only `libonnxruntime` is `onnxruntime` 1.30.0 inside `/opt/galion/stt/venv`, pulled in by faster-whisper for its Silero VAD.
  - None of the 3 Cargo.lock files under /opt/galion (openmind, openmind-node, vps-mcp) contain `ort`, `sherpa*`, `moonshine*`, `candle*`, `hf-hub`, `sentry`, `posthog`, `opentelemetry`, `reqwest` or `ureq`.
- **What the voice service is today:** **There is no live voice service on the VPS.**
  - `galion-voice-serve` does not exist: no binary in PATH, /usr/local/bin, /opt/galion/bin, /opt/galion/rust or openmind target/release, and no systemd unit. No voice, STT or TTS process is running.
  - The openmind tree has only `crates/galion-voice-core`, a pure-Rust framing/VAD/jitter/session crate with **no dependencies**. `galion-voice-cpu`, the candle crate with the `galion-voice-serve` bin, exists only in old copies under `/srv/box-archive/2026-10-21/...`.
  - Two older, idle Python setups are present:
    - `/opt/galion/stt`: venv with faster-whisper 1.2.1, ctranslate2, onnxruntime 1.30.0 and huggingface_hub 1.32.0. Its venv `bin/` directory is missing, so it cannot run as-is. The model is a HF-cache-layout `faster-whisper-large-v3-turbo` (~1.6 GB) in `/opt/galion/stt/models`.
    - `/opt/galion/tts`: venv with gradio 6.8.0, transformers 5.2.0, huggingface_hub, onnx, requests and httpx. The model is `ResembleAI/chatterbox` (~3 GB, HF cache layout) in `/opt/galion/tts/models`.
    - Neither has a unit, and neither is running.

## Data-leak indicators found
1. **No telemetry or analytics configuration anywhere.**
   - I found none of these variable names: HF_*, HUGGINGFACE*, *TELEMETRY*, *ANALYTICS*, SENTRY*, POSTHOG*, DO_NOT_TRACK, ORT_*, OTEL_*, GRADIO_* or TRANSFORMERS_*.
   - I looked in systemd units and drop-ins, /etc/environment, /etc/profile(.d), /etc/bash.bashrc, root's .bashrc/.profile, and the unit EnvironmentFiles. The one regex hit, `NERVE_REPORT_EVERY`, is unrelated.
   - The env files hold only API keys and local host/port settings (names only): `/opt/galion/hermes/.env` holds API_SERVER_KEY, RIVER_API_KEY, OPENROUTER_API_KEY, MEMORY_TENCENTDB_GATEWAY_HOST/PORT and TDAI_GATEWAY_API_KEY. `/etc/openmind-memory.env` holds OPENMIND_MEMORY_HOST/PORT/DATA_DIR/INSTANCE_ID and TDAI_GATEWAY_API_KEY. None relate to STT.
   - **The flip side:** nothing *disables* telemetry either. There is no `HF_HUB_OFFLINE`, `HF_HUB_DISABLE_TELEMETRY`, `GRADIO_ANALYTICS_ENABLED=False` or `DO_NOT_TRACK`.
2. **No HF cache or token.** `~/.cache/huggingface` does not exist for root, there is no `token` file, and no `.gradio` or `/tmp/gradio` directory exists. The HF model caches live inside /opt/galion/{stt,tts}/models, so those earlier installs downloaded from Hugging Face once.
3. **No stats or analytics files tied to voice.** "analytics" and "telemetry" names appear only in an archived console-UI source snapshot and in crew-skills docs (marketing/product skill text). None of them is a runtime data file.
4. **Latent risks in what's already on disk (not running):**
   - The TTS venv's **gradio** sends usage analytics by default when launched, unless `GRADIO_ANALYTICS_ENABLED=False` is set.
   - `huggingface_hub`, `transformers` and `faster-whisper` will **auto-download from Hugging Face** (and send HF telemetry headers) if given a model *name* instead of a local path, unless `HF_HUB_OFFLINE=1` is set. Don't reuse those venvs for Moonshine.
5. **Outbound connections right now.** The only established TCP flows are:
   - loopback: cloudflared → galion-console on 127.0.0.1:8080, and gaia-cortex → openmind-nerve on 127.0.0.1:8670;
   - two inbound SSH sessions on port 22, one of them this check (peers 54.69.x.x and 32.185.x.x).
   - cloudflared also has 4 unconnected UDP sockets, which is its QUIC tunnel to Cloudflare. That is expected and is how the public site is served.
   - All other services (galion-one, desk, health, hermes-gateway, tuner, tanga, openmind-memory, openmind-queue, postfix) listen on 127.0.0.1 only. sshd is the only public listener.
   - I saw no unexpected remote endpoints.
6. **Firewall: egress is wide open.** ufw is active with deny incoming, **allow outgoing**, and only 22/tcp allowed in. The nftables `inet filter` output policy is accept, and iptables and ip6tables OUTPUT are ACCEPT (ufw chains only). Any process can reach the internet.
7. **Important: systemd `IPAddressDeny` does NOT work on this host.** `galion-nerve.service` already has `IPAddressDeny=any` / `IPAddressAllow=localhost` in drop-in `75-no-openrouter.conf`. Its own comment records a test on 2026-10-21 at 18:16 (+02:00): because this is an LXC container, the cgroup BPF IP filter is not enforced, and a transient unit with the same lines still reached a public host. **So an `IPAddressDeny` line alone would give false confidence here.**

## Combined with the code-level findings
From `reports/stt-privacy-check-20261029.md` and `reports/streaming-stt-research-20261029.md`:
- **Moonshine core, sherpa-onnx core and their Python bindings contain no telemetry, analytics or crash reporting, and nothing in them sends audio or transcripts.**
- **ONNX Runtime telemetry is Windows-only.** On Linux it is a no-op.
- **The only network traffic is model and binary downloads:**
  - Moonshine Python helpers (`MicTranscriber.load()` without `models_from`, the spelling model, diarization, the CLI without `--model-path`) fetch from download.moonshine.ai.
  - Legacy `useful-moonshine*` uses `hf_hub_download`.
  - The Rust crates fetch at *build* time: ort-sys from the pyke CDN, moonshine-sys from GitHub Releases (no checksum), sherpa-rs-sys from GitHub Releases.
  - These downloads reveal the server IP and which model is used. They don't send audio.
- Things not to enable: transcribe-rs `openai` feature, sherpa websocket server binaries, `identify_speakers` without a local diarization dir.
- Net for this VPS: **installing Moonshine Tiny Streaming (or a sherpa-onnx Zipformer) with local model files would not send audio, transcripts, stats or analytics off the box. The code doesn't do it, and nothing on the host is configured to collect it.** The remaining risk is accidental downloads plus the fully open egress. The steps below close that.

## Lock-down steps for install time (recommendations only, nothing applied)
1. **Fetch models once, by hand, off the VPS or in a one-off step.** Check SHA-256 against a recorded value, then place them read-only, e.g. `/opt/galion/models/moonshine-tiny-streaming-en/` (root-owned, 0444 files). Always pass explicit file paths:
   - Python: `Transcriber(model_path, ModelArch.TINY_STREAMING)`, or `MicTranscriber().models_from(dir).spelling_model(None)`, with no `identify_speakers`.
   - moonshine-rs: `Transcriber::from_files`.
   - sherpa: explicit encoder/decoder/joiner/tokens paths.
2. **Build offline (Rust path).** Run `cargo vendor` + `--offline`/`CARGO_NET_OFFLINE=true`, and set `ORT_LIB_LOCATION` + `ORT_SKIP_DOWNLOAD=1` for ort, `MOONSHINE_DIR` for moonshine-sys, or `default-features=false` + `SHERPA_LIB_PATH` for sherpa-rs. Prefer building on the build box and shipping the binary plus `libonnxruntime.so` (with recorded checksums).
3. **Use a fresh, minimal venv** if going the Python route (e.g. `/opt/galion/stt-moonshine/venv`, with only `moonshine-voice` or `sherpa-onnx` + numpy). Don't reuse the TTS venv (gradio/transformers) or the broken faster-whisper venv.
4. **Set offline env vars in the unit:** `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `HF_HUB_DISABLE_TELEMETRY=1`, `HF_HUB_DISABLE_IMPLICIT_TOKEN=1`, `DO_NOT_TRACK=1`, `GRADIO_ANALYTICS_ENABLED=False`, `MOONSHINE_VOICE_CACHE=/opt/galion/models/moonshine` (read-only), `HF_HOME=/nonexistent-or-readonly`. Use `UnsetEnvironment=` to strip the API-key names listed above.
5. **Run as a dedicated system user** (e.g. `galion-stt`), with no shell and no access to /opt/galion/hermes/.env.
6. **Block egress with something that actually works in this LXC.** Use more than one layer:
   - **Primary:** an nftables output rule for that user: `meta skuid "galion-stt" oif != "lo" drop` (or `socket cgroupv2 level 2 "system.slice/galion-stt.service"`). Then switch ufw outgoing to deny for everything that doesn't need it, or at least add `ufw deny out` for that user via before.rules. Netfilter does work in this container, because ufw's chains are counting packets.
   - **If the service only needs a Unix socket or loopback:** use `PrivateNetwork=yes`. This needs testing, because network namespaces in an unprivileged LXC usually work but this host hasn't been tested.
   - **Also add:** `IPAddressDeny=any` + `IPAddressAllow=localhost`, kept only as intent or for a future non-LXC host, plus `RestrictAddressFamilies=AF_UNIX AF_INET AF_INET6`.
   - **Optionally**, at the host (Proxmox) level, add a per-container egress firewall.
7. **Sandbox the unit:** `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateTmp=yes`, `ReadOnlyPaths=/opt/galion/models`, `ReadWritePaths=` only what it truly needs, `NoNewPrivileges=yes`, `CapabilityBoundingSet=`, `MemoryMax=` (e.g. 1G) and `CPUQuota=` so it can't starve the other services.
8. **Keep no audio or transcripts on disk:** process audio in memory and don't log transcript text. Set `LogLevelMax=info` and audit what the app logs to journald.
9. **Verify after install:**
   - Run a test transcription under `strace -f -e trace=network` and confirm no `connect()` to non-loopback addresses.
   - Check `ss -tunp` during a run.
   - Run a deliberate `curl https://example.com` as the service user (or within the unit via `systemd-run -p User=galion-stt ...`) and confirm it fails.
   - Re-run this check.
10. **Keep the Cloudflare tunnel as the only way in.** Bind the STT service to 127.0.0.1 or a Unix socket, not 0.0.0.0.
