<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Decision memo: fail2ban vs Rust nftables guard (slot 283, inventory item 6 [RULING])

Prepared 2026-10-27 17:55 CET (+01:00). Owner decision for Slavik; this memo does NOT decide.

## Current state (read-only probe of the VPS, 2026-10-27 17:54 CET +01:00)
Commands: `fail2ban-client status`, `fail2ban-client status sshd`, `fail2ban-client get sshd {bantime,maxretry,findtime,actions}`,
`nft list tables`, `nft list table inet f2b-table` (IP lines filtered out), `systemctl show fail2ban`. No writes, no ban/unban.

| Item | Value |
|---|---|
| fail2ban version | 1.0.2 (Debian package, Python) |
| Service | active since 2026-10-22 22:46:47 CET (+01:00); ~21.5 MiB RSS |
| Jails | 1: `sshd` (config: `jail.d/defaults-debian.conf` + `jail.d/galion-sshd.local`) |
| sshd: total failed / currently failed | 1320 / 0 |
| sshd: total banned / currently banned | 68 / 3 |
| bantime / findtime / maxretry | 3600 s / 600 s / 5 |
| Action | `nftables` → `table inet f2b-table`, set `addr-set-sshd` (ipv4_addr), chain `f2b-chain` hook input prio filter-1, `tcp dport 22 ip saddr @addr-set-sshd reject` |
| DB purge age | 86400 s |
| Other nft tables | `inet filter`, `ip filter`, `ip6 filter`; legacy iptables f2b rules: 0 |

Banned IPs are deliberately not recorded here. Note: the set is IPv4-only, so IPv6 attackers are not banned.

## Options
**A. Keep fail2ban (status quo).** Pros: works, mature, zero migration risk, already on nftables. Cons: it is the
only Python service left in the security path, so it violates the long-term Rust-only goal; ~21 MiB; IPv4-only set today.

**B. Replace with a Rust guard (`galion-guard`).** Tail journald for sshd (`journalctl -o json -f -u ssh` as a
child process, or read the journal natively), regex-match failures, keep a findtime window, and add elements with
timeouts to an nft set (`flags timeout`, so the kernel expires bans and no unban loop is needed). Reusable MIT/Apache crates:
`nftables` 0.6.3 (JSON API over `nft -j`), `nftnl` 0.9.4 + `mnl` 0.3.1 (netlink, Mullvad), `regex`,
`systemd-journal-logger` 2.2.2 (logging only). Rejected on licence: `rustables` (GPL-3.0), `sd-journal` (AGPL-3.0).
Pros: Rust-only, small and auditable, can feed ban events to the nerve/cortex, dual-stack sets. Cons: new code in a
security-critical path; a bug could lock out SSH access (needs an allowlist for admin IPs plus console access tested);
Python fail2ban needs a careful cut-over; it is a VPS unit change (needs separate approval).

**C. Hybrid / staged.** Ship `galion-guard` first in **observe-only** mode (it parses the journal and reports would-ban
counts to the nerve, with no nft writes) next to fail2ban for about 7 days. Compare against the `fail2ban-client` totals,
then switch the action over and remove fail2ban. Pros: evidence before any risk. Cons: slower, and two parsers run for a while.

Low-cost improvement for any option: add an IPv6 set/jail (fail2ban `nftables[type=multiport]` with ip6, or dual sets in the guard).

## Owner decision items
- **D-283-1**: Choose A / B / C. (Suggested for consideration: C, because it has the lowest risk and still reaches Rust-only.)
- **D-283-2**: Netlink (`nftnl`/`mnl`, no subprocess) vs `nft -j` JSON (`nftables` crate, simpler, needs the nft binary).
- **D-283-3**: Admin allowlist source (which IPs/CIDRs are never banned), stored outside the repo.
- **D-283-4**: Ban policy: keep 3600/600/5, or add escalating bans (repeat offenders) and IPv6 coverage.
- **D-283-5**: Approve a future VPS unit change window (install guard / disable fail2ban). It is NOT done in this slot.
