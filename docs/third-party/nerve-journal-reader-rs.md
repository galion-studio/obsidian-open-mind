<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-journal-reader-rs

Slot 282 / inventory item 9 (nerve `journalctl -f` child). Research-first, reuse MIT/Apache crates. Feature-flagged prototype only; default path unchanged. NO deploy.

## Choice

| need | crate / tool | version | SPDX | source | why |
|---|---|---|---|---|---|
| in-process journal follow | **`sdjournal`** | 0.1.22 | MIT | https://crates.io/crates/sdjournal · https://github.com/lvillis/sdjournal | Pure Rust `*.journal` reader + `LiveJournal` live tail, cursor resume, no libsystemd, no `journalctl`. Matches “reuse first” and allow-list. |
| (default, unchanged) | `journalctl` (OS) | systemd 257 on box / VPS | LGPL-2.1+ (OS tool, separate process) | distro | Live nerve still spawns this unless feature + env select native. |

Optional feature on `openmind-nerve`: `journal-native` → dep `sdjournal = "=0.1.22"` (default features: `mmap`, `lz4`, `zstd`). Runtime sets `MmapPolicy::Never` and `max_open_files = 8` for a low-RSS follower.

## Rejected

| candidate | licence / issue | why not |
|---|---|---|
| `systemd` / `journald` crates | LGPL (bindings to libsystemd) | RULES.md allow-list forbids LGPL |
| `sd-journal` | AGPL | forbidden |
| `libsystemd` | LGPL | forbidden |
| `journald-query` 0.1.1 | MIT OR Apache-2.0 API, but wraps systemd journal C API (libsystemd) | pulls LGPL into the process; also host/unit-centric API less suited to MESSAGE_ID OR groups |
| `systemd-journal-reader` 0.1 | Apache-2.0 | young; no live follow (already noted in `docs/guard-rust.md` / `docs/third-party/guard-rust.md`) |
| `zbus` + systemd D-Bus | MIT — heavy (~40 crates, async runtime) | nerve-event-driven already rejected this for unit transitions; journal carries the same events |
| write our own `*.journal` parser | — | sdjournal already does it under MIT; reuse first |

## Transitive (sdjournal 0.1.22 defaults)

| crate | role | SPDX (crates.io / upstream) |
|---|---|---|
| `rustix` (linux) | inotify / poll for live | MIT OR Apache-2.0 |
| `siphasher` | hashing | MIT OR Apache-2.0 |
| `memmap2` (feature `mmap`) | optional mmap; we set Never at runtime | MIT OR Apache-2.0 |
| `lz4_flex` (feature `lz4`) | LZ4 DATA payloads | MIT |
| `ruzstd` (feature `zstd`) | zstd DATA payloads | MIT OR Apache-2.0 |

No openssl. `tokio` / `tracing` / `verify-seal` features left off.

## Wiring

- Compile: `cargo build -p openmind-nerve --features journal-native`
- Select at runtime: `NERVE_EVT_JOURNALCTL=native` (system journals) or `native:/path/to/journal/dir`
- Default / unset: unchanged `journalctl -f -o json` child
- `replay:PATH` still works (tests)

## Owner decisions

1. **Flip live nerve to native?** Prototype only. Live stays on journalctl until an owner-approved deploy compares VPS RSS/CPU and cursor behaviour under load.
2. **Cursor portability:** sdjournal cursors are not journalctl `__CURSOR` strings. Mixing backends across a restart needs a cleared cursor (document / migrate).
3. **Default feature?** Left off so the default workspace build stays dep-minimal (RULES.md).
