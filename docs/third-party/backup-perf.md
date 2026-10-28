<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: backup-perf (wave 4, slot 119)

No new dependencies and no copied code. The CRC-32 slicing-by-8 tables in
`crates/galion-backup/src/gzip.rs` are our own implementation of the
well-known public technique (Kounavis & Berry, Intel, 2005); no source was
copied. Existing deps unchanged: rusqlite 0.32 (MIT), miniz_oxide 0.8
(MIT OR Zlib OR Apache-2.0), libc 0.2 (MIT OR Apache-2.0).
