<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# S4.7 survival install (report-only)

Written Europe/Zagreb. Installs staged `guardian-tick` + `openmind-survival` (lib via guardian-tick) and Rust `galion-survival` refresh for `survival.json`.

## What lands
- `/opt/galion/rust/bin/guardian-tick` (static musl)
- `/opt/galion/rust/bin/galion-survival` (static musl; replaces `scripts/survival.py`)
- `/opt/galion/etc/probes.toml` (from `deploy/survival/probes.toml`)
- `/etc/systemd/system/guardian-tick.service` + `.timer` (report-only ExecStart; timer WantedBy=timers.target)
- Hermes skill paths fixed to `/opt/galion/...` (no `/workspace`, no python3)

## Not in this step (S4.7 later)
- Restart=always drop-ins under `deploy/survival/*.d`
- `guardian-tick heal --allow` apply mode

## Rollback
```
systemctl disable --now guardian-tick.timer
rm -f /etc/systemd/system/guardian-tick.service /etc/systemd/system/guardian-tick.timer
systemctl daemon-reload
# binaries: keep or remove /opt/galion/rust/bin/{guardian-tick,galion-survival}
```
