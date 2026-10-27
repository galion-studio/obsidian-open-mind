<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real test: survival S4.7 install

Europe/Zagreb (+01:00). Mate t276u job2. Branch swarm/survival-s47-install 97214ba.

## Box
- `cargo test -p galion-survival --release` → 3/0
- musl x86_64 static: guardian-tick sha256 007720e3…656cf (893648 B); galion-survival sha256 8f1cb532…587a0 (594640 B)

## VPS lands (2026-10-27 ~18:19 CET)
1. **Install** binaries to `/opt/galion/rust/bin/{guardian-tick,galion-survival}`, probes `/opt/galion/etc/probes.toml`, units under `/etc/systemd/system/`, skill both Hermes skill trees. `systemd-analyze verify` ok. openmind HEAD unchanged 2f5a730.
2. **Report-only one-shot** exit 0; host_ok=false expected (whisper/vision/voice/origin down; load high). Wrote state/guardian-tick.json.
3. **Classify** mode=classify-only; tuner-quick down → notify only; nothing applied.
4. **Heal** mode=dry-run allow=[]; n_actions=0.
5. **Enable** guardian-tick.timer (WantedBy=timers.target); oneshot SUCCESS via systemd.
6. **Refresh** survival.json via galion-survival: `when` 2026-10-27T18:19:50+01:00 (was 2026-10-18T19:35:50Z); disk path `/` 30%; lane action matches modes.json Flash; openrouter measured=true; cash present and above floor_usd 5; no key printed by design (SCOPE line omits raw key). Backup survival.json.bak-* kept.
7. **Post**: hermes/nerve/one 200; no Restart=always drop-ins; skill has /opt/galion + galion-survival, no python3/workspace.

## Not done (S4.7 later)
- deploy/survival/*.d Restart=always drop-ins
- heal --allow apply mode
