<!-- SPDX-License-Identifier: MIT -->
# persona-plex-relay (scaffold)

VPS side of the Persona Plex v2 voice-to-voice channel:
browser (WebSocket, Opus) -> **this relay** (auth, session, speaker-verification
policy; no inference) -> GPU host running the PersonaPlex moshi server.

Status: **scaffold**. It loads a TOML config and prints its plan. It opens no
socket, starts no listener, connects to nothing, and runs no model.

- `src/protocol.rs` - moshi wire kinds (0x00..0x07), relay-private kinds
  (0x40..0x42), audio frame, session state machine, upstream URL builder.
- `src/speaker.rs` - `SpeakerVerifier` trait, cosine similarity, accept /
  uncertain / reject thresholds, enrollment, continuous verification, and the
  action gate (voice never unlocks privileged actions alone; money always denied).
- `src/config.rs` - small TOML-subset parser (no third-party deps) + validation.
- `persona-plex-relay.example.toml` - every host and path lives here.

Run: `persona-plex-relay --config persona-plex-relay.example.toml`
Design: `docs/persona-plex-v2-design.md`.

Workspace note: this crate is picked up by the root `members = ["crates/*"]`
glob. Its only dependency is in-tree `zagreb-time`, so the next workspace
build adds exactly one `[[package]]` entry (`persona-plex-relay`) to
Cargo.lock. Commit that Cargo.lock change together with the crate, or
`cargo build --locked` (release.yml) fails.
