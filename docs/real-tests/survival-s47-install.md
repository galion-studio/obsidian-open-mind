<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: survival S4.7 install

Europe/Zagreb. Box build + VPS install of report-only guardian-tick and galion-survival refresh.

## Box
- `cargo test -p galion-survival --release` → 3/0
- musl release: guardian-tick, galion-survival (static-pie, no NEEDED)

## VPS (filled after land)
- one-shot report-only
- classify dry-run against galion-health status.json
- heal without --allow (dry-run)
- timer enable only if report-only ok
- survival.json `when` refreshed (Zagreb stamp)
- skill paths `/opt/galion/...` (no python3)
