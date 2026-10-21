<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# Third-party: ci-workflow-rust

No new crates. `crates/xtask` uses only crates already in the workspace lockfile:

| Crate | Version | Licence | Source | Use |
|---|---|---|---|---|
| serde_json | 1 (workspace) | MIT OR Apache-2.0 | https://crates.io/crates/serde_json | read `cargo metadata`, write dist/sbom-cargo.json |
| zagreb-time | path (workspace) | MIT | crates/zagreb-time | Europe/Zagreb timestamp in the SBOM |

Cargo.lock change: one new `[[package]] xtask` entry, no new external packages.

CI tools (downloaded at CI time, not vendored; pins moved from YAML/bash into
crates/xtask/src/tools.rs, versions and sha256 unchanged):

| Tool | Version | Licence | Source |
|---|---|---|---|
| gitleaks | 8.30.1 | MIT | https://github.com/gitleaks/gitleaks |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/cargo-deny |
| cargo-cyclonedx | 0.5.9 | Apache-2.0 | https://github.com/CycloneDX/cyclonedx-rust-cargo |
| comply | 0.1.1 | MIT OR Apache-2.0 | https://crates.io/crates/comply |
| zig | 0.15.2 | MIT | https://ziglang.org |
