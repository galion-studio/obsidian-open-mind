<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
# galion-nerve

NERVE for Obsidian Open Mind, made by Galion: V1 self-analysis and self-eval,
plus the V2 nervous-system hub. One loopback service, std + `serde_json` +
`zagreb-time`, no TLS, no C deps. See [`docs/ws-nerve.md`](../../docs/ws-nerve.md).

```sh
galion-nerve serve            # loop + API on 127.0.0.1:8670 (NERVE_ADDR)
galion-nerve once [--md]      # one observe-only cycle, prints the self-eval report
```

API (loopback only): `GET /` dashboard, `GET /state`, `GET /self-eval`,
`GET /self-eval.md`, `GET /alerts`, `GET /audit`, `GET /signals`,
`GET /healthz`, `POST /signal`.

Reusable probes: `galion_nerve::probe` (systemd `show` batch, /proc RSS,
journald error counts, HTTP, cloudflared metrics, df, meminfo) is a library
module so `galion-health` can depend on it instead of keeping its own copy.
