<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# Real tuner 2

- DESK-ID: openmind.REAL-TUNER-2
- Stamp: 2026-10-20T09:59:00+02:00
- Repo: /opt/galion/openmind
- Binary: /tmp/om-realtuner2/debug/tuner
- Result: PASS
- Child: 3118662 started, then killed
- pid 1196273: left running. Not signaled.

The stamp offset is numeric. It is not a UTC mark.

## Build

Source was not edited. The build command was PATH=/root/.cargo/bin:$PATH CARGO_TARGET_DIR=/tmp/om-realtuner2 nice -n 19 cargo build -j 1 -p tuner --offline. Cargo finished the dev profile. Cargo exit was 0.

Cargo.lock was copied to /tmp/om-lock-REALTUNER2-before before cargo. Cargo did not rewrite Cargo.lock. The copy was not written back. git checkout was not used on Cargo.lock.

The new binary stayed under /tmp/om-realtuner2. It was not copied to dist/bin or /opt/galion/tuner.

## Run

TUNER_ADDR=127.0.0.1:18650 and TUNER_ROOT=/tmp/om-realtuner2-www. TUNER_CONFIG was unset. A small index.html was placed in that root (45 bytes).

The process logged: tuner 127.0.0.1:18650 serving /tmp/om-realtuner2-www.

It listened on 127.0.0.1:18650. It did not listen on 8650. pid 1196273 stayed on 127.0.0.1:8650.

One GET:

| URL | HTTP status |
| --- | --- |
| http://127.0.0.1:18650/health | 200 |

The child was killed after that curl. Port 18650 was closed. pid 1196273 was not killed.

## Not done

- No commit.
- No push.
- systemctl was not used.
- Python was not used.
