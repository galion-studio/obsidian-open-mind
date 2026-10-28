<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# Oct 31 launch gates: status and owner actions

Written 2026-10-20T20:15:00+02:00 (Europe/Zagreb) on `swarm/launch-gates`.
Note: CEST ends 2026-10-25 03:00; from then on timestamps carry +01:00.
Nothing on this branch changed visibility, published, rotated a key, or touched
the VPS. Every item marked **owner** needs Slavik.

## Done on this branch (ready for the owner to merge)

| Gate | Evidence |
|---|---|
| REUSE 3.3 compliance | `REUSE.toml`, `LICENSES/MIT.txt`, 13 `.license` companions for binary files; comply 0.1.1 (Rust) lint: repo 812 files (771 passed, 41 LICENSE files skipped as licence texts, 0 failed); export 723 files, 0 failed |
| Third-party licences under `stack/` | `REUSE.toml` annotations for claude-skills (Alireza Rezvani, MIT), Hermes Agent skills (Nous Research, MIT), superpowers-derived skill (Jesse Vincent + Nous Research, MIT); upstream `LICENSE` kept verbatim at `stack/agents/personas/crew-skills/LICENSE` |
| SBOM from Cargo.lock | cargo-cyclonedx 0.5.9: 36 CycloneDX 1.5 JSON files (one per crate + omnode), 130 unique components, all with licence data |
| Licence policy over all transitive deps | `deny.toml` = RULES.md allow-list; `cargo deny check licenses bans sources advisories` ok for the workspace and `edge/openmind-node`; a GPL canary crate is rejected |
| Secret-inventory / private material out of the public tree | `.gitattributes` export-ignore: `contrib/`, `*.redacted`, `stack/kms/`, `stack/INDEX.md`, `dist/bin/`, `edge/dist/` |
| Public export + verification script | `scripts/public-export.sh`, 10 gates; real run 0 FAIL / 1 WARN; canaries caught by 6 of 6 FAIL gates |
| Secret scanning | built-in 14-shape scan + gitleaks 8.30.1 over the export: 0; gitleaks over the full private history (251 commits): 0 |
| Pinned actions | all `uses:` pinned to commit SHAs |
| CI for the above | `.github/workflows/launch-gates.yml` (PRs to main, read-only) |
| Scorecard readiness, branch protection, signing plan | `docs/launch/SCORECARD-READINESS.md` |

## Re-measured on swarm/integration-5 0d68f46 (2026-10-21, slot 231)

The table above is the launch-gates branch as written on 2026-10-20. These are
the same gates measured again at 2026-10-21T23:48:30-23:54:46+02:00 with
integration-5's `scripts/public-export.sh --allow-missing-tools` (comply 0.1.1
and cargo-deny from `/workspace/swarm/tools/bin`; gitleaks and cargo-cyclonedx
are not installed on the box, so gates 6 and 9 were SKIP). The same script was
run on all three refs. Nothing was published or pushed by these runs.

| Gate | launch-gates 79ee761 | integration-1 8fd26f6 | integration-5 0d68f46 |
|---|---|---|---|
| 1 git archive (exported / tracked) | 739 / 831 | 1070 / 1162 | 1519 / 1611 (92 export-ignored on all three) |
| 2 path policy, 3 no binaries, 4 no Python, 5 secret patterns (14 shapes) | PASS | PASS | PASS (0 secret hits) |
| 7 REUSE lint (comply) | PASS: 724 total, 684 passed, 0 failed, 40 skipped | PASS: 1053 total, 1007 passed, 0 failed, 46 skipped | **FAIL: 1502 total, 1431 passed, 7 failed, 64 skipped** (see below) |
| 8 cargo deny (bans, licenses, sources; workspace + edge/openmind-node) | PASS | PASS | PASS |
| 10 infra disclosure (WARN, owner decides) | 7 public IPv4 literals, 1 quick-tunnel hostname | 20 IPv4, 1 quick-tunnel (matches `docs/real-tests/integration-1.md:22`) | **27 IPv4 in 17 files, 1 quick-tunnel** |
| script result | 0 FAIL, 1 WARN | 0 FAIL, 1 WARN | **1 FAIL, 1 WARN** |

Other numbers on integration-5 0d68f46:

- REUSE gate 7 FAIL, 7 files: `crates/bench-banking/src/axes.rs` (fixture string
  at line 1131) and `crates/bench-scoreboard/src/lib.rs` (line 119) hold an HTML
  SPDX comment inside a Rust string, which comply reads as an invalid
  expression; five vendored candle-core test binaries in
  `vendor/candle-core/tests/` (`fortran_tensor_3d.pth`, `test.npy`, `test.npz`,
  `test.pt`, `test_with_key.pt`) have no licence or copyright data. Fix (a
  REUSE.toml change, not done on this docs-only branch): add the two .rs files to
  annotation 2 as `crates/gaia-minerva/src/lib.rs` already is, and annotate
  `vendor/candle-core/**` with its upstream licence `MIT OR Apache-2.0`
  (`vendor/candle-core/Cargo.toml:30`).
- `repo-check`: FAIL, 1519 files, 10 errors, all `portable` `core::arch::` in
  vendored candle-core (`docs/real-tests/integration-5.md:18`; main-merge
  blocker 0).
- `cargo deny ... check advisories bans licenses sources`: all ok
  (`docs/real-tests/integration-5.md:15`).
- `.license` companions: 11 tracked (`git ls-files '*.license'`). The table above
  says 13; launch-gates 79ee761 itself also tracks 11.
- Pinned actions: 39 of 40 `uses:` lines in `.github/workflows/` are pinned to
  a 40-character commit SHA; the 40th is the local reusable workflow
  `./.github/workflows/self-eval.yml` (`release.yml:24`), which needs no pin.
- SBOM (130 components above) and gitleaks (0 above) were not re-measured: the
  tools are not on the box. `Cargo.lock` lists 238 packages (`rg -c '^name = '
  Cargo.lock`, workspace crates included).
- Still open from "Known gaps": no fuzz or CodeQL-analysis workflow on integration-5
  (scorecard.yml only uploads SARIF)
  (`swarm/ci-fuzz-workflow` 3a188b9 exists on GitHub but is not merged);
  `__CONTACT_EMAIL__` is still in SECURITY.md and CODE_OF_CONDUCT.md (owner).
- GitHub `main` is still 13a296c (branch listing, 2026-10-21T23:50:33+02:00),
  so none of these gates is on main yet.

## Still needing the owner (not done by the swarm, by rule)

| ID | Gate | Owner action | Blocks launch? |
|---|---|---|---|
| SEC-1 | secret rotations + history purge of the galion-src clones | rotate every key named in the inventory (incl. the GitHub token in `galion-light/secrets/persist` that INFRASTRUCTURE.md calls leaked), then purge the history of the affected clones | **yes**, hard gate |
| SEC-2 | session log that contains a token | lock or move it (owner said on Oct 18 22:35 not to change `secrets/persist` permissions yet) | yes |
| SEC-3 | Hermes `config.yaml` / `SOUL.md` 666, `vault/` 777 | fix permissions | yes |
| SEC-4 | private key copy + secret folders in the from-grok-build snapshot | keep or remove | yes |
| SEC-5 | tuner-quick public trycloudflare URL still running | keep or stop | yes (public surface); also decide the hostname mention in `docs/session-2026-10-18-evening.md:45` |
| OPS-1 | GitHub connector lacks private-repo scope | grant scope (only if remote checks are wanted) | no |
| LAUNCH-1 | visibility / first public push | create the **public repo from the exported tree** (fresh history); do NOT flip the private repo, whose history still contains the excluded material | **yes** |
| LAUNCH-2 | RELEASE-CHECKLIST.md go/no-go; AWAITING-APPROVAL boxes all unchecked | run it, sign off | yes |
| contact | `__CONTACT_EMAIL__` in SECURITY.md, CODE_OF_CONDUCT.md (+ `release/repo/` copies) | supply a private, non-personal address | yes (Security-Policy) |
| merge | merge `swarm/launch-gates` (and the other swarm branches) | review + merge PR (the swarm opens none) | yes |
| GitHub settings | branch-protection ruleset, private vulnerability reporting, secret scanning + push protection, Dependabot alerts, tag ruleset for `v*` | per SCORECARD-READINESS.md | yes for Scorecard |
| release switch | repo variable `RELEASE_ENABLED=true` (only after public) | set when the first `v*` release should run | no |
| minisign key | offline key pair, commit `minisign.pub` only | optional fallback signing | no |
| export scope | decide on `stack/` (103 non-secret ops copies), `TEACHER/`, `admin.preferences`, `docs/TERMINAL-LOG.md` (box egress IP on the fail2ban allow-list), the France server IP in `stack/hermes/SOUL.md` and `TEACHER/QUEUE.md` | add export-ignore lines (ready-made in PUBLIC-EXPORT.md) or accept | recommended |
| Best Practices badge | register at bestpractices.dev after going public | | no |

## Known gaps (not owner-only)

- Fuzzing and CodeQL (Scorecard Fuzzing, SAST) are not set up.
- `deny.toml` keeps `wildcards = "warn"` because workspace crates lack
  `publish = false`; set it per crate (or `[workspace.package] publish = false`
  inherited) and flip to `deny`.
- comply 0.1.1 is young (Aug 2026). It agrees with the REUSE spec on what was
  tested here, but it requires `.license` companions for binary files even when
  `REUSE.toml` covers them, and it mis-applies `.gitignore` to absolute paths
  (worked around in the script). The FSFE `reuse` tool (Python) was not run.
- Scorecard "Maintained" and "Contributors" cannot be met by Oct 31.
