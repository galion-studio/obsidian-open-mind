# Release checklist — Obsidian Open Mind

Target milestone: **2026-10-31**. Every item has an owner, a status and a Europe/Zagreb
timestamp. Status values: `open`, `in progress`, `done`, `blocked`.

Written 2026-10-18T19:06:00+02:00; last updated 2026-10-18T20:55:00+02:00

## 1. Repository

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 1.1 | `README.md` draft written, facts traced to `release/FACTS.md` | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 1.2 | `LICENSE` (MIT) present in the release `repo/` folder | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 1.3 | `LICENSE` (MIT) present in the workspace root and in **every** crate | other worker / owner | open | 2026-10-18T19:06:00+02:00 |
| 1.4 | LICENSE holder line identical in workspace root, crates and release copy. Root `LICENSE` now reads `Copyright (c) 3000 Galion / galion-studio`; `crates/openmind-schema/LICENSE`, `crates/zagreb-time/LICENSE` and `release/repo/LICENSE` still carry the older 2026 holder lines | owner decision | open | 2026-10-18T20:55:00+02:00 |
| 1.5 | `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md` present | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 1.6 | Issue templates and pull request template present | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 1.7 | CI workflow present: fmt, clippy `-D warnings`, test, no-python, cjk-scan, secrets-scan | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 1.8 | Toolchain requirement pinned (`rust-version` or `rust-toolchain.toml`) | other worker / owner | open | 2026-10-18T19:06:00+02:00 |
| 1.9 | Placeholder contact `__CONTACT_EMAIL__` replaced with a real private address | owner | open | 2026-10-18T19:06:00+02:00 |
| 1.10 | Project name and slug: "Obsidian Open Mind", `galion-studio/obsidian-open-mind` in README, SECURITY, PUBLISHING, release workflow, Cargo metadata and release kit | Grok Bot | done | 2026-10-18T20:55:00+02:00 |
| 1.11 | Brand pack in `brand/` (logo, lockups, banner / social preview, icons, `BRAND.md`, `palette.json`, `tokens.css`) | Grok Bot | done | 2026-10-18T20:55:00+02:00 |
| 1.12 | Social preview image uploaded in the repository settings (`brand/banner/banner-1280x640.png`) | owner | open | 2026-10-18T20:55:00+02:00 |

## 2. Facts

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 2.1 | All claims fact-checked: `FACTS.md` 100% traced or removed | Hermes | in progress | 2026-10-18T19:06:00+02:00 |
| 2.2 | `marketing/FACT-CHECK.md` has 0 unverified claims left in non-TODO text | Hermes | open | 2026-10-18T19:06:00+02:00 |
| 2.3 | Every intentionally unproven claim carries an explicit `TODO(verify)` | Hermes | in progress | 2026-10-18T19:06:00+02:00 |
| 2.4 | Claim sources spot-checked by line (`file:line` re-read after the workspace settles) | Hermes | open | 2026-10-18T19:06:00+02:00 |

## 3. Code and CI

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 3.1 | CI green on the release commit | owner (after repo creation) | blocked | 2026-10-18T19:06:00+02:00 |
| 3.2 | `cargo fmt --check`, `clippy -D warnings`, `cargo test` pass locally | other worker | open | 2026-10-18T19:06:00+02:00 |
| 3.3 | No tracked `.py` file anywhere in the repo | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 3.4 | Secrets scan clean (`release/TEACHER/PROGRESS.md` K2) | Hermes | open | 2026-10-18T19:06:00+02:00 |

## 4. Site

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 4.1 | Page weight < 200 KB (`du -cb site/*`) | subagent A / Hermes | open | 2026-10-18T19:06:00+02:00 |
| 4.2 | No frameworks, no CDN, no external requests, no trackers | subagent A / Hermes | open | 2026-10-18T19:06:00+02:00 |
| 4.3 | Accessible: `lang="en"`, alt text, contrast AA, works without JS | subagent A / Hermes | open | 2026-10-18T19:06:00+02:00 |
| 4.4 | GitHub links are `#TODO-repo-url` placeholders until the repo exists | subagent A | open | 2026-10-18T19:06:00+02:00 |

## 5. Approval gate — nothing ships without these

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 5.1 | **Slavik approval: create the public GitHub repo** | Slavik | blocked | 2026-10-18T19:06:00+02:00 |
| 5.2 | **Slavik approval: `git push` / publish the workspace** | Slavik | blocked | 2026-10-18T19:06:00+02:00 |
| 5.3 | **Slavik approval: publish `release/site/`** | Slavik | blocked | 2026-10-18T19:06:00+02:00 |
| 5.4 | **Slavik approval: post the announcement and the short posts** | Slavik | blocked | 2026-10-18T19:06:00+02:00 |
| 5.5 | Approval list mirrored in `release/TEACHER/AWAITING-APPROVAL.md` | Hermes | done | 2026-10-18T19:06:00+02:00 |

## 6. Standing rules re-checked before release

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 6.1 | No real host names, IP addresses or ports in release files | Hermes | open | 2026-10-18T19:06:00+02:00 |
| 6.2 | No secrets anywhere in `release/` (K2 clean) | Hermes | open | 2026-10-18T19:06:00+02:00 |
| 6.3 | No Python in `release/` (K3 clean) | Hermes | done | 2026-10-18T19:06:00+02:00 |
| 6.4 | Every timestamp is Europe/Zagreb with an explicit offset (K5 clean) | Hermes | open | 2026-10-18T19:06:00+02:00 |
| 6.5 | No CJK characters (K6 clean) | Hermes | open | 2026-10-18T19:06:00+02:00 |
| 6.6 | No money-rail integration and no profit or return claims (K7 clean) | Hermes | open | 2026-10-18T19:06:00+02:00 |
| 6.7 | Nothing was pushed, deployed or posted from this session | Hermes | done | 2026-10-18T19:06:00+02:00 |

## Sign-off

| Role | Name | Date (Europe/Zagreb) |
|---|---|---|
| Owner | Slavik | |
| Release lead | Hermes | 2026-10-18T19:06:00+02:00 |

## 7. Launch gates (added 2026-10-20T20:20:00+02:00, swarm/launch-gates)

| # | Item | Owner | Status | Timestamp |
|---|---|---|---|---|
| 7.1 | REUSE 3.3: `REUSE.toml`, `LICENSES/MIT.txt`, binary `.license` companions; lint 0 failed | Grok Bot | done (branch) | 2026-10-20T20:20:00+02:00 |
| 7.2 | `deny.toml` licence allow-list over all transitive deps; `cargo deny` ok | Grok Bot | done (branch) | 2026-10-20T20:20:00+02:00 |
| 7.3 | CycloneDX SBOM from Cargo.lock (cargo-cyclonedx) | Grok Bot | done (branch) | 2026-10-20T20:20:00+02:00 |
| 7.4 | `.gitattributes` export rules + `scripts/public-export.sh` verified tree | Grok Bot | done (branch) | 2026-10-20T20:20:00+02:00 |
| 7.5 | All GitHub Actions pinned to commit SHAs | Grok Bot | done (branch) | 2026-10-20T20:20:00+02:00 |
| 7.6 | SEC-1..SEC-5 rotations, history purge, permissions, tunnel | owner | open | 2026-10-20T20:20:00+02:00 |
| 7.7 | Public repo created from the export tree (fresh history), not by flipping this repo | owner | open | 2026-10-20T20:20:00+02:00 |
| 7.8 | Branch protection, private vulnerability reporting, secret scanning (docs/launch/SCORECARD-READINESS.md) | owner | open | 2026-10-20T20:20:00+02:00 |
