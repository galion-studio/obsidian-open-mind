<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: Copyright (c) 3000 Galion / galion-studio -->
# Real-life tests: swarm/sec-runbooks (wave 2, slot 26)

Recorded 2026-10-20T20:43:16+02:00 (Europe/Zagreb) on the box. Port range
19260-19269 was assigned but not needed: everything here is a CLI or a
script; no server was started, so nothing was left running.

## 1. `galion-purge` on a scratch mirror + SEC-3 script on a fixture

Command: `GALION_REAL=1 scripts/real/sec-runbooks.sh` (source
`/workspace/swarm/base.git`, the box mirror of GitHub; main 13a296c). The
scratch dir is `$CARGO_TARGET_DIR/real-scratch` and is deleted on exit.
GitHub was not contacted, nothing was pushed, the VPS was not used.
The canary key is generated at run time (`AKIA` + 16 random base32
characters), committed only to a scratch-only branch of the scratch mirror,
and gitleaks runs with `--redact`.

Full output of the final run (exit 0):

```

== 2026-10-20T20:42:49+02:00 build galion-purge (release)
463392 /workspace/swarm/target/sec-runbooks/release/galion-purge

== 2026-10-20T20:42:57+02:00 scratch mirror clone of /workspace/swarm/base.git (main 13a296ce15b5bbd66adc26f6bfae787b2c1b5190)
refs: 27, commits: 261, remotes: origin 

== 2026-10-20T20:42:57+02:00 refusals
without --confirm-scratch-copy: refused; with a remote and no --strip-remotes: refused

== 2026-10-20T20:42:57+02:00 scan before (paths only)
scan: 261 commits, 258 distinct trees, 6 patterns
listed paths present in history: 89 (92 blob ids)
     45  contrib/
      6  dist/
      4  edge/
     34  stack/

== 2026-10-20T20:42:57+02:00 plant a canary secret in listed paths on a scratch-only branch (generated now, never committed to the repo)
canary branch added to the scratch mirror only

== 2026-10-20T20:42:58+02:00 gitleaks before (all history, redacted)
8:42PM WRN leaks found: 2
findings before: 2 (files: "File": "contrib/canary.env" "File": "stack/kms/canary.env.redacted" )

== 2026-10-20T20:42:58+02:00 rewrite
remote config removed: ["origin"]
before: 262 commits, 91 listed paths
     46  contrib/
      6  dist/
      4  edge/
     35  stack/
filter: 262 commits, dropped 172 M + 0 D lines, kept 3506 changes, 1 commits left empty (kept)
after: 262 commits, 0 listed paths
old blob ids of dropped paths: 92 gone, 2 still present only because a kept path has identical content, 0 orphaned-but-present
    shared: 127c2e723c261f488196baa498e09a7643842bd9
    shared: 6682b01f7a86fbc60854bd16cf5ac54a1593fdc4
commit map (262 commits, old<TAB>new): /workspace/swarm/target/sec-runbooks/real-scratch/scratch.git/galion-purge-commit-map.tsv
REWRITE OK (nothing pushed; the copy has no remote)
commit map: 262 lines; main 13a296c -> 13c7803

== 2026-10-20T20:43:00+02:00 verify (galion-purge)
verify: 262 commits, 258 distinct trees, 6 patterns
listed paths present in history: 0 (0 blob ids)
VERIFY OK: no listed path in any commit of any ref

== 2026-10-20T20:43:01+02:00 cross-check with plain git (every commit diff vs every parent, every reachable object path)
log -m --name-only hits: 0; rev-list --objects path hits: 0
fsck: clean
refs after: 28, commits after: 262

== 2026-10-20T20:43:01+02:00 gitleaks after
findings after: 0

== 2026-10-20T20:43:01+02:00 source mirror unchanged
source refs digest 52263b3e1353fb3a unchanged

== 2026-10-20T20:43:01+02:00 rewritten tree still builds (main, cargo check --workspace --locked)
13c7803 Merge pull request #8 from galion-studio/merge/one-source-20261020
cargo check: ok

== 2026-10-20T20:43:16+02:00 SEC-3 script on a fixture HERMES_HOME
before:   666 config.yaml 666 SOUL.md 777 vault 666 vault/.vault.lock 664 USER.md 666 notes.md 
dry-run:  666 config.yaml 666 SOUL.md 777 vault 666 vault/.vault.lock 664 USER.md 666 notes.md 
  vault/.vault.lock      600 box:box
  USER.md                600 box:box
2026-10-20T20:43:16+02:00 SEC-3 applied; remaining world-writable under HERMES_HOME: 1
applied:  600 config.yaml 644 SOUL.md 700 vault 600 vault/.vault.lock 600 USER.md 666 notes.md 
2026-10-20T20:43:16+02:00 SEC-3 applied; remaining world-writable under HERMES_HOME: 0
o-w:      600 config.yaml 644 SOUL.md 700 vault 600 vault/.vault.lock 600 USER.md 664 notes.md 
2026-10-20T20:43:16+02:00 rollback from /workspace/swarm/target/sec-runbooks/real-scratch/sec3-state/sec3-before-20261020T204316330329260.tsv done
rollback: 666 config.yaml 666 SOUL.md 777 vault 666 vault/.vault.lock 664 USER.md 666 notes.md 

== 2026-10-20T20:43:16+02:00 PASS
```

What it proves:

- the listed paths (`docs/launch/purge-paths.obsidian-open-mind.txt`) are
  gone from every commit of every ref, by three independent checks
  (`galion-purge verify` walks every distinct tree; `git log --all -m
  --name-only` sees every diff against every parent; `git rev-list --all
  --objects` sees every reachable object path);
- the old blob objects are gone from the object store (92 of 92 that were
  unique to listed paths; the 2 still present are MIT `LICENSE` texts that
  are also stored at kept paths such as `LICENSE` and `release/repo/LICENSE`);
- gitleaks over the whole scratch history: 2 findings (the canary) before,
  0 after; `git fsck --full` clean;
- shape kept: 262 commits before and after, all refs kept (27 + the canary
  branch), merges kept; one commit became empty (the canary commit) and was
  kept;
- the rewritten `main` (13c7803, mapped from 13a296c by the commit map)
  checks out without the purged directories and passes
  `cargo check --workspace --locked`;
- refusals work: no `--confirm-scratch-copy` -> refused; a configured remote
  without `--strip-remotes` -> refused; non-bare repo and `/opt/galion`
  paths -> refused (integration test);
- the source mirror's refs digest is unchanged;
- SEC-3 script: dry run changes nothing; `--apply` sets exactly 600/644/700/
  600/600; `--fix-world-writable` removes `o+w` from the other file; the
  rollback file restores every original mode.

## 2. Unit and integration tests

`cargo test -p history-purge`: 10 unit tests (glob and pattern semantics,
quoted paths, byte-exact data blocks including heredoc data and messages that
look like file changes, inline data, emptied commits, refused renames,
truncated streams) + 2 integration tests (temporary repo with branches, a
merge, annotated and lightweight tags, a quoted UTF-8 path with a space,
shared content; mirror clone; rewrite; commit map; fsck).

## 3. Cross-target

| Target | Result |
|---|---|
| x86_64-unknown-linux-musl | release build ok, 564,872 B static-pie |
| aarch64-unknown-linux-musl (`rust-lld`) | release build ok, 500,880 B |
| aarch64-linux-android | `cargo check` ok |
| armv7-unknown-linux-musleabihf | `cargo check` ok |

## 4. Read-only VPS probes (2026-10-20 20:28 to 20:42 +02:00)

Only `ls -l`, `stat`, `find -printf` (names, modes, sizes, times), `file` on
two ELF binaries and one script, `systemctl show/status/list-units`. Output
was filtered through `sed` to redact any `--token` argument (none appeared).
No file contents were read; nothing was written on the VPS. Findings are in
`docs/launch/SEC-RUNBOOKS.md` and `docs/launch/SEC-1-ROTATION.md`. Key lines
(times converted to +02:00):

```
600 root:root 34030 B, last write 2026-10-08T11:00:32+02:00  /opt/galion/galion-light/logs/gateway-deaths.log (mode changed 2026-10-18T20:04:38+02:00)
700 /opt/galion/hermes; 666 config.yaml; 666 SOUL.md; 777 vault; 666 vault/.vault.lock; 664 USER.md; 600 .env, auth.json, state.db
600 root:root 419 B  /opt/galion/from-grok-build/workspace/.ssh/id_ed25519; 700 from-grok-build/root/galion-secrets; 755 from-grok-build/workspace/galion-light/secrets (1 file: github.token.example, 0644)
tuner-quick.service: transient, active since 2026-10-18T19:19:30+02:00, PID 1198838, cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650
galion-health.service: /opt/galion/rust/bin/galion-health (503512 B), active since 2026-10-18T22:14:38+02:00
galion-nerve.service: /opt/galion/rust/bin/galion-nerve serve (816456 B), active since 2026-10-18T22:35:04+02:00
cloudflared.service: --config /opt/galion/galion-src/galion-infrastructure/cloudflared/config.yml (no --token on the command line)
```
