<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Approval packet: strict HTTP handling, `swarm/common-migrate-2`

Prepared on 2026-10-20 at 21:55 +02:00 (Europe/Zagreb). The evidence is the
Rust real-life test `crates/galion-common/tests/real_migrate.rs`, run on the
box at 21:26 +02:00. It compared old (26d98b0) and new binaries. Full output
is in `docs/real-tests/common-migrate-2.md`.

For **valid requests** nothing changes: 98 probes gave 87 byte-identical
replies, 11 identical after masking, and 0 different. Every item below is
about slow, oversized, malformed or abusive input only. Tick yes or no for
each item.

**Revert by env (no code change, restart only).** These items read
`<PREFIX>_HTTP_{HEAD_TIMEOUT_MS, BODY_TIMEOUT_MS, WRITE_TIMEOUT_MS,
MAX_HEAD_BYTES, MAX_HEADERS, MAX_BODY_BYTES, MAX_CONNECTIONS}`. Values are
whole numbers above 0; a bad value is ignored with a stderr warning.

- The prefixes are `HERMES_RS`, `OPENMIND_MEMORY`, `DESK` and `ROOM`.
- Phase 4 of the test confirmed it on all 4 servers:
  - `HEAD_TIMEOUT_MS=1000` cut a stalled head at 1.0 s;
  - `MAX_CONNECTIONS=2` made the 3rd connection get a 503.

**Revert by code.** Restore the crate from 26d98b0:
`git checkout 26d98b0 -- crates/<crate>`, then `cargo build` (Cargo.lock updates itself, offline).
The galion-common changes are additive, so a single crate can go back
without touching the others. This was checked with `cargo check --workspace
--all-targets` for each of the 7 crates; the results are in the section
"Rollback check".

## hermes-rs (`HERMES_RS_`)
- [ ] yes [ ] no **H1. Head deadline: 10 s in total.** Old: 30 s for each `read()`, with no total.
  - Evidence: under 80 slowloris connections, old let 64 hold their slot for the whole 37 s drip and gave 0/60 legit 200s; new cut them at 10.1 s, and legit GETs got 200 from 10.3 s on.
  - Risk: a client that takes more than 10 s to send its headers. No such client is known.
  - Revert: `HEAD_TIMEOUT_MS=30000`.
- [ ] yes [ ] no **H2. Body deadline: 60 s in total.** Old: 30 s for each read.
  - Risk: an 8 MiB upload slower than about 140 KB/s.
  - Revert: raise `BODY_TIMEOUT_MS`.
- [ ] yes [ ] no **H3. Strict parser.** Refusals use the OpenAI error envelope:
  - 413 for headers, header count or body;
  - 400 for framing;
  - 501 for other transfer codings;
  - 505 for HTTP versions other than 1.0/1.1;
  - 408 for a slow head.

  An empty connection is now closed silently (old: 400 "empty request"). `100 Continue` is sent when the client asks for it.
  - Evidence: 416 malformed requests gave 400:412 / 401:1 / 505:1 / closed:2 (old: 400:405 / 401:11). 1000 headers gave 413 (old: 401).
  - Risk: a sloppy client now gets 400 where it used to get 401.
  - Revert: code only.

## openmind-memory (`OPENMIND_MEMORY_`)
- [ ] yes [ ] no **M1. Head 10 s and body 30 s, both in total.** Old: 30 s for each read.
  - Evidence: old let 32 slowloris connections hold for 37 s and gave 0/60 legit 200s; new closed them at 10 s, and legit GETs got 200 from 10.3 s on.
  - Revert: `HEAD_TIMEOUT_MS` / `BODY_TIMEOUT_MS`.
- [ ] yes [ ] no **M2. An unparseable `Content-Length` is now 400.** Old: read as 0, which is a smuggling-shaped bug. Other HTTP versions are 400.
  - Evidence: malformed requests gave 400:15 / closed:401 (old: 400:5 / 404:11 / closed:400).
  - Revert: code only.
- [ ] yes [ ] no **M3. `OPENMIND_MEMORY_HTTP_MAX_BODY_BYTES` overrides `MEMORY_MAX_BODY_BYTES`.** The default stays at 1 MiB. This one is additive.

## galion-desk (`DESK_`)
- [ ] yes [ ] no **D1. New connection cap of 32.** Old: no cap.
  - Evidence: under slowloris, legit GETs got 200:49 / 503:11 (old: 200:60, because old never waited for a head).
  - Risk: 503s during a burst of more than 32 concurrent connections.
  - Revert: `MAX_CONNECTIONS=4096` comes close to "no cap", at one thread per connection.
- [ ] yes [ ] no **D2. Whole request read: head 3 s and body 3 s in total.** Old: a single `read()` of up to 8 KiB with a 3 s timeout. A request split over several TCP segments was handled from its first segment only.
  - Revert: the deadlines via `HEAD_TIMEOUT_MS` / `BODY_TIMEOUT_MS`. The single-read behaviour itself is a bug and cannot be restored by config.
- [ ] yes [ ] no **D3. Body limit of 64 KiB, with 413 above it.** Old: a 5 MB POST got 200, and only the first read was applied.
  - Revert: `MAX_BODY_BYTES`.
- [ ] yes [ ] no **D4. Strict 400/431/501/505.** Old: garbage was routed (malformed requests gave 404:414; a 100 KB header gave 404).
  - Revert: code only.

## room (`ROOM_`)
- [ ] yes [ ] no **R1. Cap of 32.** Under slowloris, legit GETs got 200:29 / 503:31 (old: 60/60).
  - Revert: `MAX_CONNECTIONS`.
- [ ] yes [ ] no **R2. Whole request read: head 8 s and body 8 s in total.** Old: one 4 KiB `read()` with an 8 s timeout.
  - Revert: `HEAD_TIMEOUT_MS` / `BODY_TIMEOUT_MS`.
- [ ] yes [ ] no **R3. Body limit of 64 KiB (413).** Old: 405/200 on oversized input.
  - Revert: `MAX_BODY_BYTES`.
- [ ] yes [ ] no **R4. Strict 400/431/501/505.** Old: garbage was routed (200:7 / 404:1 / 405:408).
  - Revert: code only.

## Clients (no config; revert = restore that crate)
The request bytes are identical in every golden capture (survival 28, one-talk 22, memory 8) and in the real-life run (guardian-tick, link-watch, desk → Ollama). The items below change only how odd replies are read:
- [ ] yes [ ] no **C1. link-watch.** An invalid status line, an incomplete reply, a reply over 8 MiB or a reply cut by the deadline is now a probe failure. Chunked replies are decoded.
- [ ] yes [ ] no **C2. openmind-survival probes.** Chunked replies are decoded. LF-only heads now return their body. A complete reply on a socket that stays open now returns at `Content-Length` (old: waited for the timeout).
- [ ] yes [ ] no **C3. one-talk.**
  - A chunked `hear` reply now parses.
  - A non-HTTP reply is now `UnexpectedEof` (old: `Ok(0)`).
  - A bare status line now gives 503 (old: an error).
  - Replies are capped at 8 MB.
- [ ] yes [ ] no **C4. memory `post_json`.** Chunked replies are decoded. Garbage now gives `Err("bad response")` (old: `Ok((0,""))`). Total deadline 60 s, cap 16 MiB.
- [ ] yes [ ] no **C5. desk's Ollama and hermes clients.** Total timeouts are 1.1 s / 92 s (old: per-operation timeouts 0.7 s / 90 s). Caps are 1 MiB / 16 MiB.

## Not configurable: parser strictness (H3, M2, D4, R4) and client parsing (C1-C5)
The old leniency is not a set of switches. It is a different parser: one `read()`, then guess. Offering it as a config mode would mean two parsers per server:
- twice the attack surface;
- it brings back the ambiguities the strict parser removes (CL+TE, unparseable CL read as 0, headers split across reads).

A per-crate code revert is the clean switch, and it was tested (next section). The numeric limits are configurable because they are plain values on one parser.

## Rollback check
For each crate, `git checkout 26d98b0 -- crates/<crate>` on this branch, followed by `cargo check --offline --workspace --all-targets`:
exit 0 for all 7 crates (hermes-rs, openmind-memory, galion-desk, room, link-watch, openmind-survival, one-talk), checked one at a time on 2026-10-20 at 21:52 +02:00. For galion-desk, room and openmind-survival, Cargo.lock only loses the galion-common dependency line(s).

## Open issue (exists before this branch, not part of the vote)
When a server is over its cap, galion-common's `reject_busy` writes the 503 and then calls `shutdown(Both)` while the request bytes are still unread. The kernel may then send a TCP RST that discards the 503, so the client sees an empty reply or a reset.
- Seen with old-hermes: 12 of 16 rejections showed as "write cut".
- Seen as an occasional empty reply in curl-style clients.
- The 12 old-hermes cases come from old hermes's own 503 path, which has the same flaw.
- Fix later in galion-common: read briefly (lingering close) before closing.
