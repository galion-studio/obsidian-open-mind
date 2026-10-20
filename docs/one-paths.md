<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# One paths

- DESK-ID: openmind.ONE-PATHS
- Written: 2026-10-19T21:04:58+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Source: crates/galion-one/src/main.rs only. The crate was not edited. The binary was not run. /etc/openmind was not read.

Missing-file defaults are the `DEFAULTS` constants. They apply when the config file is absent. A directory at the config path counts as absent. `OPENMIND_*` is not consulted in that case.

| Item | Missing-file default | Env var that wins when set |
| --- | --- | --- |
| page root | /workspace/galion-light | GALION_HOME |
| Hermes state dir | /workspace/hermes | ONE_HERMES_DIR |
| bind | 127.0.0.1:8091 | ONE_BIND |
| mouth bind | 127.0.0.1:8002 | ONE_MOUTH_BIND |

## Page root

`DEFAULTS.home` is `/workspace/galion-light`.
The header comment names this the page and state root.
`GALION_HOME` wins when set, including an empty or blank value.
The page file is read as `{home}/one/index.html`.

## Hermes state dir

`DEFAULTS.hermes_dir` is `/workspace/hermes`.
`ONE_HERMES_DIR` wins when set to a non-empty value after trim.

## Bind

`DEFAULTS.bind` is `127.0.0.1:8091`.
`ONE_BIND` wins when set to a non-empty value after trim.

## Mouth bind

`DEFAULTS.mouth_bind` is `127.0.0.1:8002`.
`ONE_MOUTH_BIND` wins when set to a non-empty value after trim.

## Stamp

The stamp offset is numeric. It is not a trailing UTC designator.
