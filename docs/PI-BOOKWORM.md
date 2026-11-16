<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Raspberry Pi OS Bookworm install

One command installs the Open Mind desktop and the local services its probes ask. The machine is Raspberry Pi OS Bookworm, 64-bit (`aarch64`). The binaries are the aarch64 half of the Pi 5 seed (`docs/PI5-SEED.md`, `scripts/linux/pi-seed.sh`). This command does not build Galion OS and does not publish a release.

```sh
sudo scripts/linux/pi-bookworm-install.sh --seed DIR
```

`DIR` is `bin/aarch64-unknown-linux-musl` from the seed tarball, or `target/pi-seed-out/aarch64-unknown-linux-musl` after `scripts/linux/pi-seed.sh build aarch64-unknown-linux-musl`. A second run with the same files prints `pi-bookworm: unchanged` and leaves running processes in place.

The script refuses a host that is not Bookworm and not aarch64. It also refuses a host that has no Raspberry Pi OS marker (`/etc/rpi-issue`, or `Raspberry Pi` in `/proc/device-tree/model`) unless `PI_BOOKWORM_EMULATOR=1` is set. That variable is the CI path below. It is not a way to install onto a different machine.

## What it starts

Binaries go to `/opt/obsidian/bin`. systemd system units go to `/etc/systemd/system/obsidian-*.service` with `WantedBy=multi-user.target`. They are not user units. Nothing in this install uses `WantedBy=default.target`, which is what `scripts/linux/pi-seed.sh` looks for when it packs user units.

Every listen address is loopback. Ports are the desktop probe defaults in `Service::default_port` (`crates/openmind-desktop/src/probe.rs`).

| unit | command | probe |
| --- | --- | --- |
| obsidian-desktop | `openmind-desktop --listen 127.0.0.1:18181 --stack off` | the listener. `DEFAULT_BIND` is `127.0.0.1:18181` (`crates/openmind-desktop/src/lib.rs`) |
| obsidian-nerve | `openmind-nerve serve` | `nerve` `GET /health` port 8670 |
| obsidian-cortex | `gaia-cortex serve` | `cortex` `GET /health` port 8674, `CORTEX_ACTIONS=dry-run` |
| obsidian-console | `galion-console-serve --bind 127.0.0.1:18080 --local-api --ui rust --quiet` | `console` `GET /api/v1/health` port 18080 |
| obsidian-hermes | `hermes-gateway --offhost-standby` | `hermes` `GET /health` port 8642. Standby does not call a model |
| obsidian-desk | `galion-desk` | `desk` `GET /health` port 8646 |
| obsidian-memory | `openmind-memory` | `memory` `GET /health` port 8420 |
| obsidian-one | `galion-one` | `galion-one` `GET /health` port 8091, and `voice` `GET /health` port 8002 |

`galion-one` `GET /health` on the main listener writes `ok` with status 200 (`crates/galion-one/src/main.rs`, the `/health` arm). The mouth on port 8002 is the voice probe. With no `GALION_MOUTH`, `local()` is the `Unavailable` stub (`crates/galion-one/src/speech.rs`, `local`) and `mouth_health` returns HTTP 503 (`crates/galion-one/src/main.rs`, `mouth_health`). The desktop then reports voice as `down` with detail `http 503` (`crates/openmind-desktop/src/probe.rs`, `classify_http`). This install does not add a speech backend, so that 503 is the stub, not a Raspberry Pi measurement.

Cortex stays dry-run (`CORTEX_ACTIONS=dry-run`, `CORTEX_LLM_ENABLED=0`). Nerve stays dry-run and does not call systemd or the journal (`NERVE_DRY_RUN=1`, `NERVE_SYSTEMD=0`, `NERVE_JOURNAL=0`, `NERVE_REFLEX=0`, `NERVE_ROLLBACK=0`). Hermes is the off-host standby, which serves health and does not start a model provider. No API key is written into a unit.

The desktop `--stack off` does not dial Galion One's intake. Stack rows in the page stay `staged` or `planned` (`crates/openmind-desktop/src/stack.rs`). Starting these processes does not mark a row live.

When systemd is pid 1, the units are enabled and started. When it is not, the script prints `pi-bookworm: systemd is not running; starting the same commands in the background` and runs the unit `ExecStart` lines as the `obsidian` user.

## Emulator job

`.github/workflows/pi-seed.yml` job `bookworm arm64 emulator` runs `scripts/linux/pi-bookworm-emulator.sh` after the aarch64 seed binaries are built. The job debootstraps Debian Bookworm `arm64` and runs the install twice under qemu-user.

That root is Debian, not Raspberry Pi OS. qemu-user is not a Raspberry Pi 5. Both scripts print:

```
pi-bookworm: emulator result, not a Raspberry Pi 5 result and not a Raspberry Pi OS boot
```

The job then runs `openmind-desktop --probe` and `check-report`. The JSON in the job log is that run's output. It is not a Raspberry Pi 5 result. `check-report` expects the seven services other than voice to be `up`, and voice to be `down` with HTTP 503, because this install has no speech backend. The desktop binary exits 1 when any enabled probe is not up (`DesktopError::Probes` in `crates/openmind-desktop/src/error.rs`). The job allows that exit code and still checks the JSON.
