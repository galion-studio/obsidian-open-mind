<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party research: fail2ban-nft-memo (slot 283)
Research only; no dependency added. crates.io API read 2026-10-27 17:55 CET (+01:00).

| Crate | Version | Licence | URL | Use | Verdict |
|---|---|---|---|---|---|
| nftables | 0.6.3 | MIT OR Apache-2.0 | https://crates.io/crates/nftables | nft JSON ruleset/set ops | allowed |
| nftnl | 0.9.4 | MIT OR Apache-2.0 | https://crates.io/crates/nftnl | netlink nftables (Mullvad) | allowed |
| mnl | 0.3.1 | MIT/Apache-2.0 | https://crates.io/crates/mnl | netlink socket for nftnl | allowed |
| systemd-journal-logger | 2.2.2 | MIT OR Apache-2.0 | https://crates.io/crates/systemd-journal-logger | write logs to journald | allowed |
| rustables | 0.9.0 | GPL-3.0-or-later | https://crates.io/crates/rustables | netlink nftables | NOT allowed |
| sd-journal | 0.1.0 | AGPL-3.0-or-later | https://crates.io/crates/sd-journal | journal reading | NOT allowed |

Transitive licences must be checked with `cargo deny check licenses` if B/C is approved. nftnl/mnl link libnftnl/libmnl (GPL C libs, dynamic system libs), so check this before choosing netlink; the `nftables` crate shells out to `nft` and links nothing.
