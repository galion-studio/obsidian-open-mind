<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: chmod-plan-refresh (slot 398, 2026-10-28 CET +01:00)

Branch `swarm/chmod-plan-refresh` off world-writable-dirs-stage c39c73a4. No
ports used (CLI only; range 22680-22689 untouched). VPS read-only.

## GALION_REAL=1, new `real_plan_refresh` (16:44:48 to 16:45:18 CET)

`GALION_REAL=1 GALION_REAL_VPS=slavik-vps cargo test --release -p repo-check --test real_plan_refresh -- --nocapture --test-threads 1`: **3/3 pass**
```
real_vps_legacy_hermes_trees_gone:
  GONE /opt/galion/tooling/hermes-agent
  GONE /opt/galion/galion-light/shadow/hermes-agent
  0                      (entries left in galion-light/shadow)
real_vps_staged_plan_still_valid:
  VPS owner stat list: 78221 entries
  verify exit Some(3): {"pending": 69}
  plan-verify 2026-10-28T16:45:14+01:00: 69 rows, 0 applied, 69 pending, 0 drift/missing/owner
  regenerate exit Some(0): repo-check: plan 69 rows, 1 hidden, 0 owner review
  regenerated rows identical to the staged plan (69)
real_vps_venv_ow_plan:
  VPS venv stat list: 35490 entries
  plan exit Some(0): repo-check: plan 17 rows, 0 hidden, 0 owner review
      1 d 777->775 root:root
     16 f 666->664 root:root
```

## GALION_REAL=1, existing `real_perms` (16:43:55 to 16:44:17 CET)

`GALION_REAL=1 GALION_REAL_VPS=slavik-vps cargo test --release -p repo-check --test real_perms -- --nocapture --test-threads 1`: **4/4 pass**
```
real_box_chmod_plan_apply_and_rollback: apply/verify/rollback/drift on a box tree ok
  (drift run: changed 8, skipped 1, verify exit 1)
real_vps_chmod_plan_pending: 78221 entries, plan 70 rows
   11 dir-o+w-no-sticky 777->775   1 secret-o+w 777->600 (idea-arch.key)
   52 tree-normalize 666->644      6 tree-normalize 777->755
   verify exit 3: 70 pending, 0 drift/missing/owner
   (10:18 CET: 71 rows; the missing row is link.env, applied 600 at 16:05 CET)
real_vps_stat_list: 78173 entries, 12 errors, 1 warning;
   "link.env not flagged (fix applied or file moved)"
```

## Manual read-only probes (16:40:14 to 16:42 CET)

* Listing 16:40:14-16:40:22: 78,221 entries; `--plan-verify` on the staged
  plan 69 pending / 0 bad; fresh plan rows `diff` identical.
* `stat`: `link.env` 600 root:root; `/opt/galion/hermes` 700, `hermes/vault`
  777 (hidden, unchanged); `tooling` and `galion-light/shadow` 775, mtime
  2026-10-28 10:08:04/10:08:05 CET.
* Pruned trees (counts only): see docs/ops/chmod-plan-refresh.md F-398-1;
  `console-ide/node_modules`, `tooling/node`, `hermes-home/node` all 700.
* Units: console + console-rust-primary active (galion-console-serve-2d846fb8,
  `ReadOnlyPaths=/opt/galion/console-dist`), galion-nerve active
  (openmind-nerve-9d1a4f3), gaia-cortex active (gaia-cortex-73583ab-llm, user
  gaia-cortex). HTTP: :8080/favicon.svg 200, :18080/favicon.svg 200,
  :8080/manifest.webmanifest 200, nerve :8670/state 200, cortex :8674/health
  200, :8091/ 200. `/proc` cwd/fd hits under planned paths: 0.
* After all probes: `favicon.svg` still 666, `g2-0/sessions` still 777
  (nothing applied). No VPS writes, no temp files, openmind untouched.
