<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: bench-harness (`crates/galion-bench`)

Written 2026-10-20 23:05 CEST (+02:00).

**No new third-party crates, no copied code.** `galion-bench` uses only what the workspace already pins:

| Dependency | Version (lock) | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| serde_json (workspace, `preserve_order`) | 1.0.154 | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parse JSONL fixtures, render bodies | already in the workspace; JSON parsing is not worth re-implementing |
| zagreb-time (in-tree) | 0.1.0 | MIT | crates/zagreb-time | Europe/Zagreb stamps | in-tree |

HTTP server, parser and client are written here on `std::net` (like the other workspace servers), so no `tiny_http`/`httparse`.

Transitive tree (`cargo tree -p galion-bench -e normal`), all allowed: chrono 0.4.45 (MIT OR Apache-2.0), chrono-tz 0.10.4 (MIT OR Apache-2.0), equivalent 1.0.3 (Apache-2.0 OR MIT), foldhash 0.2.0 (Zlib), hashbrown 0.17.1 (MIT OR Apache-2.0), iana-time-zone 0.1.65 (MIT OR Apache-2.0), indexmap 2.14.2 (Apache-2.0 OR MIT), itoa 1.0.18 (MIT OR Apache-2.0), memchr 2.8.3 (Unlicense OR MIT, used under MIT), num-traits 0.2.19 (MIT OR Apache-2.0), phf / phf_shared 0.12.1 (MIT), serde_core 1.0.229 (MIT OR Apache-2.0), siphasher 1.0.4 (MIT OR Apache-2.0), zmij 1.0.23 (MIT).
