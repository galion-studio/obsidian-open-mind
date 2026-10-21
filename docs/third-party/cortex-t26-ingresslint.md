<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: cortex-t26-ingresslint (slot 159)

- Written: 2026-10-21 08:20 CEST (+02:00), branch `swarm/cortex-t26-ingresslint`.

No new third-party dependency and no copied code.

| name | version | licence | source | used for | why |
|---|---|---|---|---|---|
| `tunnel-lint` | workspace (path `crates/tunnel-lint`) | MIT (this repo) | merged from `swarm/tunnel-audit-2` 988e47f | ingress parser, origin resolution, rule checks for the `ingress-lint` sensor | already written and tested for the tunnel audit; it has zero dependencies of its own (std only) |

`openmind-nerve` gains one path dependency (`tunnel-lint`). `Cargo.lock` changes by that one line; no
crates.io package was added, so the transitive licence set is unchanged.
