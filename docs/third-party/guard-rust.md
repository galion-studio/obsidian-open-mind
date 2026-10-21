<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: galion-guard (swarm/guard-rust)

No code was copied. The sshd filter patterns were re-expressed for the
Rust `regex` crate from the behaviour of fail2ban 1.0.2 `filter.d/sshd.conf`
as printed by `fail2ban-client get sshd failregex` on the VPS (fail2ban is
GPL-2.0; its source was read on the VPS only to learn behaviour: window
rescaling, repeat-offender weighting, history purge; the Rust code is our own
and differs structurally). The civil-date algorithm in `timefmt.rs` is Howard
Hinnant's public-domain `days_from_civil`, re-implemented.

## New crate dependencies

| Crate | Version | Licence (SPDX) | Source | Used for | Why not write it |
|---|---|---|---|---|---|
| regex | 1.13.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex | the 22 sshd failregex + prefregex | a correct, linear-time regex engine is the job; hand-written matchers for 22 rules would be a second, untested filter. Built with `default-features = false, features = ["std", "perf", "unicode-perl"]`. |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex | (transitive, regex engine) | - |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex | (transitive, parser) | - |
| aho-corasick | 1.1.5 | Unlicense OR MIT (MIT taken) | https://github.com/BurntSushi/aho-corasick | (transitive, literal prefilter) | - |
| memchr | 2.8.3 | Unlicense OR MIT (MIT taken; already in the workspace lock) | https://github.com/BurntSushi/memchr | (transitive) | - |

Already in the 13a296c workspace and reused: serde, serde_json, toml,
zagreb-time (chrono, chrono-tz). Licences checked with `cargo metadata`.

## Runtime programs (not linked, not shipped)

| Program | Licence | Use |
|---|---|---|
| `nft` (nftables) | GPL-2.0 | firewall backend, executed as a separate process (`nft -f -`); see docs/guard-rust.md 2.3 for why not a Rust netlink crate |
| `journalctl` (systemd) | LGPL-2.1+ | journald source, separate process |

## Test-only

* Debian `nftables` 1.1.3, `libnftables1`, `libnftnl11`, `libjansson4`,
  `libedit2`, `libbsd0`, `libmd0` .debs, downloaded with `apt-get download`
  and unpacked with `dpkg -x` under the target dir by
  `crates/galion-guard/tests/real_netns.rs` to run real nft inside a user namespace. Not
  committed, not shipped.

Crates evaluated and rejected: rustables (GPL-3.0-or-later), nftnl / mnl
(FFI to GPL/LGPL C libs), nftables (wraps the nft binary anyway), neli /
netlink-packet-netfilter (no nf_tables support), systemd / journald
(LGPL), sd-journal (AGPL), systemd-journal-reader 0.1 (no live follow).
