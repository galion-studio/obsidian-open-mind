# Persona Plex v2: Galion voice-to-voice channel (design)

| Field | Value |
|---|---|
| Version | v2 design, draft 1 |
| Written | 2026-10-18 20:25 CEST (+02:00), Europe/Zagreb |
| Status | **Proposed.** Nothing is deployed or wired in. No GPU provisioned. No service changed. |
| Builds on | `personas/persona-plex-v1.md` (role prompt, voice prompt, rules) |
| Code | `crates/persona-plex-relay/` (Rust scaffold: protocol types, config, speaker-verification logic; opens no sockets) |
| Project | goals.md, "Project: Galleon Voice to Voice" |

Labels in this document: **[source]** = checked against the cited page on 2026-10-18; **[inferred]** = reasoning, not verified; **[to measure]** = a number we must measure ourselves before trusting it.

---

## 1. What we are building

A voice-to-voice interface for Galion: the person speaks, Persona Plex answers aloud, **at the same time if needed** (barge-in, back-channel, interruptions). The voice also acts as a **soft identity signal**: the relay continuously checks that the voice on the line matches the enrolled speaker.

"Two-bandwidth" maps to two bands in one channel:

| Band | What flows | Direction | Rate |
|---|---|---|---|
| **Audio band** | Two concurrent Opus streams: user to model and model to user. Full duplex: both flow at the same time, no turn-taking lock. | both, at once | tens of kbit/s per direction on the wire (Opus); about 1.1 kbit/s per stream inside the model (Mimi tokens) |
| **Control band** | The model's text stream (inner monologue), control (start / end turn / pause / restart), metadata, errors, ping, plus relay-private status (speaker verification, fallback, bye). | both | low: at most one text token per 80 ms model frame [inferred], well under 1 kbit/s [to measure] |

This matches how the model works: Moshi "models two streams of audio: one corresponds to Moshi speaking, and the other one to the user speaking", and predicts text tokens for its own speech alongside them [source: Moshi README]. PersonaPlex "runs in a dual-stream configuration in which listening and speaking occur concurrently" [source: PersonaPlex model card].

## 2. Architecture

```
 Browser / phone                      VPS (<vps-ip>)                       GPU host (RunPod, not provisioned)
 ----------------                     --------------------                      ----------------------------------
 mic -> AEC -> Opus/Ogg  ==WSS==>  persona-plex-relay (Rust)       ==WSS / SSH tunnel==>  PersonaPlex moshi server
 speaker <- Opus/Ogg     <=======  - bearer-token auth                    <==========     (python -m moshi.server,
 UI: text + speaker status          - session state machine                                route /api/chat, port 8998)
                                    - forwards ONLY audio (kind 0x01)                       Mimi encode -> 7B Temporal +
                                      upstream; sets persona itself                         Depth Transformer -> Mimi decode
                                    - speaker verification on the                           24 kHz in, 24 kHz out
                                      user stream (CPU, ONNX) [planned]
                                    - action gate (voice = soft signal)
                                    - fallback -> Galion One text mode
                                      (127.0.0.1:8091) when GPU is off
```

### 2.1 Client

- Browser page with `getUserMedia`, the browser's echo cancellation on (the Moshi team recommends their web UI because it "provides additional echo cancellation that helps the overall model quality" [source: Moshi README]).
- Audio encoded as Opus in Ogg pages, sent as WebSocket binary messages with kind byte `0x01`, which is exactly what the moshi web client does [source: `kyutai-labs/moshi` `client/src/protocol/encoder.ts`]. Starting point: reuse the moshi web client (MIT) pointed at the relay instead of the GPU.
- WebSocket rather than WebRTC for v2: the PersonaPlex server speaks WebSocket only, so a WebRTC leg would need a media server in the middle and more CPU on the VPS [inferred]. WebRTC stays an option for v3 if packet loss on mobile becomes the problem.
- Microphone access needs a secure context (HTTPS or `localhost`). The VPS has no public HTTPS listener today (only SSH on `*:22`; everything else is loopback, checked with `ss -ltn` on 2026-10-18). So v2 access is through an SSH tunnel to `localhost` until a TLS front end is approved. **Decision for Slavik.**

### 2.2 VPS relay (`persona-plex-relay`, Rust, no inference)

- **Auth:** bearer token from an env var (the config holds only the variable *name*). The voice is never what opens the channel.
- **Session:** one session at a time (`max_sessions = 1`). The PersonaPlex server holds one global `asyncio.Lock` around a conversation, so one GPU process serves one conversation [source: `NVIDIA/personaplex` `moshi/moshi/server.py`].
- **Persona injection:** the relay, not the client, sets the persona. PersonaPlex reads `text_prompt` and `voice_prompt` from the WebSocket URL query [source: server.py]. The relay builds that URL from config (`upstream_url()` in `protocol.rs`) and never forwards client-supplied prompts. That also matters for safety, because the server joins `voice_prompt` onto its voice directory as a file path [source: server.py].
- **Filtering:** client to GPU, only kind `0x01` (audio) is forwarded. The server ignores anything else anyway [source: server.py `recv_loop`], and relay-private kinds `0x40..0x42` never leave the VPS (`forwardable_upstream()`, unit-tested).
- **Speaker verification:** runs on a copy of the user stream (decode Opus, resample 24 kHz to 16 kHz, embed, score). See section 6.
- **No inference** of the conversation model on the VPS, ever.

### 2.3 GPU host

- `NVIDIA/personaplex`: `pip install moshi/.`, accept the model licence on Hugging Face, `python -m moshi.server --ssl <dir>`, web UI and API on port 8998, route `/api/chat` [source: PersonaPlex README, server.py]. This is NVIDIA's Python server on the rented GPU host. **No Python is written or run by us on the VPS**; the stack's own code stays Rust.
- Weights: `nvidia/personaplex-7b-v1`, BF16, about 8B parameters by Hugging Face's count [source: model card], under the NVIDIA Open Model License. They are fetched on the GPU host only and never bundled.
- VPS to GPU link: either WSS to the pod's public endpoint, or (preferred, [inferred]) an SSH tunnel from the VPS to the pod with `ws://127.0.0.1:<port>`. The relay config refuses plain `ws://` to anything but loopback (`validate()`, unit-tested). Whether RunPod's HTTP proxy keeps long WebSocket sessions open is **[to verify]**.

### 2.4 Wire protocol (moshi-compatible)

First byte of every binary message [source: moshi `encoder.ts`, PersonaPlex server.py]:

| Kind | Name | Payload | Band |
|---|---|---|---|
| `0x00` | handshake | server sends a bare `0x00` when ready | control |
| `0x01` | audio | Opus/Ogg bytes | audio |
| `0x02` | text | UTF-8 (model text tokens) | control |
| `0x03` | control | 1 byte: start 0, endTurn 1, pause 2, restart 3 | control |
| `0x04` | metadata | JSON | control |
| `0x05` | error | UTF-8 | control |
| `0x06` | ping | none | control |
| `0x07` | colored text | color byte + UTF-8 | control |
| `0x40` | relay: speaker status | has-score, i16 score x1000, decision | control (relay to client only) |
| `0x41` | relay: fallback | reason byte | control (relay to client only) |
| `0x42` | relay: bye | UTF-8 reason | control (relay to client only) |

All of these are implemented as `WireMessage` in `crates/persona-plex-relay/src/protocol.rs` with round-trip tests.

## 3. Latency budget

| Stage | Budget | Basis |
|---|---|---|
| Mic capture + Opus encode (browser) | one Opus frame (20 ms typical) + buffering | [to measure] |
| Client to VPS (Croatia to VPS) | half RTT | [to measure] (`ping` from Slavik's network) |
| Relay processing | target < 5 ms per message (copy + filter, no inference) | [inferred], [to measure] |
| VPS to GPU host | half RTT; pick an EU RunPod region | [to measure] |
| Model | theoretical 160 ms (80 ms Mimi frame + 80 ms acoustic delay); "practical overall latency as low as 200 ms on an L4 GPU" for Moshi | [source: Moshi README] |
| PersonaPlex measured | smooth turn-taking latency 0.170 s; user-interruption latency 0.240 s (FullDuplexBench) | [source: PersonaPlex model card] |
| Return path | mirror of the above | [to measure] |

Goal: conversational turn latency **under about 500 ms mouth-to-ear** [inferred target]. For comparison, the Moshi README warns that its Gradio tunnel adds "latency (up to 500ms from Europe)" [source], which is one reason to relay through our own VPS rather than a third-party tunnel. Speaker verification runs **beside** the audio path, never in it: it must not add latency to the conversation.

## 4. Bandwidth

| Link | Figure | Basis |
|---|---|---|
| Inside the model, per stream | Mimi: 24 kHz audio to a 12.5 Hz representation, **1.1 kbit/s**, 80 ms latency | [source: Moshi README] |
| Wire, per direction (Opus) | RFC 6716 lists "16-20 kbit/s" for wideband speech and "28-40 kbit/s" for fullband speech as sweet spots | [source: RFC 6716 §2.1.1]; the actual bitrate of the moshi client and server encoders is [to measure] |
| Full duplex total | 2 x the Opus figure, both at once | [inferred] |
| Control band | well under 1 kbit/s | [inferred], [to measure] |
| Per hour of talk (VPS egress/ingress) | 2 legs (client and GPU) x 2 directions x Opus bitrate | [to measure] |

For context, the Moshi README compares Mimi with SpeechTokenizer (50 Hz, 4 kbps) and SemantiCodec (50 Hz, 1.3 kbps) [source]. A future option is to send Mimi tokens instead of Opus over the wire (the Mimi codec has a Rust implementation in the moshi repo [source: Moshi README]). That would mean running Mimi on the client and changing the server protocol, so it is **out of scope for v2**.

## 5. Persona

- **Role prompt:** the v1 text block from `personas/persona-plex-v1.md` ("## Role prompt (text)"). The relay extracts it at session start (`extract_role_prompt()`; tested on the real file: 645 characters, 7 lines) and passes it as `text_prompt`. The server wraps it in system tags [source: server.py].
- **Voice prompt:** default `NATF2.pt` (one of 18 packaged voices: NATF0-3, NATM0-3, VARF0-4, VARM0-4 [source: PersonaPlex README]) until Samantha's `samantha.wav` is tested as a custom voice prompt. The server loads non-`.pt` files through `load_voice_prompt` [source: server.py]; whether this specific clip works is **[to verify]**, as v1 already notes.
- **One persona per session.** The prompt is fixed when the WebSocket opens; no switching mid-call.
- **Where "the mind" is:** during a voice session the answers come from PersonaPlex's own language model (fine-tuned Moshi on the Helium LLM [source: PersonaPlex README]), not from Hermes. It has no tools, no files, no live numbers. That fits the v1 rules ("If you were not given a live number, say you do not have it"; "You do not write files, run commands, move money") [inferred]. Bridging Hermes knowledge into a live call is a v3 question.
- **Conflict to flag:** v1 says "Do not say you are a model or a system." The EU AI Act Art. 50(1) requires that people interacting with an AI system are told so unless it is obvious from context, and Art. 50(2) covers marking synthetic audio [source: Regulation (EU) 2024/1689]. Personal, non-professional use is outside the Act's deployer duties (Art. 2(10)), but anyone else who uses the channel (family, guests, a public demo) changes that. The current application dates and any "digital omnibus" changes are **[to verify]**. **Decision for Slavik:** keep the v1 rule for private use, but add a disclosure line in the UI for anyone else.

## 6. Speaker identity: the voice as an identity key

### 6.1 Stance (read this first)

- **The voice is a SECOND factor and a soft identity signal, never the only key** for anything privileged.
- This stack literally clones voices: Chatterbox makes `samantha` from a reference clip, and PersonaPlex itself takes an arbitrary audio voice prompt. Anyone with a few seconds of Slavik's voice can make a convincing copy. A voice match alone proves little.
- NIST SP 800-63B allows biometrics only as part of multi-factor authentication bound to a physical authenticator, never as a standalone authenticator [source: NIST SP 800-63B, biometric requirements]. We follow the same rule.
- This is enforced in code, `speaker::authorize()`, unit-tested:

| Action class | Needs |
|---|---|
| Conversation | nothing (the bearer token already opened the channel) |
| Personal (harmless personalisation, e.g. "read my notes") | voice `Verified` |
| Privileged (admin, secrets, config, running anything) | voice `Verified` **and** a second factor (e.g. TOTP or a confirmation on Slavik's phone) |
| Money (Revolut, payments, any real money) | **always denied.** Revolut / real money stays NOT approved. |

### 6.2 Models (embeddings, ONNX on the VPS CPU)

| Model | Published result | Runs on CPU via ONNX | Notes |
|---|---|---|---|
| SpeechBrain ECAPA-TDNN `speechbrain/spkrec-ecapa-voxceleb` | EER 0.80 % on VoxCeleb1-test (cleaned) [source: model card] | ONNX export needed [to verify] | 16 kHz mono input; cosine scoring [source] |
| WeSpeaker ResNet34-LM `Wespeaker/wespeaker-voxceleb-resnet34-LM` | vox1-O-clean EER 0.797 % (LM), 0.723 % with AS-Norm [source: WeSpeaker VoxCeleb v2 results] | yes: `wespeaker_en_voxceleb_resnet34_LM.onnx`, 25.3 MB [source: sherpa-onnx speaker-recognition-models release] | 6.63 M params, 4.55 GFLOPs [source]; 256-dim embedding (matches `embedding_dim = 256` in the example config) |
| WeSpeaker ECAPA (c512 / c1024), CAM++ | vox1-O-clean EER 0.707-1.069 % depending on variant [source: WeSpeaker results] | yes (ONNX listed) [source: WeSpeaker pretrained list] | CAM++ is lighter: 1.15 GFLOPs [source] |
| `microsoft/wavlm-base-plus-sv` (WavLM + x-vector head) | Model card's example uses threshold 0.86 and states "the optimal threshold is dataset-dependent" [source] | ONNX export needed [to verify] | Heavier (transformer). NVIDIA also measures PersonaPlex voice similarity with WavLM-TDNN embeddings (SSIM 0.650) [source: PersonaPlex model card] |
| NVIDIA TitaNet (small / large) | not checked here | yes: `nemo_en_titanet_small.onnx` 38.4 MB [source: sherpa-onnx release] | |

**Recommendation:** WeSpeaker ResNet34-LM ONNX (or CAM++ if CPU is tight) [inferred]. Rust runtime options: `sherpa-onnx`'s Rust API, which has a `SpeakerEmbeddingExtractor` with a `provider: "cpu"` example [source: sherpa-onnx `rust-api-examples/speaker_embedding_extractor.rs`]; or a pure-Rust ONNX runtime such as `tract` [to verify op coverage]. Both are new dependencies with real build cost, so they are **not added** to the scaffold. Each model has its own licence ("Each model has its own license" [source: sherpa-onnx release]), **[to verify]** before download. No model has been downloaded.

Published EERs come from VoxCeleb benchmark conditions, not from a browser microphone, Opus compression and 24 to 16 kHz resampling. Expect ours to differ. **[to measure]**.

### 6.3 Enrollment

- With explicit consent recorded first (section 7).
- At least 5 phrases (`enrollment_min_phrases`, minimum 3 enforced), spoken naturally on the device and mic actually used, ideally over two sittings [inferred].
- Each phrase is embedded and L2-normalised; the voiceprint is the normalised mean (centroid). Every phrase must agree with the centroid by at least `enrollment_min_consistency`, otherwise enrollment fails and names the bad phrase (wrong speaker, noise, or replay). Implemented as `speaker::enroll()`, unit-tested.
- The raw enrollment audio is deleted once the voiceprint is built. Only the centroid is kept [design rule].

### 6.4 Scoring and thresholds

- Score = cosine similarity of the live embedding against the centroid (`cosine_similarity()`: rejects empty input, length mismatch, zero vectors and NaN/Inf; unit-tested).
- **Two thresholds, three outcomes:** score >= `accept_threshold` means Accept; score < `reject_threshold` means Reject; anything between is Uncertain. A window with less than `min_speech_ms` of speech is InsufficientAudio and does not change the identity state.
- Config values `0.70 / 0.50` are **placeholders, not tuned**. Thresholds do not carry over between models (the WavLM card's example uses 0.86 and the sherpa-onnx example uses 0.6, each for its own model [source]). They must be set from our own measurements.

### 6.5 Continuous verification during the call

- Rolling window (`window_ms`, default 3000) over the **user** stream only. The model's own output is never scored.
- In full duplex the mic also picks up the model's voice. Verification only scores windows where the model is silent or after echo cancellation [inferred]; the model to user stream is the reference for that gating.
- State (`ContinuousVerifier`): Accept means `Verified`; Uncertain means `Unverified`; `max_consecutive_rejects` Rejects in a row means `Lost`, which is **sticky** for the session (re-authentication needed). Unit-tested.
- A failed verification never cuts the conversation. It only lowers what the action gate allows (section 6.1). The client UI shows the status (`0x40` message).

### 6.6 Anti-spoofing and liveness

Threats: replay of a recording; TTS clones (Chatterbox, PersonaPlex voice prompts, any commercial cloner); real-time voice conversion; the stack's own synthetic voice leaking back through the mic.

| Measure | What it stops | Limits |
|---|---|---|
| **Second factor for privileged actions** (6.1) | everything below, for what matters | the only strong control |
| Challenge-response: speak a random phrase shown in the UI, within a time window | replay of fixed recordings | needs user-side ASR (a Rust option, e.g. whisper.cpp bindings, is [to decide]); weak against live voice conversion |
| Countermeasure model (AASIST family: AASIST-L has about 85 K parameters [source: arXiv 2110.01200]) | some synthetic and replayed speech | ASVspoof 5 reports that its attacks "significantly compromise the baseline systems" (AASIST is a Track 1 baseline) [source: arXiv 2408.08739]. Treat it as weak. |
| Known-clone denylist: score the live embedding against the stack's own synthetic voices (`samantha.wav`, PersonaPlex NAT/VAR voices) and against any Chatterbox clone of Slavik made for testing | our own voices being replayed into the mic | [inferred]; cheap |
| Device and session binding: audio is only trusted on an authenticated session | random callers | not a liveness test |

`require_liveness = true` downgrades an Accept to Uncertain unless liveness Passed, and a Failed liveness check forces Reject (`decide()` / `verify()`, unit-tested: "a perfect clone that fails liveness is rejected").

### 6.7 What "very sensitive" means in practice

- Two error types: **false accept** (an impostor or clone passes; FAR / FMR) and **false reject** (Slavik is refused; FRR / FNMR). Raising the accept threshold lowers false accepts and raises false rejects. The EER is the point where both are equal. It is a way to compare models, not an operating point.
- "Very sensitive" = run at a **low false-accept** operating point, accepting more false rejects. Because voice only ever gates "Personal" actions and is never sufficient for privileged ones, a false reject costs a retry or a second factor. That makes the trade-off cheap.
- **Targets are to be measured, not invented.** Procedure:
  1. Genuine trials: Slavik on at least two days, two mics (phone, laptop), quiet and noisy rooms, through the real Opus path.
  2. Impostor trials: other speakers (with their consent) **and** clone attacks: Chatterbox and PersonaPlex clones of Slavik made from his own enrollment clips, with his consent.
  3. Plot the DET curve; report EER for comparison with published figures (0.72-0.80 % on VoxCeleb1-O for the models above [source]).
  4. Choose `accept_threshold` at the false-accept rate Slavik picks (for example "at most 1 in N impostor trials"; N is Slavik's call), report the matching FRR, and set `reject_threshold` below it to form the uncertain band.
  5. Report clone-attack acceptance separately. If clones pass at the chosen threshold (likely [inferred]), that proves the voice must stay a soft signal.

## 7. Privacy, consent, GDPR (Slavik is in Croatia)

- Speaker embeddings used to recognise a person are **biometric data** (GDPR Art. 4(14)), a **special category** under Art. 9(1) when processed "for the purpose of uniquely identifying a natural person". Processing needs an Art. 9(2) condition; here that is **explicit consent** (Art. 9(2)(a)) [source: Regulation (EU) 2016/679].
- Purely personal or household use falls outside GDPR (Art. 2(2)(c)). The moment anyone other than Slavik is enrolled, or the stack is offered to others, GDPR applies in full, likely including a DPIA (Art. 35) [source: GDPR; applicability is a legal judgement, **to confirm**].
- Croatia: supervisory authority AZOP (Agencija za zaštitu osobnih podataka). The Croatian implementing act (Zakon o provedbi Opće uredbe o zaštiti podataka, NN 42/2018) has specific provisions on biometric data; how they apply here is **[to verify with a lawyer]**.
- EU AI Act: "biometric verification" whose sole purpose is to confirm a person is who they claim to be is excluded from the high-risk biometric category in Annex III point 1(a) [source: Regulation (EU) 2024/1689; **to verify** against the current consolidated text]. Transparency duties: section 5.
- See also EDPB Guidelines 02/2021 on virtual voice assistants [source: EDPB].

**Storage rules (design):**

1. Voiceprints live **only on the VPS** (`[speaker] store_dir`), never on the GPU host, never in git, never in the MIT release, never in logs.
2. Encrypted at rest (authenticated encryption such as XChaCha20-Poly1305 or AES-256-GCM; the RustCrypto crate is to be added later). The key comes from an env var named in config (`store_key_env`); its value is never printed (the plan output shows only "set" or "NOT set").
3. File mode 0600, root-only directory.
4. Consent record per enrolled person: who, when (Europe/Zagreb with offset), what for, which model and version.
5. **Deletion:** a single command deletes the voiceprint and consent record. Withdrawing consent means deletion (Art. 7(3), Art. 17). Raw enrollment audio is never kept.
6. Retention: voiceprint kept until deleted or re-enrolled; review yearly [inferred default, **Slavik decides**].
7. The GPU host receives the live conversation audio (personal data, not biometric templates). RunPod is a processor for that audio: prefer **Secure Cloud** over Community Cloud (third-party hosts) for real conversations, and check RunPod's DPA [inferred, **to verify**].

## 8. Fallback when the GPU is off

- `upstream.enabled = false` (the shipped default) or upstream unreachable or busy: the relay sends `0x41 RelayFallback(reason)` and the UI switches to **Galion One text mode** (`[fallback] galion_one_url`, today `127.0.0.1:8091`).
- Galion One's ear (faster-whisper) and mouth (Chatterbox) are down right now, so the fallback is **text only**. No voice fallback is promised.
- GPU back means a new verification window (`Fallback -> Verifying` in the session state machine, unit-tested).

## 9. GPU plan

Requirements: NVIDIA's model card lists Ampere A100 and Hopper H100 as supported, tested on A100 80 GB [source: model card]. The model card gives no VRAM number. The Moshi README says the PyTorch version (no quantization) needs "a GPU with a significant amount of memory (24GB)" [source]. PersonaPlex is a fine-tune of the same 7B architecture, published in BF16 [source], so a **48 GB card should hold it in BF16 without reduced precision** [inferred, **to verify** on first boot]. A `--cpu-offload` flag exists for small GPUs [source: PersonaPlex README], but it is not for real time.

RunPod on-demand Pod prices, from runpod.io/pricing ("Updated October 8, 2026"; fetched 2026-10-18 about 20:10 CEST (+02:00)) [source]:

| GPU | VRAM | Community Cloud | Secure Cloud | Note |
|---|---|---|---|---|
| RTX A6000 | 48 GB | **$0.33/h** | $0.59/h | Ampere; same price as quoted before |
| A40 | 48 GB | $0.35/h | $0.59/h | Ampere |
| L40 | 48 GB | $0.69/h | $0.82/h | Ada |
| RTX 6000 Ada | 48 GB | $0.74/h | $0.99/h | Ada |
| L40S | 48 GB | **$0.79/h** | $1.09/h | same Community price as quoted before |
| A100 PCIe | 80 GB | **$1.19/h** | $1.79/h | tested class; same Community price as quoted before |
| A100 SXM | 80 GB | $1.39/h | $1.79/h | |
| H100 PCIe | 80 GB | $1.99/h | $2.89/h | |
| Serverless 48 GB (A6000/A40) | 48 GB | $1.22/h | n/a | per-second billing; cold starts and long WebSocket calls fit badly [inferred] |
| Storage | | volume $0.10/GB/mo running, $0.20/GB/mo idle; network storage $0.07/GB/mo under 1 TB | | weights about 16 GB in BF16 [inferred from 8B x 2 bytes] |

Example monthly cost at 1 h/day (30 h): A6000 $9.90 Community or $17.70 Secure; A100 PCIe $35.70 Community or $53.70 Secure; plus about $2-3/mo for a 30 GB network volume [arithmetic on the prices above].

**Recommendation:**

1. **Smoke test:** RTX A6000 48 GB, Community Cloud, $0.33/h, EU region, on-demand Pod, stopped when idle, with **test audio only** (no real conversations, no enrollment). Confirms VRAM headroom, BF16 on sm_86, and real latency.
2. **Real use:** RTX A6000 on **Secure Cloud** ($0.59/h), because live voice is personal data (section 7).
3. **If A6000 fails** (VRAM, kernels, latency): A100 PCIe 80 GB ($1.19 Community / $1.79 Secure), the class NVIDIA tested.
4. Not serverless for v2.

**Provisioning waits for `RUNPOD_API_KEY`** (not present) and Slavik's go-ahead on spend. Nothing has been provisioned.

## 10. What the scaffold does today

`crates/persona-plex-relay/` (MIT, Rust, no third-party dependencies; only the in-tree `zagreb-time`):

- `config.rs`: TOML-subset parser and validation (24 kHz, 80 ms frames, Opus, thresholds in order, env var *names* only, no plain `ws://` to remote hosts).
- `protocol.rs`: wire kinds, `AudioFrame`, `Session` state machine, `upstream_url()`, `extract_role_prompt()`, `forwardable_upstream()`.
- `speaker.rs`: `SpeakerVerifier` trait, `cosine_similarity`, `Thresholds`, `decide`, `enroll`, `ContinuousVerifier`, `authorize`.
- `plan.rs` + `main.rs`: load config, print the plan, exit. **No listener, no upstream connection, no model.**
- 24 unit tests.

## 11. Next steps (proposed, not started)

1. Slavik: `RUNPOD_API_KEY`, spend approval, Secure or Community Cloud.
2. Slavik: consent policy (who may enroll, retention, deletion), and whether anyone other than him will use it.
3. Slavik: the false-accept target for the threshold (6.7) and which second factor to use for privileged actions.
4. Smoke test PersonaPlex on an A6000 with `NATF2.pt` and the v1 role prompt; measure latency and Opus bitrate.
5. Test `samantha.wav` as a custom voice prompt.
6. Relay: add the WebSocket listener and upstream client (Rust), loopback-only first.
7. Speaker: pick a model after the licence check, add the ONNX runtime, port fbank features, measure CPU cost per window on the VPS, and run the calibration of 6.7.
8. Encrypted voiceprint store plus delete command.
9. Decide on the TLS front end or keep SSH-tunnel-only access.

## Sources

- PersonaPlex model card: https://huggingface.co/nvidia/personaplex-7b-v1 (dual-stream, 24 kHz, A100/H100, tested A100 80 GB, BF16, FullDuplexBench latencies, WavLM-TDNN SSIM)
- PersonaPlex code: https://github.com/NVIDIA/personaplex (README: install, `moshi.server`, port 8998, `--cpu-offload`, 18 voices; `moshi/moshi/server.py`: `/api/chat`, `text_prompt` / `voice_prompt` query, global lock, message kinds)
- PersonaPlex paper: https://arxiv.org/abs/2602.06053
- Moshi: https://github.com/kyutai-labs/moshi (two audio streams, Mimi 12.5 Hz / 1.1 kbps / 80 ms, 160 ms theoretical / 200 ms on L4, 24 GB for PyTorch, Rust Mimi, web UI echo cancellation); protocol: `client/src/protocol/types.ts`, `encoder.ts`
- Opus: RFC 6716 §2.1.1, https://www.rfc-editor.org/rfc/rfc6716
- RunPod pricing: https://www.runpod.io/pricing (updated 2026-10-08, fetched 2026-10-18)
- SpeechBrain ECAPA-TDNN: https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb
- WavLM SV: https://huggingface.co/microsoft/wavlm-base-plus-sv
- WeSpeaker results: https://github.com/wenet-e2e/wespeaker/blob/master/examples/voxceleb/v2/README.md ; models: https://github.com/wenet-e2e/wespeaker/blob/master/docs/pretrained.md ; https://huggingface.co/Wespeaker/wespeaker-voxceleb-resnet34-LM
- sherpa-onnx speaker models and Rust API: https://github.com/k2-fsa/sherpa-onnx/releases/tag/speaker-recongition-models ; https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/speaker_embedding_extractor.rs
- AASIST: https://arxiv.org/abs/2110.01200 ; ASVspoof 5: https://arxiv.org/abs/2408.08739
- NIST SP 800-63B: https://pages.nist.gov/800-63-4/sp800-63b.html
- GDPR: https://eur-lex.europa.eu/eli/reg/2016/679/oj ; EU AI Act: https://eur-lex.europa.eu/eli/reg/2024/1689/oj ; EDPB Guidelines 02/2021 on virtual voice assistants: https://www.edpb.europa.eu/our-work-tools/our-documents/guidelines/guidelines-022021-virtual-voice-assistants_en ; AZOP: https://azop.hr

## Changelog

- v2 draft 1, 2026-10-18 20:25 CEST (+02:00): first design; Rust relay scaffold with 24 tests; RunPod prices rechecked (page updated 2026-10-08).
