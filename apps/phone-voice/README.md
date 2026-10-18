<!-- SPDX-License-Identifier: MIT -->
# phone-voice (v0.1, not deployed)

Tiny PWA voice client for the Obsidian Open Mind Stack (MIT). Plain HTML/JS,
no framework, no build step, about 17 KB. Streams mic audio over WebSocket to
the Persona Plex relay (`crates/persona-plex-relay`), plays back relay audio,
and shows connection, backend and speaker-verification status.

Files: `index.html`, `app.js`, `sw.js`, `manifest.webmanifest`, `icon.svg`,
`config.example.json` (copy to `config.json` next to `index.html` when
serving; no host is hard-coded).

- Wire protocol = relay/moshi kinds: sends only `0x01` (Opus 24 kHz mono in
  Ogg pages, WebCodecs `AudioEncoder`), waits for the `0x00` handshake, plays
  `0x01` from the relay, shows `0x02` text, `0x40` speaker status, `0x41`
  fallback, `0x42` bye, `0x05` errors.
- Modes: push-to-talk (hold) and full duplex (tap to mute). In both, gated
  audio is sent as silence because the model steps per 80 ms input frame.
- Text box -> Galion One (`text_url`), used whenever voice is unavailable.
- Browser microphone access requires HTTPS (secure context); `http://` only
  works on `localhost`. WebCodecs Opus: current Chrome/Edge on Android.

See `docs/phone-instance.md`.
