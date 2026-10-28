<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: cortex-s2-a (T2.1 sshd, T2.4 rustsec sensors)

- Written: 2026-10-20 23:30 CEST (+02:00), branch `swarm/cortex-s2-a`, base `swarm/cortex-sensor` e536446.
- New crates in Cargo.lock: **none**. Both crates below were already in the workspace lock; only the
  `openmind-nerve` dependency list gained two lines (`semver`, `toml`).

| what | version | licence (SPDX) | source | used for | why not write it ourselves |
|---|---|---|---|---|---|
| `semver` (runtime dep of openmind-nerve) | 1.0.28 | MIT OR Apache-2.0 | https://crates.io/crates/semver, https://github.com/dtolnay/semver | matching Cargo.lock versions against RustSec `patched` / `unaffected` requirements (`VersionReq::matches`) | it is Cargo's own requirement semantics (pre-release rules, `^`/`~`/`*`, comma lists); a hand-rolled matcher would be a security-relevant source of false negatives. Zero dependencies, pure Rust, already in the lock. Release binary 1 018 704 -> 1 166 448 bytes (+147 744, snapshot 97 216 of that) |
| `toml` (dev-dependency only, used by `examples/rustsec_snapshot.rs`) | 0.8.23 (`workspace = true`) | MIT OR Apache-2.0 | https://crates.io/crates/toml | parsing RustSec advisory front matter once, offline, when the snapshot is regenerated | never in the nerve binary; already a workspace dependency. Transitive: serde_spanned, toml_datetime, toml_edit, toml_write (MIT OR Apache-2.0), winnow (MIT), indexmap/equivalent/hashbrown (MIT OR Apache-2.0) |
| RustSec advisory-db (data, vendored as `crates/openmind-nerve/data/rustsec/advisories.tsv`) | commit e6f94f3760d5e6c54c1b05f204002263bc025524 (2026-10-20 21:24:19 +02:00) | CC0-1.0 | https://github.com/rustsec/advisory-db | offline advisory snapshot embedded in the binary (97 216 bytes, 1281 advisories) | it is the data source itself. Only front-matter facts are kept (id, package, kind, CVSS v3 score computed locally, patched/unaffected requirements, alias ids); no advisory prose is copied |

Licence notes:
- advisory-db is CC0-1.0 **except** advisories imported from the GitHub Advisory Database, which are marked
  `license = "CC-BY-4.0"` in their front matter. CC-BY-4.0 is not on the swarm allowlist, so those **22 advisories are
  excluded** from the snapshot (owner decision D3 in docs/real-tests/cortex-s2-a.md). 25 withdrawn advisories are also skipped.
- Transitive licences checked with `cargo tree -p openmind-nerve -e normal,dev -f '{p} {l}'` on 2026-10-20 23:23 CEST
  (+02:00): all MIT, Apache-2.0, MIT OR Apache-2.0, Unlicense OR MIT (memchr) or (MIT OR Apache-2.0) AND Unicode-3.0
  (unicode-ident). No GPL/LGPL/MPL.
- Regenerate: `git clone --depth 1 https://github.com/rustsec/advisory-db /path/db` then
  `cargo run -p openmind-nerve --example rustsec_snapshot -- /path/db "$(git -C /path/db log -1 --format='%H %cI')" > crates/openmind-nerve/data/rustsec/advisories.tsv`.
  Tests never use the network.
