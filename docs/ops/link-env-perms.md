<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# link.env permissions: proposal (owner decision, docs only)

File: `/opt/galion/galion-light/publish/obsidian/link.env` on the VPS.
Status: **proposal only. Nothing on the VPS was changed.** The mode, owner and
contents were not touched, and the file was never opened, read or printed. All
facts below come from `stat`, `ls`, `systemctl show`, `/proc/<pid>/fd` links,
`ss -ltnp`, and reads of the *other* files around it. The chmod is Slavik's call.

Probed read-only over ssh on 2026-10-21 08:12:01 to 08:15:11 CEST (+02:00), slot 166.

## TL;DR
- `link.env` is **666 root:root, 107 bytes**. Any local uid can read it, and any
  local uid can **write** it.
- **The bigger risk is the write.** `play.sh` sources it as shell code
  (`set -a; . ./link.env`). Anyone who can write the file gets their commands
  run as whoever next runs `play.sh` or `install.sh`, which is root on this host.
  S8 (334c633) only measured the read side.
- **No live service reads it.** The live `galion-one.service` (pid 1373612)
  does not load it: there is no `EnvironmentFile`, nothing has it open, and no
  process has its cwd in `publish/obsidian`. Only the manual `play.sh` reads it,
  and only `install.sh` writes it.
- **Recommended: Option A, `chmod 600` (keep root:root).** It has no impact on
  live services and can be rolled back with one command.

## Facts (metadata only)

| Item | Value |
|---|---|
| Mode / owner | `-rw-rw-rw-` 666 `root:root` |
| Size | 107 bytes, 1 link, inode 33909 |
| mtime | 2026-09-26 18:21:49 +02:00 (same minute as `install.sh`, `play.sh`, `README.md`, `.gitignore`) |
| ctime | 2026-10-08 10:50:49 +02:00 (a later metadata change, cause unknown) |
| Siblings' ctime | 2026-10-18 20:03:58 +02:00 for every other entry. That metadata pass skipped `link.env`, which is the one git-ignored file there (`.gitignore` line 13) |
| Siblings' modes | files 664/644, scripts and dirs 775, all root:root. `link.env` is the only 666 file |
| Parent dirs | `/opt/galion` 755; `galion-light`, `publish`, `obsidian` 775 root:root. Only root can create, rename or delete entries, but the 666 mode lets others rewrite the file in place |
| Attributes / ACL | `lsattr`: none. `getfacl` is not installed |
| Other copies | none under `/opt/galion` (find, excluding `openmind` and `secrets`) |
| Open handles | none (`/proc/*/fd` scan; `lsof`/`fuser` show nothing) |
| In backups | no. `/opt/galion/rust/bin/galion-backup.sh` `FILES=` does not include `galion-light/publish` |

### What the 107 bytes most likely are (inferred, not read)
`install.sh` (lines 71 to 84) writes exactly four lines:
`GALION_EAR=<host:port>`, `GALION_MOUTH=…`, `GALION_MIND=…`, `GALION_EYE=…`.
The README calls them "addresses". With the installer defaults
(`127.0.0.1:8643`, `:8002`, `:8642`, `:8645`), that output is
25+27+26+25 characters plus 4 newlines, which is **exactly 107 bytes**. So the
file most likely holds four loopback addresses and **no credential**. This is
only an inference from the size. Nobody has looked at the contents, and the
owner can confirm with one look.

## Who reads it, who writes it

| Actor | Access | Evidence |
|---|---|---|
| `publish/obsidian/play.sh` | **reads** it by sourcing it as shell (`if [ -f link.env ]; then set -a; . ./link.env; set +a; fi`), then `rustc` builds `one/main.rs` and execs `./one/one` on :8091 | file text (lines 6 to 11) |
| `publish/obsidian/install.sh` | **writes** it (`cat > link.env << EOF`, lines 80 to 85), then `exec ./play.sh` unless :8091 is already listening | file text. It has no `umask` and no `chmod`, so a new file gets the caller's umask, and rewriting an existing file keeps its mode |
| `galion-one` (live, `galion-one.service`, pid 1373612, active since 2026-10-18 22:13:56 +02:00, exe `/opt/galion/openmind/dist/bin-ws/galion-one`, cwd `/opt/galion/galion-light`, runs as root, ProtectSystem=full) | **does not read it** | `EnvironmentFiles` is empty. The unit's `Environment=` names are `GALION_HOME`, `ONE_BIND`, `ONE_MOUTH_BIND`, `ONE_DEVICE_STORE`, `ONE_HERMES_DIR`, `ONE_HERMES_ENV`, `OPENROUTER_KEY_FILE` (values not printed), with no `GALION_EAR/MOUTH/MIND/EYE`. `crates/galion-one/src/main.rs` `endpoint()` reads those four only from the process env and falls back to default ports |
| other systemd units | none | `grep -rl 'link.env\|publish/obsidian'` over `/etc/systemd`, `/lib/systemd/system`, `/usr/lib/systemd/system` and `/run/systemd/system` finds nothing. No `EnvironmentFiles` entry on any service points at it |
| cron | none | `/etc/cron*` and `/var/spool/cron` contain no reference |
| any other code | none | a wide grep of `*.sh/*.service/*.timer/*.toml/*.rs/*.py/*.ts/*.js/*.json` under `/opt/galion` (excluding `openmind`, `secrets`, `archive`, `from-grok-build`, `.git`, `node_modules`, `target`), `/usr/local/bin` and `/etc` finds only `play.sh`, `install.sh`, `.gitignore` and `README.md` |
| sandboxed tool runs (Hermes tools) | could read it before the fix. Writes were already blocked by the read-only bind | 334c633 S8: ESCAPED readable. ad0ed9b hides `/opt/galion` in every tool preset (0 of 48 readable on the VPS), **but that fix is not deployed yet** (no Hermes build with ad0ed9b is live) |

Non-root uids that currently run processes on the host are `tanga-proto`
(ProtectSystem=strict, so it cannot write `/opt`), `postfix`, `syslog`,
`systemd-resolve`, `systemd-network` and `messagebus`. There is also a
`galion-offsite` account (uid 999). Any of these, if compromised, could
rewrite `link.env`. There are no interactive non-root login users.

## Risk
1. **Integrity / root code execution (main issue).** Because the file is
   world-writable and is sourced by `play.sh`, a write by any uid becomes code
   that runs as root the next time someone runs `./play.sh` or
   `./install.sh`. Today nothing runs them automatically, so this is a latent
   escalation path, not an active one.
2. **Confidentiality (S8).** This risk is low if the content is the four
   addresses inferred above. It also stops mattering for tool runs once
   ad0ed9b is deployed. Other host uids can still read the file.
3. **Pattern.** `install.sh` writes without a umask, so a re-run under a
   permissive umask can bring 666 back after a chmod.

## Options (owner picks)

| # | Change | Fixes write risk | Fixes read risk | Live impact | Notes |
|---|---|---|---|---|---|
| **A (recommended)** | `chmod 600 …/link.env` (owner stays root:root) | yes | yes | **none**: no live process reads it | `play.sh`/`install.sh` keep working as root. A non-root user running `play.sh` would now stop under `set -e` (`[ -f ]` is true but sourcing fails). Nobody does this on the VPS today |
| B | `chmod 644` | yes | no (still world-readable) | none | enough if Slavik confirms the content is only addresses. S8 would still flag it until ad0ed9b is deployed |
| C | `chmod 640` | yes | yes | none | the same as A here, because group `root` has no other members |
| D | move it out (for example `mv link.env /root/link.env.20261021` and set it to 600) | yes | yes | none | `play.sh` handles a missing file (`if [ -f ]`) and `galion-one` uses default ports, which (inferred) equal the file. It is reversible, but it changes the folder layout. A and B do not |
| E | leave it at 666 and rely on ad0ed9b | no | only inside tool runs, after deploy | none | **not recommended**: it leaves the root-exec path open |

Follow-up for the source tree (separate task, not done here): make
`install.sh` run `umask 077` before writing (or `chmod 600 link.env` after the
heredoc). Optionally, `play.sh` could refuse to source a group- or
world-writable `link.env`. Then a reinstall cannot bring 666 back. These
scripts live only on the VPS under `galion-light/publish/obsidian`. This repo
holds only `crates/galion-one`, so this would be a galion-light change.

## Commands for the owner (not run)
Apply A:
```sh
F=/opt/galion/galion-light/publish/obsidian/link.env
stat -c '%a %U:%G %s' "$F"        # expect: 666 root:root 107
chmod 600 "$F"
stat -c '%a %U:%G %s' "$F"        # expect: 600 root:root 107
systemctl is-active galion-one    # expect: active (it does not read the file)
```
Verify with no restart: `curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:8091/`
returns the same code before and after. The S8 runner from 334c633 (VPS only,
as a temp non-root user) now reports `link.env` as CONTAINED (Permission
denied), even with the old preset.

Rollback (exact previous state):
```sh
chmod 666 /opt/galion/galion-light/publish/obsidian/link.env
```
The owner stays root:root, so no `chown` is needed in either direction. The
mode change does not affect the contents or mtime. Only the ctime changes.

## Owner decisions
1. **Pick A, B, C, D or E** (recommended: A, `chmod 600`).
2. Confirm or correct the inference that `link.env` holds only the four
   `GALION_*` addresses. This decides whether B is acceptable.
3. Decide whether to schedule the `install.sh` umask follow-up in galion-light.
4. Deploy a Hermes build that includes ad0ed9b (sandbox hides `/opt/galion`).
   This is independent of the chmod.

## Safety record for this task
- Read-only probes only: `stat`, `ls -la`, `lsattr`, `systemctl show/cat`
  (`Environment=` values masked), `ss -ltnp`, the `/proc/*/fd` and cwd scan,
  `grep` of scripts and units with `--exclude=link.env`, and `sed` of
  `install.sh`, `play.sh`, `README.md` and `galion-backup.sh`. No temp files on
  the VPS. `link.env` was never opened. Nothing under `galion-light/secrets`
  or `/root/galion-secrets` was read.
- One probe, a `journalctl | grep` for past `play.sh` runs, was too slow. It
  was stopped on the VPS, and a check confirmed no journalctl process was left.
  Whether `play.sh` ran in the past is not established.
- End state at 08:15:11 +02:00: `link.env` was still `666 root:root 107` with
  mtime unchanged. `/opt/galion/openmind` HEAD was 2f5a730 (untouched).
  `galion-one`, `galion-nerve` and `hermes-rs` were active, and the nerve
  `127.0.0.1:8670/state` answered (the cortex is not deployed). No service was
  stopped, restarted or reconfigured. There were no DNS, HADES or Hermes model
  changes.
