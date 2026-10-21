<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Third-party code and dependencies: desk-linkwatch (slot 18)

No new crates.io dependency and no copied code.

| change | name | version | licence (SPDX) | source | why |
|---|---|---|---|---|---|
| new **dev**-dependency of galion-desk (tests only) | serde_json | 1.0.154 (already in the workspace lock) | MIT OR Apache-2.0 | https://github.com/serde-rs/json | parse `/desk` responses in tests. The binary still writes its JSON by hand, as live does |
| feature trim in desk-hermes | rusqlite | 0.32.1 (unchanged) | MIT | https://github.com/rusqlite/rusqlite | `default-features = false` (CODE.md flagged the default features); `bundled` kept. The lock is unchanged because other workspace crates resolve the same graph |
| runtime tool (not linked) | curl | system package (box: 8.14.1) | curl (MIT-style) | https://curl.se | link-watch fetches `https://` watch URLs through `curl`, the same TLS route hermes-rs uses, so no TLS crate enters the tree. Plain `http://` stays in-crate. Only used when `LINK_WATCH_URLS` lists an https URL |

Transitive licences of galion-desk, desk-hermes and link-watch (from
`cargo metadata`): all MIT, Apache-2.0, MIT OR Apache-2.0, Zlib (foldhash),
Unicode-3.0 (unicode-ident), BSD-2-Clause OR Apache-2.0 OR MIT (zerocopy), or
Unlicense OR MIT (memchr, used under MIT). None is GPL/LGPL/AGPL/MPL/SSPL/BUSL.

Test tooling only (not a dependency): the cross checks reused the zig 0.15.2
`cc` wrappers already on the box (`/workspace/swarm/target/ucpu/ucpu-zig/`,
written by the ucpu agent) to compile the bundled SQLite C for aarch64/armv7.
