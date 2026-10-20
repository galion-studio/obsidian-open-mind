<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# OpenSSF status

- DESK-ID: openmind.B45
- Generated: 2026-10-19T16:28:29+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- HEAD: `783cc1c` on main (author date 2026-10-19 14:26:46 +0000, message stamp 2026-10-19T16:26:46+02:00). First read was `ae72938`. Counts below were taken again at the Generated time.
- Security contact: TODO
- This file is a status map. It is not a Scorecard score and not a Best Practices badge.

No Scorecard binary was run. scorecard.dev was not queried. bestpractices.dev was not given a project entry. No crate was edited. Nothing was committed. No secret value is written here.

The copyright line above is the line already in `LICENSE`. It is not a security contact.

## Verdict

Not done.

The workspace has an MIT license in git, a written `SECURITY.md` that is not in git, and untracked workflow drafts for tests, gitleaks, Scorecard, an SBOM, and keyless signing. None of those drafts are on `HEAD`. This checkout has no git remote and no tag. There is no Scorecard result and no Best Practices badge.

Status words used below:

- done: the evidence is in `HEAD`, or it is a fact of this checkout.
- draft: the file is on disk and `git status` shows it untracked, or the text describes a step that has not run.
- not done: absent, or present in a form that does not meet the check.
- not verified: this ticket did not run the check, and the tree has no result file.

## SECURITY.md review

Read: `/opt/galion/openmind/SECURITY.md` (untracked; `git status --short -- SECURITY.md` prints `?? SECURITY.md`). It is not in `git ls-files`. A Scorecard run against `HEAD` would not see it.

What the file already says, and what was checked:

| Item | Status | Evidence |
| --- | --- | --- |
| File exists at the repo root | draft | Present on disk. Not committed. |
| Supported versions | done as text | Table says `0.1.x` yes, anything older no. `Cargo.toml` `[workspace.package] version = "0.1.0"`. Matches the only version line in this workspace. No older release exists here. |
| Do not use a public issue for a vulnerability | done as text | First line under "Reporting a vulnerability". |
| GitHub private advisory URL | not verified | The file names `https://github.com/galion-studio/open-mind-stack/security/advisories/new`. This checkout has no `git remote`. `docs/goals.md` (priority dated 2026-10-18 19:01 CEST) says the public repo and the first publish still need approval, and that a GitHub login on the build machine is not set up. `TEACHER/BACKLOG.md` LAUNCH-1 names a different repo, `galion-studio/obsidian-open-mind`, status not started. The URL was not confirmed from this host. |
| `docs/PUBLISHING.md` | not done | `SECURITY.md` and `.github/workflows/self-eval.yml` both point at it. `ls docs/PUBLISHING.md` fails. The sentence that private reporting "is switched on ... when the repository goes public" is a plan, not evidence that a setting is on. |
| Private contact | TODO | Was the placeholder `__CONTACT_EMAIL__`. No real address was in the file. This ticket replaced that token with the word TODO. No email and no person were added. |
| What to include, and the expected reply | done as text | Crate and version, impact, reproduction, whether already public. Acknowledgement, assessment, then a fix or a reasoned "will not fix". No numeric response window. |
| Out of scope | done as text | Third-party dependencies, hosting, and anything the maintainers do not ship. |
| Key handling claim | done for the schema types that were read | `crates/openmind-schema/src/lib.rs`: `Provider.key_slot` at line 187, `KeySlot` fields `name`, `present`, `hint` at lines 193-196. Tests assert `!raw.contains("sk-")` at lines 415, 436, 513, 573, and 616. The policy text says "no key-shaped string". The assertions that were read check the substring `sk-` only. |

`release/repo/SECURITY.md` is an older kit copy. It still says `__CONTACT_EMAIL__`, and it has no advisory URL. This ticket did not edit `release/`. The kit is not the workspace policy.

`release/repo/CODE_OF_CONDUCT.md` also still says `__CONTACT_EMAIL__`. The workspace root has no `CODE_OF_CONDUCT.md` and no `CONTRIBUTING.md`. Those kit files do not make a root contact exist. Contact remains TODO.

## Goal items named in docs/goals.md

`docs/goals.md` asks for an inventory of LICENSE, SECURITY.md, OpenSSF Scorecard, Best Practices badge, REUSE, SBOM, signed releases, and CI secret scanning. This table is that inventory for this workspace only.

| Item | Status | Evidence |
| --- | --- | --- |
| OSI MIT LICENSE | done | `LICENSE` is tracked. First line is `MIT License`. Grant text is present. `Cargo.toml` has `license = "MIT"` under `[workspace.package]`. MIT is an OSI license. This row is this repo, not "every repo". |
| SECURITY.md | draft | Written, contact TODO, not in `HEAD`. See the review above. |
| OpenSSF Scorecard | not done | No `results.sarif`. No badge in `README.md`. Workflow draft is untracked. Not executed. |
| OpenSSF Best Practices badge | not done | No project id. No badge markup in `README.md`. No application was filed. |
| REUSE | not done | No `REUSE.toml`. No `LICENSES/` directory. 63 `crates/**/*.rs` files, 53 contain `SPDX-License-Identifier`. 10 do not: `crates/zagreb-time/examples/clock.rs`, `crates/zagreb-time/src/lib.rs`, `crates/room/src/main.rs`, `crates/galion-one/src/main.rs`, `crates/galion-one/build.rs`, `crates/link-watch/src/main.rs`, `crates/link-watch/src/jsonout.rs`, `crates/desk-hermes/src/jsonout.rs`, `crates/desk-hermes/src/main.rs`, `crates/openmind-schema/src/lib.rs`. Headers on some files are not REUSE compliance. |
| SBOM | draft | No CycloneDX file and no `dist/sbom-cargo.json` at this re-check. Two drafts exist, both untracked, neither run by this ticket. `release.yml` would emit CycloneDX JSON via `anchore/sbom-action` on a `v*` tag. `sbom.yml` (DESK-ID openmind.B44, `workflow_dispatch` only, the file says DRAFT and not pushed) calls `scripts/sbom.sh`, which runs `cargo metadata --format-version 1 --offline` and would write `dist/sbom-cargo.json`. `scripts/sbom.sh` is untracked too. Not on `HEAD`. |
| Signed releases | not done | `git tag` is empty. No `*.sigstore.json` and no cosign bundle. `dist/bin/SHA256SUMS` is a checksum list of five binaries. A checksum is not a signature. |
| CI secret scanning | draft | `.github/workflows/self-eval.yml` has a gitleaks job (version 8.30.1, `gitleaks git --redact`). The whole `.github/` directory is untracked. No gitleaks report is in the tree. This ticket did not run a secret scan and prints no match. |

`TEACHER/BACKLOG.md` still lists SEC-1, SEC-2, SEC-3, and SEC-4 as owner work (rotation, a log, permissions, a snapshot). This map does not repeat paths or values, and it does not claim the tree is free of credentials.

## Scorecard checks

Names and risk levels are the default checks on the OpenSSF Scorecard README (https://github.com/ossf/scorecard/blob/main/README.md), read 2026-10-19. The tool was not run. Do not turn these words into a number.

| Check | Risk | This tree | Why |
| --- | --- | --- | --- |
| Binary-Artifacts | High | not done | `git ls-files dist` lists ELF binaries: `dist/bin/galion-one`, `dist/bin/galion-desk`, `dist/bin/room`, `dist/bin/desk-hermes`, `dist/bin/link-watch`, plus `dist/bin/SHA256SUMS`. `file` says those five are x86-64 ELF. `dist/bin-ws/galion-one` is a sixth ELF and is untracked. |
| Branch-Protection | High | not verified | No remote. Branch protection is a host setting. Nothing in this checkout configures it. History is direct commits on `main`. |
| CI-Tests | Low | draft | Test files exist (for example `crates/openmind-schema` unit tests, and `crates/*/tests/*.rs`). `.github/workflows/self-eval.yml` would run `cargo test --workspace` on push to `main` and on pull requests. That file is not in `HEAD`. Actions have not run from this repo. This ticket did not run `cargo test`. |
| CII-Best-Practices | Low | not done | No passing, silver, or gold badge. |
| Code-Review | High | not done | 20 commits, all author string `Hermes (OpenMind)`. `git log --format=%b` has zero `Signed-off-by` lines. No pull requests exist here because there is no remote. `.github/pull_request_template.md` is untracked, so it is not the process on `HEAD`. |
| Contributors | Low | not done | One author string in `git log`. Not two organizations. This map does not guess who that author is. |
| Dangerous-Workflow | Critical | draft | The four untracked workflows (`self-eval.yml`, `release.yml`, `scorecard.yml`, `sbom.yml`) do not contain `pull_request_target`. They are not on `HEAD`, so the check has nothing committed to grade. Not a pass. |
| Dependency-Update-Tool | High | draft | `.github/dependabot.yml` is untracked. It would schedule weekly cargo and github-actions updates. Dependabot is not on because the file is not on a GitHub default branch. |
| Fuzzing | Medium | not done | No `fuzz/` directory, no OSS-Fuzz config, no cargo-fuzz target. |
| License | Low | done | Tracked `LICENSE` plus `license = "MIT"` in `Cargo.toml`. |
| Maintained | High | not done | Scorecard looks for a project at least 90 days old. First commit `9ddf211` author date is 2026-10-18 17:02:54 +0000 (2026-10-18T19:02:54+02:00). `HEAD` `783cc1c` is 2026-10-19. Activity exists. The age rule is not met. |
| Packaging | Medium | not done | No package publish has run. `release.yml` would build on a `v*` tag and call `gh release create`. No tag, workflow untracked, never run. Checked-in `dist/bin` binaries are not a CI package. |
| Pinned-Dependencies | Medium | not done | `Cargo.lock` is tracked. The draft Actions use tags, not commit digests: `actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `anchore/sbom-action@v0`, `sigstore/cosign-installer@v3`, `ossf/scorecard-action@v2.4.0`, `actions/upload-artifact@v4`, `github/codeql-action/upload-sarif@v3`. `sbom.yml` uses the same style (`actions/checkout@v4`, `dtolnay/rust-toolchain@stable`, `actions/upload-artifact@v4`). |
| SAST | Medium | not done | No CodeQL analysis workflow. `scorecard.yml` only uploads a Scorecard SARIF. Clippy appears as a draft job in untracked `self-eval.yml`. This ticket did not run clippy. |
| Security-Policy | Medium | draft | `SECURITY.md` is on disk with a disclosure process, but it is untracked and the private contact is TODO. |
| Signed-Releases | High | draft | `release.yml` would run `cosign sign-blob --bundle` (keyless, GitHub OIDC) after a `v*` tag. No tag, no bundle, workflow untracked. `SHA256SUMS` is not a signature. |
| Token-Permissions | High | draft | Untracked workflows set a top-level `permissions` block (`contents: read` or `read-all`). The release job also asks `contents: write` and `id-token: write`. The scorecard job also asks `security-events: write` and `id-token: write`. Not on `HEAD`. Not scored. |
| Vulnerabilities | High | not verified | No OSV result, no `cargo audit` output, no advisory file. This ticket did not scan dependencies. Do not read a blank as "zero vulnerabilities". |
| Webhooks | Critical | not verified | Webhook secrets are a host setting. This checkout has no remote and no webhook config to read. |

Draft notes that are not scores:

- `scorecard.yml` sets `publish_results` only when the repository is not private, on a weekly cron plus push to `main`. It has never run here.
- `self-eval.yml` also drafts `cargo fmt`, `cargo clippy -D warnings`, a no-Python check, a CJK scan, a warning-only Zagreb-timestamp grep, and an MIT license check. Same status: untracked, not run by this ticket.
- `sbom.yml` is `workflow_dispatch` only. It is marked DRAFT and not pushed. It would run `scripts/sbom.sh`. It has not been run here. Same untracked status.

## Best Practices badge (passing)

Not applied. There is no bestpractices.dev project for this tree, so there is no badge at passing, silver, or gold. Silver and gold were not reviewed.

The passing criteria were read at https://www.bestpractices.dev/en/criteria/0 on 2026-10-19. This is not a filled questionnaire. A MUST is listed only where this checkout makes the answer obvious. Everything else is not verified, which is not a pass.

Locally true, and still not a badge:

- `floss_license` / `license_location`: MIT text at the repo root, SPDX short name in `Cargo.toml`.
- `version_unique`: workspace version `0.1.0` only. No second release.
- `build`: `README.md` documents `cargo build` and `cargo test`. Cargo is the build tool.
- `test`: Rust tests are in the tree. They were not executed for this ticket.
- `english`: the root docs that were read are in English.

Blocking MUST gaps (each one is enough to refuse a passing badge):

- `repo_public`: no remote, no public URL. `release/repo/README.md` still has `<TODO-repo-url>` and says nothing has been pushed.
- `sites_https`: no project website URL in the root README.
- `description_good`, `interact`, `contribution`: the root README describes the workspace, but there is no public site, no public issue tracker, and no root `CONTRIBUTING.md`. Issue and pull request templates exist only as untracked `.github/` files.
- `repo_track` / `repo_interim`: git exists locally. Interim changes are not on a public shared branch. Not met as a public project.
- `version_tags`: no git tags. SUGGESTED by the badge, listed because signed release and packaging depend on it.
- `release_notes` and `release_notes_vulns`: no changelog, no GitHub release, no notes.
- `report_process` and `vulnerability_report_process`: text exists in an untracked `SECURITY.md`. There is no public tracker.
- `vulnerability_report_private`: contact is TODO. The advisory URL is not confirmed.
- `vulnerability_report_response`: no report has a recorded answer. Not verified, and there is no channel that works today.
- `maintained`: commits on 2026-10-18 and 2026-10-19 only. Too new to show an ongoing public response record.
- `delivery_mitm` and `delivery_unsigned`: nothing is published, and nothing is signed. The unsigned checked-in binaries do not meet delivery.
- `no_leaked_credentials`: not verified. Owner items SEC-1 through SEC-4 are still open in the backlog. Not claimed met.
- `static_analysis` / `static_analysis_fixed`: no recorded CodeQL or clippy run for this ticket.
- `know_secure_design` and `know_common_errors`: no evidence recorded. Not claimed.

Crypto criteria were not scored. This ticket did not review cryptographic implementations. The schema types that were read do not store a key value; that is not a crypto review.

## What this ticket changed

- `SECURITY.md`: the fallback contact token is now the word TODO. No other policy text was rewritten.
- `docs/openssf.md`: this map.

Not changed by this ticket: crates, `release/`, workflows, `scripts/`, `LICENSE`, git history. While this file was open, `HEAD` moved from `ae72938` to `783cc1c` (B38). `scripts/sbom.sh` and `.github/workflows/sbom.yml` appeared (DESK-ID openmind.B44). They were read and not edited.

## Commands

Read-only, except the two writes named above. `PATH` was not required. No `cargo`, no Python.

```
TZ=Europe/Zagreb date -Iseconds
git remote -v
git tag
git status --short -- SECURITY.md .github docs/openssf.md
git ls-files LICENSE README.md SECURITY.md Cargo.toml .github dist
git log --format='%an' | sort | uniq -c
git log --format='%b' | grep -c 'Signed-off-by:' || true
git log --reverse --format='%H %ci %s' | head -1
find crates -name '*.rs' | wc -l
grep -l SPDX-License-Identifier -r crates --include='*.rs' | wc -l
file dist/bin/* dist/bin-ws/*
```
