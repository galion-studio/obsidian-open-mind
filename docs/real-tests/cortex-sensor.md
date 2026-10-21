<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Real tests: cortex-sensor (T1.2, slot 43, ports 19430-19439)

Box only. No VPS access was used (the live galion-nerve 8670 is the old a63b730 build without a cortex registry, so
there is nothing to compare there yet). All times Europe/Zagreb.

## 1. No behaviour change (replay goldens)

Pre-trait binary = integration-1 8fd26f6 built twice: `cargo build --release -p openmind-nerve` and
`cargo build --workspace` (the second has serde_json `preserve_order` through workspace feature unification).
Run as `env -i NERVE_DATA_DIR=/tmp/x openmind-nerve replay DIR [--md]`; JSON compared with `jq -S` after removing
the two additive finding keys `kind` and `source` (2026-10-20 23:06 CEST):

| capture | build | md5 report (pre / new) | md5 markdown (pre / new) | verdict |
|---|---|---|---|---|
| /workspace/swarm/nerve-capture/replay1 (live VPS capture) | `-p` release | 1ab42ae5 / 1ab42ae5 | fce3711a / fce3711a | 61/100 D, FAIL (unchanged) |
| same | `--workspace` debug | e537e490 / e537e490 | dee44c2d / dee44c2d | same |
| crates/openmind-nerve/tests/fixtures/cortex-replay (synthetic, all 5 sensors fire) | both | identical (committed goldens) | identical | 21/100 F, FAIL, ddos firing |

`tests/cortex_replay.rs` (runs in every `cargo test`) asserts the committed goldens; it passes both with
`-p openmind-nerve` and with `--workspace`.

## 2. GALION_REAL=1 box test

`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_cortex_sensor -- --nocapture`
(2026-10-20 23:05 CEST, release binary; also passed with `--workspace` debug at 23:05:43):

```
replay fixture: score 21 grade "F" pass false (= pre-trait golden)
replay1 live capture: score 61 grade D (unchanged)
serve :19430 cycle 1: sensors ["listeners", "scanners", "edge", "components", "ddos"] us [4, 0, 0, 0, 0], cycle_ms 23, rss 3200 KiB; event {"ts":"2026-10-20T23:05:32+02:00","source":"listeners","kind":"vulnerability","severity":"warn","data":{"findings":1,"ids":["unmanaged-listeners"],"top":{"cause":null,"component":"host","evidence_n":2,"fix":"identify the owners (ss -ltnp), then stop them or give them a unit (owner approval; the nerve never kills processes)","id":"unmanaged-listeners","title":"1 loopback listener(s) no monitored unit owns: :19433"}}}
serve :19430 cycle 2: stray gone, finding cleared
serve :19434 NERVE_SENSORS_OFF=listeners cycle 2: sensors ["scanners", "edge", "components", "ddos"], stray :19435 not reported
test real_cortex_sensors_on_the_box ... ok
```

What it does: runs the real binary's `replay` on the fixture and on replay1; starts `openmind-nerve serve` on
127.0.0.1:19430 with a live HTTP service (:19431, a test thread), a dead component (:19432) and a stray listener
(:19433); reads `/api/v1/state` (`cortex_sensors`, `cortex_events` parsed as `openmind_schema::NerveEvent`) and
`/api/v1/cortex`; drops the stray and waits for the finding to clear; then a second nerve on :19434 with
`NERVE_SENSORS_OFF=listeners` and a stray on :19435. All processes and listeners are torn down by the test
(`ss -ltn | grep :1943` empty afterwards). Built-in sensor cost: 0-4 us each in release, cycle 23 ms, RSS 3.2 MiB.

`tests/real_box.rs` (slot 10, ports 19105-19107) was not run: those ports are outside this slot's range.

## 3. Checks (2026-10-20 23:04 CEST)

- `cargo fmt --check`: ok. `cargo clippy --workspace --all-targets -- -D warnings`: ok.
- `cargo test --workspace --no-fail-fast`: 965 passed, 0 failed, 4 ignored (integration-1 had 950; +15 new).
- `scripts/no-python.sh`: ok. `repo-check`: exit 0 (clean, same as base). `scripts/spdx-scan.sh`: 0 missing.
- `cargo check -p openmind-nerve`: x86_64/aarch64 musl, aarch64-linux-android, armv7-unknown-linux-musleabihf ok.
- Release builds: x86_64-unknown-linux-musl 1127200 bytes; aarch64-unknown-linux-musl links with `-C linker=rust-lld`
  (host `ld` rejects `--fix-cortex-a53-843419`, as in docs/real-tests/nerve.md section 5).
