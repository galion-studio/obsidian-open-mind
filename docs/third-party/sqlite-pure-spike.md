<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# sqlite-pure-spike: third-party dependencies (spikes only, not shipped)

Reviewed on 2026-10-20 at 22:05 +02:00 (Europe/Zagreb). Every package lives
under `spikes/` with its own `[workspace]` and `Cargo.lock`. Nothing is
added to the main workspace.

| name | version | SPDX | URL | use | why |
|---|---|---|---|---|---|
| turso_core | =0.8.3 | MIT | https://github.com/tursodatabase/turso | `spikes/sqlite-pure`: candidate pure-Rust SQLite engine, option (a) | the only maintained SQLite-file-compatible Rust engine |
| redb | 2.6.4 | MIT OR Apache-2.0 | https://github.com/cberner/redb | `spikes/store-b`: row store, option (b) | pure Rust, single file, ACID, no C |
| tantivy | 0.26.2 (`default-features = false`, `lz4-compression`, `mmap`) | MIT | https://github.com/quickwit-oss/tantivy | `spikes/store-b`: BM25 full-text search, option (b) | the FTS5 replacement; the default features would pull zstd-sys (C) |
| rusqlite | 0.32.1 (`bundled`) | MIT | https://github.com/rusqlite/rusqlite | `spikes/sqlite-pure/fixtures`: host-only oracle that creates fixtures and runs `integrity_check` with real SQLite | it is the reference the engine under test is compared with; the version and features are the ones main already uses |

Transitive licences (`cargo tree -e normal,build --format '{l}'`) are all on
the allowlist or are a choice that includes an allowlisted licence:
- MIT, Apache-2.0, BSD-2/3-Clause, ISC (libloading), Zlib, Unicode-3.0 and
  Unlicense-or-MIT;
- `Apache-2.0 OR BSL-1.0` (ryu);
- `Apache-2.0 / MIT / MPL-2.0` (htmlescape);
- `zlib-acknowledgement OR MIT` (fastdivide).

`cfg_block` 0.1.1 has no SPDX field; its `license-file` is Apache-2.0.
No copyleft-only licence appears.
