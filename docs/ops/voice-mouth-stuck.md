<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# inc-865945dd "galion-one mouth stuck loading" — diagnosis and staged fix (slot 358)

Prepared 2026-10-28 05:20 CET (+01:00) on `swarm/voice-mouth-stuck`
(base `swarm/main-staging` 98466ab9 + `swarm/mouth-honest-stub-rs` d552fc4b,
fast-forward). **Nothing deployed, no VPS write, no service restart.**

## 1. Verdict

The mouth is **not stuck**. Nothing is loading and nothing ever will:

* Live `galion-one` (VPS, `/opt/galion/rust/bin/galion-one`, sha256 prefix
  `24cb4976b1325ae0` = build of `40454f9`, drop-in `50-rust-bin.conf`, up since
  2026-10-22 22:46:47 CET, NRestarts 0) serves `:8002` itself. In `40454f9`
  `GET /health` answers
  `{"ok":false,"status":"loading","provider":"local","model":"chatterbox","loaded":false}`
  whenever `speech::local().ready()` is false — and `local()` is the
  `Unavailable` stub (B14 removed the Python spawn). So the body is
  **hard-coded "loading" forever**.
* No voice backend exists on the VPS: no `GALION_MOUTH` in the unit, no
  voice/chatterbox process, `/var/lib/galion/voice-models` absent, nothing on
  `:8003`. `POST /synthesize` fails at once (500 `{"error":"voice"}`).
* The galion-one journal since 2026-10-22 has one line (the start); there is
  no queue, no job, no "last success" — the mouth has never spoken on this
  build.
* The nerve's "stuck loading for 10.8 h" counts from its own restart
  (incident open since 2026-10-27 18:15 CET); the real age is ~5.3 days of
  this process and the same since the 2026-10-21 deploy (slot 181 finding).

Proved on the box with the real `40454f9` binary (section 3 of
`docs/real-tests/voice-mouth-stuck.md`): body identical before and after a
SIGKILL + respawn. **A restart cannot fix it**, but the live nerve's advice
was "restart that one service" — that advice is the bug this branch fixes on
the nerve side.

## 2. What this branch changes (code, staged)

Nerve (`crates/openmind-nerve`):

| Change | Why |
|---|---|
| `health_body_summary` keeps `reason` (80-char cap) | the honest body's "voice not deployed" was dropped, so the nerve could not show why |
| new `cortex::not_deployed()` + finding `not-deployed-<name>` (warn) for 503 `"status":"unavailable"` | an honest "nothing deployed" is a known gap, not an error and not "stuck loading" |
| signal text "-- not deployed for N (honest 503, nothing loading)" + incident next "a restart will not help; deploying the backend is an owner decision" | correct operator advice |
| `stuck-loading` fix text when no unit owns the listener (the mouth is served by galion-one): "...hard-coded (no model worker exists)...; a restart will not help" | stops the restart advice for the live case |
| incident `next` for stuck loading mentions the hard-coded case | same |
| component note "503 unavailable while no voice backend is deployed" | was "503 while the model loads" (untrue) |
| goldens: frozen pre-trait golden kept; `support::s358_delta(_md)` applies the two documented text changes (like `t32_delta`) | no silent golden rewrite |

galion-one: unchanged here beyond d552fc4b (already honest on main-staging:
`mouth_health()` -> 503 `"unavailable"` + "voice not deployed";
`galion-mouth-stub` binary).

## 3. What clears the incident on the live VPS (owner decisions, not done)

* **D-358-1 deploy honest galion-one** (main-staging lineage incl. d552fc4b)
  in place of `40454f9` via the existing drop-in pattern. Effect: `:8002`
  says "unavailable / voice not deployed"; with today's live nerve 9d1a4f3 the
  incident drops from error "stuck loading" to warn "degrading HTTP 503"; with
  this branch's nerve it becomes warn `not-deployed-galion-one-mouth` with the
  right advice. Needs a galion-one restart = owner approval.
* **D-358-2 deploy this nerve change** (fold into the next nerve deploy
  candidate; cherry-pick check against nerve-merge-6 9baa450c recorded in the
  real-test doc). Without D-358-1 it already changes the live advice to "a
  restart will not help".
* **D-358-3 real voice** (galion-voice-serve on :8003 + `GALION_MOUTH`) or
  retire the `:8002` probe from the nerve table. Model files, SIMD ruling and
  RAM budget are the owner's call (slot 181 option 2).

Rollback for D-358-1/2: previous binaries stay (`40454f9` 24cb4976…, nerve
9d1a4f3); remove the drop-in / revert the binary and restart that one unit.
