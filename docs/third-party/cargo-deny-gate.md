<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# cargo-deny-gate — third-party notes

Docs + `deny.toml` policy only. **No new runtime dependencies** and no copied
code on branch `swarm/cargo-deny-gate`.

## Tools used (not linked / not shipped)

| Name | Version | Licence | Source | Use |
|---|---|---|---|---|
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/cargo-deny | Box-local binary at `/workspace/swarm/tools/bin/cargo-deny`; runs licences/bans/sources/advisories checks against `deny.toml`. Already referenced by `.github/workflows/launch-gates.yml` and `scripts/public-export.sh`. |

## Owner-pending Apache-2.0 exception (config, not a new dep)

| Crate | Version in lock | Licence | Why listed |
|---|---|---|---|
| safetensors | 0.7.0 | Apache-2.0 | Pre-existing transitive dep via candle; named in `deny.toml` `[licenses].exceptions` as OWNER-PENDING AP-1 until Slavik picks apache-allowlist option A or B. |

Cross-refs: `docs/decisions/apache-2-allow-list.md`,
`docs/security/cargo-deny-gate-20261027.md`,
`docs/third-party/apache-allowlist-memo.md`.
