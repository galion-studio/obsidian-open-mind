<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# SPDX gap

- DESK-ID: openmind.SPDX-SCAN
- Written: 2026-10-19T18:11:29+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Script: `scripts/spdx-scan.sh`

## Status

B42 is not done.

One run of `scripts/spdx-scan.sh` listed every `*.rs` file under `crates/` that lacks a line containing `SPDX-License-Identifier`. The script does not edit files. It exits 0 even when some are missing. It does not add headers, and it does not write `REUSE.toml` or `LICENSES/MIT.txt`.

The stamp above is the time line from that run. The offset is `+02:00`. It is not `Z`.

## Count

| item | value |
| --- | --- |
| scanned `crates/**/*.rs` | 108 |
| missing `SPDX-License-Identifier` | 10 |

## Missing

- `crates/desk-hermes/src/jsonout.rs`
- `crates/desk-hermes/src/main.rs`
- `crates/galion-one/build.rs`
- `crates/galion-one/src/main.rs`
- `crates/link-watch/src/jsonout.rs`
- `crates/link-watch/src/main.rs`
- `crates/openmind-schema/src/lib.rs`
- `crates/room/src/main.rs`
- `crates/zagreb-time/examples/clock.rs`
- `crates/zagreb-time/src/lib.rs`

## Not claimed

B42 is not done. SPDX headers are not on every `.rs` file. `REUSE.toml` was not added. `LICENSES/MIT.txt` was not added. No crate file was edited. Nothing was committed.
