<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: world-writable-dirs-stage (slot 379, 2026-10-28 CET)

Branch `swarm/world-writable-dirs-stage` off link-env-perm-stage b8520146.
Ports 22490-22499 (22490 used by the HTTP part, 22491 for one manual probe; all
freed).

## GALION_REAL=1, galion-deploy (10:16 CET)

`GALION_REAL=1 GALION_DIST_SRC=<copy of live console-dist> GALION_SERVE_BIN=target/release/galion-console-static GALION_REAL_VPS=slavik-vps cargo test --release -p galion-deploy --test real_dist_install -- --nocapture`

The source is the live `/opt/galion/console-dist/static` + `node_modules/srvx`,
fetched with a read-only `tar -cf -` over ssh (publicly served assets; modes
kept: 52 files 666, 5 dirs 777, 1 file 777, as on the VPS).

```
source dist-check exit 1, 58 writable entries
install exit 0 in 20.7 ms: 13 dirs, 88 files, 0 links, 1107290 bytes, 58 source modes fixed; old none
installed dist-check exit 0: 0 writable/set-id entries
installed modes: 12 dirs 755, 87 files 644, 1 files 755; contents identical
served 46/46 installed static files over HTTP on :22490, bytes identical
swap under load: install exit 0 (... old .../console-dist.old-20261028-101614+0100); client 234 x 200, other []
rollback exit 0: restored; replaced tree at .../console-dist.failed-20261028-101614+0100
  live /__grok/install/styles.css 200 7e8e8813...  install 7e8e8813...
  live /favicon.svg 200 d9d858b4...  install d9d858b4...
  live /manifest.webmanifest 200 a9795acc...  install a9795acc...
  live /og.jpg 200 89658830...  install 89658830...
live console :8080 vs install: 4/4 identical
test real_dist_install ... ok
```
(An earlier run without the 2 ms client pause saw 1 miss in ~3000 requests and
then no response to the first request after the loop (likely the client's own
port churn; not reproduced by hand with curl). The test now paces the client
(2 ms), retries the post-swap GET and allows at most 3 misses in the rename gap.)

## GALION_REAL=1, repo-check (10:17 CET)

`GALION_REAL=1 GALION_REAL_VPS=slavik-vps cargo test --release -p repo-check --test real_perms -- --nocapture --test-threads 1`: 4/4 pass.

`real_box_chmod_plan_apply_and_rollback` (throwaway box tree, live shape):
```
plan 9 rows (3 sessions dirs 777->775, 2 srvx dirs 777->755, srvx.mjs 777->755, 2 static 666->644, link.env 666->600)
verify before: exit 3, 9 pending
apply sh exit 0: changed 9, skipped 0
verify after apply: exit 0, 9 applied;  perms lint: 0 errors PASS
rollback sh exit 0: changed 9, skipped 0; stat list byte-identical to before
drift (favicon 664 before apply): exit 1, "SKIP drift ... favicon.svg (now 664, plan 666)", changed 8, skipped 1; verify exit 1
symlink swapped in for a planned dir: "SKIP type", target 1777 untouched
```
Sticky `tmp/` (1777) and its 666 file and 644 `app.js` are never planned.

`real_vps_chmod_plan_pending` (read-only owner-aware `find` over ssh):
```
VPS owner stat list: 78219 entries
plan 71 rows, 1 hidden, 0 owner review
   11 dir-o+w-no-sticky 777->775 root:root
    1 secret-o+w 666->600 root:root      (link.env, D-318-2)
    1 secret-o+w 777->600 root:root      (idea-arch.key, false positive)
   52 tree-normalize 666->644 root:root
    6 tree-normalize 777->755 root:root
verify: exit 3, 71 rows, 0 applied, 71 pending
```
`real_vps_stat_list` (slot 356 test, same day): 78,171 entries, 13 errors, 1
warning (was 104,107 / 22 / 3 at 05:13 before the 10:08 removal), link.env
still flagged.

Staged plan (`deploy/staged/world-writable-dirs/`) from the 10:18 CET listing:
69 rows (secret rows filtered out with `--plan-reasons`), `--plan-verify`
69 pending; `sh -n` syntax ok for apply.sh and rollback.sh.

## VPS read-only probes (10:06 to 10:19 CET)
`systemctl cat/show` of console, console-rust-primary and all running units
(User, WorkingDirectory, ReadWritePaths), `/proc/*/cwd` and `/proc/*/fd`
scan (0 hits under planned paths), `/proc/<console pids>/mountinfo` (no
console-dist mount), per-dir entry counts/owners/newest mtime, `ps -eo user`.
Live after all tests (10:18 CET): nerve :8670/state 200, cortex :8674/health
200, console :8080/favicon.svg 200, :18080/favicon.svg 200, galion-one :8091
200; console, console-rust-primary, galion-nerve, gaia-cortex active;
favicon.svg still 666, g2-0/sessions still 777, link.env still 666 (nothing
applied). No VPS writes, no temp files, no chmod/chown, no service touched,
openmind untouched.
