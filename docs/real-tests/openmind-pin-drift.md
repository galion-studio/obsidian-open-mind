<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real-server evidence: openmind pin drift (slot 391)

All on the VPS over ssh, read-only, `GIT_OPTIONAL_LOCKS=0`, 2026-10-28 CET (+01:00).
No secrets in any output below.

## 14:49:30 CET — HEAD and reflog
```
$ git -C /opt/galion/openmind rev-parse HEAD            -> 202887d56ddbede97acfc47bebd262a6eedd2a39
$ git -C /opt/galion/openmind rev-parse --abbrev-ref HEAD -> swarm/overnight-loop-20261027
$ git rev-parse --verify 2f5a730^{commit}               -> 2f5a730b0c4d0a124c881e98c0b65c4b52d56b72
$ git log --format='%h %ci %an %s' -3
202887d5 2026-10-27 19:43:11 +0100 Hermes (OpenMind) overnight: loop log bootstrap 2026-10-27 19:43 CET (t312u)
9fa897b9 2026-10-27 19:41:27 +0100 Galion Swarm t312u: overnight-loop-log start (t310u+t311u) 2026-10-27 CET
13a296ce 2026-10-20 18:36:15 +0200 Galion Studio Merge pull request #8 from galion-studio/merge/one-source-20261020
$ TZ=Europe/Zagreb git log -g --date=iso-local -6
202887d5 HEAD@{2026-10-27 19:43:11 +0100} commit: overnight: loop log bootstrap 2026-10-27 19:43 CET (t312u)
9fa897b9 HEAD@{2026-10-27 19:43:11 +0100} reset: moving to origin/swarm/overnight-loop-20261027
b3b95e33 HEAD@{2026-10-27 19:43:11 +0100} checkout: moving from swarm/overnight-loop-20261027 to swarm/overnight-loop-20261027
b3b95e33 HEAD@{2026-10-27 19:42:58 +0100} commit: overnight: loop log bootstrap 2026-10-27 19:42 CET (t312u)
98466ab9 HEAD@{2026-10-27 19:42:42 +0100} checkout: moving from main to swarm/overnight-loop-20261027
2f5a730b HEAD@{2026-10-20 10:44:56 +0200} commit: ONE-PROBE: spare One probes its own bind
```

## 14:50 CET — ancestry
```
13a296ce..2f5a730  : 0     (2f5a730 fully inside GitHub main)
2f5a730..13a296ce  : 60
2f5a730..202887d5  : 62
202887d5..2f5a730  : 0     (nothing lost)
merge-base(2f5a730, 202887d5) = 2f5a730
rev-parse main origin/main origin/swarm/overnight-loop-20261027
  2f5a730b..  13a296ce..  9056c37a..  (origin branch 4 ahead, tip 19:58 CET t314u)
```

## 14:51 CET — content
```
git log --stat -2: 202887d5 +docs/swarm-overnight/overnight-loop-log.md (31); 9fa897b9 +swarm/reports/overnight-loop-log.md (31)
git diff --stat 2f5a730 202887d5 : 507 files changed, 23820 insertions(+), 8614 deletions(-)
name-status counts: 185 A, 282 M, 30 D, 10 R
dirstat: crates/ 45.5%, docs/ 17.7%, brand/ 10.6%, deploy/ 4.9%, crates/openmind-memory 4.5%
git diff --name-status 2f5a730 202887d5 -- dist deploy/systemd : (empty)
git diff --name-only HEAD | wc -l : 0      git ls-files -o --exclude-standard | wc -l : 240
```

## 14:52–14:54 CET — live impact
```
grep -rl /opt/galion/openmind /etc/systemd/system -> galion-one.service only
  ExecStart=/opt/galion/openmind/dist/bin-ws/galion-one
  drop-in 50-rust-bin.conf: ExecStart= ; ExecStart=/opt/galion/rust/bin/galion-one
systemctl status galion-one: active (running) since 2026-10-22 22:46:47 CET, Drop-In 50-rust-bin.conf, 60-no-warm.conf
/proc/*/exe and /proc/*/cwd under /opt/galion/openmind: none
sha256 dist/bin-ws/galion-one: 090c15d0fbb32d372edd87ea3cb154a5be40cbadbb3ea3d8bb4a7a5880e50191 (828,640 B, 2026-10-18)
dist/bin: sha256sum -c SHA256SUMS -> galion-one, galion-desk, room, desk-hermes, link-watch: OK
nerve 127.0.0.1:8670/state -> 200 (recent alert: galion-one-mouth stuck loading 20.2 h, 14:30 CET)
cortex 127.0.0.1:8674/health -> 200
console 127.0.0.1:8080 /, /overview, /api/v1/overview: no "stays at 2f5a730" string served
```
