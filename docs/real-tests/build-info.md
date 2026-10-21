<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# build-info real test (2026-10-20 23:05 CEST +02:00)

`GALION_REAL=1 cargo test -p galion-common --test real_version -- --nocapture`
builds 10 binaries in release with GALION_BUILD_COMMIT=realtest0 and
GALION_BUILD_TIME=2026-10-20T23:00:00+02:00, then runs `<bin> --version` with an empty env.
All exit 0 in under 1.1 ms (the limit is 100 ms):

```
1.084002ms desk-hermes 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
780.304µs galion-boot 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
551.566µs galion-desk 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
898.612µs galion-one 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
742.732µs galion-origin 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
660.783µs link-watch 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
508.875µs one-talk 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
615.113µs openmind-memory 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
498.779µs room 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
599.835µs tuner 0.1.0 (commit realtest0, built 2026-10-20T23:00:00+02:00)
```
No new dependencies. Health/status JSON is unchanged on purpose (see the plan, S2.2: "no new HTTP fields").
