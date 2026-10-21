<!--
SPDX-License-Identifier: MIT
Copyright (c) 3000 Galion / galion-studio
-->
# Real-life tests: vps-strays (slot 117)

## Box: mock gateway RSS under the VPS bench mix
`GALION_REAL=1 cargo test -p hermes-gateway --release --target x86_64-unknown-linux-musl --test real_mock_rss -- --nocapture`
The test starts the real release binary `hermes-gateway --mock` on 127.0.0.1:19170, with a throwaway HERMES_HOME, cron off and the call cap lifted, as the VPS strays had it.
Each round sends 5000 GET /health (c8), 3000 GET /v1/models (c8), 600 chats (c3) and 1200 chats (c12). After each round it reads VmRSS/VmHWM from /proc, idles 5 s, reads them again, and kills the process at the end.
The c12 stage is above `max_concurrent_runs`, so most of its requests are non-200, as in the VPS bench, which got 429s.

Unpatched main 13a296c, 2026-10-21 02:43:49 CEST, 8 rounds:
```
REAL start rss_kb=1944 hwm_kb=1944
REAL round=1 ok=8673 non200=1127 secs=6.4 rss_kb=8292 ...
REAL round=4 ok=8628 non200=1172 secs=6.1 rss_kb=25428 ...
REAL round=8 ok=8629 non200=1171 secs=6.2 rss_kb=47120 idle5s_rss_kb=47120 hwm_kb=47536
REAL verdict round1_growth_kb=6348 later_round_growth_kb=[5856, 5724, 5556, 5524, 5524, 5428, 5216]
```
With the fix (this branch), 2026-10-21 02:45:58 CEST, 8 rounds:
```
REAL start rss_kb=1900 hwm_kb=1900
REAL round=1 ok=9340 non200=460 secs=6.6 rss_kb=3192 ...
REAL round=8 ok=8628 non200=1172 secs=6.2 rss_kb=4908 idle5s_rss_kb=4908 hwm_kb=5992
REAL verdict round1_growth_kb=1292 later_round_growth_kb=[488, 380, 300, 312, 192, -8, 52]
```
With the fix, default 6 rounds and the regression gate (last 3 rounds < 2048 kB) active, 2026-10-21 02:48:21 CEST: `later_round_growth_kb=[456, 356, 308, 348, 204]`, test ok.
The first gnu-target run (02:41 CEST) hit the default mock call cap after 500 chats, so it is not counted. That cap is lifted in the test now.
Afterwards no listener was left on 19170-19179 (`ss -ltn | grep -c :1917` = 0).

## VPS: read-only comparison (same metric on the strays)
| Sample (CEST) | 8431 | 8432 | 8433 | 8652 | 8653 | live 8642 |
|---|---|---|---|---|---|---|
| 10-20 22:52 (vps-baseline) | 4040 | 6728 | 3304 | 53172 | 29820 | - |
| 10-21 02:35 | 4040 | 6728 | 3312 | 53172 | 29820 | 4312 (HWM 20600) |
| 10-21 02:49 | 4040 | 6728 | 3312 | 53172 | 29820 | - |
These RSS values are in kB. They are flat while idle: the memory was retained during the 10-18 bench and has not grown since. Established connections were 0 in every sample, and all five strays answered GET /health with 200.
