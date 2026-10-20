# Survival audit

- DESK-ID: openmind.B22
- Generated: 2026-10-19T16:01:26+02:00 (Europe/Zagreb)
- Host: ip50
- Repo: /opt/galion/openmind
- Read-only: `systemctl show -p Id -p Restart -p WantedBy -p UnitFileState`, `systemctl is-enabled`, `systemctl cat`. No enable, start, restart, or daemon-reload.

Survives reboot is yes only when `is-enabled` is `enabled` and `WantedBy` is `multi-user.target` (symlink under `multi-user.target.wants`). `Restart=` is the crash policy, not the boot policy.

| unit | Restart= | is-enabled | WantedBy | survives reboot |
| --- | --- | --- | --- | --- |
| hermes-rs.service | on-failure | enabled | multi-user.target | yes |
| console.service | always | enabled | multi-user.target | yes |
| galion-desk.service | on-failure | enabled | multi-user.target | yes |
| link-watch-rs.service | always | enabled | multi-user.target | yes |
| openmind-memory.service | always | enabled | multi-user.target | yes |
| tuner.service | always | enabled | multi-user.target | yes |
| galion-one.service | always | enabled | multi-user.target | yes |

## galion-one

The expected gap (no systemd unit, orphan process) is not the live state.

- Fragment: `/etc/systemd/system/galion-one.service`. Loaded, active, running since 2026-10-18 20:13:56 UTC. Main process parent is pid 1, cgroup `/system.slice/galion-one.service`. No second `./one/one` process.
- `Restart=always` is in the fragment. `systemctl is-enabled` prints `enabled`. `systemctl show -p WantedBy` prints `multi-user.target`.
- Enablement symlink: `/etc/systemd/system/multi-user.target.wants/galion-one.service`.
- The fragment has no `[Install]` section and no `WantedBy=` line. Its comment still says staged and not installed. The `WantedBy` value in the table is the runtime reverse dependency from that symlink, not a line in the file. Boot still starts the unit, because multi-user.target follows the symlink.
