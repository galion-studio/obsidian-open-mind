# Swarm 9 desktop / licence

Lane: licence. Written 2026-11-15T15:31:00+01:00 (Europe/Zagreb).

## Plan

Searched open pull requests titled "Swarm 9 desktop plan" in galion-studio/obsidian-open-mind at 2026-11-15T14:19Z and again at 2026-11-15T15:31+01:00. No such pull request was returned. Work followed the lane brief and the swarm rules.

## What was checked

Source file: `https://raw.githubusercontent.com/galion-studio/galion-operating-system/main/LICENSE`

```
size 1257
sha256 33a955cb5bedbd5a58802766dd6a692f252dd7b3c663809496c21782ec21b22a
```

`cmp` of that download against the repository root `LICENSE` after the copy: identical.

Every tracked `LICENSE` whose sha256 was `3909e2e87365a701b98ffe0c5d69198c940b2d56728b18f3ec459042f4070bcf` (1079 bytes, the old root text) was replaced with that download. Third-party files were left as they were:

- `stack/agents/personas/crew-skills/LICENSE` sha256 `a20126646f93d32a8989c3cf4772d59194f405034f7babe7daebeac22b8ab151`
- `vendor/candle-core/LICENSE` sha256 `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4`

Cargo `license` fields were listed. Workspace and member crates say `MIT` or `license.workspace = true`. `vendor/candle-core` says `MIT OR Apache-2.0`. None misstate MIT, so none were edited.

`scripts/desktop/check-package.sh` is not on `main`. It is on draft pull request #52 (`cursor/desktop-ci-artifacts-c14b`). That branch was not edited. On `main`, `scripts/desktop/verify-local.sh` now compares the packaged `LICENSE` to the repository file and requires line 4 (the last of the four brand lines). `.github/workflows/openmind-desktop.yml` drops that line and expects the checker to exit 1. `scripts/desktop/package-local.sh` copies the repository file and does not append the brand lines again.

REUSE: comply 0.1.1 skips a file named `LICENSE`. `REUSE.toml` still attributes unmarked source to `3000 Galion / galion-studio`, which is what the SPDX headers say. The bulk copyright was not rewritten.

## What changed

- Root `LICENSE` and the identical project copies.
- `repo-check` accepts an MIT heading that is not the first line. The self-eval job pins the sha256, the size, and `Copyright (c) 2026 Galion Labs`, and passes `--copyright "2026 Galion Labs"`.
- The desktop page reads the MIT body from `LICENSE`. A test checks the first four lines against `BRAND_LINES`.
- Brand pack lines that quoted `Copyright (c) 3000 Galion / galion-studio` as the text of `LICENSE` now point at that file (`brand/`, the release kit brand copy, the root README, `gaia/README.md`).

## Tests

Toolchain: `rustc 1.100.0 (0ef1bc386 2026-11-09)`.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo test -p repo-check --locked` | checks 13, deps_pin 12, galion_console_brand 2, real_serde_order 3, all passed |
| `cargo test -p openmind-desktop --locked` | 44 passed, including `license_file_matches_the_brand_lines` |
| `cargo clippy -p repo-check --all-targets --locked -- -D warnings` | exit 0 |
| `cargo clippy -p openmind-desktop --all-targets --locked -- -D warnings` | exit 0 |
| `cargo run -p repo-check --locked -- --copyright "2026 Galion Labs"` | `2026-11-15T15:29:43+01:00`: 2353 files, 0 errors, 93 warnings (utc) -> PASS |
| same with `--copyright "3000 Galion / galion-studio"` | exit 1, 1 error |
| self-eval licence snippet (sha256, 1257 bytes, MIT heading, copyright line, crate `cmp`) | ok |
| `bash -n` on `package-local.sh` and `verify-local.sh` | exit 0 |
| stub tarball through `verify-local.sh` | exit 0; after deleting line 4, exit 1, `last brand line does not match` |
| `comply` 0.1.1 on a copy of this tree without `.git` and without `target/` | 2414 total, 2343 passed, 0 failed, 71 skipped |
| `comply` 0.1.1 on a clean `main` git worktree (`de7480e7`) | 2425 total, 2352 passed, 1 failed, 72 skipped. The failure is `crates/openmind-speech/fixtures/galion_voice_tts.wav` missing a `.license` file. Pre-existing. `git archive main` then comply: 2332 total, 0 failed. Launch-gates uses `git archive`. |

Not run: `cargo test --workspace`, `cargo clippy --workspace`, `scripts/desktop/package-local.sh` (it builds a release binary), `scripts/public-export.sh`.

## Open questions for Maciej

1. Draft #52 still builds a packaged `LICENSE` by appending `scripts/desktop/brand-lines.txt` after the repository file, and its negative test deletes the last line of that file. After this change the four lines are already at the top. That branch was not edited.
2. SPDX headers in source still say `3000 Galion / galion-studio`. This lane did not rewrite them.
3. No Swarm 9 desktop plan pull request was found.
