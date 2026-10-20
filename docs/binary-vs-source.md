<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Binary vs source

- DESK-ID: openmind.BINARY-STRINGS
- Stamp: 2026-10-20T09:58:31+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Method: `strings` and `strings -a` on the two dist binaries only. Source names were read from the two main.rs files. Binaries were not executed. Binaries were not copied.

The stamp offset is numeric. It is not a trailing UTC designator.

## Names

| Name | Where | Hits |
| --- | --- | --- |
| ONE_HERMES_DIR | dist/bin/galion-one via strings | 0 |
| ONE_HERMES_DIR | dist/bin/galion-one via strings -a | 0 |
| ONE_HERMES_DIR | crates/galion-one/src/main.rs | 6 |
| DESK_STATE_DIR | dist/bin/galion-desk via strings | 0 |
| DESK_STATE_DIR | dist/bin/galion-desk via strings -a | 0 |
| DESK_STATE_DIR | crates/galion-desk/src/main.rs | 4 |

## Compare

dist/bin/galion-one does not contain ONE_HERMES_DIR. crates/galion-one/src/main.rs does, on lines 44,54,251,3733,3875,3899.

dist/bin/galion-desk does not contain DESK_STATE_DIR. crates/galion-desk/src/main.rs does, on lines 15,168,1013,1033.

strings on dist/bin/galion-one still finds other names: ONE_BIND (1), ONE_MOUTH_BIND (1), GALION_HOME (1), GALION_MOUTH (1). It does not find ONE_HERMES_DIR.

strings on dist/bin/galion-desk still finds DESK_BIND (1) and DESK_HERMES_BIN (1). It does not find DESK_STATE_DIR.

## Not done

Binaries were not executed. Binaries were not copied. Nothing was committed. Nothing was pushed. cargo was not run. systemctl was not used. Python was not used.
