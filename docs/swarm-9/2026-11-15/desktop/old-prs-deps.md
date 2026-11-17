# Swarm 9 desktop / old-prs-deps

Lane `old-prs-deps`. Written 2026-11-15T15:26:01+01:00 (Europe/Zagreb).
Base is `main` `de7480e71ce98191d2ee11f018fa15be6c5a1707` ("Console: bring the connectors screen onto the current shell (#53)").

The open draft titled "Swarm 9 desktop plan" was not in the open pull request list at that time (22 open pull requests; none with that title). GitHub search for `Swarm 9` returned only closed #9, "Swarm/nerve dryrun live". This lane did not edit #51, #52, or #57.

No pull request was closed, merged, approved, or commented on. Draft #57 was read and not pushed to.

## What this branch carries

Still-needed changes that apply cleanly and stay small:

- #7 brand icon tables, cherry-picked as the docs-only commit onto `main`. The nine SVG files are already on `main` (ancestor `6f3c9ba0`). Each `<title>` in `brand/icons/{cpu,config,galion-one,guardian,hermes,omnode,sandbox,security,voice}.svg` matches the table.
- The safe subset of #28's secrets gitignore. Patterns already present were not repeated. `*.pt` was not added again, because a later `*.pt` would override `!vendor/candle-core/tests/*.pt`. `*secret*` and `*secrets*` were not added: a match against the tracked tree hits `crates/galion-boot/src/secrets.rs` and the redacted files under `stack/kms/root/galion-secrets/`.

No dependency bump is in this branch. The patch and minor bumps that #57 already folded (#24, #26, #30, #56) were not copied again. Every other open Dependabot pull request is a major bump or a 0.x bump whose CI is red. Those are listed under decisions.

## Old pull requests

`merge-tree` is `git merge-tree --write-tree --name-only origin/main <head>`. Ahead and behind are `git rev-list --count` against `main`.

| PR | Head | Verdict | Why | Carried |
|---|---|---|---|---|
| [#7](https://github.com/galion-studio/obsidian-open-mind/pull/7) | `959790ac` | still needed | Docs only. 1 commit ahead, 824 behind. `merge-tree` exit 0. Section 7 of `brand/BRAND.md` and the end of `brand/STYLE-GUIDE.md` on `main` do not list the nine icons. | yes |
| [#14](https://github.com/galion-studio/obsidian-open-mind/pull/14) | `1e0f427b` | conflicting | Base is `swarm/main-staging`, not `main`. 1 commit ahead, 528 behind `main`. `merge-tree` exit 1: `crates/hermes-gateway/src/main.rs`, `crates/hermes-rs/src/api.rs`. `crates/hermes-rs/src/offhost.rs` is not on `main`. | no |
| [#15](https://github.com/galion-studio/obsidian-open-mind/pull/15) | `498244ec` | conflicting | 2 commits ahead, 56 behind. `merge-tree` exit 1: `docs/crate-index.md`. The vault half is superseded by merged #50 `e4c8add6` (`openmind-mcp-notes`). `main` still describes `openmind-mcp-stack` as the `stack_status` stub. Taking #15 would replace that stub with a second vault server. | no |
| [#16](https://github.com/galion-studio/obsidian-open-mind/pull/16) | `5c7bad5f` | still needed | 12 commits ahead, 56 behind. `merge-tree` exit 0. `crates/hermes-rs/src/memory.rs`, the nerve memory-freshness sensor, and the session-scope client are not on `main`. Not carried: about 5189 lines of behaviour, not a docs or dependency fold, and this lane did not re-run those tests. A scan of the diff found no PEM block and no `ghp_` token. Two documentation addresses in the diff are TEST-NET; they are not copied here. | no |
| [#17](https://github.com/galion-studio/obsidian-open-mind/pull/17) | `9dad0a7a` | still needed | 1 commit ahead, 56 behind. `merge-tree` exit 0. `main` already has `deploy/gaia-cortex/cortex-escalation.SKILL.md` ("You change nothing") and does not have the sandbox-gate sentence from this branch. Not carried: the branch adds `deploy/staged/hermes-teaching/SOUL.md`, and this lane does not edit `SOUL.md`. The README also describes a live restart, which was not run and not copied. | no |
| [#28](https://github.com/galion-studio/obsidian-open-mind/pull/28) | `10e3d478` | still needed | 1 commit ahead, 56 behind. `merge-tree` of the raw commit exit 0. The raw block repeats ignores already on `main` and adds the two unsafe groups named above. | safe subset |
| [#29](https://github.com/galion-studio/obsidian-open-mind/pull/29) | `00f2d443` | noted only | Pricing draft. Base is `swarm/galion-one-ask-anything-20261101`, not `main`. `crates/galion-one/home/pricing.json` is absent on `main`. Billing and payments are out of scope. Not carried, not closed, not commented. | no |

## Dependabot

Open Dependabot pull requests on 2026-11-15T15:26:01+01:00. "Folded into #57" means draft [#57](https://github.com/galion-studio/obsidian-open-mind/pull/57) (`ace5c5ed`, branch `swarm8/deps-batch`) already contains that bump. This lane did not re-run #57's tests. The local-test sentences below are from #57's description, not from a command run here.

Versions on `main` were read from `Cargo.lock` and the workflow pins.

| PR | Bump | Class | On `main` | CI observed | Verdict |
|---|---|---|---|---|---|
| [#24](https://github.com/galion-studio/obsidian-open-mind/pull/24) | `miniz_oxide` 0.8.9 → 0.9.1 | 0.x second component; #57 calls it minor | 0.8.9 | gitleaks failed: [job 124771535769](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280592038/job/124771535769) | folded into #57, not copied |
| [#26](https://github.com/galion-studio/obsidian-open-mind/pull/26) | `pageant` 0.2.4 → 0.2.5 | patch | 0.2.4 in `contrib/grokbot/mcp/vps-mcp` | gitleaks failed: [job 125459611309](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41498496704/job/125459611309) | folded into #57, not copied |
| [#30](https://github.com/galion-studio/obsidian-open-mind/pull/30) | `russh` 0.64.1 → 0.65.1 | minor | 0.64.1 in that crate | public export, cargo test, and gitleaks failed: [run 43527916188](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43527916188) | folded into #57, not copied |
| [#56](https://github.com/galion-studio/obsidian-open-mind/pull/56) | `anchore/sbom-action` 0.24.0 → 0.24.3 | patch | `release.yml` pins v0.24.0 | 25 success, 1 skipped (`miri full`): [run 43684504228](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43684504228) | folded into #57, not copied |

#57's description says the dependency commit passed `cargo test` and `cargo clippy` for `galion-backup` and `openmind-desktop`, `cargo test` for `vps-mcp` (3 tests), and `cargo fmt --all -- --check`. It also says `vps-mcp` clippy `-D warnings` still fails on two pre-existing `collapsible_if` lints. That description was not re-checked by running those commands here.

## Major-version decisions

Not carried. A major bump here means the first numeric component changes (including 0.x → 1.x, and GitHub Actions majors).

| PR | Bump | CI | Decision for Maciej |
|---|---|---|---|
| [#2](https://github.com/galion-studio/obsidian-open-mind/pull/2) | `toml` 0.8.23 → 1.2.1+spec-1.1.0 | `cargo test` failed: [job 132552939053](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43629559600/job/132552939053). The failing test is `every_release_bin_alone_has_the_full_set_features` at `crates/galion-feature-pin/tests/release_feature_unify.rs:94` (solo feature graph differs from the full release set). The other checks on that rollup succeeded, including gitleaks. | Take toml 1.x only after the feature-pin mismatches are pinned. |
| [#19](https://github.com/galion-studio/obsidian-open-mind/pull/19) | `actions/upload-artifact` 4.6.2 → 7.0.2 | gitleaks failed: [job 124771361526](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280532185/job/124771361526). Other checks on that rollup succeeded. | Stay on v4.6.2 until you accept a v7 pin. `main` workflow [run 43659206156](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43659206156) for `de7480e7` concluded success, so this gitleaks result is the old head, not current `main`. |
| [#20](https://github.com/galion-studio/obsidian-open-mind/pull/20) | `actions/checkout` 4.4.0 → 7.0.1 | public export failed: [job 132528891031](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43621793731/job/132528891031). `cargo test` and gitleaks failed: [job 132528891819](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43621793736/job/132528891819), [job 132528891812](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43621793736/job/132528891812). | Stay on v4.4.0. Workflows on `main` pin `actions/checkout@11d5960a326750d5838078e36cf38b85af677262` (# v4.4.0). |
| [#54](https://github.com/galion-studio/obsidian-open-mind/pull/54) | `sigstore/cosign-installer` 3.9.1 → 4.1.3 | 25 success, 1 skipped (`miri full`): [run 43684433869](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43684433869) | CI is green. Still a major. `release.yml` on `main` pins v3.9.1. Say if v4 should land. |
| [#55](https://github.com/galion-studio/obsidian-open-mind/pull/55) | `github/codeql-action/upload-sarif` 3.39.0 → 4.39.3 | 25 success, 1 skipped (`miri full`): [run 43684453433](https://github.com/galion-studio/obsidian-open-mind/actions/runs/43684453433) | CI is green. Still a major. `scorecard.yml` on `main` pins v3.39.0. The v4 notes include a higher minimum CodeQL bundle. Say if v4 should land. |

These three are not first-component majors. Cargo treats a 0.y change as incompatible. #57 folded one bump of that shape (`miniz_oxide` 0.8.9 → 0.9.1) after its own local tests. These three are not folded because their CI is red. Not carried.

| PR | Bump | CI | Decision for Maciej |
|---|---|---|---|
| [#21](https://github.com/galion-studio/obsidian-open-mind/pull/21) | `rusqlite` 0.32.1 → 0.41.1 | 9 failed, 1 skipped: clippy, cargo test, box stack, four ucpu jobs, gitleaks. Clippy: [job 124771426302](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280554340/job/124771426302). cargo test: [job 124771426342](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280554340/job/124771426342). | Leave 0.32.1 until a rebase compiles. |
| [#22](https://github.com/galion-studio/obsidian-open-mind/pull/22) | `candle-nn` 0.9.2 → 0.11.0 | clippy, cargo test, four ucpu jobs, and gitleaks failed. Clippy: [job 124771461398](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280566274/job/124771461398). cargo test: [job 124771461258](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280566274/job/124771461258). | Leave 0.9.2. |
| [#23](https://github.com/galion-studio/obsidian-open-mind/pull/23) | `getrandom` 0.2.17 → 0.3.4 | clippy, cargo test, five ucpu jobs, and gitleaks failed. Clippy: [job 124771509003](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280582527/job/124771509003). cargo test: [job 124771508750](https://github.com/galion-studio/obsidian-open-mind/actions/runs/41280582527/job/124771508750). | `Cargo.lock` on `main` already contains both 0.2.17 and 0.3.4. The 0.2.17 line is still there. Leave the bump until CI is green. |

No other open Dependabot pull request was outside #57. No remaining patch or minor bump had green CI.

## Checks run for this branch

- `git check-ignore -v` ignores `id_rsa`, `foo.pem`, `foo.key`, `credentials.json`, `.aws/credentials`, `.ssh/config`, `link.env`, and `model.cbxvoice`.
- The same command does not ignore `vendor/candle-core/tests/test.pt`, `vendor/candle-core/tests/test_with_key.pt`, `crates/galion-boot/src/secrets.rs`, or `stack/kms/root/galion-secrets/github.token.redacted`. A scan of tracked paths against the new patterns found 0 hits.
- CJK scan of `brand/BRAND.md`, `brand/STYLE-GUIDE.md`, and `.gitignore`: no Han, Hiragana, Katakana, or Hangul.
- `nice -n 19 cargo fmt --all -- --check` exit 0. Toolchain `rustc 1.83.0 (90b35a623 2024-11-26)`.
- `nice -n 19 cargo check -p galion-common --locked --offline` exit 101: no local `chrono` crate. That is an empty registry, not a compile error.
- `cargo clippy --workspace` and `cargo test --workspace`: not run.

## Open decisions for Maciej

1. The five major rows above (#2, #19, #20, #54, #55). #54 and #55 are green apart from skipped `miri full`.
2. The three red 0.x bumps (#21, #22, #23).
3. #14 conflicts and is still absent from `main`. #16 applies clean and was left for its own review. #17 applies clean and was left because it adds `SOUL.md`.
4. #29 stays open. Billing is out of scope, so this lane did not judge the pricing copy.
