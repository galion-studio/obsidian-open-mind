<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Real tests: nerve-t36-consumer (wave-4 slot 180, ports 20500-20509)

All times Europe/Zagreb (CEST +02:00), 2026-10-21. No secrets in this file.
Matrix and breaks: docs/nerve-consumers.md.

## 1. Read-only VPS probe of the live galion-nerve (08:57:02 +02:00)

GETs only (curl on the VPS to 127.0.0.1:8670), `systemctl show` of ExecStart
and Environment *names* only, `ss -ltn`. Nothing written, nothing restarted.

```
200 text/plain 3 /healthz              body "ok"
200 text/html; charset=utf-8 4046 /    <title>galion-nerve</title>
404 text/plain 10 /health              <- B1 (gaia-live healthy() false)
200 application/json 13229 /state
404 /incidents /cortex /ddos /events /api/v1/*   <- /events: B2
200 /self-eval (5809 B) /self-eval.md (3726 B) /alerts /audit /signals ({"rejected","sources"})
405 /signal (GET)
unit: ExecStart=/opt/galion/rust/bin/galion-nerve serve
      Environment names: NERVE_ADDR NERVE_INTERVAL NERVE_DATA_DIR NERVE_DRY_RUN NERVE_FAIL_CYCLES NERVE_REPORT_EVERY
      (no NERVE_EVENT_LOG -> B2 also after cutover)
state: cycle 7004 mode live system degrading; 15 components (galion-one, cloudflared restarts_1h 1; rest 0); 10 reflex budgets, live_1h 0
top keys: alerts components cycle cycle_ms gaia interval_s mode probe_errors reflex signals started system tool ts version
```

## 2. Box: release binary, `real_consumer_compat::box_binary` (3 runs: 09:04:09, 09:09:25, 09:09:30 +02:00; 3/3 pass, 5.2 s each)

`GALION_REAL=1 cargo test --release -p openmind-nerve --test real_consumer_compat box_binary -- --nocapture --test-threads 1`
(nerve 20500, dummy 20501, no-log nerve 20502; dry-run, no systemd/journal; temp dir removed; ports free after.)

```
phase0 /state body before/at first cycle: null
phase0 realtest nerve-healthz -> Ok("HTTP 200 text/plain body \"ok\"")
phase0 realtest nerve-dashboard -> Ok("HTTP 200 text/html; charset=utf-8 (1556 body bytes, not recorded)")   <- B3: pre-cycle page, passes only via the new title
phase1 cycle 1 mode "dry-run" system "healthy"
realtest live row nerve-healthz -> Ok (354 us); nerve-dashboard -> Ok (6673 B, 218 us)
realtest ingest scenario: 202 / visible / 400 ok
nerve-client 100 signals in 20 ms: Stats { queued: 100, sent: 100, dropped: 0, failed: 0, rejected: 0 }
nerve-client burst: client Stats { queued: 180, sent: 170, dropped: 0, failed: 0, rejected: 10 }; nerve hermes-rs dropped 10   <- B4
gaia-live healthy() = true (151 us); post() = Ok (179 us); tail(10) = 1 record, ts "2026-10-21T09:04:09+02:00"
routes: /healthz 200 2B; / 200 6673B; /health 200 145B; /state 200 15322B; /self-eval 200 13747B; /self-eval.md 200 8063B;
        /signals 200 2208B; /events 200 141B; /incidents 200 42B; /cortex 200 7327B; /ddos 200 545B; /alerts 200 2B; /audit 200 2B; /api/v1/state 200 15322B
gaia /state contract on real engine state (cycle 2): Ok((3 components, 10 budgets)); t36-dummy healthy http 200 restarts_1h 0
after SIGKILL + torn line + restart: 2 readable records (before 1), last kind "after-crash"   <- B5
  nerve log: <4>openmind-nerve: GET /events skipped 1 unreadable line(s) in .../events.jsonl
B2 nerve without event log: nerve POST /event: HTTP 404: {"ok":false,"error":"no event log configured (--log / NERVE_EVENT_LOG)"}
```

## 3. VPS live, `real_consumer_compat::vps_live` (09:04:24 and 09:05:02 +02:00; pass)

`GALION_REAL=1 GALION_REAL_VPS=1 cargo test --release ... vps_live` (fixed GET-only ssh script).
Observed through the nerve (127.0.0.1:8670 /state); the cortex is not deployed yet
(slot 175 read-only check 08:44: 404), so the nerve is the observation point.

```
live realtest row nerve-healthz -> Ok("HTTP 200 text/plain body \"ok\"")
live realtest row nerve-dashboard -> Ok("HTTP 200 text/html; charset=utf-8 (3989 body bytes, not recorded)")
B1 live GET /health -> Some(404); gaia-live healthy() would be false
B2 live GET /events -> Some(404) (gaia-live tail fails)
live /state cycle 7019 mode "live" system "degrading"; gaia contract Ok((15, 10))
ss: 1 listener on :8670; galion-nerve active
round 2: cycle 7020, same results; live nerve cycles observed: [7019, 7020]
```

## 4. Workspace and builds (box)

- `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings`
  (excl. openmind-sandbox, cgroup-detect, hermes-tools) ok; `clippy -p hermes-tools --all-targets` ok.
- `cargo test --workspace --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools`:
  **1241 passed, 0 failed, 6 ignored** (new: 11 consumer_compat + 2 gated real + 2 nerve-client + 1 server = 16).
- consumer_compat run 4x in a row: 11/11 each (first draft failed 2 tests, which found the
  split-status-line issue fixed in B4).
- openmind-nerve + nerve-client: x86_64 and aarch64 musl release builds (static), aarch64-android
  and armv7 `cargo check` ok. `scripts/no-python.sh` ok.
- Not re-run: `real_wiring` (slot 155's ports 20250-20258, outside this slot's range); its
  heartbeat parse (`failed=`) is unchanged by the appended `rejected=`.
