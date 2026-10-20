<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# SPDX gap

- DESK-ID: openmind.SPDX-3
- Written: 2026-10-19T21:04:55+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: regular *.rs files under crates/

## Status

Every regular *.rs file under crates/ was checked again. A file is missing when the text SPDX-License-Identifier does not appear in its first 5 lines. Lines after line 5 do not count as a header. No .rs file was edited. Cargo.toml was not edited. Nothing was committed. Nothing was pushed.

The nine files from the previous list now have headers. REUSE.toml is still not added.

The stamp above is the time of this scan. The offset is numeric (+02:00 or +01:00). It is not a trailing UTC designator.

## Count

| item | value |
| --- | --- |
| scanned crates/**/*.rs | 119 |
| missing SPDX-License-Identifier in the first 5 lines | 0 |

## Missing

No path is missing.

## Previous list

These nine files were missing on the SPDX-2 scan. Each now has SPDX-License-Identifier in its first 5 lines:

- `crates/desk-hermes/src/jsonout.rs`
- `crates/desk-hermes/src/main.rs`
- `crates/galion-one/build.rs`
- `crates/galion-one/src/main.rs`
- `crates/link-watch/src/jsonout.rs`
- `crates/link-watch/src/main.rs`
- `crates/room/src/main.rs`
- `crates/zagreb-time/examples/clock.rs`
- `crates/zagreb-time/src/lib.rs`

## Not claimed

The nine files from the previous list now have headers. REUSE.toml is still not added. LICENSES/MIT.txt was not added. No crate file was edited. Cargo.toml was not edited. Nothing was committed. Nothing was pushed.
