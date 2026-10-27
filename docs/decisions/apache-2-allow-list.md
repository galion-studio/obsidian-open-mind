<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Decision memo: Apache-2.0 on the allow-list (for Slavik / Mate)

Written 2026-10-27 17:45 CET (+01:00), branch `swarm/apache-allowlist-memo`, base main-staging 7c95348. Docs only. Nothing is decided here; RULES.md and deny.toml are unchanged.
Listed as an open owner decision in PUBLISH-BLOCKERS-20261027.md item 8.

## Key finding: Apache-2.0 is already allow-listed
- RULES.md "Open-source borrowing": allowed = MIT, **Apache-2.0**, MIT OR Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, 0BSD, CC0.
- deny.toml `[licenses] allow` contains `"Apache-2.0"` (cargo-deny accepts an `OR` expression if any arm is allowed, so Apache-2.0-only crates already pass).
So "allow-list Apache-2.0 alone" is not a rule change. What is actually open is the set of **Apache-2.0 side questions** below (obligations, the LLVM-exception variant, ported code and model weights).

## 1. Apache-licensed deps in tree today
Source: `cargo metadata --locked` on main-staging 7c95348, 2026-10-27 17:40 CET; 171 registry packages.
| Licence expression | Crates |
|---|---|
| MIT OR Apache-2.0 / Apache-2.0 OR MIT / MIT/Apache-2.0 | 97 + 12 + 6 = 115 |
| Other ORs containing Apache-2.0 (Zlib/BSD-2/0BSD/BSD-1/LGPL arms, `(MIT OR Apache-2.0) AND Unicode-3.0`) | 9 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` (wasi 0.11.1, wasip2 1.0.4, wit-bindgen 0.57.1) | 3 |
| **Apache-2.0 only**: safetensors 0.7.0 (transitive via candle, voice) | 1 |
| No Apache arm (MIT, Unlicense OR MIT, Unicode-3.0, BSD-3, Zlib) | 43 |
Vendored workspace crate: candle-core (MIT OR Apache-2.0). All other workspace crates MIT.
Register cross-check: docs/third-party/README.md:53 (`Apache-2.0` count 1), :185 (safetensors), :332 (cargo-cyclonedx 0.5.9 Apache-2.0, CI tool). Apache-2.0 **ported code / weights**: docs/third-party/voice-rust.md:110-111 (CosyVoice, S3Tokenizer ported to Rust), voice-mouth.md:126-127 (Kokoro-82M, KittenTTS weights).
Related prior memos: docs/decisions/licences-tools.md (slot 42; libFuzzer row c), slot 234 `swarm/sec-tooling-licence-memo` de85656 docs/security/tooling-licence-memo.md L4 + L7 (local branch, not on main-staging).

## 2. Open question for Slavik: what changes between options
| Option | Effect |
|---|---|
| A. Keep as is (Apache-2.0 allowed alone, as RULES says) | No change. safetensors stays. We still owe Apache-2.0 §4 duties when we *distribute*: ship the licence text and any upstream NOTICE, and mark modified files. Dual-licensed crates we can take under MIT and skip this. |
| B. Narrow to "dual MIT OR Apache-2.0 only" (current *practice*, not current rule) | safetensors (Apache-only) fails cargo-deny -> candle/voice must drop or replace it; ported CosyVoice/S3Tokenizer code and Apache weights would need replacement or an explicit exception. Requires RULES.md + deny.toml edits. |
| C. A + add `Apache-2.0 WITH LLVM-exception` (slot 234 L7) | Covers LLVM libunwind statically linked into omnode musl builds and the wasi/wit-bindgen arm. Exception only *relaxes* Apache (no attribution needed for compiled-in runtime), so lower risk than plain Apache. One deny.toml line + RULES list entry. |
Practical difference A vs B: under A the MIT repo becomes "MIT, with some Apache-2.0 components" - releases need a THIRD-PARTY/NOTICE bundle (cargo-cyclonedx SBOM exists; NOTICE generation does not yet). Apache-2.0 also has a patent-termination clause (§3) that MIT lacks; it binds us only if we sue a contributor over patents.

## 3. Not decided here
No edit to RULES.md or deny.toml. No new deps.

## 4. Owner decisions needed (yes/no)
- **AP-1** Confirm option A: Apache-2.0-only crates (today: safetensors 0.7.0) stay allowed as RULES.md already says.
- **AP-2** Confirm Apache-2.0 *ported source* (CosyVoice, S3Tokenizer in voice-rust) is allowed in our MIT repo, keeping upstream copyright + NOTICE in those files.
- **AP-3** Apache-2.0 *model weights* (Kokoro-82M, KittenTTS) allowed as runtime data, not vendored in git?
- **AP-4** Add `Apache-2.0 WITH LLVM-exception` to the allow-list (= slot 234 **L7**; also its L4 libFuzzer note).
- **AP-5** Require a generated NOTICE / third-party licence bundle in release tarballs before public launch (ties to publish blockers).
- **AP-6** Write the RULES.md wording (if any) - Mate/Slavik only; proposed text if wanted: "Apache-2.0 components keep their LICENSE/NOTICE; releases ship a third-party notice file."
