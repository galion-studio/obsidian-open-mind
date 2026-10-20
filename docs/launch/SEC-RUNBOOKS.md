<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# Owner-only launch blockers SEC-1 to SEC-5: runbooks (prepared, not executed)

Written 2026-10-20T20:44:00+02:00 (Europe/Zagreb) on `swarm/sec-runbooks`
(wave 2, slot 26), base `main` 13a296c, gate list from `swarm/launch-gates`
79ee761 (`docs/launch/GATES-OCT31.md`). CEST ends 2026-10-25 03:00; after
that, timestamps carry +01:00.

**Nothing in this branch was executed against production.** On the VPS the
swarm ran only `ls -l`, `stat`, `find -printf` (names, modes, sizes, times),
`file` on two binaries and `systemctl show/status/list-units`, between
2026-10-20 20:28 and 20:42 +02:00. No file content was read, no secret file
was opened, nothing was written on the VPS, nothing under `secrets/persist`
was changed, the galion-control key was not touched, no unit was restarted.

| ID | Runbook | Tooling on this branch | Owner time |
|---|---|---|---|
| SEC-1 | `SEC-1-ROTATION.md` (18 credentials, consumers, order), `SEC-1-HISTORY-PURGE.md` | `crates/history-purge` (`galion-purge`, Rust, no deps), `docs/launch/purge-paths.obsidian-open-mind.txt` | Oct 21 to 30 |
| SEC-2 | below | none needed | 10 min |
| SEC-3 | below | `galion-purge sec3-perms` (Rust, same crate, std only; dry run by default, rollback file) | 10 min |
| SEC-4 | below | none needed | 15 min |
| SEC-5 | below | none needed (staged `deploy/staged/tuner-publish.service` exists from ws/one-desk-tuner) | 5 min + DNS |

Real-life test evidence: `docs/real-tests/sec-runbooks.md`
(Rust test `crates/history-purge/tests/real_sec_runbooks.rs`, run with
`GALION_REAL=1 cargo test --release -p history-purge --test real_sec_runbooks -- --ignored --nocapture`).
The runbook docs `docs/launch/SEC-*.md` are `export-ignore` (`docs/launch/.gitattributes`):
they name secret paths and a tunnel-id prefix and stay out of the public tree.

---

## SEC-2: the log that contains a token (paths only, contents never read)

The backlog item says "the session log that contains the token". The repo
docs name one log explicitly; the VPS listing shows one more class of file
that can hold tokens. Paths, modes and times (probe 2026-10-20 20:40 +02:00):

| Path | Mode | Size | Last write | Mode last changed | Why it is a candidate |
|---|---|---|---|---|---|
| `/opt/galion/galion-light/logs/gateway-deaths.log` | `0600 root:root` | 34,030 B | 2026-10-08 11:00:32 +02:00 | 2026-10-18 20:04:38 +02:00 | `docs/ws-survival.md:44,173`: the cloudflared tunnel token was on the `keep.sh` command line (`--token`) and was captured into this log through `ps`. **This is the documented one.** |
| `/opt/galion/hermes/sessions/` (2,010 entries: 2,009 `request_dump_*.json`, 1 `api-*.jsonl`) | dir `0700 root:root` | | 2026-10-18 19:33 +02:00 | 2026-10-18 19:55 +02:00 | Hermes session transcripts and request dumps; request dumps can carry request headers or pasted keys. Not checked (contents never read). |

Already true today: the log is `0600` (someone locked it on 2026-10-18 at
20:04 +02:00), its directory `galion-light/logs` is `0775`, and the whole
`secrets/persist` directory is now `0700` (it was a `0666` dir in the 19:42
inventory; changed 2026-10-18 22:01 +02:00). So the remaining exposure is
"root can read it" plus every **backup** that contains it: the 6-hourly
`/var/backups/galion/galion-*.tar.gz` (0600) and their age-encrypted
offsite copies in `/var/backups/galion/offsite/` (0640, group
`galion-offsite`, no members on 2026-10-20).

Owner steps (choose one; rotation row 3 of SEC-1 makes the token dead
either way, which is the real fix):

1. Rotate the tunnel credential first (SEC-1 row 3).
2. Then either **move** the log out of the logs tree into root's secret area
   (`install -d -m 700 /root/quarantine && mv /opt/galion/galion-light/logs/gateway-deaths.log /root/quarantine/`),
   or **scrub** it: `sed -i -E 's/(--token[= ])[^ ]+/\1<REDACTED>/g' <path>`
   (run without looking at it; verify with
   `grep -c -- '--token [^<]' <path>` which prints only a count), or delete
   it (a death log last written on Oct 8; first check with
   `fuser /opt/galion/galion-light/logs/gateway-deaths.log` that no process has it open).
3. Hermes `sessions/`: keep `0700`; if the owner wants, check for key shapes
   with a count-only command, e.g.
   `grep -l -E 'sk-or-v1-|Bearer [A-Za-z0-9._-]{20,}' /opt/galion/hermes/sessions/* | wc -l`,
   and delete request dumps older than needed (`find ... -name 'request_dump_*' -mtime +7 -delete`).
4. Backups: after rotation, old tarballs only hold dead values; keep or
   prune per retention.

## SEC-3: Hermes permissions (staged script, not run)

Current state (probe 2026-10-20 20:31 +02:00), all `root:root`:

| Path | Now | Target |
|---|---|---|
| `/opt/galion/hermes/` | 700 | 700 (unchanged) |
| `config.yaml` (111,389 B; holds `api_key`, `client_secret`, `sudo_password` fields per the redacted copy) | **666** | 600 |
| `SOUL.md` | **666** | 644 |
| `vault/` | **777** | 700 |
| `vault/.vault.lock` | **666** | 600 |
| `USER.md` (operator profile) | 664 | 600 |
| `.env`, `auth.json`, `state.db` | 600 | unchanged |
| `sessions/`, `logs/`, `memories/` | 700 | unchanged |

Mitigation already in place: the Hermes home itself is `0700 root`, so the
world-writable bits are reachable only by root today. They still matter for
copies (backups, `from-grok-build`, archive) and for any future non-root
service user. All consumers (`hermes-rs`, `console`, `galion-one`) run as
root (`systemctl show -p User` is empty), and `hermes.service` (Python) is
inactive, so the change cannot lock a live service out.

Tool: `galion-purge sec3-perms` (Rust, std only, in `crates/history-purge`;
port of the earlier bash draft, same behaviour; never reads file contents;
refuses symlinks (uses `lchown`, never follows links), wrong file types,
`secrets/persist`, anything named galion-control, and HERMES_HOME values like
`/` or `/root`; does not cross into other filesystems; rollback file is
written 0600 in a 0700 dir **before** any change; exit 3 on a refusal).

```
# on the box (never build on the VPS): static binary
CARGO_TARGET_DIR=/workspace/swarm/target/sec-runbooks cargo build --release -p history-purge --target x86_64-unknown-linux-musl
scp /workspace/swarm/target/sec-runbooks/x86_64-unknown-linux-musl/release/galion-purge root@VPS:/root/galion-purge   # owner
ssh root@VPS '/root/galion-purge sec3-perms'                          # dry run: prints now -> target, changes nothing
ssh root@VPS '/root/galion-purge sec3-perms --apply'                  # writes /root/sec3-perms/sec3-before-*.tsv first
ssh root@VPS 'curl -fsS http://127.0.0.1:8091/health; systemctl is-active hermes-rs console galion-one'
# optional: also chmod o-w the other world-writable entries the dry run listed
ssh root@VPS '/root/galion-purge sec3-perms --apply --fix-world-writable'
# rollback (restores modes and numeric owners exactly):
ssh root@VPS '/root/galion-purge sec3-perms --rollback /root/sec3-perms/sec3-before-<stamp>.tsv'
```

No restart is needed (modes are checked at open time). Tested on a fixture
tree on the box: dry run changes nothing, `--apply` gives exactly the target
modes, `--fix-world-writable` clears `o+w` on the rest, `--rollback`
restores the original modes byte for byte (see the real-test record).
Related, same pattern, optional: the `.env.example` templates in
`/opt/galion/galion-src/{galion-console,galion-rs}` (0777) and
`tactical-belts-pro` (0666) should be `chmod 644`.

## SEC-4: private key copy in the from-grok-build snapshot (location only)

`/opt/galion/from-grok-build` (661 MB, copied 2026-10-08, dir `0755 root`):

| Path | Mode | Size | Note |
|---|---|---|---|
| `/opt/galion/from-grok-build/workspace/.ssh/id_ed25519` | `0600 root:root` (dir `.ssh` 0700) | 419 B | the private key copy (O2). Contents not read; size fits an OpenSSH ed25519 private key |
| `/opt/galion/from-grok-build/root/galion-secrets/` | dir `0700` (parent `root/` 0700), 1 file at 0600 | | secret folder copy |
| `/opt/galion/from-grok-build/workspace/galion-light/secrets/` | dir **0755**, 1 file `github.token.example` at 0644 (230 B) | | an example template, not a token by name; the dir is world-listable |
| older copies of state: `/opt/galion/hermes/archive/grok-files/...` | | | archive tree (also holds `galion-src/voice-backend/bin/galion-health`, a shell script) |

Owner choice: **remove** (recommended) or keep locked.

1. Find which GitHub key it is: `ssh-keygen -lf /opt/galion/from-grok-build/workspace/.ssh/id_ed25519`
   prints only the fingerprint; compare with GitHub, Settings, SSH keys and
   with `ssh-keygen -lf /root/galion-secrets/github_ed25519`.
2. If it is a live key: rotate it (SEC-1 row 2), delete it on GitHub.
3. Then `shred -u` the copy (or move the whole `from-grok-build` tree to
   offline storage and delete it from the VPS; none of the running units
   checked on 2026-10-20 has its ExecStart or WorkingDirectory under it;
   check `lsof +D /opt/galion/from-grok-build` before deleting).
4. Remove `from-grok-build/root/galion-secrets/` the same way, and
   `chmod 700 /opt/galion/from-grok-build/workspace/galion-light/secrets`
   if the snapshot is kept.

## SEC-5: tuner-quick public trycloudflare URL (stop / replace)

State (probe 2026-10-20 20:33 +02:00): `tuner-quick.service` is a
**transient** unit (`/run/systemd/transient/tuner-quick.service`), active
since 2026-10-18 19:19:30 +02:00, PID 1198838,
`/usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650`
(no token: quick tunnels are anonymous). It publishes `tuner.service`
(127.0.0.1:8650, active) on a random `*.trycloudflare.com` hostname, which
is written in `docs/session-2026-10-18-evening.md:45`.

| Option | Steps (owner) | Public surface after |
|---|---|---|
| **A. Stop, no public tuner (recommended for launch)** | `systemctl stop tuner-quick` (a transient unit disappears when stopped; it will not come back on reboot either). `tuner.service` keeps running on loopback. | none |
| B. Replace with a stable hostname on the existing named tunnel | owner adds DNS `tuner.galion.app` CNAME `b2c049f7-7d08-4ec9-a7bc-eecfa76484a6.cfargotunnel.com`; add an ingress rule `hostname: tuner.galion.app` / `service: http://127.0.0.1:8650` above the catch-all in `/opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml`; `cloudflared tunnel ingress validate`; restart `cloudflared` (do it together with SEC-1 row 3 to save a restart); check `https://tuner.galion.app`; then option A | one known hostname behind the named tunnel (Cloudflare Access can be added) |
| C. Keep a random URL but supervised | install the staged `deploy/staged/tuner-publish.service` (from ws/one-desk-tuner; writes the current URL to `/run/tuner/quick-url.json`) and stop `tuner-quick` | random public URL, changes on restart (not recommended for a launch) |

Rollback for A: `systemd-run --unit tuner-quick /usr/local/bin/cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650` (new random URL).
After A or B, the old hostname in `docs/session-2026-10-18-evening.md:45` is
dead; the line can stay (history) or be replaced by `<retired quick tunnel>`
before LAUNCH-1 (gate 10 of `scripts/public-export.sh` warns on it).

---

## LAUNCH-1 / LAUNCH-2 dependencies

- LAUNCH-1 (public repo from the exported tree) needs SEC-1 rotation done
  first; the purge of the private repo is not required for LAUNCH-1 because
  the public repo starts with fresh history.
- LAUNCH-2 (RELEASE-CHECKLIST go/no-go) should tick SEC-1..5 with links to
  these runbooks and the date each was done.

## Live `galion-health` (8660) and `galion-nerve` (8670): where the source is

Neither binary has source in `main` (13a296c has `crates/openmind-nerve`, a
different crate, and no `galion-health`). Read-only findings:

| Live binary (VPS) | Unit | Built / staged | Source found |
|---|---|---|---|
| `/opt/galion/rust/bin/galion-health` (503,512 B, static-pie x86-64, stripped, installed 2026-10-18 22:14:38 +02:00) | `galion-health.service` "Galion health dashboard (127.0.0.1:8660)", User root, active since 22:14:38 +02:00 | staged from the box as `/root/galion-foundation-staging/galion-health` (same size, owner uid 1000 = the box user, 22:12:41 +02:00) with `galion-health.service`, `galion-backup*`, `galion-deploy`, `journald-50-galion.conf`; backups `/var/backups/galion/deploy/galion-health-20261018-221417+0200.bin`, `...-221438+0200.bin` | **`ws/foundation` e43d7ab** (in base.git / GitHub): `crates/galion-health/{Cargo.toml,src/main.rs}`, commit 2026-10-18 22:15:08 +02:00 "ws/foundation: galion-health, galion-backup (+timer, restore test), galion-deploy, journald caps". Built about 2.5 minutes before that commit, so the committed tree is very likely the shipped source; not proven byte-identical |
| `/opt/galion/rust/bin/galion-nerve` (816,456 B, static-pie, installed 2026-10-18 22:35:03 +02:00, run as `galion-nerve serve`) | `galion-nerve.service` "galion-nerve: Galion self-eval + nervous system hub (127.0.0.1:8670)", active since 22:35:04 +02:00; state dir `/var/lib/galion-nerve` | staged as `/root/nerve-stage/galion-nerve` (same size, uid 1000, 22:35:00 +02:00) with `install.sh`, `galion-nerve.service`, `FIRST-self-eval.{json,md}`; backups `/var/backups/galion/nerve/galion-nerve-20261018-223248.bin`, `-223503.bin` | **`ws/nerve` a63b730** (head; the crate arrives in a80c5b3, 2026-10-18 22:36:15 +02:00): `crates/galion-nerve/{Cargo.toml,src/main.rs,src/config.rs}` + `crates/nerve-client`. Built about 1 minute before that commit |
| not the same thing | | `/opt/galion/galion-src/voice-backend/bin/galion-health` (+ copies in `from-grok-build` and `hermes/archive`) is a 1,253 B **bash** script from 2026-09-22, unrelated to the Rust dashboard | |

Not searched on purpose: `/opt/galion/openmind` (the live working copy;
`crates/openmind-nerve` exists there but is the main-branch crate). No
`Cargo.toml` for either crate exists elsewhere on the VPS (`find -name
Cargo.toml -newermt 2026-10-15` lists only `session-handoff/gaia` and `/tmp`
worktrees). Recommendation: merge `ws/foundation` (galion-health) and the
`ws/nerve` crates into main through a swarm branch, then rebuild with the
same toolchain and compare sizes/sha256 with the live binaries (sha256 of a
binary is not a secret, but computing it on the VPS is outside this task's
read-only probe list).
