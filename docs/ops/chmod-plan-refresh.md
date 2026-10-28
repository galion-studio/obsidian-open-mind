<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# chmod plan refresh (slot 398, 2026-10-28 16:40 to 16:46 CET +01:00)

Status: **staged only. Nothing on the VPS was changed.** No chmod, no chown,
no unit touched. Read-only `find -printf`, `stat`, `test -e`, `systemctl
show/is-active`, `/proc/*/cwd|fd` links and HTTP GETs. Branch
`swarm/chmod-plan-refresh` off `world-writable-dirs-stage` c39c73a4 (slot 379).

## Result in one line

The 69-row plan in `deploy/staged/world-writable-dirs/` is **still exactly
right**: every row is present on the VPS, mode and owner unchanged, 69/69
pending, and a fresh plan from the live listing gives byte-identical rows. **No
rows were dropped** (the plan was built at 10:18 CET, after the 10:08 CET
removal, so it never had a row under the removed trees).

## What was re-verified (16:40:14 to 16:41:50 CET)

| Check | Result |
|---|---|
| Live owner-aware listing (same `find` as slot 379) | 78,221 entries (10:18: 78,219) |
| `repo-check --plan-verify chmod-plan.tsv` | exit 3: 69 rows, 0 applied, **69 pending, 0 drift/missing/owner** |
| Fresh `--chmod-plan --plan-tree /opt/galion/console-dist --plan-reasons dir-o+w-no-sticky,tree-normalize` | 69 rows, **identical** to the staged rows |
| F-379-1 paths `tooling/hermes-agent`, `galion-light/shadow/hermes-agent` (+ their `venv`) | **GONE**; `galion-light/shadow/` empty; `tooling` and `shadow` mtime 10:08:04/10:08:05 CET |
| `link.env` (D-318-2, applied 16:05 CET) | now **600** root:root; no longer flagged, no longer in any plan |
| Full perms lint of the listing | 17 errors (11 dirs + 5 srvx dirs + `idea-arch.key` 777) + 1 warn (`etc/migrate-ram.env` 644) |
| Process cwd/fd under planned paths | 0 |
| Live after probes | console, console-rust-primary, galion-nerve, gaia-cortex active; :8080 and :18080 `favicon.svg` 200, `manifest.webmanifest` 200; nerve :8670/state 200; cortex :8674/health 200; galion-one :8091 200 |

Both console units still run `galion-console-serve-2d846fb8` with
`ReadOnlyPaths=/opt/galion/console-dist`, so the 13 static files (666) are
still served live; the reasoning of slot 379 (no restart needed, root
ignores mode bits) still holds.

## New finding: trees the plan's `find` prunes (F-398-1)

The plan listing prunes `venv`, `node_modules` (except console-dist), `target`
and `__pycache__`. Counted read-only inside them:

| Tree | o+w dirs (no sticky) | o+w files | reachable by other uids? |
|---|---|---|---|
| `/opt/galion/console-ide/node_modules` | 5,084 | 65,116 (198 executable) | **no**: `node_modules` itself 700 root |
| `/opt/galion/tooling/node/lib/node_modules` | 568 | 2,433 (71 exec) | **no**: `tooling/node` 700 |
| `/opt/galion/galion-light/hermes-home/node/lib/node_modules` | 456 | 1,926 (34 exec) | **no**: `hermes-home/node` 700 |
| `/opt/galion/tts/venv` | 1 | 8 | **yes** (`tts` 775, `venv` 775) |
| `/opt/galion/stt/venv` | 0 | 3 | **yes** |
| `/opt/galion/vision/venv` | 0 | 5 | **yes** |

All owned by root. No running unit uses `console-ide`, `tooling/node` or
`hermes-home/node` (ExecStart/WorkingDirectory scan); `console-ide` (336 MB
node_modules) is the old TypeScript console, retired by the Rust cutover.

The 17 reachable venv entries are the names the secret rule would flag
(`_git_credential.py`/`.pyc`, `_credentials.py`/`.pyc`,
`pip/_vendor/certifi/cacert.pem`, `transformers/models/vaultgemma/*`) and one
dir `vaultgemma` 777. A non-root uid could rewrite pip's CA bundle or a module
the venv imports; if any of those venvs runs again as root, that is code
execution as root. Forcing these to 600 (the secret rule) would break
non-root readers, so a new plan mode removes only the `o+w` bit.

### New: `repo-check --plan-ow-tree DIR` (reason `tree-o-w`)

Inside DIR every other-writable dir or file (never a symlink or special) gets
`mode & ~o+w`: 777 -> 775, 666 -> 664, 1777 -> 1775; group bits, x bits and
sticky are kept; secret-like names inside are NOT forced to 600; `--plan-tree`
wins when a path is under both. Same guarded `--sh`, `--rollback`,
`--plan-verify` as the rest.

### Supplementary staged plan: `deploy/staged/world-writable-dirs/venv-o-w/`

From the 16:42 CET venv listing (35,490 entries): **17 rows** (1 dir 777 ->
775, 16 files 666 -> 664), all root:root, 0 hidden; `apply.sh` / `rollback.sh`
(same guards: type + exact mode + owner, else `SKIP`), `sh -n` ok,
`--plan-verify` 17 pending. NOT applied.

Apply (owner, VPS root, after or with D-356-3a):
```sh
sh deploy/staged/world-writable-dirs/venv-o-w/apply.sh      # expect 17 changed
sh deploy/staged/world-writable-dirs/venv-o-w/rollback.sh   # exact previous modes
```

## Owner decisions

1. **D-356-3a** (unchanged, re-verified): run the 69-row `apply.sh`. The plan
   is current as of 16:40 CET; re-run `--plan-verify` right before applying.
2. **D-398-1**: apply the 17-row `venv-o-w/apply.sh` (o-w only on the
   tts/stt/vision venvs). Rec: yes, same window as D-356-3a. Alternative:
   archive the three Python venvs on the Python-retire track.
3. **D-398-2**: the three 700-hidden node trees (70k+ o+w entries) are safe
   only while their parent stays 700. Rec: archive/delete
   `console-ide/node_modules` (TS console retired); for the others either
   delete with the Python/Node retire track or `chmod -R o-w` (guarded by a
   plan built with `--plan-ow-tree` from an unpruned listing). Not staged here.
4. **D-398-3**: `idea-arch.key` 777 (Keynote file, secret-rule false
   positive) and `etc/migrate-ram.env` 644 warn: rec chmod 644 / check
   content owner-side; not in any plan.
5. **F-379-1**: removal of the legacy Hermes trees is confirmed still in
   place at 16:41 CET; owner still to confirm it was intended.
