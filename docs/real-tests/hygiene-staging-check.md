<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hygiene staging check — slot 322 (ports 21920-21929)

Throwaway fold, 2026-10-27 22:53-23:03 CET (+01:00). **Not** a main-staging update, **no** deploy.

## What was folded
| Step | Commit | Result |
|---|---|---|
| Base | `3f015986` = tree snapshot of GitHub `swarm/main-staging` `38a07700` (PR #13 merge) on `98466ab9` | tree `4864589f` re-checked 22:54 CET via GitHub API: `swarm/main-staging` head still `38a07700`, tree `4864589f` (identical) |
| Merge 1 | `6b28fd60` --no-ff `swarm/crate-publish-hygiene` `29a717f1` (slot 296) | 0 conflicts |
| Merge 2 | `cd448037` --no-ff `swarm/safetensors-mit-alt` `97141c1c` (slot 295) | 0 conflicts (Cargo.lock, deny.toml auto-merged) |

Both feature branches sit on `400e93a5` (cargo-deny-gate) over `98466ab9`; PR #13 touched only CI/test/script files
(19 files, no `Cargo.toml`), so the two hygiene changes land cleanly.

## Gates (box, `CARGO_TARGET_DIR=/workspace/swarm/target/hygiene-staging-check`, cargo-slot -j 2)
| Gate | Result |
|---|---|
| `cargo deny --offline check licenses bans sources` (cargo-deny 0.20.2, `wildcards = "deny"`, `allow-wildcard-paths = false`, licence exceptions = []) | **bans ok, licenses ok, sources ok** (warn only: duplicate getrandom/hashbrown/syn) |
| `cargo deny check advisories` (fresh DB) | **advisories ok** |
| Negative control: drop `version` from `galion-voice-core` path dep in `crates/galion-voice-cpu/Cargo.toml` (reverted) | `error[wildcard]` → **bans FAILED rc 2** (gate is live) |
| `cargo metadata --no-deps`: 68 workspace packages | 68/68 `publish = []` (false); 0 path deps with `req = "*"` |
| Cargo.lock `safetensors 0.7.0` | no `source` line → patched to `vendor/safetensors-mit` (MIT, publish = false) |
| `cargo fmt --all --check` | ok |
| `cargo clippy --workspace --all-targets --exclude openmind-sandbox --exclude cgroup-detect -- -D warnings` | ok |
| `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` | 379 suites, **2010 passed / 0 failed / 18 ignored** (incl. repo-check `publish_hygiene` 5/5) |
| `cargo test -p safetensors -p galion-voice-cpu -p galion-chatterbox` | 10 + 21 + 14 + 2 + 1 passed, 0 failed |
| `scripts/no-python.sh` | ok |
| `scripts/spdx-scan.sh` | scanned 752, missing 0 |

`hermes-tools` excluded so its `sandboxed.rs` tests never run on the box (sandbox_vps_only; slot 294 slip).

## Real-life tests (release, GALION_REAL=1)
Port remapped 21650 → 21920 in the working tree only for the run (reverted, not committed).

* `real_safetensors_mit` 22:59-23:01 CET: galion-voice-serve on 127.0.0.1:21920, health 200 after 0.95 s
  (stt whisper-tiny.en, tts kitten-tts-nano-0.8, rss 234 MB); STT JFK text exact
  ("And so my fellow Americans ask not what your country can do for you, ask what you can do for your country.");
  TTS 140 044 B, sha256 `c1a6ef38c18e6e10aff58783355cfdc2b43dff821b9a4fae4733bd2dfec53bb9` =
  **byte-identical** to slot 295's upstream-safetensors baseline. 1/1 pass, port free after.
* `real_chatterbox` 23:01-23:02 CET: chatterbox (MIT safetensors) loaded in 7 992 ms, 24 chars → 1.60 s audio,
  Whisper round-trip ok. 2/2 pass.

## VPS (read-only, 22:55 CET)
nerve 8670 `/health` 200, `/state` 200; hermes-rs 8642 `/health` 200; console 8080 `/api/v1/health` 200;
9 galion/hermes/gaia/nerve/cortex units active. Open alert seen in nerve `/state`: inc-865945dd galion-one-mouth
stuck loading 4.2 h (HTTP 503) — unrelated to this fold, already tracked. Cortex not queried for this fold
(no runtime change). Nothing deployed; live services untouched.

## Owner decisions
* **D-322-1**: land both onto `swarm/main-staging` by merging 29a717f1 then 97141c1c onto the real `38a07700`
  (0 conflicts expected). Do NOT fast-forward to this branch: its base `3f015986` is a tree-identical snapshot,
  not the GitHub commit `38a07700`.
* Still pending from the source slots: AP-1 / ST-1..ST-4 (accept clean-room MIT `safetensors` option C) and
  the slot 296 hygiene decisions (publish = false everywhere, wildcards = deny in CI).
