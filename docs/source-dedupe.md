<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Source dedupe

- DESK-ID: openmind.B52
- Generated: 2026-10-19T17:11:42+02:00 (Europe/Zagreb, TZ=Europe/Zagreb date -Iseconds)
- Repo: /opt/galion/openmind
- Scope: old copies named by this ticket (desk-rs, one/src, room, publish copies, desk-hermes) versus the openmind workspace.
- Policy: KEEP. Those trees were listed with find and ls only. Nothing in them was deleted, moved, or edited. This file is the only write.

The source of truth is the crate under /opt/galion/openmind/crates. The workspace member glob is crates/*. A path outside that directory is not the source of truth.

Duplicate Rust roots: 4. desk-hermes has no second Rust tree.

## Inventory

| family | path | kind | files seen | lines | source of truth |
| --- | --- | --- | --- | --- | --- |
| desk-rs | /opt/galion/galion-light/desk-rs | duplicate root | main.rs | 569 | no. truth is /opt/galion/openmind/crates/galion-desk |
| desk-rs | /opt/galion/openmind/crates/galion-desk | crate | src/main.rs, Cargo.toml | 1045 | yes |
| one/src | /opt/galion/galion-light/one/src | duplicate root | main.rs | 2806 | no. truth is /opt/galion/openmind/crates/galion-one |
| one/src | /opt/galion/openmind/crates/galion-one | crate | src/main.rs, src/models_refresh.rs, build.rs, Cargo.toml | 3944 + 350 | yes |
| room | /opt/galion/galion-light/room | duplicate root | src/main.rs | 298 | no. truth is /opt/galion/openmind/crates/room |
| room | /opt/galion/openmind/crates/room | crate | src/main.rs, Cargo.toml | 352 | yes |
| publish | /opt/galion/galion-light/publish/obsidian/one | duplicate root | main.rs | 2026 | no. truth is /opt/galion/openmind/crates/galion-one |
| desk-hermes | /opt/galion/bin/desk-hermes.py | Python predecessor, not a Rust root | desk-hermes.py | 229 | no. truth is /opt/galion/openmind/crates/desk-hermes |
| desk-hermes | /opt/galion/openmind/crates/desk-hermes | crate | src/main.rs, src/jsonout.rs, Cargo.toml | 530 + 120 | yes |

diff -q: every old .rs file differs from the crate it maps to. The publish main.rs also differs from one/src/main.rs. No duplicate is byte-identical to its crate.

## Paths listed

desk-rs (ls): /opt/galion/galion-light/desk-rs contains only main.rs. No Cargo.toml.

one/src (ls): /opt/galion/galion-light/one/src contains only main.rs. find of *.rs under /opt/galion/galion-light/one returned that file only. The parent also holds HTML, Python helpers, and built binaries (one, one.prev, one.prev-boot, one.prev-cortex, one.prev-four, one.prev-heal, one.prev-hearth, one.prev-note, one.prev-pipe, one.prev-score, one.prev-zombies, one.prev2, one.bak-pyramid). Those are not extra Rust sources.

room (ls): /opt/galion/galion-light/room contains index.html, a built binary named room, and src/main.rs. One .rs file.

publish (ls and find): the only .rs under /opt/galion/galion-light/publish is /opt/galion/galion-light/publish/obsidian/one/main.rs. That directory also contains font/, check.sh, FONT.md, mark.svg, DESIGN.md, models.py, and index.html. Sibling publish folders with no .rs: ald-gaa, ald-gaa-live, grok-build, grok-build-paper, the-machine, tuner. /opt/galion/backups/tuner-20261018-191738+0200/publish-tuner has no .rs. /opt/galion/from-grok-build/workspace/galion-light/publish/obsidian/one exists and has no .rs.

desk-hermes (find of the name): the Rust source is only /opt/galion/openmind/crates/desk-hermes. The predecessor is /opt/galion/bin/desk-hermes.py. Hits under /opt/galion/openmind/target and /opt/galion/openmind/dist/bin/desk-hermes are build products, not source.
