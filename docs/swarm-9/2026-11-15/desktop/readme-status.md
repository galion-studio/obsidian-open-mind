# Swarm 9 desktop / readme-status

Lane: readme-status. Swarm: desktop. Written 2026-11-15T15:28:16+01:00 (Europe/Zagreb).

## Plan

Searches for an open pull request titled "Swarm 9 desktop plan" returned 0 items:

- `repo:galion-studio/obsidian-open-mind is:open "Swarm 9 desktop"`
- `repo:galion-studio/obsidian-open-mind is:open "Swarm 9"`

`docs/swarm-9/` was not on `main`. Work followed the lane text in the task: refresh `README.md`, `docs/STATUS.md`, and `docs/crate-index.md` from `main`, and leave `LICENSE` alone.

## Checked

- `git fetch origin main`. Local `HEAD` and `origin/main` were both `de7480e71ce98191d2ee11f018fa15be6c5a1707` (2026-11-15 01:38:46 +0100, "Console: bring the connectors screen onto the current shell (#53)").
- `GET /repos/galion-studio/obsidian-open-mind`: `private` false, `visibility` public.
- `GET /repos/galion-studio/obsidian-open-mind/releases`: empty list.
- `cargo metadata --no-deps --format-version 1 --locked --offline`: exit 0, Cargo 1.83.0, 75 workspace packages.
- GitHub Actions on that commit: self-eval run 43659205932 success (16 jobs, including `cargo test --workspace --locked`); openmind-desktop run 43659205942 success (4 jobs); miri run 43659205926 success; scorecard run 43659206156 success. A later scheduled miri run 43686845489 on the same commit also succeeded.
- Workflow triggers, re-read from the files on this checkout: `launch-gates.yml` and `realtest.yml` run on `pull_request` to `main` and `swarm/main-staging`, and on `workflow_dispatch`. `sbom.yml` is `workflow_dispatch` only. `release.yml` runs on tags `v*`. The earlier note that launch-gates and realtest were dispatch-only was wrong. None of the four was a completed run on the push of `de7480e`. None was dispatched by hand.
- Local rustc 1.83.0 failed `cargo test -p openmind-desktop --locked` with `E0658` on `is_multiple_of` in `galion-common`.
- rustc 1.100.0 (`0ef1bc386`, 2026-11-09): `cargo test -p openmind-desktop --locked` 43 passed, 0 failed. `cargo build -p openmind-desktop --locked` finished. `--help` and `--version` exited 0. No flags wrote a 59313-byte page and exited 0 (6 staged, 4 planned). `--listen` was not used.
- `cargo fmt --all -- --check` and `cargo fmt --manifest-path edge/openmind-node/Cargo.toml --all -- --check` with rustc 1.100.0: both exit 0.
- `scripts/status-percent.sh` after the status edit, 2026-11-15T15:28:16+01:00, and again at 2026-11-15T15:28:53+01:00: 89 items, D 54, P 12, N 2, O 21, weight 122. Matches the fence in `docs/STATUS.md`.
- `find` for `vite.config.*` and `package.json`: no paths.
- `LICENSE` still starts `MIT License` / `Copyright (c) 3000 Galion / galion-studio`. Not edited.

## Changed

- `README.md`: dropped the 2026-10-31 pre-release as the current status. Pointed at the 75-package index. Desktop listen `127.0.0.1:18181`, console probe `18080`, stack rows staged or planned.
- `docs/crate-index.md`: regenerated from the metadata JSON. Appendix for `Cargo.toml` files the metadata run did not return.
- `docs/STATUS.md`: measured section for 2026-11-15. Backlog letters unchanged. Swarm-branch tables marked as the 2026-10-21 record.
- This notes file.

## Not run

- `cargo test --workspace` locally.
- `cargo clippy --workspace` locally.
- `edge/openmind-node` tests locally.
- A REUSE compliance command.
- Any bind, including `--listen`.
- Any host or production check. The October "live" rows were not re-measured.

## Review on pull request 60

2026-11-15T15:35:57+01:00 (Europe/Zagreb). Comment https://github.com/galion-studio/obsidian-open-mind/pull/60#issuecomment-6896168353. Both items checked against files, then edited. `scripts/status-percent.sh` still matched the fence.

- `docs/hermes-mcp.md` is absent on this branch. It was read from draft #51 head `ff9d400387f598c1c2091f84ad19c3544f9dc962`. The README now has one pointer: `HERMES_RS_MCP_GITHUB=1` is opt-in and read-only, and the contract is that file. The file text does not name ports 18181 or 18080. Those ports stay on the desktop sentences.
- The workflow trigger sentences in `README.md` and `docs/STATUS.md` now match the `on:` blocks.

## Open questions for Maciej

- The 89 backlog letters were left as the 2026-10-21 audit. Gaia crates and `openmind-desktop` are on `main`. Re-scoring those letters needs a decision; this lane did not guess.
- FE-1 still says the console runs vite. No `vite.config.*` or `package.json` was found. The letter stays N until someone re-audits the item.
- `LICENSE` is still the short MIT text with `Copyright (c) 3000 Galion / galion-studio`. The licence lane owns that file.
- The October swarm-branch SHA tables were not rebuilt. Say if they should move to an archive doc.
- No "Swarm 9 desktop plan" pull request was visible when this lane started.
