# What voice model does Grok use? (as of 29 Oct 2026)

Research only, from the web. Nothing was changed on any server. Pages were read 29 Oct 2026.
Note: xAI now calls itself **SpaceXSI** ("formerly xAI", per SiliconANGLE). x.ai and docs.x.ai now use that name.

## Short answer
Grok's voice runs on **Grok Voice 3.0** (`grok-voice-3.0`; the alias `grok-voice-latest` points to it). It is one end-to-end **speech-to-speech** model, released **14 Oct 2026**. xAI says it powers voice in the Grok apps, in Tesla cars, in Starlink call centres, and in Grok Bot's voice mode. It is **proprietary and closed-weights**, and xAI built it in-house. The size and parameter count have **not been published**.

## The parts
| Part | Current model | Price (official) | Notes |
|---|---|---|---|
| Speech-to-speech (realtime) | `grok-voice-3.0` (since 14 Oct 2026) | $0.08/min ($4.80/hr) | Endpoint `wss://api.x.ai/v1/realtime`. Follows the OpenAI Realtime API spec, and there is a LiveKit plugin. Uses server VAD and tool calls (web_search, x_search, MCP, functions, file_search). Reasoning effort can be "high" or "none". |
| Speech-to-text | `grok-voice-transcribe-2.0` (since 17 Sep 2026; v1.0 retired 2 Oct 2026) | $0.10/hr batch, $0.20/hr streaming | 38+ languages, diarization, "Smart Turn" end-of-turn detection, word timestamps. |
| Text-to-speech | `/v1/tts`. The docs give no versioned model name. | $15.00 per 1M characters | 20 listed languages, speech tags ([laugh], [pause], whisper), WebSocket streaming. Custom voice cloning since 1 May 2026, from a clip of up to 120 s. |

**Voices:** Ara, Eve (default), Leo and Rex are named in the docs and posts. All voices can speak every supported language, and you can also clone your own.

**Latency (xAI's claims):** under 1 second. For Grok Voice 2.0, xAI gave 0.70 s time-to-first-audio, versus 1.25 s for Voice 1.0. For 3.0, it says latency is "40% lower" than GPT-Live-1 (Astra) at a similar τ-Voice score.

**Benchmarks (as xAI reports them, citing Artificial Analysis, 13 Oct 2026):** Grok Voice 3.0 scores 86.9 on the AA Speech-to-Speech Index. Grok Voice 2.0 scored 81.3, GPT-Live-1 83.2 and Gemini 3.8 Live 82.6. It scores 98.5% on Big Bench Audio and 97% on Full Duplex Bench. These are vendor-published figures and I did not check them on Artificial Analysis directly.

## Timeline
- Jan 2025: Leaks (TestingCatalog) showed the iOS voice mode in development with **both an xAI voice model and an ElevenLabs model** to choose from. This was a report of a hidden feature, not something xAI confirmed.
- Dec 2025: The Grok Voice Agent API launched at $0.05/min. xAI said: "We built the entire voice stack in-house, training our own voice activity detection (VAD), tokenizer, and audio models from scratch."
- Apr 2026: Grok Voice "Think Fast" 1.0, at $0.05/min (dates per third-party dev.to/xstack trackers).
- 29 Jul 2026: Grok Voice 2.0 ("Think Fast 2.0") at $0.08/min. The alias switched to it on 5 Aug. xAI says the model "reasons through queries while speaking".
- 17 Sep 2026: grok-voice-transcribe-2.0.
- 14 Oct 2026: **Grok Voice 3.0**. Voice 2.0 was deprecated the same day, with the price unchanged.

## In-house or third party?
- **Official position: in-house.** xAI says it trained its own VAD, tokenizer and audio models. In its Voice 2.0 post, xAI benchmarked itself *against* ElevenLabs Scribe v2 and Deepgram Nova 3, treating them as competitors rather than suppliers.
- **ElevenLabs:** the only evidence of ElevenLabs is the Jan 2025 pre-launch leak. I found no official source saying ElevenLabs is used today. Treat "Grok uses ElevenLabs" as **outdated or unconfirmed**.
- On whether it is truly one model or a cascade: xAI calls it a "speech-to-speech model". A dev.to article says it is "a single model rather than a cascaded ASR-LLM-TTS pipeline". That wording is third-party, and xAI has not published architecture details.

## Open source / open weights
- **Voice models: none released.** There are no weights, model card or licence for Grok Voice 1.0, 2.0 or 3.0, or for transcribe or TTS. They are API-only and proprietary.
- The open-weights releases are text LLMs, not voice models:
  - **Grok-1** (314B MoE): released 17 Mar 2024 under **Apache 2.0**.
  - **Grok 2 / 2.5** (about 268B MoE according to Beebom; about 500 GB on Hugging Face, `xai-org/grok-2`): released Aug 2025 under the custom **Grok 2 Community License**. That licence has commercial-use and acceptable-use limits and forbids using the model to train other models. It is source-available, not OSI open source.
  - **Grok 3**: Musk said in Aug 2025 that it would be open-sourced in about 6 months. As of mid-Oct 2026 that **has not happened** (per GrokMag).
  - Musk said on 25 May 2026 that a "0.5T model" would be open-sourced toward the end of 2026. This is **unconfirmed and still pending** (per GrokMag).
  - Grok Build (the coding agent's *code*, not a model) was released under Apache 2.0 on 15 Jul 2026.

## Not published / unconfirmed
- Parameter count or size of any Grok Voice model: **not officially published**.
- Which text LLM, if any, sits behind the voice model's reasoning: not stated.
- A version name for the TTS model: not stated in the docs.

## Sources
- xAI: Introducing Grok Voice 3.0 (14 Oct 2026): https://x.ai/news/grok-voice-3
- xAI: Introducing Grok Voice 2.0 (29 Jul 2026): https://x.ai/news/grok-voice-think-fast-2
- xAI: Grok Voice Agent API (Dec 2025): https://x.ai/news/grok-voice-agent-api
- Docs, Voice overview and pricing: https://docs.x.ai/docs/guides/voice
- Docs, Speech to Speech: https://docs.x.ai/developers/model-capabilities/audio/speech-to-speech
- Docs, Speech to Text: https://docs.x.ai/developers/model-capabilities/audio/speech-to-text
- Docs, Text to Speech: https://docs.x.ai/developers/model-capabilities/audio/text-to-speech
- Docs, Release notes: https://docs.x.ai/developers/release-notes
- @SpaceXSI post (14 Oct 2026, via mirror): https://teapawt.org/SpaceXSI/status/2110416580899168305
- SiliconANGLE (14 Oct 2026): https://siliconangle.com/2026/10/14/grok-voice-3-0-from-spacexsi-outscores-openai-and-google-on-speech-to-speech-index/
- TestingCatalog, ElevenLabs leak (26 Jan 2025): https://www.testingcatalog.com/xai-develops-voice-mode-for-ios-app-with-grok-and-elevenlabs-models/
- Hugging Face, Grok 2 weights: https://huggingface.co/xai-org/grok-2
- Beebom, Grok 2.5 open weights: https://beebom.com/elon-musk-xai-open-sources-grok-2-5/
- GrokMag, open-source status (as of 18 Oct 2026): https://grokmag.com/grok-models/open-source/
- dev.to, Voice 2.0 pricing and architecture (third-party): https://dev.to/lukeocodes/grok-voice-20-ships-with-a-quiet-60-price-rise-3eoh
