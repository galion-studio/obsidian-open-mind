<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party material: swarm/launch-gates

Written 2026-10-20T20:15:00+02:00 (Europe/Zagreb).

No Cargo dependency was added to any crate. The tools below are build/CI
tools only; none is linked into or shipped with a binary. All were downloaded
as release binaries with a sha256 check, or built from crates.io with
`--locked`, into `/workspace/swarm/target/launch-gates/tools/bin` on the box.

| Name | Version | Licence (SPDX) | Source | Used for | Why not write it ourselves |
|---|---|---|---|---|---|
| gitleaks | 8.30.1 | MIT | https://github.com/gitleaks/gitleaks (release `gitleaks_8.30.1_linux_x64.tar.gz`, sha256 `551f6fc8...70eb`, full hash in launch-gates.yml) | secret scan of the export tree and history | ~200 maintained rules; already the CI secret gate in `self-eval.yml` |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/cargo-deny (musl release, sha256 `9f12ed4c...8d5f`, full hash in launch-gates.yml) | licence/ban/source/advisory policy over the full dependency graph | full SPDX expression evaluation, licence-file confidence scoring, RustSec DB |
| cargo-cyclonedx | 0.5.9 | Apache-2.0 | https://github.com/CycloneDX/cyclonedx-rust-cargo (release `cargo-cyclonedx-x86_64-unknown-linux-gnu.tar.xz`, sha256 `fb8dbee9...f6d8`, full hash in launch-gates.yml) | CycloneDX 1.5 SBOM from Cargo.lock | the reference CycloneDX implementation for Cargo, purl + hashes + licences |
| comply | 0.1.1 | MIT OR Apache-2.0 | https://crates.io/crates/comply, https://github.com/elioseverojunior/comply (`cargo install --locked`) | REUSE 3.3 lint in Rust (the FSFE `reuse` tool is Python, which the rules forbid) | implementing the REUSE spec (REUSE.toml precedence, globbing, comment styles, SPDX expression parsing, licence inventory) is weeks of work; its own transitive deps pass our `deny.toml` licence check |
| MIT licence text | SPDX License List | MIT text (list data CC0-1.0) | https://github.com/spdx/license-list-data/blob/main/text/MIT.txt | `LICENSES/MIT.txt` (REUSE requires the SPDX text) | it is the canonical text |
| GitHub Actions (pinned, unchanged versions) | see workflows | MIT (checkout, upload-artifact, codeql-action, rust-toolchain), Apache-2.0 (cosign-installer, scorecard-action, sbom-action) | github.com/actions/*, dtolnay/rust-toolchain, ossf/scorecard-action, github/codeql-action, anchore/sbom-action, sigstore/cosign-installer | CI | existing usage; this branch only pinned them to commit SHAs |

Third-party content already in the tree that is now declared in `REUSE.toml`
(not added by this branch):

| Path | Upstream | Licence | Copyright |
|---|---|---|---|
| `stack/agents/personas/crew-skills/**`, `stack/agents/teams/crew-skills/**` | https://github.com/alirezarezvani/claude-skills | MIT | 2025 Alireza Rezvani |
| `stack/research-agent/hermes/skills/**` | https://github.com/NousResearch/hermes-agent | MIT | 2025 Nous Research |
| `stack/agents/workers/galion-light/skills/subagent-driven-development/**` | https://github.com/obra/superpowers via hermes-agent | MIT | 2025 Jesse Vincent; 2025 Nous Research |
