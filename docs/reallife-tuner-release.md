<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Real life tuner release

- DESK-ID: openmind.TUNER-REL
- Stamp: 2026-10-20T10:27:37+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Binary: /tmp/om-tunerrel/release/tuner
- Result: PASS
- Child pid 3142088 was started, then stopped. pid 1196273 was not signaled.

The stamp offset is numeric. It is not a trailing UTC designator.

## Build

Source was not edited. Build command: `PATH=/root/.cargo/bin:$PATH CARGO_TARGET_DIR=/tmp/om-tunerrel nice -n 19 cargo build -j 1 -p tuner --release --offline`. It finished the release profile in 2.28s. Exit code 0. The new binary was run from `/tmp/om-tunerrel/release/tuner` only. It was not copied onto `dist/bin`, `/opt/galion/tuner`, or any live path.

`strip` is at `/usr/bin/strip`. It was run on that release binary. Byte size before strip: 331952. Byte size after strip: 331944. The recorded byte size is 331944.

The release profile already asks the compiler to strip. The extra `strip` pass removed 8 bytes.

Before cargo, `Cargo.lock` was copied to `/tmp/om-lock-TUNERREL-before`. After the build the file matched that copy. sha256 was `2098030715ceb1363634d495e96d5d9231ce46e9c45e3faafcbfc4da4f1fefea` both times. Cargo did not rewrite the lock, so the snapshot was not put back. `git checkout` was not used.

A second name for the same inode is `/tmp/om-tunerrel/release/deps/tuner-52c0b2ef79ad6eae`. That name is inside the cargo target dir. It is not a live path.

## Run

The child was started once. The env was only:

- `TUNER_ADDR=127.0.0.1:18650`
- `TUNER_ROOT=/tmp/om-tunerrel-www`
- `PATH=/usr/bin:/bin`

The log line was `tuner 127.0.0.1:18650 serving /tmp/om-tunerrel-www`. The listen socket was only `127.0.0.1:18650`. It was not on 8650. `TUNER_ROOT` was the empty dir `/tmp/om-tunerrel-www`.

One GET. No retry. Loopback only. The body was not copied into this file.

| URL | HTTP status | content-type | bytes |
| --- | --- | --- | --- |
| http://127.0.0.1:18650/health | 200 | application/json | 12 |

## After

pid 3142088 is gone. Port 18650 is not listening. pid 1196273 is still `/opt/galion/tuner/bin/tuner` on `127.0.0.1:8650` (started Sun Oct 18 17:18:07 2026, start tick 56263217, same as before this run). `dist/bin/tuner` is absent. No unit was changed. systemctl was not used. Nothing was committed. Nothing was pushed. Python was not used.
