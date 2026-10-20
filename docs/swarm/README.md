<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# docs/swarm: the durable record of the PROJECT 42 / Gaia coding swarm

Written 2026-10-20 23:45 CEST (+02:00).

**Why this exists:** Slavik does not want anything important to live only on the swarm box, which is temporary and lost on restart. Everything important is kept on GitHub (this branch, `swarm/swarm-record`) and on the VPS (`/opt/galion/archive/swarm-<timestamp>/`), with text that explains each process, why it was done, and the bigger picture.

| File | What |
|---|---|
| [BIG-PICTURE.md](BIG-PICTURE.md) | Vision, Gaia's 9 functions and 52 systems, the 7 sectors, benchmarks, Galion OS, how the swarm works, merged vs waiting |
| [PROCESSES.md](PROCESSES.md) | Every agent slot run on 2026-10-20: what, why, branch + sha, status, owner decisions |
| [ARCHIVE.md](ARCHIVE.md) | GitHub safety sweep (branch table), VPS archive path, sha256 manifest check, excluded secret-like paths |
| [TOOLING.md](TOOLING.md) | The box helpers (push through the VPS, Python blockers) as text |
| [record/](record/) | Snapshots of the call record, call recap, RULES, WAVE2, WAVE3 preamble, SLAVIK-TODO, slot prompts |
| [plans/](plans/) | The 7 sector plans plus benchmarks, galion-os, non-rust inventory, planner brief |
| [galion-os-staging/](galion-os-staging/) | The staged contents of the planned `galion-studio/galion-os` repo (not created yet) |

The VPS address is written `<vps>`. No secret values are in these files. Secret-like files on the box were excluded by path; see ARCHIVE.md.
