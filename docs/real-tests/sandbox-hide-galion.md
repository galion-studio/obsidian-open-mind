<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# sandbox-hide-galion: hide /opt/galion in the tool presets (S8 fix), VPS proof

Wave-4 slot 150. Branch `swarm/sandbox-hide-galion`, based on
`swarm/sandbox-real-problems` 334c633. Times are Europe/Zagreb, CEST +02:00.

## Change
- `crates/openmind-sandbox/src/preset.rs`: `HIDE` is now `/root`, `/home`,
  `/run/secrets`, `/opt/galion` for all three tool presets
  (read-only-tool, build-tool, net-tool). The kernel on the VPS has no
  Landlock, so with `ro_root` the whole host tree is readable and Unix
  permissions were the only barrier; a world-readable file under
  `/opt/galion` (`publish/obsidian/link.env`, mode 666) was readable (S8 FAIL
  in 334c633).
- `deploy/sandbox/presets/*.toml` and `docs/sandbox-presets.md` say the same.
- `hermes-tools`: no code change (its coding tools start from these presets and
  extend `hide_paths`); the `CodingConfig::hide` doc comment updated.
  If a tool's workdir, write root, read root or program is under `/opt/galion`,
  openmind-sandbox keeps it visible and reports `hide_home` as
  "kept visible because needed" (existing behaviour, unchanged).
- New test `tool_presets_hide_opt_galion_and_match_toml`
  (crates/openmind-sandbox/tests/preset.rs): every preset hides `/opt/galion`,
  covers 6 known live S8 paths, has no read root under `/opt/galion`, and its
  TOML `hide_paths` line equals the Rust preset.
- `tools/sandbox-real-problems` S8 now judges the shipped preset (must be 0
  readable of N, templates included) and only measures two baselines:
  `preset-before-fix` (the old HIDE) and `nohide`.
- No new dependencies.

## Box tests (no sandbox runs on the box)
- `cargo fmt --check` OK (root workspace and tools/sandbox-real-problems).
- `cargo clippy --workspace --all-targets -- -D warnings` OK; runner clippy
  (musl release, all targets) OK.
- `cargo test -p openmind-sandbox --test preset`: 2 passed, 0 failed (pure
  policy tests, no process spawned).
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  957 passed, 0 failed, 3 ignored (205 test binaries).
- `cargo test -p hermes-tools`: 12 passed, 0 failed. Note: its
  `tests/sandboxed.rs` (5 tests) runs real tool calls through openmind-sandbox,
  so this was a local sandbox run, against sandbox_vps_only. It was run once,
  by mistake, before the workspace run excluded hermes-tools; nothing was
  changed on the host. Owner may want hermes-tools' sandboxed tests added to
  the local exclude list.

## VPS run (2026-10-21 07:28:33-07:29:22 +02:00)
- Binary: `tools/sandbox-real-problems`, static x86_64 musl, sha256
  `b445148d60c887e98ad5c7f9755617d0e30e0325bb145c61b7fd60f770601bdc` (869 KB).
- Throwaway dir `/var/tmp/sandbox-test-20261021-072833` (bin/ and data/ root
  0755, w/ owned by the temp user). Temp user `sbx150` (system, nologin, uid
  996); a slot-specific name so a parallel slot's `sbxtest` is never touched.
- Path list: the same 48 paths as run 3 of sandbox-real-problems (41 files and
  dirs, all still present, plus /proc/<MainPID>/environ of galion-nerve,
  galion-nerve-shadow, hermes-rs, openmind-memory, cloudflared, tuner and
  pid 1; same PIDs as 05:10). Read at most 1 byte, only the word ESCAPED is
  printed; no content was shown or copied.
- Run: `systemd-run --wait --pipe --collect --unit=sbx150-hidegalion-20261021-072833 --slice=sbxtest.slice -p User=sbx150 -p Group=sbx150 -p Delegate=yes -p MemoryMax=1G -p TasksMax=256 -p CPUQuota=200% -p Nice=10 -E SRP_ONLY=S3,S7,S8,S10 <base>/bin/srp run <base>`
  (07:28:56.752-07:29:07.765, rc=0). S3, S7 and S10 were re-run as a
  regression check of the other layers with the new preset; S1/S2/S4-S6/S9
  need live-data copies or a root tmpfs and were not re-run (preset change
  does not touch them).

### Result: SUMMARY 4 scenarios, 0 FAIL
| Scenario | Result |
|---|---|
| S8 shipped preset | **0 readable of 48** (0 non-template, 0 .env.example templates); 47 contained, 1 hidden (/root). link.env: `CONTAINED No such file or directory` |
| S8 baseline preset-before-fix | 11 readable of 48 (link.env + 10 .env.example templates) - the 334c633 finding, reproduced |
| S8 baseline nohide | 11 readable of 48 (same) |
| S3 crash loop | PASS: 193/193 SIGABRT, killed at 4003 ms wall, 0 processes left, cgroup removed |
| S7 network | PASS: 12 probes 0 escaped, only lo, decoys 0 conns/0 bytes/0 datagrams, positive control +1/+7/+1 |
| S10 /proc gap | measured: 36 other-uid cmdlines readable (known GAP, unchanged), 0 environ readable |

```
uid=996 euid=996
FEATURES Features { landlock_abi: 0, userns: true, userns_full: true, mount_setattr: true, caps: true, android: false } landlock_abi=None
HOST OK [S8] preset: shipped read-only-tool hides /opt/galion
HOST OK [S8] preset: 48 of 48 paths judged
HOST OK [S8] preset: 0 readable of 48 (0 non-template [], 0 .env.example templates); 47 contained, 1 hidden
NOTE [S8] baseline preset-before-fix: 11 readable of 48 (1 non-template ["/opt/galion/galion-light/publish/obsidian/link.env"], 10 .env.example templates); 36 contained, 1 hidden
NOTE [S8] baseline nohide: 11 readable of 48 (1 non-template ["/opt/galion/galion-light/publish/obsidian/link.env"], 10 .env.example templates); 37 contained, 0 hidden
SUMMARY 4 scenarios, 0 FAIL
```

S8 shipped-preset lines (all 48):
```
RESULT secret /opt/galion/galion-light/secrets CONTAINED No such file or directory (os error 2)
RESULT secret /root/galion-secrets CONTAINED No such file or directory (os error 2)
RESULT secret /root HIDDEN listed 0 entries (empty cover)
RESULT secret /etc/shadow CONTAINED Permission denied (os error 13)
RESULT secret /etc/openmind-memory.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/galion-light/publish/obsidian/link.env CONTAINED No such file or directory (os error 2)
RESULT secret /root/.ssh CONTAINED No such file or directory (os error 2)
RESULT secret /root/.ssh/authorized_keys CONTAINED No such file or directory (os error 2)
RESULT secret /etc/memory-gateway.env CONTAINED Permission denied (os error 13)
RESULT secret /opt/galion/tooling/hermes-agent/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/hermes-home/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/shadow/hermes-agent/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-light/tulu/llama.cpp/tools/ui/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/root/galion-secrets/cloudflare.read.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.local CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/tactical-belts-pro/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-rs/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-console/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/magic-merlin-showcase/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/merlin-element/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/from-grok-build/workspace/galion-src/galion-backend/.env.backup CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-rs/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-console/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/tactical-belts-pro/.env.local CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/archive/grok-files/galion-src/galion-backend/.env.backup CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/.env.bak-20261018-193614+0200 CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/hermes/.env.sealed CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/merlin-element/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-platform/Documents/project-nexus/frontend/galion-console-v3/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/magic-merlin-showcase/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-backend/.env.backup CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-backend/.env CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-console/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/galion-rs/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.local CONTAINED No such file or directory (os error 2)
RESULT secret /opt/galion/galion-src/tactical-belts-pro/.env.example CONTAINED No such file or directory (os error 2)
RESULT secret /proc/1466797/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4008991/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1361728/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1374875/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1468472/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/4004553/environ CONTAINED Permission denied (os error 13)
RESULT secret /proc/1/environ CONTAINED Permission denied (os error 13)
```

## Cleanup proof (07:29:22 +02:00)
```
LEFT_PROCS_BEFORE_KILL 0
VERIFY user: id: 'sbx150': no such user
VERIFY group: none
VERIFY dirs: 0          (/var/tmp/sandbox-test-*)
VERIFY units: 0         (sbx150*)
VERIFY cgroups: 0 0     (sbx150*, om-sbx-*)
VERIFY procs: 0         (ps for sandbox-test or /bin/srp)
VERIFY mounts: 0
link.env 666 root (unchanged)
/opt/galion/openmind HEAD 2f5a730 (untouched)
galion-nerve, galion-nerve-shadow, hermes-rs: active
```
Observer: the nerve (127.0.0.1:8670 /state, mode live, components healthy);
the cortex is not deployed yet. Nothing on the VPS was chmod-ed or changed:
`link.env` is still 666 (owner decision; this branch makes it unreachable
from tool presets, it does not fix the file). No service was stopped,
restarted or reconfigured; no DNS, HADES or Hermes model changes.
The box target dir was deleted after the run.

## Open / owner decisions
- `chmod 600 /opt/galion/galion-light/publish/obsidian/link.env` is still
  recommended (defence in depth; other host users can still read it).
- Callers that pass a workdir under `/opt/galion` lose this hide (reported in
  `hide_home`). Hermes coding worktrees should stay outside `/opt/galion`.
- Hermes' tool runs only get the fix once a Hermes build with this branch is
  deployed (not done here).
