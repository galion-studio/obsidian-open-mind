<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# console-bank-reader-plan (wave 4 slot 351): test evidence

Full plan and numbers: `docs/plans/console-bank-reader.md` (sections 2 and 5). Times CET +01:00.
Box ports 22210-22217, all released afterwards. No VPS writes, no deploy.

- VPS read-only 04:05-04:09: unit/env-var names, `ss -ltn`, `stat` of key/spend files (no content),
  `jq` shapes of `survival.json` and `rs-spend.json`, GETs: `:8080`/`:18080 /api/v1/bank` 404,
  `:8642 /health` 200, `:8642 /api/spend` + `/api/v1/bank` 401 (gateway key). Nerve 04:00:01
  cycle 48 live: hermes-rs healthy, console healthy. Temp files `/tmp/.s351b`, `/tmp/.s351n` created
  and removed.
- Box 04:07-04:09 (release galion-console-static 7888d8fb + scratch Rust fake reader, not committed):
  fixture proxied byte-identical (200, 754 B), `/bank` renders all fixture figures (15434 B, 1 ms,
  0 `<script>`), 200 proxied GETs 1259 ms total, reader down -> 502 and honest page, gate cases
  (POST 405, evil host 401, `/api/v1/bankx` 404, `/api/arena` not listed 404) never reached the reader.
  Header capture shows Cookie and Authorization forwarded (F-351-1).
- Box 04:10: the staged console argv (`console-bank.conf`, bind/dist swapped to box values) starts;
  `/api/v1/health` 200, `/api/v1/bank` 502 (no reader on box :8649), `/bank` 200. Config guards:
  `--data-upstream 0.0.0.0:8649` refused (loopback only), `--data-path /api/v1/bank/` refused.
