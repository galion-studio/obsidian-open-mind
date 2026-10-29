# STT privacy check: Moonshine vs sherpa-onnx (29 Oct 2026)

Research only. Nothing was installed or built. I read source through the GitHub API, raw files, and the published crate and wheel archives (a few MB in /tmp). No models were downloaded.
Versions checked: moonshine-ai/moonshine `main` @ `234f60f` (2026-08-24), moonshine-voice 0.1.5 (PyPI), useful-moonshine-onnx 20251121; k2-fsa/sherpa-onnx `master` @ `a7423b6` plus the v1.12.9 source vendored in sherpa-rs-sys 0.6.8; moonshine-rs/moonshine-sys 0.2.6, transcribe-rs 0.3.12 (+ ort 2.0.0-rc.12), sherpa-rs 0.6.8; ONNX Runtime v1.23.2.

## Bottom line
- **Both run fully offline once the model files are on disk.** I found no telemetry, analytics, crash reporting (Sentry, PostHog, Segment, Mixpanel), update checks or phone-home in any of the code paths below. No code sends audio or transcripts anywhere.
- **The only network access is fetching models and binaries.** Some of it is automatic: on first run when no local path is given, and at `cargo build` time through the Rust crates' default features. Every case can be turned off.
- **ONNX Runtime telemetry is Windows-only (ETW).** On Linux it compiles to a no-op.

## Moonshine
**Docs:** the README says "Everything runs on-device — fast, private, and with no account or API keys" ([README.md#L11](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/README.md#L11)). The docs say the high-level helpers download on first `load()` and that you can warm the cache "before going offline" ([docs/using/downloading-models.md#L10](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/docs/using/downloading-models.md#L10), [#L36](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/docs/using/downloading-models.md#L36)). I found no separate privacy policy in the repo.

**Core C++ library (libmoonshine):** has no HTTP client. Code search found no socket, curl or telemetry use in `core/`, and CMake links no network library. The C API only *returns* download manifests (CDN URLs, sizes, CRC32C) for the bindings to fetch ([core/moonshine-c-api.h#L1074-L1076](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/core/moonshine-c-api.h#L1074-L1076)). The download test tool says "HTTP lives entirely in the bash harness (curl); this tool never touches the network" ([core/moonshine-download-smoke.cpp#L22-L23](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/core/moonshine-download-smoke.cpp#L22-L23)). The model catalog is compiled into the binary, not fetched. ORT 1.23.2 is a prebuilt Microsoft binary committed to the repo, so building with CMake does not download it ([find-ort-library-path.cmake#L47-L51](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/core/third-party/onnxruntime/find-ort-library-path.cmake#L47-L51)).

**Python `moonshine-voice` (current):**
- `Transcriber(model_path, arch)` loads from disk and makes no network calls.
- The one network path is `download_file.py`, which uses `requests.get` against `https://download.moonshine.ai/...` ([L162](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/download_file.py#L162), [L169](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/download_file.py#L169)). If the cached file exists and matches its size, it returns without importing `requests` ([L138-L145](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/download_file.py#L138-L145)).
- These calls download automatically:
  - `MicTranscriber.load()` without `.models_from(dir)` ([mic_transcriber.py#L266](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/mic_transcriber.py#L266)).
  - `AgentFlow` ([agent_flow.py#L1009](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/agent_flow.py#L1009)).
  - The CLI or `python -m` demo with no `--model-path` ([transcriber.py#L920](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/transcriber.py#L920)).
- **Gotcha:** `MicTranscriber` still tries to fetch the *spelling* model even when you use `.models_from(dir)`, unless you call `.spelling_model(path_or_None)` ([#L277](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/mic_transcriber.py#L277); a failure here is not fatal). It also fetches diarization models if `identify_speakers` is set without `diarization_model_dir` ([#L293](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/mic_transcriber.py#L293)).
- The cache lives in the platformdirs user cache. The location can be overridden with `MOONSHINE_VOICE_CACHE` ([download_file.py#L42-L45](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/language-bindings/python/src/moonshine_voice/download_file.py#L42-L45)).

**Legacy `useful-moonshine-onnx` / `useful-moonshine`:** these call `huggingface_hub.hf_hub_download("UsefulSensors/moonshine", ...)` when you don't pass `models_dir` (`moonshine_onnx/model.py` L2-L11 and L26-L30 in wheel 20251121; `moonshine/model.py` L632-L637 in useful-moonshine 20241016). Passing `models_dir` skips it. They are not needed for the streaming models. If used anyway, set `HF_HUB_OFFLINE=1`.

**Android/Swift/Web (not our target):** the Android library manifest declares the INTERNET permission for its downloader ([downloading-models.md#L102](https://github.com/moonshine-ai/moonshine/blob/234f60faa0eb388b01cdf7e60aca232af37aefda/docs/using/downloading-models.md#L102)). The web build caches models from the CDN.

**Rust `moonshine-rs` 0.2.6 (third-party, ghchinoy/moonshine-rs):**
- **Build time:** `moonshine-sys` looks for a local moonshine checkout (`MOONSHINE_DIR` or a sibling `../moonshine`). If it finds none, it **downloads a prebuilt libmoonshine + ORT tarball from GitHub Releases** (`moonshine-ai/moonshine/releases/download/v0.1.5/...`) with `ureq` ([build.rs#L65-L103](https://docs.rs/crate/moonshine-sys/0.2.6/source/build.rs), fallback at L266). The download has no checksum check (I saw none in build.rs).
- **Runtime:** it downloads diarization models only if `identify_speakers=true` and no dir is set (`src/lib.rs` L360, L434, L1368, L1436 — [source](https://docs.rs/crate/moonshine-rs/0.2.6/source/src/lib.rs)). Otherwise `Transcriber::from_files(path, …)` is local only.

**Rust `transcribe-rs` 0.3.12 (Moonshine streaming via `ort`):**
- No model downloads, no telemetry.
- `ureq` appears only in the opt-in `whisperfile` feature, which talks to a server it starts on localhost. The `openai` feature is a cloud engine; do not enable it ([Cargo.toml](https://docs.rs/crate/transcribe-rs/0.3.12/source/Cargo.toml)).
- **Build time:** it uses `ort` with default features, which include `download-binaries`. `ort-sys` fetches prebuilt ORT 1.24.2 from `https://cdn.pyke.io/...` and checks its SHA-256 ([ort-sys build/download/dist.txt L11](https://docs.rs/crate/ort-sys/2.0.0-rc.12/source/build/download/dist.txt)). Set `ORT_LIB_LOCATION`/`ORT_LIB_PATH` for a local ORT, or `ORT_SKIP_DOWNLOAD=1` / `CARGO_NET_OFFLINE=true` to skip ([build/vars.rs L3, L9](https://docs.rs/crate/ort-sys/2.0.0-rc.12/source/build/vars.rs)).
- `ort`'s `fetch-models` feature (model download by URL) is off by default.

## sherpa-onnx
**Docs:** the README says it runs speech-to-text and the other functions "**locally**" ([README.md#L99](https://github.com/k2-fsa/sherpa-onnx/blob/a7423b6cd581457a4e1581cfebe471f94cc832a0/README.md#L99)). Models such as `sherpa-onnx-streaming-zipformer-en-20M-2023-02-17` are downloaded by hand from GitHub Releases or Hugging Face. I found no privacy statement and no telemetry mentions (the only repo hit for "telemetry" is unrelated README text).

**Core C++ / C API:** no network code. The only networking is the optional `sherpa-onnx-online/offline-websocket-server/client` *executables* (asio/websocketpp). They are separate binaries, not part of `sherpa-onnx-core` ([sherpa-onnx/csrc/CMakeLists.txt#L570-L583](https://github.com/k2-fsa/sherpa-onnx/blob/v1.12.9/sherpa-onnx/csrc/CMakeLists.txt#L570-L583)), and they are built only with `SHERPA_ONNX_ENABLE_WEBSOCKET`+`BINARY`. Code search on `master` csrc found no curl.

**Python `sherpa_onnx` (2.0.1):** I read all 7 modules on master. None import requests, urllib or socket, and there is no auto-download. You always pass model file paths.

**Build time (CMake from source):** `FetchContent` downloads ORT (csukuangfj/onnxruntime-libs 1.17.1 from GitHub, with an hf-mirror.com fallback) plus deps: kaldi-native-fbank, kaldi-decoder, kaldifst, openfst, simple-sentencepiece, eigen, and asio/websocketpp if websocket is on. All are SHA-256 pinned ([cmake/onnxruntime-linux-x86_64.cmake#L17-L30](https://github.com/k2-fsa/sherpa-onnx/blob/v1.12.9/cmake/onnxruntime-linux-x86_64.cmake#L17-L30)). Each cmake file first checks local files (`~/Downloads/...`, `/tmp/...`). `SHERPA_ONNXRUNTIME_LIB_DIR`/`_INCLUDE_DIR` point it at a local ORT (`cmake/onnxruntime.cmake` L138-L174).

**Rust `sherpa-rs` 0.6.8:**
- **Build time:** default features include `download-binaries`. `sherpa-rs-sys` build.rs downloads prebuilt sherpa-onnx v1.12.9 from `github.com/k2-fsa/sherpa-onnx/releases` and checks SHA-256 ([build.rs#L386](https://docs.rs/crate/sherpa-rs-sys/0.6.8/source/build.rs), `dist.json`). If that fails it builds from vendored source with CMake (L417), and that CMake build then does the FetchContent downloads above. The CMake fallback sets websocket and binaries OFF (L441-L443). Setting `SHERPA_LIB_PATH` skips both (L421).
- **Runtime:** no network code in `sherpa-rs/src`.

## ONNX Runtime telemetry
- Microsoft's Privacy.md says: "Currently telemetry is only implemented for Windows builds…". Builds from source collect nothing ([docs/Privacy.md#L14](https://github.com/microsoft/onnxruntime/blob/v1.23.2/docs/Privacy.md#L14)).
- On Windows, ETW TraceLogging events go to the OS, which may forward them to Microsoft depending on the OS diagnostic-data setting.
- On Linux, `PosixEnv` uses the base `Telemetry` class ([posix/env.cc#L584-L596](https://github.com/microsoft/onnxruntime/blob/v1.23.2/onnxruntime/core/platform/posix/env.cc#L584-L596)). In that class `IsEnabled()` returns `false` and every `Log*` is a no-op ([telemetry.cc#L15-L16](https://github.com/microsoft/onnxruntime/blob/v1.23.2/onnxruntime/core/platform/telemetry.cc#L15-L16)). **So there is no ORT telemetry on Linux.**
- The `ort` crate defaults `telemetry: true`, but its own docs say pyke and source builds have none. Only the separate `ort-web` backend sends telemetry (to pyke), and we would not use it ([ort src/environment.rs L486-L506](https://docs.rs/crate/ort/2.0.0-rc.12/source/src/environment.rs)). On Windows, call `.with_telemetry(false)`, or `DisableTelemetryEvents` in the C API.

## Privacy concerns
1. First-run auto-downloads, from download.moonshine.ai, GitHub or Hugging Face, reveal your IP and which model you use to the CDN. They send no audio or transcripts.
2. Supply chain: the build-time binary downloads (pyke CDN, GitHub Releases) are prebuilt code you didn't compile. ort-sys and sherpa-rs-sys verify SHA-256. moonshine-sys does not appear to.
3. Third-party wrappers (moonshine-rs, transcribe-rs, sherpa-rs) are not maintained by the model authors. Pin exact versions and re-audit when you upgrade.
4. Don't enable transcribe-rs `openai` (cloud), sherpa-onnx websocket server binaries, or moonshine `identify_speakers` without a local diarization dir.

## Lock-down checklist
- **Pin local model paths:**
  - Moonshine Python: use `Transcriber(model_path, ModelArch.TINY_STREAMING)` directly, or `MicTranscriber().models_from(dir).spelling_model(None or path)`, and set `MOONSHINE_VOICE_CACHE` to a read-only vendored dir.
  - moonshine-rs: `Transcriber::from_files` with an explicit dir and no speaker ID.
  - sherpa: always uses explicit paths.
- **Build offline:**
  - Vendor the crates (`cargo vendor`) and build with `--offline` / `CARGO_NET_OFFLINE=true`.
  - transcribe-rs: `ORT_LIB_LOCATION=<local ORT>` (+ `ORT_SKIP_DOWNLOAD=1`).
  - moonshine-rs: `MOONSHINE_DIR=<local moonshine checkout>`.
  - sherpa-rs: `default-features=false` + `SHERPA_LIB_PATH=<local libs>`.
  - sherpa CMake: `SHERPA_ONNXRUNTIME_LIB_DIR`, with dep tarballs pre-placed.
- **Block egress at runtime:** run the STT process in a network namespace with no network (`systemd` `PrivateNetwork=yes` / `IPAddressDeny=any`, `unshare -n`, `docker --network none`, or firewall rules). Then verify with `strace -f -e trace=network` or `ss -tp` during a test run that no sockets open.
- Also set `HF_HUB_OFFLINE=1` (only matters for the legacy Moonshine packages).

## Not verified
- I did not run anything or capture network traffic. Everything above comes from reading source.
- Prebuilt binaries (the ORT .so files in the moonshine repo, the pyke CDN builds, sherpa-onnx release tarballs, PyPI wheels' native libs) were not disassembled. I am assuming they match the published source.
- I did not do a full line-by-line read of every C++ file. I relied on code search (GitHub indexes the default branch only) plus targeted reads; for sherpa I also grepped the full vendored v1.12.9 source locally.
- The moonshine-rs GitHub `main` may differ from crate 0.2.6, so the line numbers above refer to the crate sources on docs.rs. That moonshine-sys does no checksum check is based on reading build.rs only.
