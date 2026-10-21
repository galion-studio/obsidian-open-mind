<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# OpenSSF Scorecard readiness checklist (Oct 31 launch)

Written 2026-10-20T20:10:00+02:00 (Europe/Zagreb) on `swarm/launch-gates`.
Supersedes the status columns of `docs/openssf.md` (2026-10-19, VPS checkout).
The Scorecard tool was not run (it needs a public repo); this is a readiness
map, not a score. Check names: https://github.com/ossf/scorecard/blob/main/docs/checks.md

Legend: **ready** = satisfied by files on this branch; **owner** = a GitHub
setting or decision only the owner can make; **time** = cannot be met by Oct 31.

| Check | Risk | Status | What is in place / what is missing |
|---|---|---|---|
| Binary-Artifacts | High | ready (in the export) | `dist/bin/**` and `edge/dist/**` are `export-ignore`; gate 3 of `scripts/public-export.sh` fails on any ELF/PE/Mach-O/class file. They stay in the private history, which is why the public repo must start from the export (docs/launch/PUBLIC-EXPORT.md) |
| Branch-Protection | High | owner | see "Branch protection recommendation" below |
| CI-Tests | Low | ready | `self-eval.yml` (fmt, clippy, test, tier-1 musl, no-Python, CJK, gitleaks, licence) on push/PR; `launch-gates.yml` on PRs; `realtest.yml` on `swarm/real-harness` |
| CII-Best-Practices | Low | owner | register at https://www.bestpractices.dev after the repo is public; the criteria gaps are listed in `docs/openssf.md` |
| Code-Review | High | owner / process | requires PR review before merge (branch protection), then history accrues |
| Contributors | Low | time | needs contributors from several organisations |
| Dangerous-Workflow | Critical | ready | no `pull_request_target`, no `${{ github.event.* }}` interpolated into `run:` from untrusted input in any workflow |
| Dependency-Update-Tool | High | ready | `.github/dependabot.yml` (cargo + github-actions, weekly). Dependabot keeps SHA pins updated and keeps the `# vX.Y.Z` comments |
| Fuzzing | Medium | missing | no cargo-fuzz target. Candidate: `openmind-schema` / `openmind-config` TOML+JSON parsers (follow-up ticket) |
| License | Low | ready | `LICENSE` (MIT) at the root + `LICENSES/MIT.txt` + `REUSE.toml`; REUSE lint passes (gate 7) |
| Maintained | High | time | Scorecard wants a repo older than 90 days; a fresh public repo created on Oct 31 scores 0 here until late January 2027 |
| Packaging | Medium | ready (unused) | `release.yml` builds and publishes on a `v*` tag, locked until the repo is public and `RELEASE_ENABLED=true` |
| Pinned-Dependencies | Medium | ready | **every** `uses:` in all 6 workflows is now a 40-hex commit SHA with a `# vX.Y.Z` comment (this branch); tool downloads in `self-eval.yml`/`launch-gates.yml` are sha256-verified; `Cargo.lock` tracked and `--locked` everywhere. Remaining: `comply` is built with `cargo install --locked` (version-pinned, not hash-pinned) |
| SAST | Medium | partial | clippy `-D warnings` runs on every PR. Scorecard only credits CodeQL/Semgrep-style SARIF tools: adding CodeQL for Rust (`github/codeql-action` with `languages: rust`) is the follow-up; free on public repos |
| Security-Policy | Medium | owner | `SECURITY.md` exists and is tracked; the fallback contact is still `__CONTACT_EMAIL__` (owner to fill), and "Private vulnerability reporting" must be switched on in Settings |
| Signed-Releases | High | ready (plan) | see "Signed releases" below |
| Token-Permissions | High | ready | every workflow has a top-level `permissions:` block (`contents: read` or `read-all`); write scopes only at job level where needed (release: `contents`/`id-token`; scorecard: `security-events`/`id-token`) |
| Vulnerabilities | High | ready | `cargo deny check advisories` (RustSec) passes for the workspace and `edge/openmind-node` at 2026-10-20T19:52+02:00; runs in `launch-gates.yml` |
| Webhooks | Critical | owner | if any webhook is added, it must use a secret |

## Branch protection recommendation (owner, Settings -> Branches / Rulesets)

Create a ruleset for `main` (and the same for `release/*` if used):

1. Require a pull request before merging; 1 approving review; dismiss stale
   approvals on new commits; require review from Code Owners once a
   `CODEOWNERS` file exists; require conversation resolution.
2. Require status checks to pass, branches up to date (check names are the
   job `name:` values): `cargo fmt --check`, `cargo clippy -D warnings`,
   `cargo test`, `omnode (edge/openmind-node, own workspace)`, both
   `tier-1 musl release build (...)` matrix entries, `repo-check self-eval`,
   `no Python (files or shebangs)`, `no CJK characters`,
   `gitleaks secret scan`, `license check (MIT)`, `public export + gates`.
3. Block force pushes and deletions; require linear history.
4. Do not allow bypass, including for administrators (Scorecard Tier 2+).
5. Restrict who can push to matching branches to the owner; the swarm keeps
   pushing only `swarm/*` branches.
6. Tag ruleset for `v*`: only the owner may create tags (tags trigger
   `release.yml`).
7. Settings -> Code security: secret scanning + push protection (free and on
   by default for public repos), private vulnerability reporting, Dependabot
   alerts and security updates.

## Signed releases

Primary: **Sigstore cosign keyless** (already in `release.yml`). On a `v*` tag,
after the self-eval gate, every artefact (`*.tar.gz`, `SHA256SUMS`, the
CycloneDX SBOM) is signed with `cosign sign-blob --bundle <f>.sigstore.json`
using the workflow's GitHub OIDC identity; no long-lived key exists. Scorecard
recognises `*.sigstore.json` bundles. Verification:

```
cosign verify-blob --bundle <file>.sigstore.json \
  --certificate-identity-regexp '^https://github.com/galion-studio/obsidian-open-mind/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com <file>
```

Keyless signing writes to the public Rekor transparency log, which is why the
job only runs when the repository is public AND `vars.RELEASE_ENABLED == 'true'`.

Fallback / offline: **minisign** (ISC, Ed25519) for users without cosign and
for the edge/phone install path (`edge/dist/install.sh`). Plan:

1. Owner generates the key pair offline, on his own machine, never on the VPS
   or the box: `minisign -G -p minisign.pub -s ~/.minisign/obsidian.key`
   (passphrase-protected).
2. Commit only `minisign.pub` to the repo root and print its key id in
   `SECURITY.md` and the README.
3. Owner signs locally after downloading the CI artefacts and checking the
   cosign bundles: `minisign -Sm SHA256SUMS -t "obsidian-open-mind vX.Y.Z"`,
   then uploads `SHA256SUMS.minisig` to the release.
4. Users verify: `minisign -Vm SHA256SUMS -p minisign.pub` then
   `sha256sum -c SHA256SUMS`.
5. The secret key never goes into GitHub secrets (no CI-held long-lived key).
   Scorecard credits `*.minisig` as well.

Releases must not include the committed `dist/bin` binaries; CI builds them
from the tagged source.
