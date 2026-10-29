<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party: hermes-tools-restore-20261029

- Written: 2026-10-29 (Europe/Zagreb, CET +01:00).
- New Cargo dependencies: **none**. The tools use our own `openmind-sandbox` (MIT), `std::fs`, the existing `CurlProvider`, and the Playwright-installed `chrome-headless-shell` binary (run as a CLI, not linked).
- Runtime binary (not a crate): Chromium headless shell from Playwright (`npx playwright install chromium-headless-shell`), BSD-3-Clause (Chromium). It is found under `~/.cache/ms-playwright`.

## Crates considered (steering t414u)

The bar: MIT/Apache-2.0, maintained, solid code, few deps, and it fits the deny-list hook.
The size budget went from 2 MiB to 200 MB (t416u), so size is no longer a reason to reject. The verdicts below stand on licence, deps, async runtime and hook fit.

| Need | Crate | Licence | Verdict |
| --- | --- | --- | --- |
| browser | chromiumoxide 0.9.1 | MIT/Apache | rejected: tokio plus the full generated CDP API; we need one page per call |
| browser | fantoccini | MIT/Apache | rejected: needs a WebDriver server, tokio/hyper |
| browser | playwright-rs 0.20.1 | Apache-2.0 | rejected: tokio plus a Node driver process |
| tools / MCP | modelcontextprotocol 0.2.4 | MIT | rejected: new, no command filter, no SSRF guard |
| tools / MCP | rmcp (rust-sdk) | MIT/Apache | protocol SDK only, no tools |
| shell sandbox | Chisel (ckanthony) | no LICENSE file | rejected on licence |
| files + shell | fs-mcp-rs 1.2.5 | GPL-3.0-or-later | rejected on licence; terminal unsandboxed |
| shell sandbox | codex-linux-sandbox | Apache-2.0 | rejected: tokio, many deps; openmind-sandbox already does bwrap-style ns + seccomp + landlock |
| agent | goose | Apache-2.0 | rejected: a full tokio app |
| agent | rig | MIT | LLM framework, no sandboxed tools |
| agent | swiftide | MIT | tokio, docker executor |
| agent | agentkit | MIT/Apache | reqwest/rmcp, too heavy |

Picked: our own code behind the deny list and path confinement.
