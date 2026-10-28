<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Third-party: publish-gates-main (slot 428)

No new crate dependency. `repo-check` stays std + zagreb-time.

Tools run (not vendored, not linked):

| tool | version | licence | source | use |
|---|---|---|---|---|
| gitleaks | 8.30.1 | MIT | https://github.com/gitleaks/gitleaks | gates 6 + history scan |
| cargo-deny | 0.20.2 | MIT OR Apache-2.0 | https://github.com/EmbarkStudios/cargo-deny | gate 8 |
| comply | box build | MIT OR Apache-2.0 | (see docs/decisions/licences-tools.md) | gate 7 REUSE |
| cargo-cyclonedx | 0.5.9 | Apache-2.0 | https://github.com/CycloneDX/cyclonedx-rust-cargo | gate 9 (cargo install --locked into the slot target dir) |
