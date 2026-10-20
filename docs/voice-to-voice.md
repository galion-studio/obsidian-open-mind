# Voice to voice

- DESK-ID: openmind.B46
- Written: 2026-10-19T16:10:25+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Status: design only. No GPU was rented, no weight was fetched, no process was started, and no unit was added.

This page is the integration map for Galleon Voice to Voice: Galion One is the front end, the VPS only relays, and PersonaPlex runs on an external GPU. It does not implement that path. The deeper channel notes (speaker model choice, latency budget, provider prices) stay in `docs/persona-plex-v2-design.md` and are not repeated and not executed.

## Scope

| Rule | What this page does | Source |
| --- | --- | --- |
| Design only | Records the three hops. Changes no Rust, no page, no unit, no tunnel. | `TEACHER/QUEUE.md` B46 |
| No GPU spend | Upstream ships disabled. No host is named as live. No price is chosen here. | `crates/persona-plex-relay/persona-plex-relay.example.toml` `[upstream] enabled = false`; `crates/persona-plex-relay/src/config.rs` test that upstream must ship disabled |
| No new service | `deploy/` has `galion-one.service` and no persona-plex unit. This page does not add one. The relay binary prints a plan and exits. | `deploy/galion-one.service`; `find` over `deploy/` has no persona-plex file; `crates/persona-plex-relay/src/main.rs` |
| Persona Plex v1 | The role text the relay would send is the fenced block in the v1 persona file. | `personas/persona-plex-v1.md`; `crates/persona-plex-relay/src/protocol.rs` `extract_role_prompt` |
| MIT fit | Integration code stays MIT. Model weights do not. | `docs/goals.md` Project: Galleon Voice to Voice; root `Cargo.toml`; `LICENSE` |

## The three hops

```
person in the browser
    Galion One page
    today: 16 kHz WAV, POST /reply, half duplex
    later (not built): Opus WebSocket to the relay only
        |
        v
VPS relay  persona-plex-relay
    scaffold: no socket
    would bind 127.0.0.1:8095
    auth, one session, persona set by the relay, audio forwarded, no model
        |
        v  (off)
external GPU
    PersonaPlex moshi server, route /api/chat
    not provisioned
```

When the GPU hop is off, the person stays on Galion One. The relay config names that fallback `galion-one-text` at `http://127.0.0.1:8091` (`crates/persona-plex-relay/persona-plex-relay.example.toml`). That port is the Galion One bind (`deploy/galion-one.service` `ONE_BIND`, and `crates/galion-one/src/main.rs` `bind_addr("ONE_BIND", 8091)`).

Heavy inference stays off the VPS (`docs/goals.md` placement line; `personas/persona-plex-v1.md` Rules inherited). The relay crate states that it does no inference (`crates/persona-plex-relay/README.md`, `crates/persona-plex-relay/src/lib.rs`).

## Hop 1. Galion One front end

Galion One is the voice front end the project already names (`docs/goals.md` goal line and the Galleon Voice to Voice section: full-duplex voice, most likely via the Galion One voice front end and Hermes personas). The live binary is the existing unit, not a unit this ticket creates (`deploy/galion-one.service` `ExecStart=/opt/galion/openmind/dist/bin/galion-one`).

What the page does today:

| Fact | Detail | Source |
| --- | --- | --- |
| Page | `GET /` reads `GALION_HOME/one/index.html`. Default home is `/workspace/galion-light`. The same file is `/opt/galion/galion-light/one/index.html`. | `crates/galion-one/src/main.rs` `home`, `one_page` |
| Public origin | Hostname `one.galion.app` is sent to `http://127.0.0.1:8091`. No second hostname is added for the relay. | `stack/kms/galion-src/galion-infrastructure/cloudflared/config.yml` |
| Pairing | The page will not open the mic until `/me` says the browser is paired. Pairing posts `/pair`. | `/workspace/galion-light/one/index.html` `fetch("/me")`, form submit to `/pair`; `crates/galion-one/src/main.rs` `POST /pair` |
| Mic | `getUserMedia`, one channel, echo cancellation and noise suppression on. Capture is resampled to 16 kHz and wrapped as WAV. | `/workspace/galion-light/one/index.html` `listen`, `rate = 16000`, `wav` |
| Turn taking | A silence hang or an 8 second cap posts the wav to `/reply`. While `busy` is set, further mic samples are dropped, then the returned wav is played. That is half duplex, not full duplex. | `/workspace/galion-light/one/index.html` `finish`, `onAudio` |
| Server routes | `POST /hear` and `POST /reply` take the wav. `POST /voice` stores `samantha` or `jarvis`. | `crates/galion-one/src/main.rs` |
| Ear, mind, mouth | Defaults: ear `127.0.0.1:8643`, mind `127.0.0.1:8642`, mouth `127.0.0.1:8002`. The page shows ear, mouth, mind, and eye from `GET /link`. | `crates/galion-one/src/main.rs` `ear_addr`, `mind_addr`, `mouth_addr`, `eye_addr`; `/workspace/galion-light/one/index.html` `loadMachine` |
| Mind text | The live reply path calls Hermes with `voice_setting()` as the system text. That string still says "Reply in exactly four sentences." | `crates/galion-one/src/main.rs` `pull_hermes`, `voice_setting` |
| Mouth | A second listener on `ONE_MOUTH_BIND` (default `127.0.0.1:8002\)) serves the local mouth. Voice id 0 is Samantha, 1 is Jarvis. | `crates/galion-one/src/main.rs` `serve_tts`, `voice_id`, `set_voice` |
| Voice buttons | The page has Samantha and Jarvis buttons. They `POST /voice`. They do not set a PersonaPlex voice prompt. | `/workspace/galion-light/one/index.html` `[data-voice]` |
| No duplex socket | The page has `getUserMedia` and no WebSocket client. | `/workspace/galion-light/one/index.html` |

The channel copy in this repo matches the short Samantha line the page is built around: warm, then one short line, plain English, no tools, no lists, say so if a live number was not given (`stack/hermes/channels/galion-one.md`).

Design for this hop, not built:

- The person stays on this page. A second web client is not the front.
- Duplex, when a later ticket builds it, is a mode of this page on the same origin. It would send binary WebSocket frames to the relay, not to the GPU.
- Until that mode exists, the page keeps `POST /reply`. That is also the fallback the relay config already names.
- The page must not send `text_prompt` or `voice_prompt`. The relay sets both (`crates/persona-plex-relay/src/protocol.rs` `upstream_url`; `docs/persona-plex-v2-design.md` section 2.2).
- The Samantha and Jarvis buttons stay on the half-duplex mouth. They do not retarget the GPU voice mid-call. v1 allows one persona per session and no switch mid-session (`personas/persona-plex-v1.md` Limits).
- Reaching `127.0.0.1:8095` from a remote browser is not solved here. The relay bind is loopback, and the cloudflared file has no route to port 8095. This ticket does not add a hostname, a path, or a tunnel (`crates/persona-plex-relay/persona-plex-relay.example.toml` `[relay] bind`; `stack/kms/galion-src/galion-infrastructure/cloudflared/config.yml`).

## Hop 2. VPS relay

The relay is the crate `persona-plex-relay`. It is a scaffold: load a TOML config, print a plan, exit. It opens no socket and connects nowhere (`crates/persona-plex-relay/README.md`, `crates/persona-plex-relay/src/main.rs`).

| Piece | Design value already in the tree | Source |
| --- | --- | --- |
| Would-be bind | `127.0.0.1:8095`, not bound | example toml `[relay]`; `src/plan.rs` prints "not bound" |
| Sessions | `max_sessions = 1` | example toml; `src/plan.rs` |
| Auth | Config holds the name of an env var. The plan prints set or not set, never the value. | example toml `auth_token_env`; `src/plan.rs`; `src/main.rs` |
| Upstream | `enabled = false`. URL is `wss://GPU_HOST_NOT_PROVISIONED:8998/api/chat`. | example toml `[upstream]` |
| Plain ws | `ws://` is rejected unless the host is loopback. Remote links must be `wss://`. | `src/config.rs` `validate` |
| Persona URL | `voice_prompt` and `text_prompt` are query parameters on the upstream URL. | `src/protocol.rs` `upstream_url` |
| Role text | First fenced block after the heading `## Role prompt`. The example points that file at `/opt/galion/openmind/personas/persona-plex-v1.md`. | `src/protocol.rs` `extract_role_prompt`; example toml `role_prompt_file` |
| Voice file name | `NATF2.pt`, one of the packaged names. The toml comment says a custom Samantha clip is untested. | example toml `[upstream]` |
| Audio | 24000 Hz, Opus, 80 ms frames. User-to-model and model-to-user at once. | example toml `[audio]`; `src/config.rs` `MIMI_SAMPLE_RATE_HZ`, `MIMI_FRAME_MS`; `src/protocol.rs` `AudioStream` |
| Forward filter | Only kind `0x01` audio is forwardable upstream. Kinds `0x40` to `0x42` stay on the VPS. | `src/protocol.rs` `forwardable_upstream`, `MessageKind` |
| Session states | Authenticating, Enrolling, Verifying, Active, Fallback, Closed. GPU loss moves to Fallback. | `src/protocol.rs` `SessionState` |
| Fallback | Mode must be `galion-one-text` or `none`. Shipped mode is `galion-one-text` at `http://127.0.0.1:8091`. | `src/config.rs` `validate`; example toml `[fallback]` |
| Money | `authorize` returns false for the money class, with or without a second factor. | `src/speaker.rs` `authorize` |
| Clock | The plan timestamp is `zagreb_time::now_rfc3339`. | `src/main.rs` |

Wire kinds already typed, and not spoken on a socket: `0x00` handshake, `0x01` audio, `0x02` text, `0x03` control, `0x04` metadata, `0x05` error, `0x06` ping, `0x07` colored text, plus relay-private speaker (`0x40`), fallback (`0x41`), and bye (`0x42`) (`crates/persona-plex-relay/src/protocol.rs` `MessageKind`).

The relay does not listen, so the live page never receives `0x41`. Fallback today is simply that the page never leaves `POST /reply`.

Speaker checks (enrollment, cosine thresholds, liveness) are policy types in `src/speaker.rs`. The example model path says the file is not downloaded, and the thresholds are marked placeholders (`persona-plex-relay.example.toml` `[speaker]`). This ticket does not download a model, create a voiceprint store, or calibrate thresholds.

## Hop 3. External GPU PersonaPlex

Recorded facts, not re-fetched for this ticket:

| Fact | Statement in the repo | Source |
| --- | --- | --- |
| What it is | NVIDIA PersonaPlex `nvidia/personaplex-7b-v1`, released 2026-01-15. 7B, real-time, full duplex, speech to speech, built on Moshi. Not a component shipped inside Moshi. | `docs/goals.md` verified facts |
| Persona control | One text role prompt plus one audio voice prompt. 18 packaged voices. One persona per session. "Multiplexed" here means it listens and speaks at the same time, not several personas at once. | `docs/goals.md`; `personas/persona-plex-v1.md` Context and Limits |
| Language | English only. | `docs/goals.md`; `personas/persona-plex-v1.md` Limits |
| Where it runs | External GPU (RunPod or similar). Never the CPU-light VPS. The VPS only relays and orchestrates. Supported class named in goals: Ampere A100 and Hopper H100, tested on A100 80 GB. CPU offload and CPU-only offline evaluation exist and are not for real-time use. | `docs/goals.md` Hardware and Placement |
| This tree | Upstream disabled. Placeholder host. No GPU process is part of the design work. | example toml `[upstream]`; `docs/persona-plex-v2-design.md` status line |

The GPU hop, when it exists later, is the moshi server route `/api/chat` on port 8998, which is the URL shape in the example config. The relay would open that URL with the v1 role prompt and `NATF2.pt`. It would not run the model (`crates/persona-plex-relay/src/lib.rs`).

Hermes stays the mind of the half-duplex path (`pull_hermes` in `crates/galion-one/src/main.rs`). On the GPU hop the "Hermes persona" is the v1 role prompt, injected once per session by the relay. The GPU hop does not call Hermes for each spoken turn. That is how `docs/goals.md` ("Galion One voice front end and Hermes personas") maps onto the three hops without putting a model on the VPS.

## Persona Plex v1 map

The file exists: `personas/persona-plex-v1.md`. Status there: proposed, not wired, evaluation not started. Created 2026-10-18 19:25 CEST (+02:00). The official length rule was set 2026-10-18 19:27 CEST (+02:00): one or two short sentences.

The relay reads the role-prompt block, not the whole markdown (`extract_role_prompt`). That block is the text that would be `text_prompt`.

| v1 rule | Where it applies | Source |
| --- | --- | --- |
| Name Persona Plex. Voice of Galion One. Not the admin desk. | First lines of the role prompt. | `personas/persona-plex-v1.md` Role prompt |
| Warm, close, then one short line. One or two short sentences. | Role prompt. This is the GPU-path length. It replaces the four-sentence line still compiled into `voice_setting`. This ticket does not edit `voice_setting`. | persona file Length and Role prompt; `crates/galion-one/src/main.rs` `voice_setting` |
| Plain English. No greeting unless greeted. Do not say it is a model or a system. Not a copy of another model. | Role prompt. | persona file Role prompt |
| No lists, headings, markdown, paths, or code. Say the answer, not the path. | Role prompt. Same spoken-form rule in the channel copy. | persona file; `stack/hermes/channels/galion-one.md` |
| If no live number was given, say so. | Role prompt. | persona file; channel copy |
| No files written, no commands, no money, no spawn. | Role prompt. Money is also hard-denied in `authorize`. | persona file; `crates/persona-plex-relay/src/speaker.rs` |
| Obsidian Hearth: knowledge, prosperity, and integrity. | Role prompt. | persona file |
| Audio voice | Persona file points at the local Samantha reference clip and says that use is inferred, not verified. If it fails, use the closest warm packaged voice. The relay example does not use that clip. It uses `NATF2.pt`. The clip stays on the GPU host and is not published with the MIT tree. | persona file Voice prompt; example toml |
| English only. One persona per session. No mid-session switch. | Limits. Matches one upstream URL per connection and `max_sessions = 1`. | persona file Limits; `upstream_url`; example toml |
| No stray CJK unless the person writes that or asks. | Inherited hygiene rule. Not a relay filter in the scaffold. The role prompt asks for plain English. | persona file Rules inherited |
| Timestamps in Europe/Zagreb with an offset. | Persona context. Relay plan clock is Zagreb RFC 3339. Stack standard is the same. | persona file Context; `src/main.rs`; `docs/goals.md` Standards |
| Real money not approved. | Persona rule and goals standard. Relay money class always denies. | persona file; `docs/goals.md` Standards; `speaker.rs` |
| Not wired yet. | Persona status line. Relay status is scaffold. | persona file; relay README |

The half-duplex path and the GPU path disagree on length until a later code change: `voice_setting` still requires four sentences (`crates/galion-one/src/main.rs`), and the v1 role prompt requires one or two. The design uses the v1 prompt on the GPU hop only. The live page is left as it is.

## License

The Open Mind stack is MIT (`docs/goals.md` Standards; root `Cargo.toml` `[workspace.package] license = "MIT"`; `LICENSE`). `persona-plex-relay` takes `license.workspace = true` (`crates/persona-plex-relay/Cargo.toml`).

Fit, copied from the goals note and not re-checked against upstream sites in this ticket (`docs/goals.md`, verified-facts list, checked 2026-10-18):

- PersonaPlex code is MIT.
- Weights are under the NVIDIA Open Model License. They are gated. They are not MIT.
- Base Moshiko weights are CC-BY-4.0.
- Our integration code can stay MIT.
- The weights must not be redistributed as part of the stack. Users fetch them after accepting the NVIDIA license.

The same limit is in the persona file: weights are never bundled or redistributed in the MIT stack, and the voice clip is not published with it (`personas/persona-plex-v1.md` Voice prompt and Limits).

Nothing in this ticket fetches weights, copies a voice clip into the repo, or changes `LICENSE`.

## Not done

- No listener on 8095, and no unit file for the relay.
- No edit to `one.galion.app`, to `index.html`, or to `voice_setting`.
- No GPU host, no spend, no weight download, no speaker-model download.
- Goals checkboxes for license re-check, provider choice, this design, a prototype, and a fact-check stay unchecked. This page is the design. It does not edit `docs/goals.md`.
