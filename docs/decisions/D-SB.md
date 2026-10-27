<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# D-SB1..4: owner decision pack, sandbox for the hermes-tools coding path

Written 2026-10-27 21:10 CET (+01:00), slot 305 (`swarm/sandbox-dsb-pack`, base
`swarm/main-staging` 98466ab9). Docs and tests only. **Nothing was deployed.** The sources are
t287u-C failure #3, the slot 191 seebystander results (b47f53d, VPS 2026-10-21 14:16 +02:00),
and tonight's VPS run (`docs/real-tests/sandbox-dsb-pack.md`).

## Where things stand tonight (VPS, read-only, 21:03–21:07 CET)
- Kernel `6.8.12-43-pve`. Proxmox LXC with 9 lxcfs mounts in `/proc/mounts`. `/dev/.lxc/proc` is absent, so nesting is off. Landlock is absent.
- The live `hermes-rs.service` runs `hermes-gateway-b7fbbc78` as **root** (`User=` empty, `DynamicUser=no`). It has **no** `HERMES_RS_CODING_*` env, so the coding tools are **off** whatever is compiled in. The drop-ins are 50-rust-bin, 60-nerve-url, 70-research-budget and 90-deploy-t291u; there is no coding-tools drop-in.
- The host file `/opt/galion/galion-light/publish/obsidian/link.env` is still mode **666** root:root. Only its metadata was read.
- The staged preset (the exact `Environment=` lines of `deploy/staged/hermes-rs.service.d/coding-tools.conf`) was proven on the VPS as uid 65534. All 3 tool kinds ran 3 times: /proc Masked (6 static files, 0 pid dirs, no `/proc/self`), link.env HIDDEN, `/opt/galion` empty, a 0666 canary hidden (the control run with no hide could read it), and the workdir quota hit ENOSPC at exactly the cap.

## D-SB1: what /proc does hermes-tools get when a private proc is refused?
| Option | Effect on the VPS | Tested |
|---|---|---|
| **Mask** (default today, all presets) | ro tmpfs with cpuinfo, meminfo, stat, loadavg, uptime and version. No pid dirs, so host processes are invisible. Tools lose `/proc/self`, `/proc/net`, `/proc/<pid>`, so `current_exe`, util-linux `umount` and `ps` fail | yes: VPS 14:16 (slot 191) and 21:05 tonight, 3 runs × 3 kinds, 0 pid dirs |
| Host | Every host process is listed, with `cmdline` and `status` readable (slot 191 control: 41 pid dirs; earlier S10: 46 other-uid cmdlines) | yes: it is the escape |
| Refuse | The run fails closed on this VPS. Every coding tool fails | yes: fails closed |

**Recommendation: keep Mask.** It is the only option that both runs on the VPS and hides the
host. cargo, rustc, git and sh all ran fine under Mask (git and cargo exited 0 tonight; slot 191
hermes-tools sandboxed 10/0). The cost is that a tool needing `/proc/self` errors out instead of
seeing the host, which is the safe way to fail. Revisit if D-SB2 is accepted.

## D-SB2: Proxmox `features: nesting=1` on the CT?
- **Gain:** the kernel would allow a real private proc (`ProcView::Private`), so Mask is no longer needed and tools get `/proc/self` back.
- **Cost:** it is a host-side Proxmox change, and the CT has to restart, which means downtime for every live service. Nesting also exposes clean host procfs/sysfs at `/dev/.lxc`, which widens the container's own attack surface. It has never been tested here.
- **Recommendation: no for now.** Mask already contains the host. If it is ever done, do it in a planned window and re-run `staged_preset` + `real_escape` straight after. The code picks Private automatically, with no code change.

## D-SB3: rely on `subset=pid`?
- `proc` with `subset=pid` in a user namespace only relaxes `mount_too_revealing` in Linux **≥ 7.2**. The VPS runs 6.8.12, so on this host the code path is always tried, refused, and then falls back to Mask.
- **Recommendation: no decision needed now.** Keep the path in the code (it is ordered before Mask) and mark it untested. Test it when the Proxmox host kernel is ≥ 7.2.

## D-SB4: hermes-rs runs as root
- Fact: live and staged `hermes-rs.service` both have no `User=`. Every sandbox test so far ran as a non-root user (sbx191, sbxtest, and nobody tonight). **The euid-0 path has never been tested.** As root, the user namespace maps uid 0 onto the host's root, and a sandbox bug would land with root's rights. Landlock, the second wall, is missing on this kernel.
- **Recommendation: add a dedicated `hermes-rs` user** (`User=hermes-rs`, owning `/var/lib/hermes-rs`) **before** any coding-tools drop-in goes live. The sandbox works unprivileged, which is what was proven. Expected side effects: the delegated cgroup works for a non-root User= with `Delegate=`; `ProtectHome=read-only` is unchanged; check that the state dir and any files Hermes reads under `/opt/galion/hermes` are readable by that user. This is a live unit change, so it needs a separate owner OK with a rollback (remove `User=`, then daemon-reload and restart).
- Alternative if root must stay: first run the `staged_preset` + `real_escape` suites as root in a throwaway CT (never on the live host) and record the result.

## Related small items (not D-SB, but found here)
1. **`link.env` mode 666** on the host. The hide layer makes it unreachable from tools (proven), but every other local user and process can still read it. Recommend `chmod 600` (owner OK; the reader runs as root, but check that first). Not done by this agent.
2. **Pin the workdir quota in the staged drop-in.** Today it relies on the compiled default of 512 MiB. Add `Environment=HERMES_RS_CODING_WORKDIR_MB=512`, so that a later default change cannot silently move the cap. The `staged_dropin_sets_the_required_keys` test prints a FINDING line until this is done.
3. The cgroup layer was **not** exercised tonight, because an ordinary user has no delegated cgroup. The staged `HERMES_RS_CODING_CGROUP=delegate` needs the first `--mock` smoke run on the real unit (as already noted in `docs/staged/hermes-tools-unit.md`).

## One-line asks for Slavik
- D-SB1: keep Mask as the hermes-tools default? (recommend **yes**)
- D-SB2: enable Proxmox nesting? (recommend **no**, for now)
- D-SB3: nothing to decide until the kernel is ≥ 7.2 (recommend **park**)
- D-SB4: run hermes-rs as a dedicated non-root user before coding tools go live? (recommend **yes**)
- Extra: `chmod 600` link.env? (recommend **yes**). Pin WORKDIR_MB=512 in the staged drop-in? (recommend **yes**)

## Update 2026-10-27 22:40 CET (slot 318, `swarm/sandbox-nonroot-stage`)
- **D-SB4 is now staged**: `deploy/staged/hermes-rs.service.d/80-nonroot.conf` + `deploy/staged/sysusers.d/hermes-rs.conf`, docs `docs/staged/hermes-rs-nonroot.md`. A bare `User=` would have broken the live gateway: config.yaml silently skipped (routes/aliases lost) and a crash loop on the 0600 legacy state.db (both reproduced on the VPS as uid 65534). The drop-in hands both over via `LoadCredential=` and a privileged `ExecStartPre=+install` copy, with no host permission change. Still needs the owner's OK (live restart).
- **Item 2 done**: `HERMES_RS_CODING_WORKDIR_MB=512` is pinned in `coding-tools.conf`; the test asserts it.
- **Item 1 (link.env)** is worse than a read leak: the file is world-*writable* and `play.sh` sources it (`. ./link.env`), so any local user can run code as whoever runs `play.sh`. Command staged (`chmod 600`, plus `umask 077` in `install.sh`), not run.
