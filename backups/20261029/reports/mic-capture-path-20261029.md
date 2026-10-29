# Mic capture path for live STT (Moonshine Tiny Streaming / sherpa-onnx): research report
Date: 2026-10-29 (Europe/Zagreb). Research only: nothing installed, cloned or changed.
Scope: microphone input only. TTS is out of scope.
Labels: **[V]** = I checked it in source or docs this session. **[U]** = unverified, inferred or estimated.
Sample-rate detail is deferred by the owner, so §4 is short.

## TL;DR
- **Moonshine's own library is the best fit for "words appear while speaking".** Its C core has a true streaming API: `add_audio` → `transcribe_stream` → lines with `is_complete`, `is_updated` and `has_text_changed` flags. It also has `TINY_STREAMING` as a model arch, and the VAD and line endpointing are built in. [V]
- **sherpa-onnx runs Moonshine only as an offline (non-streaming) recognizer**, in v1 and v2. For near-live text it uses a loop where Silero VAD finds speech and the recognizer re-decodes the growing segment every 0.2 s ("simulated streaming"). It works, but every partial re-encodes the whole utterance so far. [V]
- **Rust:** the official `sherpa-onnx` crate (2.0.1, Apache-2.0) has cpal mic examples. The third-party `moonshine-rs` (0.2.6, MIT/Apache-2.0, ghchinoy) wraps libmoonshine and has a cpal live demo. Both crates download prebuilt native libs at build time. [V]
- **Remote mic:** browser getUserMedia + AudioWorklet → f32 (or i16) PCM frames over a WebSocket → an axum/tokio-tungstenite handler that feeds the stream. Partial text goes back as JSON. **This needs a small piece of browser JS**, because there is no JS-free way to capture a browser mic. It also needs HTTPS (or localhost). [V]

## 1. How sherpa-onnx captures the mic in its examples
| Example family | Capture lib | Loop shape | Source |
|---|---|---|---|
| Python `speech-recognition-from-microphone-with-endpoint-detection.py` | `sounddevice` (wraps PortAudio, `pip install sounddevice`) | `sd.InputStream(channels=1, dtype="float32", samplerate=48000)`, blocking `s.read(0.1 s)` → `stream.accept_waveform(48000, samples)` (resamples internally) → decode → `is_endpoint` | [V] [src](https://github.com/k2-fsa/sherpa-onnx/blob/master/python-api-examples/speech-recognition-from-microphone-with-endpoint-detection.py) |
| Python `*-alsa.py` | `sherpa_onnx.Alsa(device)` (built-in ALSA binding, Linux only, needs libasound) | blocking `alsa.read(1600)`, 100 ms at 16 kHz | [V] [src](https://github.com/k2-fsa/sherpa-onnx/blob/master/python-api-examples/speech-recognition-from-microphone-with-endpoint-detection-alsa.py) |
| Python `vad-microphone.py`, `simulate-streaming-sense-voice-microphone.py` | sounddevice | capture **thread** → `queue.Queue` → main thread feeds VAD in 512-sample windows. Resamples with librosa if the mic rate is not 16 kHz | [V] [src](https://github.com/k2-fsa/sherpa-onnx/blob/master/python-api-examples/simulate-streaming-sense-voice-microphone.py) |
| C++ binaries `sherpa-onnx-microphone`, `-microphone-offline`, `-vad-microphone-offline-asr`, `-vad-microphone-simulated-streaming-asr` | **PortAudio** (CMake fetches pa_stable_v190700) | PortAudio callback calls `stream->AcceptWaveform` directly. The main loop decodes and checks `IsEndpoint` | [V] [microphone.cc](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/microphone.cc), [cmake/portaudio.cmake](https://github.com/k2-fsa/sherpa-onnx/blob/master/cmake/portaudio.cmake) |
| C++ `sherpa-onnx-alsa*`, `c-api-examples/asr-microphone-example/c-api-alsa.cc`, `cxx-api-examples/*-alsa-cxx-api.cc` | **ALSA** (`snd_pcm_open`, S16_LE, `set_rate_near`, creates a resampler if the device rate differs) | blocking read loop | [V] [alsa.cc](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/alsa.cc) |
| **Rust, official crate** `rust-api-examples/examples/*_microphone.rs` | **cpal 0.16** (feature `mic`) | cpal callback downmixes to mono (F32/I16/U16) → `std::sync::mpsc::Sender<Vec<f32>>` → main loop: `LinearResampler` (sherpa's own) to 16 kHz → VAD in 512-sample windows → interim decode every 0.2 s → final decode per VAD segment | [V] [Cargo.toml](https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/Cargo.toml), [sense_voice_simulate_streaming_microphone.rs](https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/sense_voice_simulate_streaming_microphone.rs) |
| Rust Moonshine example `moonshine_v2.rs` | none (wav file only). **No Moonshine mic example in Rust**, but swapping the model config into the sense_voice mic example should work | | [V] (file); [U] (swap) |
| sherpa-rs (thewh1teagle, MIT, 0.6.8, last update 2025-10) | no mic examples. It has `moonshine.rs` and `silero_vad.rs` wrappers | | [V] [repo](https://github.com/thewh1teagle/sherpa-rs) |
| WebSocket client `online-websocket-client-microphone.py` | sounddevice callback (16 kHz, f32, blocksize 0.05 s = 800 samples) → asyncio queue → `websockets` lib sends raw f32 bytes | | [V] [src](https://github.com/k2-fsa/sherpa-onnx/blob/master/python-api-examples/online-websocket-client-microphone.py) |
| WebSocket server `streaming_server.py` (Python `websockets`) and C++ `sherpa-onnx-online-websocket-server` (websocketpp + asio) | n/a (receives audio) | binary frame = raw float32 samples at the server's rate. Text frame `"Done"` = end. Replies JSON `{"text","segment"}`, calls `reset` on endpoint. C++ sends `"Done!"`. **Online (streaming) models only, so no Moonshine** | [V] [streaming_server.py](https://github.com/k2-fsa/sherpa-onnx/blob/master/python-api-examples/streaming_server.py), [online-websocket-server-impl.cc](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/online-websocket-server-impl.cc) |
| Browser page for that server `python-api-examples/web/js/streaming_record.js` | `getUserMedia({audio:true})` + **ScriptProcessorNode** (deprecated, buffer 2048) → JS linear downsample to 16 kHz → `socket.send(Float32Array)` over wss:// | ships `generate-certificate.py` and `start-https-server.py` because the mic needs HTTPS | [V] [src](https://github.com/k2-fsa/sherpa-onnx/blob/master/python-api-examples/web/js/streaming_record.js) |
| WASM in-browser demos `wasm/asr`, `wasm/vad-asr` | getUserMedia + ScriptProcessor (4096), `AudioContext({sampleRate:16000})`. Recognition runs fully in the browser via Emscripten | no Moonshine WASM build seen for streaming. vad-asr is VAD + offline | [V] [app-asr.js](https://github.com/k2-fsa/sherpa-onnx/blob/master/wasm/asr/app-asr.js) |

## 2. Does sherpa-onnx support Moonshine streaming?
**No. It is offline only.** [V]
- The sherpa-onnx tree has only `offline-moonshine-*` and `offline-recognizer-moonshine(-v2)-impl`. There is no `online-moonshine`.
- The docs label the Moonshine mic mode "Real-time/Streaming … with VAD". In practice that is `sherpa-onnx-vad-microphone-simulated-streaming-asr`, i.e. VAD plus the offline recognizer. [docs](https://k2-fsa.github.io/sherpa/onnx/moonshine/models-v2.html)
- sherpa's "v2" Moonshine packs (for example `sherpa-onnx-moonshine-tiny-en-quantized-2026-02-27`, 43 MB, encoder plus merged decoder `.ort`) are re-packs of Moonshine's newer models ([scripts/moonshine/v2](https://github.com/k2-fsa/sherpa-onnx/tree/master/scripts/moonshine/v2) references moonshine PR #73). **[U]**: I did not confirm whether these are the exact `TINY_STREAMING` weights. Either way, sherpa runs them in offline mode: the whole segment goes through the encoder each call.

**How the simulated-streaming loop works** (Rust/Python examples above; defaults [V]):
1. Mic → resample to 16 kHz → Silero VAD in 512-sample windows (32 ms), with `threshold 0.5`, `min_silence_duration 0.1 s`, `min_speech_duration 0.25 s` and `max_speech_duration 8 s` (on the Python side the threshold is raised to 0.9 near the cap).
2. Until speech starts, keep only the last 10 windows (~320 ms) as pre-roll.
3. While speech is ongoing, every **0.2 s** create a fresh offline stream on the whole buffer, decode it, and show the result as the partial.
4. When VAD emits a completed segment (after 0.1 s of trailing silence), decode it once more as the final and clear the buffer.

**Latency cost** [U, estimate]:
- First partial ≈ 0.25 s (min speech) + 0.2 s + decode time.
- Each partial costs roughly O(utterance length), so the total compute is quadratic per utterance. The 8 s cap bounds it.
- Data point [V]: the sherpa docs report tiny-en v2 on a 3.8 s clip at RTF 0.056 with 2 threads, i.e. ~0.21 s. The CPU is not stated, so a small VPS is likely slower. That puts an 8 s segment at ~0.45 s per re-decode, on a 0.2 s cadence. This can saturate 2 vCPUs with one user, so expect to need a 0.4 to 0.5 s cadence.
- Final text arrives ~0.1 s of silence + decode time after the user stops.

## 3. Moonshine's own library (moonshine-ai/moonshine, MIT)
- **Python `MicTranscriber`** [V] ([mic_transcriber.py](https://github.com/moonshine-ai/moonshine/blob/main/language-bindings/python/src/moonshine_voice/mic_transcriber.py), [example](https://github.com/moonshine-ai/moonshine/blob/main/examples/python/mic_transcription.py)):
  - Capture: `sounddevice`/PortAudio `InputStream(samplerate=16000, channels=1, blocksize=1024, dtype=float32)`. If the device rejects 16 kHz it falls back to the device's default rate, and the C core resamples.
  - The callback only does `queue.put((audio, rate))`. A worker thread drains and coalesces the queue, then calls `stream.add_audio`. This is a deliberate fix so inference never blocks the PortAudio callback (moonshine issue #196).
  - Callbacks: `on_text` for partials and `on_line` for completed lines. Default `update_interval` is 0.5 s.
- **C API** [V] ([moonshine-c-api.h](https://github.com/moonshine-ai/moonshine/blob/main/core/moonshine-c-api.h)):
  - Calls: `moonshine_create_stream` → `moonshine_start_stream` → `moonshine_transcribe_add_audio_to_stream(f32 [-1,1], len, sample_rate)` → `moonshine_transcribe_stream(flags)` → `moonshine_stop_stream` → final `transcribe_stream`.
  - `add_audio` only buffers. The header says it is "safe to call frequently even from time-critical threads", and the chunk size doesn't matter.
  - `transcribe_stream` is the heavy call, so run it on another thread. It skips full analysis unless >200 ms of new audio has arrived; `MOONSHINE_FLAG_FORCE_UPDATE` overrides that.
  - Output is lines. Lines are only ever appended, and only the last one can be incomplete. Each line carries `is_complete`, `is_updated`, `is_new` and `has_text_changed`, plus a stable `id` you can use for minimal UI diffs.
  - Options: `decode_incomplete_lines` (default true; this is what gives live partials) and `use_speculative_decoding` (default true).
  - Archs: `TINY_STREAMING=2`, `SMALL_STREAMING=4`, `MEDIUM_STREAMING=5`.
  - Internal defaults ([transcriber.h](https://github.com/moonshine-ai/moonshine/blob/main/core/transcriber.h)): `transcription_interval 0.5`, `vad_threshold 0.5`, `vad_window_duration 0.5 s`, `vad_hop_size 512`, look-behind 8192 samples, `vad_max_segment_duration 15 s`.
- **WASM `MicTranscriber`** [V] ([mic-transcriber.ts](https://github.com/moonshine-ai/moonshine/blob/main/language-bindings/wasm/src/mic-transcriber.ts)):
  - Pipeline: getUserMedia → **AudioWorklet** (falls back to ScriptProcessor 4096) → downmix to mono → linear `resampleTo16k` → `addAudio` + `transcribe` on each chunk.
  - Constraints default to `audio:true` and can be set with `.audioConstraints()`.
  - It explicitly calls `audioContext.resume()`, because a context created without a user gesture stays suspended silently.
  - This runs the model in the browser. That is an alternative architecture with no VPS compute, but the model downloads to the client. [U: performance on phones]
- **moonshine-rs** (third-party, [ghchinoy/moonshine-rs](https://github.com/ghchinoy/moonshine-rs), MIT OR Apache-2.0, 0.2.6, ~167 downloads, created 2026-07) [V]:
  - Safe wrapper over libmoonshine (`moonshine-sys` uses bindgen/cc/cmake and downloads prebuilt libmoonshine plus a statically linked ONNX Runtime from moonshine GitHub releases).
  - API: `Transcriber::create_stream` → `add_audio(&[f32], rate)` → `poll(force)` → `finalize()`. `OwnedTranscriberStream` exists for long-lived streams.
  - The `audio` feature pulls in symphonia and rubato. The `demo/stream-cli` uses **cpal 0.18** with `ModelArch::TinyStreaming`.
  - **Gotcha in the demo:** the cpal callback does `Mutex::lock()` on the same stream that the poller thread holds during `poll()`, which is inference. The audio callback therefore blocks during inference and can cause xruns or dropped audio. Use a lock-free SPSC ring or a channel instead (the same lesson as the Python #196 fix).
  - Maturity is low and it is a single maintainer. **[U]** whether the prebuilt Linux x86_64 lib download works on our VPS.

## 4. Audio format (kept brief, owner deferred this)
- Both engines want **16 kHz mono float32 in [-1, 1]** internally. [V]
- Both resample internally if you pass a different rate: sherpa `accept_waveform(rate, …)` and Moonshine `add_audio(…, rate)`. [V]
- The 512-sample VAD window is 32 ms at 16 kHz. [V]
- Capture chunks in the examples: 50 to 100 ms (sherpa) and 1024 frames (Moonshine Python). Moonshine says the chunk size doesn't matter. [V]
- Browsers usually run at 44.1 or 48 kHz. `AudioContext({sampleRate:16000})` is supported in Chrome and Firefox (sherpa's wasm demo uses it) [V use; U cross-browser]. Otherwise downsample in the worklet or on the server.
- int16 halves the WebSocket bandwidth: 32 kB/s versus 64 kB/s for f32. Convert to f32 with `/32768.0` before feeding.
- Resamplers: `rubato` for good quality, or the built-in engine resamplers (sherpa `LinearResampler`, Moonshine internal), which are fine for ASR. [U: quality difference is negligible for ASR]

## 5. The audio loop and threading (recommended shape)
```
[capture thread / cpal callback | WS recv task]
   │  downmix→mono, (optional) resample; NO inference, NO blocking locks
   ▼  SPSC ring (rtrb/ringbuf) or bounded channel (crossbeam / tokio mpsc)
[ASR worker thread (one per session; or a pool)]
   loop: drain all queued chunks (coalesce) → engine.add_audio(...)
         every 200–500 ms: transcribe/poll → diff lines (id, is_updated, is_complete)
         → send partial {id,text,final:false}; on is_complete → {final:true}
   ▼  tokio mpsc → WS send task → browser updates text
```
- Inference is blocking CPU work. Keep it off the tokio reactor by using `spawn_blocking` or a dedicated `std::thread`.
- Bound the queue. If the worker falls behind, coalesce the backlog (as Moonshine's Python does) rather than letting latency grow.
- **Endpointing:**
  - Moonshine: VAD-driven. A line becomes `is_complete` after a pause, with a forced split at 15 s. [V]
  - sherpa online models: rule1 = 2.4 s silence with nothing decoded, rule2 = 1.2 s silence after text, rule3 = 20 s maximum utterance ([endpoint.h](https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/endpoint.h)). These don't apply to Moonshine.
  - sherpa VAD loop: VAD segment end at `min_silence_duration` (0.1 s in the example; 0.25 to 0.5 s is saner for chat). [V values; U recommendation]
  - Also send an explicit "stop" (as the sherpa `"Done"` text frame does) so the stream flushes. For Moonshine, that maps to `stop_stream` + final `transcribe_stream`.

## 6. Rust wiring
| Crate | Version (crates.io, 2026-10-29) | License | Pure Rust? |
|---|---|---|---|
| cpal | 0.18.2 | Apache-2.0 | No on Linux: **needs ALSA dev files (`libasound2-dev`) even when using the PipeWire/Pulse/JACK features**. Those features need `libpipewire-0.3-dev`, `libpulse-dev` or `libjack-jackd2-dev` respectively ([README](https://github.com/RustAudio/cpal)) [V] |
| rubato | 5.0.1 | MIT OR Apache-2.0 | Pure Rust [V license; U "pure" assumed from crate nature] |
| audioadapter (rubato 5 buffer API) | 5.0.0 | MIT OR Apache-2.0 | Pure Rust [U] |
| tokio-tungstenite / tungstenite | 0.30.0 | MIT / MIT OR Apache-2.0 | Pure Rust (TLS via rustls or native-tls is optional) |
| axum (built-in `ws` feature, uses tungstenite) | 0.8.9 | MIT | Pure Rust |
| rtrb / ringbuf | 0.4.1 / 0.5.3 | MIT OR Apache-2.0 | Pure Rust, lock-free SPSC (good for audio callbacks) |
| crossbeam-channel | 0.5.17 | MIT OR Apache-2.0 | Pure Rust |
| alsa / alsa-sys (direct ALSA) | 0.12.1 / 0.6.1 | Apache-2.0/MIT, MIT | Needs libasound |
| portaudio (Rust) | 0.8.0 (stale, 2024) | MIT | Needs the PortAudio C lib, so not recommended |
| sherpa-onnx (official) | 2.0.1 | Apache-2.0 | FFI to C++; build.rs **downloads prebuilt static libs** from GitHub releases (or set `SHERPA_ONNX_LIB_DIR`) [V] |
| sherpa-rs | 0.6.8 | MIT | FFI; older |
| moonshine-rs / moonshine-sys | 0.2.6 | MIT OR Apache-2.0 | FFI to libmoonshine plus static ORT (downloads prebuilt) |

- **Local mic (VPS):** a VPS normally has **no sound card**, so cpal or ALSA there is only useful for dev or testing on a laptop. Production audio will come from the browser. [U, but near-certain]
- **Remote mic:**
  - Browser: `getUserMedia` → `AudioWorkletNode` (this needs JS: a ~40-line page script plus a ~15-line worklet module) → post 20 to 100 ms chunks → `ws.send(Int16Array|Float32Array)`.
  - Server: an axum `WebSocketUpgrade` handler. Binary frame → push to the ASR worker. Text frames carry control (`start`, `stop`, `sample_rate`). The server sends back JSON partials.
  - **The no-JS ruling:** live mic capture needs some JS. The minimum is one static `mic.js` plus `worklet.js` served by the Rust server, and the rest of the console can stay server-rendered. If the partial text must avoid JS DOM updates, the server could stream HTML fragments via SSE instead (htmx or similar is still JS). Practically, the same small script just writes into a `<div>`. [U, design judgement]
  - Alternative: run Moonshine WASM in the browser (moonshine npm/wasm package) and only post the final text to Hermes, which puts no audio load on the VPS. The trade-offs are model download and client CPU. [U]

## 7. Gotchas
- **Secure context:** `getUserMedia` and AudioWorklet only work over **HTTPS or http://localhost**. Over plain http on a LAN IP, `navigator.mediaDevices` is undefined. Use wss:// on the same origin. sherpa ships a self-signed cert helper for this reason. Phones reject self-signed certs awkwardly, so use a real cert (Let's Encrypt or a Tailscale/Cloudflare tunnel). [V for sherpa; standard web platform behavior]
- **User gesture:** create or resume the AudioContext inside the click handler, otherwise it stays `suspended` and delivers no samples, with no error (Moonshine wasm comment). [V]
- **ScriptProcessorNode is deprecated:** use AudioWorklet, as Moonshine does. The sherpa web demo still uses ScriptProcessor. [V]
- **Constraints:** `echoCancellation`, `noiseSuppression` and `autoGainControl` default to **true** in Chrome [U: browser-dependent]. For a chatbox:
  - Keep echoCancellation on if the page also plays audio (TTS later), otherwise the model will transcribe its own playback.
  - Consider `noiseSuppression:false` and `autoGainControl:false` if accuracy suffers. AGC pumping and NS artifacts can hurt ASR and confuse VAD.
  - Request `channelCount:1`. A browser-requested `sampleRate:16000` constraint is often ignored. [U]
- **Never block the audio callback:** no inference, no contended Mutex, no allocation-heavy work (see the moonshine #196 fix and the moonshine-rs demo flaw).
- **Latency budget** (rough [U]): capture chunk 20 to 100 ms + WS RTT 20 to 150 ms + queue + update cadence 200 to 500 ms + decode 50 to 400 ms on VPS CPU. Expect partials ~0.4 to 1.0 s behind speech, and finals ~0.3 to 0.8 s after the user stops. Set ORT threads (`num_threads` 1 to 2) per session, and cap concurrent sessions.
- **Endpoint tuning:** too short a silence splits sentences, and too long delays the final. The 0.1 s default in the sherpa VAD example is aggressive. Moonshine forces a split at 15 s and sherpa's VAD example at 8 s.
- **Reconnects and discontinuities:** on mute or reconnect, call Moonshine `start_stream`/`restart` (or reset the sherpa stream) so stale audio isn't stitched together. [V API exists]
- **Mobile Safari:** may run the AudioContext at 48 kHz regardless. Resample defensively and treat the reported `sampleRate` as truth. [U]

## Open items / unverified
- Whether sherpa "v2" Moonshine packs equal Moonshine `TINY_STREAMING` weights.
- Real decode latency of Moonshine Tiny Streaming on our VPS CPU (needs a benchmark, which is not allowed in this pass).
- Whether moonshine-rs prebuilt-lib download and the Linux x86_64 build work cleanly.
- Browser default constraint values and 16 kHz AudioContext support on iOS Safari.
