<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Swarm 9 desktop plan

- Swarm: desktop (Obsidian desktop, `obsidian-open-mind`, repo hygiene).
- Orchestrator: agent 1. This file is the shared plan. Workers read it before starting and stay in their lane.
- Written: 2026-11-15T15:28:31+01:00 (Europe/Zagreb). Base: `main` `de7480e71ce98191d2ee11f018fa15be6c5a1707`.
- Find this plan on the open draft pull request titled **Swarm 9 desktop plan**. After that pull request merges, the same path on `main` is the plan.
- Notes for each lane go in that lane's own draft pull request: `docs/swarm-9/2026-11-15/desktop/<lane>.md`. Lane slugs: `licence`, `old-prs-deps`, `readme-status`.
- Draft pull request titles start with `Swarm 9 desktop/` so the orchestrator can see them. Workers mark a pull request ready for review when its required checks are green. Workers do not merge.

A separate Swarm 9 **core** swarm owns Rust, CPU, and Linux inside the crates of this repo. Its plan is the open draft **Swarm 9 core plan** (#59, branch `cursor/swarm-9-core-plan-7126`). Desktop does not take that work, except the exceptions in [File ownership](#file-ownership) and the 15:29 list below.

## Owner decisions at 15:29

Owner FRXDEL, 2026-11-15 15:29 CET (`+01:00`). These two decisions override earlier sentences in this file that left a listed bump for later, or that left #14 and #15 as notes only.

1. Every bump in this list is allowed. Merge it once every required, non-skipped check on its **current** head is green: **#2, #19, #20, #21, #22, #23, #24, #26, #30, #54, #55**. Lane `old-prs-deps` (worker run `bc-004d8f8c`, draft #58) is already on this queue. One at a time. After each merge, update the next from `main` and let CI finish on the new head. A bump that cannot go green stays open with the failing check and the job URL. #56 is not in this list. It stays inside #57.
2. Older pull requests from before Swarm 9: this swarm takes **only** the ones that are blocked or failing. Rebase them, or recreate them as Swarm 9 pull requests, until the required checks are green, then the orchestrator merges them. The owner named conflicting **#14** and **#15**, and **#57 if its CI fails**. Pull requests the owner named as already ready (#51, #52, #16, #17) stay for the owner. Do not merge them and do not edit their branches.

Observed at 2026-11-15T15:30:15+01:00, from `gh pr list` check rollups (success / failure / skipped):

| PR | Rollup | Merge state | This swarm |
|---|---|---|---|
| #57 | 25 success, 1 skipped | CLEAN | CI is not failing. Do not take it. Owner merges. |
| #51, #52 | 25 success, 1 skipped each | CLEAN | Leave for the owner. |
| #16, #17 | 21 failure, 1 skipped each | UNSTABLE, mergeable | The owner still named these as owner-merged. Do not edit them. The failing rollup is recorded here so it is not described as green. |
| #14 | no checks | CONFLICTING, base `swarm/main-staging` | Take. Recreate on a new branch from `main`. |
| #15 | 21 failure, 1 skipped | CONFLICTING | Take. Recreate on a new branch from `main`. |
| #7 | 12 success | CLEAN | Already green. Do not edit #7. The doc tables are already copied on #58. |
| #54, #55 | 25 success, 1 skipped each | CLEAN | In the bump list and green now. Merge these pull requests one at a time. |
| #2, #19, #20, #21, #22, #23, #24, #26, #30 | at least one failure each | UNSTABLE | Recreate on new branches from current `main`. Do not push to `dependabot/*`. |

#29 stays a note. Its base is not `main`, and payments are out of scope. Do not recreate the pricing pages.

## Design direction

Owner FRXDEL, 2026-11-15 15:25 CET (`+01:00`). This applies to every Swarm 9 desktop lane.

- Ship something that works. It does not have to be perfect.
- The person using it should get a genuinely great experience, with a very minimal and beautiful interface.
- The desktop, the apps, and the browsers on Linux should feel like one product with the rest of the operating system: connected, one flow, not a pile of separate tools.
- Prefer a working, connected flow over extra polish.
- Reuse `brand/STYLE-GUIDE.md` and `brand/BRAND.md`. Do not invent a second brand, a second palette, or a second desktop.

For this hygiene swarm that means: describe and repair the surfaces that already exist (`openmind-desktop` on loopback port 18181, the console probe on 18080, `/one` and `/research`, the opt-in read-only GitHub MCP flag). Do not start a parallel app, a parallel licence, or a parallel UI kit. A dependency bump is finished when the required checks are green, not when surrounding code has been restyled.

## Standing merge rule

Owner FRXDEL, by voice, 2026-11-15 15:21 `+01:00`, and the 15:26 `+01:00` instruction to keep going until the swarm's work is finished and the tests pass. No pause for a further approval on work this plan already assigns.

The orchestrator merges a **Swarm 9 desktop** pull request when every required, non-skipped CI check is green on the **current** head and the review has no open issue. One pull request at a time. After each merge, the next pull request is updated from `main` and its CI runs again on that new head before it can merge.

Still in force:

- No force-push, no history rewrite, no branch-protection bypass, no admin merge, no skipped or disabled check.
- No secrets, tokens, keys, or IP addresses in commits, pull requests, comments, or logs.
- No server credentials. Existing GitHub workflows may run after a merge to `main`. Do not push a `v*` tag. `release.yml` runs only on a `v*` tag and is the owner's publish switch.
- Do not edit `SOUL.md`, `config.yaml`, `LOCKS.toml`, `voices/`, `weights/`, or `runtimes/`.
- Do not merge or edit #51, #52, #16, or #17. Do not take #57 while its CI stays green (it was green at 15:30). Dependabot heads that are still red are recreated as Swarm 9 pull requests. #54 and #55 are already green, so those two pull requests themselves are the ones that merge.
- Do not settle an owner decision. Record it, and merge only the safe default named in this plan.
- Do not edit the branches of #51 (`cursor/hermes-mcp-client-3083`), #52 (`cursor/desktop-ci-artifacts-c14b`), or #57 (`swarm8/deps-batch`).

### Bumps named at 15:29

The 15:24 decision allowed the risky majors once tests passed. A 15:25 hold covered #2, #19–#24, #26, #30, #54, and #55. The 15:26 instruction lifted that hold. The 15:29 decision is the current list and includes #24, #26, and #30 as well as the majors.

`old-prs-deps` takes them one at a time. #54 and #55 are already green, so the orchestrator merges those pull requests directly, one at a time. Every other number on the list is recreated from current `main` because its own head is still failing. Do not edit #57 to extract #24, #26, or #30: #57's CI is green, and the owner merges #57. A second copy of those three bumps will conflict with #57; keep the recreated diff to the same version pins #57 already uses, and record the overlap.

A bump that cannot go green stays open, with a comment that names the failing check and the job URL. Do not close it and do not merge it.

## File ownership

| Owner | Paths |
|---|---|
| Desktop licence | Root `LICENSE`. Every project `LICENSE` that is still a byte-for-byte copy of today's root `LICENSE`. `.github/workflows/self-eval.yml` **license job `run:` block only** (not the `uses:` pins). `crates/repo-check` licence gate: `check_license` and the tests that read the root `LICENSE` text. `scripts/desktop/verify-local.sh` packaged-`LICENSE` compare. `docs/swarm-9/2026-11-15/desktop/licence.md`. |
| Desktop old-prs-deps | One bump at a time, the files that bump lists under [Bump queue](#bump-queue). `brand/BRAND.md` and `brand/STYLE-GUIDE.md` for the #7 refresh. `docs/swarm-9/2026-11-15/desktop/old-prs-deps.md`. |
| Desktop readme-status | `README.md`, `docs/STATUS.md`, `docs/crate-index.md`, `docs/swarm-9/2026-11-15/desktop/readme-status.md`. |
| Desktop plan | This file only, on the plan pull request. Workers do not edit it. |
| Core | All other `crates/**`, `Cargo.toml`, and `Cargo.lock`, including Rust, CPU, and Linux. Core leaves the licence-gate functions and the files of the **one open bump** alone until that pull request merges. |

`docs/desktop/README.md` stays with #52 until the owner merges #52. Desktop lanes do not edit it. `docs/hermes-mcp.md` stays with #51. Do not copy that file onto a new branch.

## Backlog-to-lane map

| Item | Lane | What the lane does |
|---|---|---|
| Canonical GOS `LICENSE`, byte for byte | licence | Copy, update the gate that would reject the new first line, compare packaged bytes |
| #51 decision 5 and #52 decision 3 (copyright line / packaged `LICENSE`) | licence | Safe default: the GOS file unchanged. Record the "name Maciej Grajczyk?" question. Do not rename the holder |
| `check-package.sh` brand-line test | licence | That script is only on #52. Do not edit #52. Byte-compare on `main` via `verify-local.sh`. Record the #52 follow-up |
| #7 brand icon list | old-prs-deps | New branch from `main`. Docs only. Icons are already on `main` |
| #14, #15, #16, #17 | old-prs-deps | Triage in the lane notes. Do not replay crate edits and do not copy any `SOUL.md` change. `docs/crate-index.md` belongs to readme-status |
| #28 secrets-guard gitignore | old-prs-deps | Note only. Do not apply the patch (it would ignore vendored `*.pt` fixtures again) |
| #29 pricing | old-prs-deps | Note only. Payments stay out of scope. Do not merge `main` into it and do not close it |
| Bumps #2, #19, #20, #21, #22, #23, #24, #26, #30, #54, #55 | old-prs-deps | Allowed. Merge each when its required checks are green, one at a time |
| #56 | — | Not in the 15:29 list. Stays inside #57 |
| #14, #15 | old-prs-deps | Blocked. Recreate as Swarm 9 pull requests until green, then merge |
| #16, #17, #51, #52 | owner | Do not merge or edit. #16 and #17's rollups were failing at 15:30; the owner still keeps them |
| #57 | owner while CI is green | 25 success and 1 skipped at 15:30. Take it only if that CI goes red |
| #7 | owner | Green at 15:30. Doc tables already copied onto #58 |
| README, STATUS, crate index | readme-status | Refresh from the tree. Pointer to the MCP flag and the 18181/18080 ports. Do not duplicate `docs/hermes-mcp.md` |
| Design direction | all three lanes | Working and connected, minimal UI, existing brand files |

## Lane: licence

Title: `Swarm 9 desktop/licence`. Branch from `main`. Do not push to #51, #52, or #57.

### Work

1. Download `LICENSE` from `galion-studio/galion-operating-system` branch `main` and copy it byte for byte. Acceptance is `cmp` against that download, not a retyped file. Swarm 8 measured 1257 bytes on 2026-11-15; measure again with `wc -c` and record the number.
2. Replace every **project** `LICENSE` that is still byte-identical to today's root file (the `Copyright (c) 3000 Galion / galion-studio` MIT text, 1079 bytes on `de7480e`). The self-eval job compares `crates/*/LICENSE` and `edge/*/LICENSE` to the root. Also replace other identical copies (`release/repo/LICENSE`, `release/obsidian-open-mind-kit/LICENSE`, `docs/swarm/galion-os-staging/LICENSE`, `contrib/grokbot/LICENSE`) when `cmp` says they match the old root. Leave third-party licences under `vendor/candle-core` and `stack/agents/personas/crew-skills`.
3. The GOS file's first line is the G.A.L.I.O.N. banner, not `MIT License`. Today's gate rejects that. Update only:
   - `.github/workflows/self-eval.yml` license job `run:` block, so it accepts this file (MIT grant still required, copyright line contains `Copyright (c) 2026 Galion Labs`, and the four branding lines below are present).
   - `crates/repo-check` `check_license`, same rule, plus tests that fail only because they read the root `LICENSE`.
   Do not change `uses:` pins in the workflow. Do not rewrite SPDX headers in other crates. Those headers stay `3000 Galion / galion-studio`.
4. The four branding lines inside the canonical file are:
   - `G.A.L.I.O.N. — General Autonomous Local Intelligence Open Network`
   - `Developed and distributed by Galion Labs`
   - `Perpetual Open MIT License`
   - `Copyright © 2026 Galion Labs. All rights reserved.`
   The MIT body also contains `Copyright (c) 2026 Galion Labs`.
5. `scripts/desktop/package-local.sh` already copies the root `LICENSE` into the local tarball. `scripts/desktop/verify-local.sh` only checks that the packaged file exists. Make `verify-local.sh` fail unless the packaged `LICENSE` is byte-identical to the root `LICENSE` and those four lines are in it.

### Decision 5 (recorded, not settled)

#51 decision 5 and #52 decision 3 ask whether the copyright line should name Maciej Grajczyk. The canonical file names Galion Labs. The safe default is that file, unchanged. Do not write `Maciej Grajczyk` or `FRXDEL STUDIOS` into `LICENSE`.

#52's `scripts/desktop/check-package.sh` (function `desktop_assemble_license` on branch `cursor/desktop-ci-artifacts-c14b`) still builds the old MIT text plus four appended lines, and its brand-line test fails when the last appended line is missing. Required end state: the packaged `LICENSE` is the canonical file, and the brand-line test checks the four lines inside that file. Do not edit #52. A later branch from #52's head, with only that fix, may be opened as `Swarm 9 desktop/licence-package`. This swarm does not merge it while its base is #52, because that would take #52 with it. Say so in `licence.md`.

### Acceptance

- `cmp` of root `LICENSE` against a fresh GOS download: identical. Record `wc -c`.
- `cmp` of each replaced project `LICENSE` against the root: identical.
- `vendor/candle-core` and `stack/agents/personas/crew-skills` `LICENSE` files: unchanged (`git diff` empty for those paths).
- Self-eval license job logic and `cargo test -p repo-check --test checks --locked` pass locally or on CI. Record the command and the result. Unrun stays "not run".
- `verify-local.sh` rejects a packaged `LICENSE` that differs by one byte. Show that command's exit code.
- No SPDX header outside the licence-gate files was rewritten.

## Lane: old-prs-deps

Title prefix: `Swarm 9 desktop/`. Worker run `bc-004d8f8c`. One **bump** pull request at a time. Notes: `docs/swarm-9/2026-11-15/desktop/old-prs-deps.md` on the first pull request this lane opens (the notes can ride with the first bump, or with the #7 docs pull request).

### Older pull requests

Do not push to their branches.

| PR | Base observed 2026-11-15 | Lane action |
|---|---|---|
| #7 `ws/brand` | Green at 15:30 (12 success, CLEAN). | Do not edit #7. #58 already carries the icon tables. |
| #14 | Conflicting. Base `swarm/main-staging`. Conflicts on `crates/hermes-gateway/src/main.rs` and `crates/hermes-rs/src/api.rs`. | Recreate on a new branch from `main` as `Swarm 9 desktop/hermes-offhost`. Core's plan already leaves `crates/hermes-rs/**` alone. This lane owns the recreated files until that pull request merges. Do not push to #14's branch. |
| #15 | Conflicting, and 21 checks failed at 15:30. Conflict is `docs/crate-index.md`. #58 notes that merged #50 already covers the vault half. | Recreate on a new branch from `main` as `Swarm 9 desktop/mcp-stack`. Do not edit `docs/crate-index.md` (readme-status #60 owns it). Do not add a second vault server on top of #50. Do not push to #15's branch. |
| #16 | Owner keeps it. Rollup at 15:30 was 21 failures, not green. | Do not merge or edit. |
| #17 | Owner keeps it. Rollup at 15:30 was 21 failures. The branch adds `SOUL.md`. | Do not merge or edit. Do not copy `SOUL.md`. |
| #28 | One failing check at 15:30. The raw gitignore would ignore vendored `*.pt` fixtures again. | The safe subset is already on #58. Do not push to #28 and do not add `*.pt` or `*secret*`. |
| #29 | Base `swarm/galion-one-ask-anything-20261101`, not `main`. Pricing and footer. | Note only. Payments and billing are out of scope. Galion OS #16 already has the live price copy. Do not merge `main` into #29 and do not close #29. One difference to record: #29's body prices Galion Research as a one-time amount; the merged site prices it per month. The merged site is the live copy. |

### Bump queue

#54 and #55 merge as they are, one at a time, because their checks were already green. Every other row is recreated from the Dependabot diff onto current `main`. Do not push to `dependabot/*`. Open the next recreation only after the previous bump has merged, or after it is recorded as blocked with a failing-check URL.

Swarm 8 triage (`docs/swarm-8/2026-11-15/a2-deps-triage.md` on #57) classified these. Re-read the Dependabot pull request and CI before recreating. Fix only the fallout that stops a required check.

| Order | Source | Bump | Files the lane owns for that pull request | Known fallout from the triage (re-check) |
|---|---|---|---|---|
| 1 | #55 | `github/codeql-action/upload-sarif` 3.39.0 → 4.39.3 | `.github/workflows/scorecard.yml` pin only | Pull-request checks were green. `scorecard.yml` does not run on `pull_request`. |
| 2 | #54 | `sigstore/cosign-installer` 3.9.1 → 4.1.3 | `.github/workflows/release.yml` pin only | Checks were green at 15:30. Merge after #55, once this head is updated from `main` and CI is green again. The release job runs on a `v*` tag. Do not push a tag. |
| 3 | #24 | `miniz_oxide` 0.8.9 → 0.9.1 | `crates/galion-backup/Cargo.toml`, `Cargo.lock` | Own head still failed at 15:30. Same pin #57 already tested. Recreate. Do not edit #57. |
| 4 | #26 | `pageant` 0.2.4 → 0.2.5 | `contrib/grokbot/mcp/vps-mcp` lock and manifest | Same as #24. Own head still failed. Recreate. Core's plan does not own `contrib/`. |
| 5 | #30 | `russh` 0.64.1 → 0.65.1 | same package as #26 | Recreate after #26, from the new `main`. |
| 6 | #19 | `actions/upload-artifact` 4.6.2 → 7.0.2 | `uses: actions/upload-artifact` pins in `.github/workflows/launch-gates.yml`, `realtest.yml`, `sbom.yml`, `scorecard.yml`, `self-eval.yml` | v6 wants Node 24. Call sites use the default zip upload. |
| 7 | #20 | `actions/checkout` 4.4.0 → 7.0.1 | `uses: actions/checkout` pins, including `.github/workflows/openmind-desktop.yml` | After the licence pull request, and after core's linux lane if it is editing `self-eval.yml`. v7 blocks fork checkout from `pull_request_target` and `workflow_run`. |
| 8 | #2 | `toml` 0.8.23 → 1.2.1+spec-1.1.0 | Root `Cargo.toml` `toml`, `edge/openmind-node/Cargo.toml` `toml`, `Cargo.lock`, and call sites that fail the required checks | Triage: `galion-feature-pin` test `every_release_bin_alone_has_the_full_set_features` failed. Confirm it is still this bump. Core hygiene owns that test file. If the failure is there, comment on #59 before editing it. |
| 9 | #23 | `getrandom` 0.2.17 → 0.3.4 | `crates/hermes-rs/Cargo.toml`, `edge/openmind-node/Cargo.toml`, `Cargo.lock`, `crates/hermes-rs/src/enroll/mod.rs` call sites that use `getrandom::getrandom` | Triage: `getrandom` was removed in 0.3 (`enroll/mod.rs` around lines 348 and 502 on the old base). Core leaves `hermes-rs` alone. |
| 10 | #21 | `rusqlite` 0.32.1 → 0.41.1 | Root `Cargo.toml` `rusqlite`, `crates/hermes-gateway/Cargo.toml`, `spikes/sqlite-pure/fixtures/Cargo.toml`, `Cargo.lock`, and `prepare_cached` call sites that fail (triage named `crates/openmind-memory/src/store.rs`) | Edition 2024, MSRV, iterator API. |
| 11 | #22 | `candle-nn` 0.9.2 → 0.11.0 | `crates/galion-chatterbox/Cargo.toml`, `crates/galion-voice-cpu/Cargo.toml`, `Cargo.lock`, and the compile errors those two crates hit | Triage: does not compile against vendored `candle-core` 0.9.2. If the required checks still fail after a direct fix, leave the pull request open and stop. Do not upgrade the vendored candle tree (core does not list `vendor/`, and this lane does not take it). |

Core's plan (#59) leaves `hermes-rs` alone and may still touch root `Cargo.toml` (`rust-version` only) and `Cargo.lock` (one fetch edge). Before opening bump rows 8 through 11, re-read #59. If a core pull request already changes the same path, comment on both and wait until one side owns that path.

### Acceptance

- The pull request diff is the bump plus the fallout fix, on current `main`.
- Every required, non-skipped check on the current head is green before the worker marks it ready. Paste the check rollup URL in the pull request body.
- A blocked bump's comment names the check, the job URL, and the error that stops it.
- #7's diff is limited to `brand/BRAND.md` and `brand/STYLE-GUIDE.md`.
- `SOUL.md`, `config.yaml`, and the Dependabot branches are untouched.

## Lane: readme-status

Title: `Swarm 9 desktop/readme-status`. Branch from `main`.

### Work

1. `README.md` on `de7480e` still says the first public release is 2026-10-31 and that `openmind-config`, `openmind-survival`, `guardian-tick`, `openmind-sandbox`, `persona-plex-relay`, and `edge/openmind-node` are not in the tree. Those directories are on `main`. Update the overview so it matches the tree. Do not claim a public release or a production cutover. Point status at `docs/STATUS.md`.
2. Add a short pointer, not a second article:
   - `HERMES_RS_MCP_GITHUB=1` turns on read-only GitHub MCP. It stays off until that variable is set. The contract is `docs/hermes-mcp.md`.
   - That doc is on draft #51 (`ff9d400387f598c1c2091f84ad19c3544f9dc962` when this plan was written) and is **not** on `main`. Do not copy it. Link the path. If the file is still absent on `main`, say the contract lands with #51.
   - Desktop listen address is `127.0.0.1:18181`. The console probe stays on 18080. Do not bind 18081.
   - The #51 copy of `docs/hermes-mcp.md` that was read for this plan does not mention 18181 or 18080. Cite `docs/desktop/README.md` and `docs/bind-defaults.md` for the ports. Do not add a port section by duplicating the MCP doc.
3. `docs/STATUS.md`: run `scripts/status-percent.sh` and paste its output. Do not type a percentage by hand. Do not change a D/P/N/O cell without evidence from `main` in the same pull request. Do not rewrite the October swarm-branch tables from memory. Under the computed block, add a note that is **not** a backlog row and does not change the count: `openmind-desktop` is on `main`, default bind `127.0.0.1:18181`, console probe 18080, stack rows `staged` or `planned` (`docs/desktop/README.md`). Stamp the run with `TZ=Europe/Zagreb date -Iseconds`.
4. `docs/crate-index.md`: one row per `crates/*/Cargo.toml`. Copy `name` and `description` from that file. A missing description is `none`. Do not edit any `Cargo.toml`.

   Count on `de7480e` at 2026-11-15T15:24:40+01:00: 75 crate directories, 38 index rows.

   Missing from the index: `bench-banking`, `bench-scoreboard`, `console-static`, `console-ui`, `gaia`, `gaia-aether`, `gaia-apollo`, `gaia-artemis`, `gaia-cortex`, `gaia-demeter`, `gaia-eleuthia`, `gaia-hades`, `gaia-hephaestus`, `gaia-live`, `gaia-minerva`, `gaia-poseidon`, `galion-backup`, `galion-bench`, `galion-boot`, `galion-chatterbox`, `galion-common`, `galion-deploy`, `galion-feature-pin`, `galion-guard`, `galion-health`, `galion-jsonout`, `galion-origin`, `galion-realtest`, `galion-tar`, `galion-vending`, `galion-voice-core`, `galion-voice-cpu`, `galion-wallstreet`, `hermes-bench`, `hermes-gateway`, `hermes-rs`, `hermes-tools`, `nerve-client`, `nerve-cortex-contract`, `openmind-desktop`, `openmind-persona`, `openmind-queue`, `repo-check`, `tunnel-lint`, `xtask`.

   In the index, not on disk: `gaia-registry`, `galleon-voice`, `hades-journal`, `openmind-boot`, `openmind-fallback`, `openmind-origin`, `openmind-route`, `openmind-units`.

   Re-list from the tree before editing. The lists above are the baseline, not a permission to skip the listing.

### Acceptance

- A command lists `crates/*/Cargo.toml` and the index has the same set of names. Show the command output (a diff of names is enough).
- Each description matches the `description` field. Show the command that compared them.
- `scripts/status-percent.sh` output is pasted, with the Zagreb timestamp of the run.
- `README.md` names `HERMES_RS_MCP_GITHUB=1` as opt-in and read-only, and names ports 18181 and 18080, with the citations above. It does not contain a paste of `docs/hermes-mcp.md`.
- `docs/desktop/README.md` and `docs/hermes-mcp.md` are absent from the diff.
- `git diff --stat` shows only `README.md`, `docs/STATUS.md`, `docs/crate-index.md`, and the lane notes file.

## Merge order

### This swarm (orchestrator merges)

1. **#55**, then **#54**. Both were green at 15:30. Merge #55 first. Update #54 from `main` and wait for its new head to be green before merging it.
2. **Swarm 9 desktop plan** (this file), when its required checks are green.
3. **Swarm 9 desktop/licence**, updated from `main` after the plan merges.
4. **#58** (brand tables and the safe gitignore) and **#60** (readme-status), after the review notes in the log are addressed and the required checks are green. One at a time. They do not carry a dependency bump.
5. Bump queue rows 3 through 11, one at a time. Checkout (#20) waits until the licence pull request has merged and core is not mid-edit on `self-eval.yml`.
6. Recreated **#14** and **#15**, after the crate-index pull request has merged so #15 does not touch that file.
7. `Swarm 9 desktop/licence-package`, if opened, stays open until the owner has merged #52 and the branch has been updated onto `main`. This swarm does not merge it before that.

Before each merge, re-read **Swarm 9 core plan** (#59). If core is landing a pull request that touches the same paths, desktop waits and comments.

A merge to `main` runs the workflows that already trigger on `push` to `main`: `self-eval`, `openmind-desktop`, `scorecard`, and `miri` (`on: push` with no branch filter). `release.yml` does not run on a branch push. `launch-gates` and `realtest` run on `pull_request`. `sbom.yml` is `workflow_dispatch`. There is no Pages deploy workflow in `.github/workflows`.

### Left for the owner (do not merge, do not edit)

Observed green at 15:30, except #16 and #17:

1. **#57** — CI was green (25 success, 1 skipped, CLEAN). It holds `miniz_oxide` 0.9.1, `pageant` 0.2.5, `russh` 0.65.1, `anchore/sbom-action` 0.24.3, and a GOS `LICENSE` copy. Take it only if that CI goes red. If the desktop licence pull request merges first, the owner rebases #57 and drops the duplicate `LICENSE` commit.
2. **#51** — Hermes MCP client and `docs/hermes-mcp.md`.
3. **#52** — desktop CI artifacts. `check-package.sh` still assembles the old MIT text plus four lines. This swarm does not edit #52.
4. **#16** and **#17** — the owner named these with the green set. Their rollups at 15:30 were 21 failures each. They stay with the owner anyway.
5. **#7** — 12 success, CLEAN. The same doc tables are on #58.

## Conflicts already known

- Core #59 gives the whole of `.github/workflows/self-eval.yml` to the linux lane, and the root `Cargo.toml` `rust-version` line plus a possible `Cargo.lock` edge to hygiene and linux. Desktop licence edits only the license job `run:` block. Desktop bumps edit dependency lines and `Cargo.lock`. Checkout (#20) edits `uses:` pins in that same workflow. Comment on #59 before either side edits those files. One owner per hunk.
- Core #59 does not edit `crates/hermes-rs/**`. The #14 recreation does. Desktop owns that recreation. Core linux may edit `crates/openmind-desktop/src/lib.rs`, `files.rs`, and `http.rs` for bind defaults. This plan does not.
- #15's conflict is `docs/crate-index.md`, which #60 owns. The recreation stays out of that file.
- `docs/desktop/README.md` stays on #52.
- Licence vs #57: both copy the GOS `LICENSE`. #57 is green, so the owner merges it. If licence lands first, the bytes should match and the owner drops the duplicate.
- #24, #26, and #30 overlap #57. Recreated pins should match #57 so the owner's merge is a duplicate, not a second version.
- #22 vs vendored `candle-core` 0.9.2: likely to stay open. Do not treat a red check as a merge.
- #60 (readme-status) does not yet name `HERMES_RS_MCP_GITHUB`. That is an open review note. #58's notes still say the majors are undecided. The 15:29 section above replaces that.

## Orchestrator log

Checked for this plan. Times are Europe/Zagreb (`+01:00`).

- 15:20–15:28. Local `main` is `de7480e71ce98191d2ee11f018fa15be6c5a1707`. `git fetch origin main` returned the same SHA.
- Open pull requests listed from GitHub: 22. Drafts among them include #29, #51, #52, #57. Dependabot pull requests not in #57: #2, #19, #20, #21, #22, #23, #54, #55. In #57: #24, #26, #30, #56.
- Read #51 body (decision 5), #52 body (decision 3, `check-package.sh`), #57 body (the four bumps and the `LICENSE` copy).
- Read `docs/hermes-mcp.md` at #51 head `ff9d400387f598c1c2091f84ad19c3544f9dc962`. It documents `HERMES_RS_MCP_GITHUB=1`. It does not mention port 18181 or 18080.
- Read `scripts/desktop/check-package.sh` on #52's branch. It calls `desktop_assemble_license`.
- Crate-index name diff against `crates/*/Cargo.toml`: 75 directories, 38 rows, the missing and extra names listed above. Command: a local Python listing at 2026-11-15T15:24:40+01:00.
- Root `LICENSE` is 1079 bytes and starts with `MIT License` / `Copyright (c) 3000 Galion / galion-studio`.
- GitHub search `repo:galion-studio/obsidian-open-mind is:open "Swarm 9"` and `"Swarm 9 desktop"`: `total_count` 0, before this pull request existed.
- Cloud-agent lookup for `bc-004d8f8c`: not available to this run (not found or not accessible).
- 15:30. `gh pr list` rollups recorded in the 15:29 table. Open Swarm 9 drafts: #58 `Swarm 9 desktop/old-prs-deps` (`ed3da063`, brand tables and the safe gitignore, CI still running), #60 `Swarm 9 desktop/readme-status` (`610458a6`, CI still running), #59 `Swarm 9 core plan` (`8eb597f2`).
- Read #59's plan and #58's diff. #60's README names ports 18181 and 18080 and does not name `HERMES_RS_MCP_GITHUB`. Its STATUS text calls `launch-gates.yml` and `realtest.yml` `workflow_dispatch` only; both also run on `pull_request` (read from those files on `de7480e`).
- Not run: `cargo test`, `scripts/status-percent.sh`, a GOS `LICENSE` download, CI on this plan. Those belong to the lane acceptance and to this pull request's CI.
