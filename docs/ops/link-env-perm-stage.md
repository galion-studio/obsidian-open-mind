<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# link.env permission fix: staged plan (D-318-2, slot 356)

Status: **staged only. Nothing on the VPS was changed.** No chmod, no chown, no
unit change, no restart. `link.env` was never opened. All facts are file
metadata (`stat`, `find -printf '%m %y %p'`, `systemctl show`, `/proc/*/fd`
links). Earlier analysis: `docs/ops/link-env-perms.md` (slot 166, 2026-10-21).

Re-probed read-only on 2026-10-28 05:09 to 05:15 CET (+01:00).

## Live facts (2026-10-28 05:09 CET)

| Item | Value |
|---|---|
| File | `/opt/galion/galion-light/publish/obsidian/link.env` |
| Mode / owner / size | **666** `root:root`, 107 B, inode 33909 (unchanged since 2026-10-21) |
| mtime / ctime | 2026-09-26 18:21:49 +02:00 / 2026-10-08 10:50:49 +02:00 |
| Parents | `/opt/galion` 755, `galion-light` 775, `publish` 775, `obsidian` 775, all root:root (others can traverse, not create) |
| Attributes | `lsattr`: none |
| Readers | only `play.sh` line 9 (`. ./link.env`, sourced as shell). Writer: `install.sh` line 80 (`cat > link.env << EOF`, no umask, no chmod) |
| Live service use | none. `galion-one.service` active, NRestarts 0, `EnvironmentFiles=` empty; no process has the file open; 0 processes with cwd in `publish/obsidian`; no unit or cron references it |
| Live probes | galion-one `:8091/` 200, nerve `:8670/state` 200, console `:8080/` 302; galion-one, hermes-rs, galion-nerve, console-rust-primary active (before and after) |
| Non-root uids running processes | gaia-cortex, messagebus, openmind-queue, postfix, syslog, systemd-network, systemd-resolve, tanga-proto. Any of them, if compromised, can rewrite `link.env` and get root the next time `play.sh` runs |

Cortex/nerve: the nerve answered 200 on `/state`; the cortex was not consulted
(this task only reads file metadata).

## Staged fix (owner applies, one VPS window, no restart)

### Step 1: chmod (D-318-2 option A)
```sh
F=/opt/galion/galion-light/publish/obsidian/link.env
stat -c '%a %U:%G %s %i' "$F"     # expect: 666 root:root 107 33909
chown root:root "$F"               # no-op today (already root:root); pins the owner
chmod 600 "$F"
stat -c '%a %U:%G %s %i' "$F"     # expect: 600 root:root 107 33909
systemctl is-active galion-one    # expect: active (it does not read the file)
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8091/   # same code as before (200)
```
Verify with the new check (read-only, from the box):
```sh
ssh slavik-vps "find /opt/galion/galion-light/publish -printf '%m %y %p\n'" \
  | repo-check --stat-list - --only perms
```
Before: `error perms /opt/galion/galion-light/publish/obsidian/link.env: secret-like file mode 666 is world-writable` (exit 1).
After: no link.env line.

Rollback (exact previous state; contents and mtime are not touched by chmod):
```sh
chmod 666 /opt/galion/galion-light/publish/obsidian/link.env
```

### Step 2: stop 666 coming back (galion-light source, not in this repo)
`install.sh` and `play.sh` live only on the VPS under `galion-light/publish/obsidian`.
Proposed patch (not applied):
```diff
--- install.sh
+++ install.sh
@@ line 80
-cat > link.env << EOF
+umask 077
+cat > link.env << EOF
 ...
 EOF
+chmod 600 link.env
--- play.sh
+++ play.sh
@@ line 6
-if [ -f link.env ]; then
+if [ -f link.env ]; then
+  m=$(stat -c %a link.env); case "${m#"${m%??}"}" in *[2367]*) echo "link.env is group/world-writable; run: chmod 600 link.env" >&2; exit 1;; esac
```
(`chmod 600` after the heredoc is needed too: rewriting an existing file keeps its old mode, so umask alone does not fix an existing 666 file.)

### Step 3 (optional): systemd instead of shell sourcing
Staged drop-in: `deploy/staged/link-env/galion-one-link-env.conf`
(`EnvironmentFile=-/etc/galion/link.env`, file 600 root:root). systemd parses
it as KEY=VALUE data, so a tampered file can no longer inject commands. This
**is** a live change (galion-one restart), so it is a separate owner decision.
`LoadCredential=` is not recommended today: galion-one reads the four
`GALION_*` addresses from its environment and has no code that reads
`$CREDENTIALS_DIRECTORY`; the file holds (inferred) addresses, not a secret.

## New Rust check: `repo-check` `perms`

`crates/repo-check/src/perms.rs`, std only, metadata only (never opens a flagged file).

| Rule | Level |
|---|---|
| secret-like file (`link.env`, `.env`, `.env.*`, `.envrc`, `*.env`, `*.pem`, `*.key`, `*.p12`, `id_*` keys, `authorized_keys`, `.git-credentials`, names with secret/token/passw/credential/api_key, excluding `.example`/`.sample` and source-code suffixes) other-writable | error |
| same, group-writable | warn |
| same, other-readable | warn (stat-list mode only; repo files are normally 644) |
| directory other-writable without sticky bit | error (stat-list mode) |

* Tree mode (default `repo-check`): checks file modes in the repo. Repo result unchanged: 2120 files, 0 errors, 83 warnings (same as base 5ff76fad); `--only perms` 0/0.
* `--stat-list FILE|-`: lint a `find ROOT -printf '%m %y %p\n'` listing from any host, so a host can be audited with a read-only `find` and no binary copied there. Entries under a listed dir without other-execute are skipped (`/opt/galion/hermes` is 700, so its 777 `vault/` and 666 `config.yaml` are not reachable and not flagged).

## VPS audit with the new check (2026-10-28 05:13 CET, read-only)

`find /opt/galion -xdev` (pruning `openmind`, `*/secrets`, `.git`, `node_modules`, `target`, `venv`, `__pycache__`): **104,107 entries, 22 errors, 3 warnings, exit 1.**

Errors:
* `galion-light/publish/obsidian/link.env` 666 (D-318-2, sourced as root)
* 2x `.envrc` 666 (`tooling/hermes-agent`, `galion-light/shadow/hermes-agent`): sourced by direnv if anyone uses it there
* `galion-light/tulu/llama.cpp/docs/.../idea-arch.key` 777: a Keynote file, false positive (name rule)
* 18 world-writable dirs without sticky bit: `arena/homes/g2-{0..3}/sessions`, `galion-light/hermes-home/sessions`, `secret_sources` / `vault_backends` / `get-app-token` in both hermes-agent copies, `galion-src/*/sessions`, `get-mapbox-token`

Warnings: `/opt/galion/etc/migrate-ram.env` 644 (world-readable, 1046 B, not opened), 2x `tokenwatch.yaml`.

Not flagged by the check but seen in the same pass (not secrets, still worth an owner look):
* 53 files under `/opt/galion/console-dist` are 666 (for example `static/favicon.svg`, `static/manifest.webmanifest`); `console-rust-primary`/`console.service` reference that tree, so any local uid could alter what the live console serves.
* 76,557 world-writable entries in total under `/opt/galion` (incl. node_modules, venvs): the hermes-agent Python tree, `stt/tts/vision` venvs and `hermes-home/node` were unpacked with 666/777 modes.

## Owner decisions
1. **D-318-2 yes/no**: run Step 1 (`chmod 600`, one command, no restart). Rollback `chmod 666`.
2. **D-356-1**: patch `install.sh`/`play.sh` in galion-light as in Step 2.
3. **D-356-2**: Step 3 drop-in (EnvironmentFile) yes/no; it needs a galion-one restart.
4. **D-356-3**: `chmod -R o-w` (and `+t` where shared) for the 18 dirs and the 53 console-dist files; separate window, list above.
5. **D-356-4**: confirm `migrate-ram.env` holds no secret (else `chmod 600`).
