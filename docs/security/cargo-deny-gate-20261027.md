<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cargo-deny gate (slot 288) — 2026-10-27

Written 2026-10-27 17:59 CET (+01:00), branch `swarm/cargo-deny-gate`, base
main-staging `98466ab9f33d3bad9b4849b0e202c400b4dbcd00`. Follow-up to
`swarm/apache-allowlist-memo` `56993b7a` (decision memo copied onto this branch
as `docs/decisions/apache-2-allow-list.md`). **No main merge. No VPS write.**

Tool: cargo-deny 0.20.2 (`/workspace/swarm/tools/bin/cargo-deny`).

## What changed in deny.toml

1. **MIT enforced as the house licence.** Plain `"Apache-2.0"` removed from the
   general `[licenses].allow` list so dual-licensed crates pass via their MIT
   arm and Apache-2.0-only crates must be named.
2. **Owner-pending Apache-2.0 list** lives in `[licenses].exceptions`:
   - `safetensors` → allow `Apache-2.0` (AP-1; see below).
3. **`Apache-2.0 WITH LLVM-exception`** added to allow (OWNER-PENDING AP-4 / L7
   from `swarm/sec-tooling-licence-memo`). Three crates already declare it as an
   OR choice and pass via MIT today (`wasi` 0.11.1, `wasip2` 1.0.4,
   `wit-bindgen` 0.57.1).
4. **bans / sources / advisories** unchanged in spirit: openssl / openssl-sys /
   native-tls / pyo3 denied; unknown registry/git denied; yanked = deny;
   advisories ignore = [].
5. **wildcards** stay `"warn"` (not `"deny"`): 66/69 workspace crates still lack
   `publish = false` on this tip, so path deps look public. Flip after slot 201
   (`sec-deny-wildcards`) lands.

## Gate results (workspace, this tip)

| Check | Result | Notes |
|---|---|---|
| licenses | **ok** | with safetensors exception |
| bans | **ok** | 49 warnings (wildcards + a few duplicate-version) |
| sources | **ok** | crates.io only |
| advisories | **ok** | RustSec DB fetched 2026-10-27 17:59 CET; 0 ignored |

Commands (box, Europe/Zagreb CET +01:00):

```
export PATH=/workspace/swarm/tools/bin:/workspace/swarm/bin:$PATH
export CARGO_TARGET_DIR=/workspace/swarm/target/cargo-deny-gate CARGO_INCREMENTAL=0
nice -n 10 cargo-deny --offline check licenses bans sources   # exit 0
nice -n 10 cargo-deny check advisories                        # exit 0
```

`edge/openmind-node` with the same `deny.toml`: licenses/bans/sources **ok**
(after `cargo fetch --locked --manifest-path edge/openmind-node/Cargo.toml`).
Warns `license-exception-not-encountered` for `safetensors` (not in the edge
graph) — expected.

## Offenders

### A. Owner-pending Apache-2.0 list (would fail MIT-enforcing deny)

Probed by temporarily dropping the exception and plain Apache-2.0 from allow
(dual MIT\|Apache still pass via MIT). **One offender:**

| Crate | Version | Licence | Why in tree | Owner item |
|---|---|---|---|---|
| safetensors | 0.7.0 | Apache-2.0 only | transitive via candle-core / candle-nn (voice / chatterbox) | **AP-1** |

Update 2026-10-27 19:40 CET (slot 295, `swarm/safetensors-mit-alt`): prototype MIT replacement `vendor/safetensors-mit`
removes this offender and the AP-1 exception; see docs/security/safetensors-mit-alt.md (owner decision ST-1).

No other registry package on this tip is Apache-2.0-only.

Also noted (pass via MIT arm today; relevant to AP-4):

| Crate | Version | Licence expression |
|---|---|---|
| wasi | 0.11.1+wasi-snapshot-preview1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wasip2 | 1.0.4+wasi-0.2.12 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| wit-bindgen | 0.57.1 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |

Apache-2.0 **ported source / weights** (not cargo-deny scope; from the memo):
CosyVoice, S3Tokenizer (voice-rust); Kokoro-82M, KittenTTS weights (voice-mouth).
Owner items **AP-2**, **AP-3**.

### B. Wildcard / publish=false offenders (bans warn today)

`wildcards = "deny"` probe fails on **46 workspace crates** that use path deps
without `publish = false`. Only 3 crates already set it on this tip:
`galion-tar`, `galion-deploy`, `edge/openmind-node`.

Warned crates (workspace gate, wildcards=warn): bench-banking, bench-scoreboard,
console-static, desk-hermes, gaia, gaia-aether, gaia-apollo, gaia-artemis,
gaia-core, gaia-demeter, gaia-eleuthia, gaia-hades, gaia-hephaestus, gaia-live,
gaia-minerva, gaia-poseidon, galion-backup, galion-bench, galion-boot,
galion-desk, galion-guard, galion-health, galion-one, galion-origin,
galion-realtest, galion-voice-cpu, galion-wallstreet, guardian-tick, hades-gate,
hermes-bench, hermes-gateway, hermes-rs, hermes-tools, link-watch, one-talk,
openmind-discovery, openmind-mcp-stack, openmind-memory, openmind-nerve,
openmind-queue, openmind-schema, openmind-survival, persona-plex-relay,
repo-check, room, tuner.

Duplicate-version warnings (informational): getrandom, hashbrown, syn.

### C. Licence inventory (registry packages, cargo metadata --locked)

| Count | Expression |
|---:|---|
| 97 | MIT OR Apache-2.0 |
| 29 | MIT |
| 12 | Apache-2.0 OR MIT |
| 6 | MIT/Apache-2.0 |
| 4 | BSD-3-Clause |
| 4 | Unicode-3.0 |
| 4 | Unlicense OR MIT |
| 3 | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| 2 | BSD-2-Clause OR Apache-2.0 OR MIT |
| 2 | Unlicense/MIT |
| 2 | Zlib OR Apache-2.0 OR MIT |
| 1 | (MIT OR Apache-2.0) AND Unicode-3.0 |
| 1 | 0BSD OR MIT OR Apache-2.0 |
| 1 | **Apache-2.0** (safetensors) |
| 1 | MIT OR Apache-2.0 OR BSD-1-Clause |
| 1 | MIT OR Apache-2.0 OR LGPL-2.1-or-later (r-efi; passes via MIT) |
| 1 | MIT OR Zlib OR Apache-2.0 |
| 1 | Zlib |

171 registry packages. Workspace crates: MIT (candle-core vendored MIT OR Apache-2.0).

## Owner decision items (do not decide here)

Carried from `docs/decisions/apache-2-allow-list.md` (AP-1..AP-6) plus gate-local:

| ID | Question | Evidence |
|---|---|---|
| **AP-1** | Confirm option A (keep Apache-2.0-only, today: safetensors) **or** B (narrow to dual MIT\|Apache → drop/replace safetensors)? | deny.toml exception; mit-no-apache probe exit 4, sole reject safetensors 0.7.0 |
| **AP-2** | Apache-2.0 ported source (CosyVoice, S3Tokenizer) allowed in MIT repo? | memo §1 / voice-rust third-party |
| **AP-3** | Apache-2.0 model weights (Kokoro-82M, KittenTTS) as runtime data, not in git? | memo §1 / voice-mouth |
| **AP-4** | Confirm adding `Apache-2.0 WITH LLVM-exception` to RULES + deny allow (already in deny.toml on this branch)? | L7 tooling-licence-memo; wasi/wasip2/wit-bindgen |
| **AP-5** | Require generated NOTICE / third-party bundle in release tarballs before public launch? | publish blockers |
| **AP-6** | RULES.md wording for Apache components + NOTICE (Mate/Slavik only)? | memo §4 |
| **CD-1** | Land `publish = false` on the 66 crates (slot 201) then flip `wildcards = "deny"`? | 49 wildcard warnings this run |
| **CD-2** | Promote safetensors exception → general `"Apache-2.0"` allow (option A) once AP-1 is YES? | keeps edge from warning unmatched exception |

## Not done / out of scope

- No RULES.md edit (owner AP-6).
- No `publish = false` mass fix (slot 201).
- No main merge, no PR, no VPS / cortex / console / nerve deploy.
- No VPS IP in committed files.
