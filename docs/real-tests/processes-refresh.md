<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Evidence: processes-refresh (wave 4, slot 142)

Docs-only slot: refresh `docs/swarm/PROCESSES.md` and `docs/swarm/BIG-PICTURE.md` on `swarm/swarm-record` 749ddbd with everything after 23:33 CEST (2026-10-20). Nothing was built, started or written outside this worktree, except the START/DONE lines in the box call record. All times Europe/Zagreb, CEST (+02:00).

## 1. Source: the call record

`/workspace/swarm/CALL-RECORD.md`, lines from "23:42 cutover-prep stopped" to the 06:35 entries (wave 3 end, Wave 4, VPS migration + deployment). Snapshot refreshed into `docs/swarm/record/CALL-RECORD.md` (with RULES.md and `plans/wave4-tasks.md`). Checked before copying: no token patterns (`ghp_`, `github_pat_`, `sk-`, private-key headers), no VPS address, no quick-tunnel hostname.

## 2. Branch heads vs the record (box, 06:36 CEST)

`git rev-parse` of every `/workspace/swarm/wt/<slug>` for slots 100-145 and the side jobs, compared with the SHAs in the call record and then with the new tables (scripted check over the table rows).

- Slots 100-139: 40/40 heads match the SHA logged as DONE; all worktrees clean.
- One moved head: `hermes-core-docs` logged DONE at c069839 (04:40), worktree head is 3611fc6 (04:46, "re-check after resume"). The push of 3611fc6 is not logged. The table shows 3611fc6 and says so.
- Side jobs: live-ai-plan 644c4f7, whitepaper-vending 5ddd828, settings-snapshot 241aecf, integration-4 42de672 (squash 9b5f6ad present as an object), nerve-shadow-20261021 4800770, vps-deploy-20261021 d9bd829, sandbox-real-problems 334c633: all match.
- 140, 141, 142 worktrees still at their bases (8c0ac37, 87fdd91, 749ddbd) at 06:36; 143 worktree at 2b55908 (older staging), 144 not created, 145 at df42e0f.
- Table cross-check after writing: 35 slot rows parsed with a 7-hex head, 35 match the worktree, 0 mismatch; console-port-1..5, nerve-shadow, integration-4 checked separately, all match.
- base.git `main` = 13a296c.
- GitHub itself was not read by this slot: the repository is private, an unauthenticated `git ls-remote` fails (exit 128), and the token stays on the VPS (rule 2). "On GitHub" in the docs relies on the push-helper lines and verifying ls-remotes recorded at 03:04, 04:18 and 04:53.

## 3. Live VPS, read-only (06:36-06:38 CEST)

Over the usual SSH key, GET and status reads only (curl to loopback, `systemctl is-active/is-enabled/show`, `sha256sum`, `ss -ltn`, `stat`, `getent`). Addresses masked in the capture. Nothing written; the one temp file (`/tmp/.s142`, the /state body) was deleted in the same command.

| Check | Result | Matches the record? |
|---|---|---|
| nerve 8670 `/state` | 200, 13,090 B, 15 keys, mode live, cycle 6723, 15 components: 11 healthy, degrading galion-one-mouth + cloudflared, absent voice-worker + gaia | yes (live nerve untouched) |
| shadow 8672 | `/healthz` 200, mode observe-only, cycle 238, 5 cortex sensors, 3 incidents (covered: edge dead origins, galion-one-mouth stuck loading, cloudflared error rate) | yes; cortex only in shadow, not live |
| units | galion-nerve active/enabled, galion-nerve-shadow active/disabled, tuner active/enabled, tuner-quick active/transient, hermes-rs, cloudflared, galion-one, console active/enabled | yes |
| binaries | /opt/galion/rust/bin/galion-deploy 2a60e1ac, galion-nerve-shadow 3a566c56, /opt/galion/tuner/bin/tuner 89c7fcbc | yes (04:31-04:37 deploy) |
| strays | 8431, 8432, 8433, 8652, 8653 not listening; 8642, 8650, 8670, 8672 listening | yes (stopped 04:35) |
| file modes | cloudflared config.yml 644 root; obsidian `link.env` 666 root (mode only, not read) | yes; link.env still open (owner decision) |
| tuner.galion.app | NXDOMAIN from the VPS | yes (D118-1 open) |
| galion.app from the VPS | 403 (also with a browser user agent) | record says 502 at origin; 403 is likely Cloudflare answering the VPS itself, not checked further |
| failed units | 3: run-u11093/4/5.service, inactive since 2026-10-18 22:13:59 CEST | older than the swarm; the 04:39 post-check said "no failed units", so it likely counted differently. Not touched |

## 4. Checks on the branch

Run just before the commit:

Run 06:40 CEST: `scripts/no-python.sh` ok; `scripts/self-check.sh` ok (c6 23 pre-existing allow-listed, c7 ok, secret ok); `repo-check` (release build of crates/repo-check, own target dir, removed after) 758 files, 0 errors, 43 warnings (all pre-existing in crates/, 0 in docs/) -> PASS; SPDX MIT header on every new or changed file; no `Z`/UTC timestamps and no VPS address in the changed docs.
