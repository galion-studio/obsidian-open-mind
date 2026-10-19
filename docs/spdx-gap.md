<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# SPDX gap

- DESK-ID: openmind.SPDX-2
- Written: 2026-10-19T18:49:14+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Scope: regular *.rs files under crates/

## Status

B42 is not done.

Every regular *.rs file under crates/ was checked. A file is missing when the text SPDX-License-Identifier does not appear in its first 5 lines. Lines after line 5 do not count as a header. No .rs file was edited. Cargo.toml was not edited. Nothing was committed. Nothing was pushed.

The stamp above is the time of this scan. The offset is numeric (+02:00 or +01:00). It is not a trailing UTC designator.

## Count

| item | value |
| --- | --- |
| scanned crates/**/*.rs | 119 |
| missing SPDX-License-Identifier in the first 5 lines | 9 |

## Missing

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

B42 is not done. SPDX headers are not on every .rs file. REUSE.toml was not added. LICENSES/MIT.txt was not added. No crate file was edited. Cargo.toml was not edited. Nothing was committed. Nothing was pushed.

