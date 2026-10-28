<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: nerve-dep-growth (slot 385)

No new dependency and no copied code. Three existing `openmind-nerve`
dependencies became optional behind default-on features (no version change):

| crate | version | licence (SPDX) | source | now behind |
|---|---|---|---|---|
| semver | 1.0.28 | MIT OR Apache-2.0 | https://crates.io/crates/semver | `sensor-rustsec` |
| galion-common | path (workspace) | MIT | crates/galion-common | `sensor-drift` |
| tunnel-lint | path (workspace) | MIT | crates/tunnel-lint | `sensor-ingress-lint` |

The embedded RustSec snapshot (`data/rustsec/advisories.tsv`, CC0-1.0 data
from https://github.com/rustsec/advisory-db) is behind `rustsec-snapshot`.
Size attribution used only binutils already on the box (`size -A`,
`nm -C --size-sort`) and A/B builds; no cargo-bloat install.
Cargo.lock unchanged.
