<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code and dependencies: tunnel-audit

**None.** `crates/tunnel-lint` has an empty `[dependencies]` table and uses only
the Rust standard library. No code was copied.

- YAML: no `serde_yaml` (archived/unmaintained upstream) or `yaml-rust2`. The
  cloudflared ingress file uses a tiny, fixed subset (a list of maps with three
  scalar keys plus nested blocks to skip), which a ~150-line hand parser covers
  with tests; that keeps the binary < 450 KB and avoids a parser dependency for
  a lint tool.
- Listener checks: `std::net::TcpStream::connect_timeout`, or parsing `ss -ltn`
  text. No `netstat2`/`procfs` crates.
- Real-life test origin: the workspace's own `tuner` crate (MIT, ours).

`cargo metadata` for the package lists no dependencies, so there are no
transitive licences to check.
