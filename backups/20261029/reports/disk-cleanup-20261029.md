# Disk cleanup 2026-10-29 (owner approval t351u)

Run 08:14-08:21 CET by the cleanup worker. df -h / before: 123G used / 4.6G free (97%) at 08:14. After: 89G used / 39G free (70%) at 08:20.
Untouched: /workspace/cargen, /opt/galion/openmind (no git run there). No force-push, no git push of any kind, no Auto-review retries.

## Protected set (written before any deletion)
    # Protected set recorded 2026-10-29 08:16 CET before any deletion
    Running processes: gate loop pid 2220805 (cwd /workspace/swarm/logs/v1029, gate.sh over nerve-merge-9-redact console-studio-names-retire nerve-merge-9 hub-incident-redact cortex-escalations-persist hermes-gateway-recipe)
      cargo test pid 2472654 cwd wt/hermes-gateway-recipe CARGO_TARGET_DIR=target/v1029-hermes-gateway-recipe
      timeout/cargo test real_deploy_recipes pid 2435560/2435561 cwd wt/nerve-merge-9-redact CARGO_TARGET_DIR=target/v1029-nerve-merge-9-redact
      test binary 2436370 -> cargo build in /tmp/nerve-recipes-2436370 (CARGO_TARGET_DIR under /tmp)
    Protected target dirs: target/v1029-* (all 10), target/hub-incident-redact (gate.sh override), target/console-mic-ruling, target-nm4-real (not in scope)
    Protected worktrees: wt/nerve-merge-9-redact wt/console-studio-names-retire wt/nerve-merge-9 wt/hub-incident-redact wt/cortex-escalations-persist wt/hermes-gateway-recipe wt/console-mic-ruling
    Also protected: anything modified in the last 60 min; /tmp/nerve-recipes-*; /workspace/swarm/artifacts; /workspace/cargen

## 1. /workspace/swarm/target (~17.7G freed by this cleanup)
Removed (stale cargo caches, newest file 27-28 Oct, no process using them): cortex-escalations-diag 5.0G, nerve-evt-mode-port 12G, cortex-r3-planned-stop-base 615M, cortex-r3-planned-stop-nerve 91M, opt-bin-py 46M.
Kept: all v1029-* dirs, hub-incident-redact 23G (gate.sh points CARGO_TARGET_DIR there; its debug/ was written at 08:11), console-mic-ruling. Also kept the small non-cache items: cortex-r3-planned-stop-{base-src,nerve-src,data} (source snapshots and data, 140M), release-repro-unify-work 31M, psp-*.log, r325-logs, r328-logs.
Note: the deploy worker removed its own finished dirs v1029-console-base, v1029-console-studio-names-retire and v1029-cortex-escalations-persist around 08:15. This cleanup did not remove them.

## 2. /workspace/swarm/wt (15.25G -> 5.9G, ~9.3G freed)
Rule: delete only if git status is clean, HEAD + all local branches + stash are reachable from GitHub branch tips (452 branches listed via the GitHub API, saved in gh-branches-20261029.txt), no linked worktrees, nothing modified in the last 60 min outside .git, and the worktree is not protected. All checks were repeated just before each delete.
Docs copied first to reports/kept-docs/: ws-voice.md (from voice-mouth-stuck @ cab4a6b0, which was removed afterwards) and escalations-lifecycle.md (cortex-escalations-persist is kept).
Removed 123 worktrees (clones were removed with rm -rf; the linked console-v2-merge-1 and nerve-restore-component were removed with git worktree remove, then prune). The full list with branch and HEAD SHA is in cleanup-work/wt-removed.txt.
Per-worktree target/ dirs removed from kept worktrees:
- 2613M cortex-staging-fold-reword/target
- 323M hades-adversarial/target
- 360M hades-rustflags-guard/target
- 388M hades-safety-corpus/target

### Kept: uncommitted changes and/or unpushed commits
- android-admin-plan (dirty=0, unpushed= HEAD:1 swarm/android-admin-plan:1, links=0)
- apache-allowlist-memo (dirty=17, unpushed=none, links=0)
- backup-restore-parity (dirty=17, unpushed=none, links=0)
- backup-rust-unit-prep (dirty=17, unpushed=none, links=0)
- cargo-deny-gate (dirty=17, unpushed= dependabot/cargo/rusqlite-0.40.2:1 dependabot/cargo/toml-1.1.8spec-1.1.0:1 dependabot/github_actions/actions/checkout-7:1 dependabot/github_actions/actions/upload-artifact-7:1 dependabot/github_actions/github/codeql-action-4:1 dependabot/github_actions/ossf/scorecard-action-2.5.0:1 swarm/hades-adversarial:1 swarm/release-size-trim:1, links=0)
- cf-403-evidence (dirty=17, unpushed=none, links=0)
- ci-artefact-rehearsal (dirty=17, unpushed=none, links=0)
- ci-fuzz-workflow (dirty=17, unpushed=none, links=0)
- ci-realtest-with-nerve (dirty=17, unpushed=none, links=0)
- ci-release-artefacts (dirty=17, unpushed=none, links=0)
- console-pairing-e2e (dirty=10, unpushed=none, links=0)
- console-restart-loop-diag (dirty=10, unpushed=none, links=0)
- console-rust-serve-impl (dirty=10, unpushed= dependabot/cargo/rusqlite-0.40.2:1 dependabot/cargo/toml-1.1.8spec-1.1.0:1 dependabot/github_actions/actions/checkout-7:1 dependabot/github_actions/actions/upload-artifact-7:1 dependabot/github_actions/github/codeql-action-4:1 dependabot/github_actions/ossf/scorecard-action-2.5.0:1 swarm/hades-adversarial:1 swarm/release-size-trim:1, links=0)
- console-rust-serve (dirty=10, unpushed=none, links=0)
- console-session-pack (dirty=10, unpushed=none, links=0)
- console-v2-audit (dirty=10, unpushed= swarm/main-staging:90, links=0)
- console-v2-css-budget (dirty=10, unpushed=none, links=2)
- console-v2-merge-2 (dirty=0, unpushed= HEAD:3 swarm/console-v2-merge-2:3, links=0)
- console-v2-sprite-health (dirty=10, unpushed=none, links=0)
- cortex-api-load (dirty=17, unpushed= swarm/cortex-api-load-live:1, links=0)
- cortex-d245-pack (dirty=17, unpushed=none, links=0)
- cortex-event-driven (dirty=17, unpushed=none, links=0)
- cortex-flap-dedupe (dirty=17, unpushed=none, links=0)
- cortex-hermes-boss (dirty=17, unpushed=none, links=0)
- cortex-incident-backfill (dirty=17, unpushed=none, links=0)
- cortex-live-fixes (dirty=17, unpushed=none, links=0)
- cortex-live-merge-2 (dirty=17, unpushed=none, links=1)
- cortex-live-merge-3 (dirty=18, unpushed=none, links=0)
- cortex-nerve-io-unlock (dirty=17, unpushed=none, links=0)
- cortex-oom-onto-bd6fbe6 (dirty=17, unpushed=none, links=0)
- cortex-r3-planned-stop (dirty=0, unpushed= HEAD:2 swarm/cortex-r3-planned-stop:2, links=0)
- cortex-sandbox-oom-info (dirty=17, unpushed=none, links=0)
- cortex-staging-fold-reword (dirty=0, unpushed= dependabot/cargo/rusqlite-0.40.2:1 dependabot/cargo/toml-1.1.8spec-1.1.0:1 dependabot/github_actions/actions/checkout-7:1 dependabot/github_actions/actions/upload-artifact-7:1 dependabot/github_actions/github/codeql-action-4:1 dependabot/github_actions/ossf/scorecard-action-2.5.0:1 swarm/hades-adversarial:1 swarm/release-size-trim:1, links=0)
- cortex-staging-fold (dirty=0, unpushed= HEAD:ERR swarm/cortex-staging-fold:ERR, links=0)
- cortex-t31-calibrate (dirty=17, unpushed=none, links=0)
- cortex-t44-e2e (dirty=17, unpushed=none, links=0)
- cortex-t51-chaos (dirty=17, unpushed=none, links=0)
- cortex-t53-replay-corpus (dirty=17, unpushed=none, links=0)
- cpu-only-rust (dirty=17, unpushed=none, links=0)
- crashloop-r3-rebase (dirty=1, unpushed=none, links=0)
- crate-publish-hygiene (dirty=17, unpushed=none, links=0)
- desk-py-callers-audit (dirty=8, unpushed= refs/stash:3, links=0)
- docs-publish-readme (dirty=17, unpushed=none, links=0)
- docs-status-gates-20261027 (dirty=17, unpushed=none, links=0)
- docs-status-refresh (dirty=17, unpushed=none, links=0)
- fail2ban-nft-memo (dirty=17, unpushed=none, links=0)
- gaia-heal-live-shadow (dirty=17, unpushed=none, links=0)
- gaia-lan-fallback-test (dirty=17, unpushed=none, links=0)
- gaia-live-classify (dirty=17, unpushed=none, links=0)
- gaia-poseidon-handle (dirty=17, unpushed=none, links=0)
- gaia-queue-fuzz (dirty=17, unpushed=none, links=0)
- gaia-realtest-scenarios (dirty=17, unpushed=none, links=0)
- hades-adversarial (dirty=11, unpushed= HEAD:1 swarm/hades-adversarial:1, links=0)
- hades-key-custody (dirty=17, unpushed=none, links=0)
- hades-rustflags-guard (dirty=17, unpushed=none, links=0)
- hades-safety-corpus (dirty=17, unpushed=none, links=0)
- hermes-gateway-rss (dirty=17, unpushed=none, links=0)
- hermes-offhost-review (dirty=0, unpushed= pr14-repro:2, links=0)
- hermes-telegram-notify (dirty=17, unpushed=none, links=0)
- int5-candle-repocheck (dirty=17, unpushed=none, links=0)
- legacy-tooling-archive-plan (dirty=17, unpushed=none, links=0)
- main-merge-dryrun (dirty=10, unpushed=none, links=0)
- main-staging (dirty=84, unpushed= HEAD:4 swarm/main-staging:4, links=0)
- memory-schema-mergefix (dirty=10, unpushed=none, links=0)
- memory-search-local-time (dirty=0, unpushed= HEAD:1 swarm/memory-search-local-time:1, links=0)
- mouth-honest-health (dirty=17, unpushed=none, links=0)
- mouth-honest-stub-rs (dirty=17, unpushed=none, links=0)
- nerve-cross-target (dirty=17, unpushed=none, links=0)
- nerve-deploy-t273u (dirty=18, unpushed=none, links=0)
- nerve-event-driven (dirty=17, unpushed=none, links=0)
- nerve-flood-baseline (dirty=17, unpushed=none, links=0)
- nerve-flood-cycle-gap (dirty=17, unpushed=none, links=0)
- nerve-journal-reader-rs (dirty=17, unpushed=none, links=0)
- nerve-merge-6 (dirty=0, unpushed=none, links=1)
- nerve-own-services (dirty=17, unpushed=none, links=0)
- nerve-parser-fuzz (dirty=17, unpushed=none, links=0)
- nerve-publish-recoveries (dirty=17, unpushed=none, links=0)
- nerve-recoveries-push (dirty=17, unpushed=none, links=0)
- nerve-t23-specs (dirty=17, unpushed=none, links=0)
- nerve-t24-coverage-gate (dirty=17, unpushed=none, links=0)
- nerve-t24-wire-gaps-3 (dirty=17, unpushed=none, links=0)
- nerve-t24-wire-gaps (dirty=17, unpushed=none, links=0)
- nerve-t32-reflex-safety (dirty=17, unpushed=none, links=2)
- nerve-t33-backpressure (dirty=17, unpushed=none, links=0)
- nerve-t41-statedir (dirty=17, unpushed=none, links=0)
- nerve-t61-incidents (dirty=17, unpushed=none, links=0)
- no-openrouter-mergefix (dirty=17, unpushed=none, links=0)
- non-rust-inventory-refresh (dirty=10, unpushed=none, links=0)
- openmind-pin-drift (dirty=1, unpushed=none, links=0)
- opt-bin-py-port (dirty=17, unpushed=none, links=0)
- os-timers-trim-memo (dirty=17, unpushed=none, links=0)
- overnight-loop-20261027 (dirty=0, unpushed= dependabot/cargo/rusqlite-0.40.2:1 dependabot/cargo/toml-1.1.8spec-1.1.0:1 dependabot/github_actions/actions/checkout-7:1 dependabot/github_actions/actions/upload-artifact-7:1 dependabot/github_actions/github/codeql-action-4:1 dependabot/github_actions/ossf/scorecard-action-2.5.0:1 swarm/hades-adversarial:1 swarm/release-size-trim:1, links=0)
- poseidon-auth (dirty=17, unpushed=none, links=0)
- poseidon-crash-soak (dirty=17, unpushed=none, links=0)
- pr12-ci-green (dirty=17, unpushed= HEAD:94 pr12-ci-green-local-5420313:92 swarm/pr12-ci-green:94, links=0)
- release-artifacts-matrix (dirty=17, unpushed=none, links=0)
- release-artifacts-refresh (dirty=0, unpushed= HEAD:2 swarm/release-artifacts-refresh:2, links=0)
- release-changelog-draft (dirty=10, unpushed=none, links=0)
- release-repro-unify (dirty=0, unpushed= HEAD:4 swarm/release-artifacts-refresh:2 swarm/release-repro-unify:4, links=0)
- research-mit-reuse (dirty=17, unpushed=none, links=0)
- reuse-gate-fix (dirty=17, unpushed=none, links=0)
- safetensors-mit-alt-baseline (dirty=19, unpushed=none, links=0)
- sandbox-dsb-pack (dirty=0, unpushed= HEAD:1 swarm/sandbox-dsb-pack:1, links=0)
- sec-denylist-parity (dirty=17, unpushed=none, links=0)
- sec-ledger-race (dirty=17, unpushed=none, links=0)
- sec-testnet-fixtures (dirty=17, unpushed=none, links=0)
- sec-tooling-licence-memo (dirty=17, unpushed=none, links=0)
- sec-unsafe-safety (dirty=17, unpushed=none, links=0)
- serde-pin-mergefix (dirty=17, unpushed=none, links=0)
- shared-tar-mergefix (dirty=18, unpushed=none, links=0)
- slavik-todo-oct22 (dirty=10, unpushed=none, links=0)
- staging-ff-land (dirty=18, unpushed= HEAD:4 swarm/main-staging:4, links=0)
- staging-ff-mergechecks (dirty=17, unpushed=none, links=0)
- staging-ff-plan-20261027 (dirty=17, unpushed=none, links=0)
- staging-rehearsal-1028-gate (dirty=0, unpushed= r407-attempt1:1, links=0)
- staging-rehearsal-1028 (dirty=0, unpushed= r407-attempt1:1, links=1)
- survival-s47-install (dirty=19, unpushed=none, links=0)
- t273u-rulings (dirty=17, unpushed=none, links=0)
- vps-live-table (dirty=17, unpushed=none, links=0)
- vps-rollback-table (dirty=17, unpushed=none, links=0)
Most dirty=17 or dirty=10 cases are only deleted tracked dist/ or edge/dist/ binaries, so they can be restored from git. They were still kept, as the rule requires.
The 'unpushed dependabot/...' entries are stale local dependabot branch refs.

### Also kept: protected or other repos
- Protected: nerve-merge-9-redact, console-studio-names-retire, nerve-merge-9, hub-incident-redact, cortex-escalations-persist, hermes-gateway-recipe, console-mic-ruling
- Kept to be conservative: galion-os-admin-cli, galion-os-box-escape, galion-os-image-format, galion-os-threat-model, publish-blocker-memo. These are linked worktrees of galion-os-repo, about 1M each. Also gff-ev1.keep (a file).

## 3. /workspace/oom-models (4.9G, nothing removed)
Contents: chatterbox 3.0G, whisper-large-v3-turbo 1.6G, whisper-tiny.en 147M (sha db596959... matches ws-voice.md), kitten-tts-nano 57M (sha b45307e7... matches), cmudict 3.5M, opus-testvectors 121M, kitten-out 1.9M (wav outputs). There were no partial downloads or duplicates.
Kept whisper-tiny.en, kitten-tts-nano and cmudict: the galion-voice-cpu defaults use them.
chatterbox and whisper-large-v3-turbo were NOT deleted because current code still references them. galion-voice-serve takes --tts chatterbox and --stt whisper-large-v3-turbo (the 'legacy ear'). There are tests/real_chatterbox.rs and the galion-chatterbox crate, and real_voice_rust and real_int5_gates read GALION_VOICE_MODELS=/workspace/oom-models. Deleting them (4.6G) is an owner call.
opus-testvectors are kept because ws-voice.md lists them as the voice test inputs. kitten-out is kept too: it holds the symbol-table comparison wavs and is tiny.

## 4. Verification after cleanup (08:20 CET)
- All 7 protected worktrees exist at the same HEADs: nerve-merge-9-redact f66974c3, console-studio-names-retire a96e4c6e, nerve-merge-9 4ebd4260, hub-incident-redact d19f643e, cortex-escalations-persist 4b036bc9, hermes-gateway-recipe 0df394ce, console-mic-ruling 50f53a38.
- Remaining protected targets exist: hub-incident-redact, console-mic-ruling, v1029-{console-rel,cortex-real,cortex-rel,hermes-gateway-recipe,hermes-gateway-recipe-recipe-test,nerve-merge-9,nerve-merge-9-redact,nerve-rel}.
- The deploy worker is still running: cargo test -p xtask real_release_recipe (pid 2680349, cwd wt/hermes-gateway-recipe, CARGO_TARGET_DIR target/v1029-hermes-gateway-recipe).
- The gate loop finished (ALLDONE). Its hermes-gateway-recipe rerun ended at 08:18 with 2 test failures, systemd_backend_scenarios and default_and_fixture_do_not_open_a_socket. Both are test logic using /tmp fixtures, not missing files from this cleanup. The first run at 08:15:00 failed before any deletion, while the disk had only 4.6G free.
- Stray: pids 3197472/3197717 (an old galion_desk --list pipeline) have cwd in an already-deleted target/pr12-flake-bundle. They were left alone.
