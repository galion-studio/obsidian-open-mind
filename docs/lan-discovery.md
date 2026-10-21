<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# LAN discovery (`openmind-discovery`)

- Written: 2026-10-20T19:30:00+02:00 (Europe/Zagreb)
- Branch: `swarm/lan-discovery` (swarm slot 11), off main 13a296ce

## What it does

| Command | Network use | Writes |
|---|---|---|
| *(no subcommand)* | none (seed list, optional `--fixture`) | nothing |
| `--scan` (legacy flag) | one mDNS browse, 2 packets | nothing |
| `find` | mDNS browse: one PTR query + one repeat (at 1/3 of the timeout) to 224.0.0.251 from a random port, bounded wait (default 2 s, max 30 s). With `--probe A.B.C.0/24` only: paced TCP connects (default 20/s, max 100/s, 300 ms timeout) plus one `GET` on HTTP ports. | new rows as `pending` provider rows (`kind: lan`); `found` / `seen` log lines |
| `list` | none | nothing |
| `approve ID` / `deny ID` | none | approval `approved` / `rejected`; log line |
| `join ID` | approved rows only: one TCP connect (no payload) to the stored endpoint, and only if it is loopback / RFC 1918 / link-local / IPv6 ULA | health `up` / `down`; log line (`join` or `join-refused`) |
| `announce` | binds the mDNS port with SO_REUSEADDR/SO_REUSEPORT (shares 5353 with avahi), joins 224.0.0.251, sends 2 startup announcements 1 s apart, answers only questions about its own services/host, at most 20 replies per second, exits after `--duration-s` (max 3600) | nothing |

Browsed service types by default: `_galion._tcp`, `_exo._tcp`,
`_llama-rpc._tcp`, `_llamacpp._tcp`, `_ollama._tcp`, `_openmind._tcp`.

Rules the code enforces:

- Every found device becomes a provider row in `pending`. A device found
  again keeps its approval (a denied device stays `rejected`). The same
  endpoint found by mDNS and by the probe is one row.
- Only `approve` admits a device. `join` refuses anything not approved.
- The probe only accepts a `/24` inside 127/8, 10/8, 172.16/12, 192.168/16
  or 169.254/16, refused before any socket. Host range `--probe-hosts N-M`.
- All arguments are validated before any socket is opened.
- Registry: `--registry PATH` (default `./discovery-providers.json`, the
  `openmind-provider` v1 JSON). Log: `--log PATH` (default
  `<registry>.approvals.log`), JSON lines, each with `ts` in Europe/Zagreb
  RFC 3339 with offset (e.g. `2026-10-20T19:23:16+02:00`), `event`, `id`,
  `endpoint`, `by`, and `from`/`to` for decisions. No secrets are stored.

Provider ids are `<kind>-<instance id>` (mDNS; the TXT `id` wins over the
instance label) or `<kind>-<a-b-c-d>-<port>` (probe). Endpoints are
`http://ip:port`, or `tcp://ip:port` for llama.cpp `rpc-server`.
Capabilities come from TXT `caps=llm,stt,tts,vision`, else `llm` for
Exo/Ollama/llama.cpp.

## Real Exo, Ollama and llama.cpp do not announce `_exo._tcp` etc.

Checked 2026-10-20 (upstream sources/issues, read-only):

- **Ollama** has no mDNS / DNS-SD announcement. It only listens on HTTP
  (default 127.0.0.1:11434; LAN only with `OLLAMA_HOST=0.0.0.0`).
- **llama.cpp** `llama-server` (HTTP, default 8080) and `rpc-server`
  (binary, default 50052) do not announce anything.
- **Exo**: current Exo (1.x, Rust networking) finds its own peers with
  libp2p mDNS (`_p2p._udp.local`, peer id + multiaddr in TXT), see
  https://github.com/exo-explore/exo/blob/1e51dc89/rust/networking/src/discovery.rs.
  That is not a DNS-SD `_exo._tcp` record and does not carry the HTTP API
  port (52415). Older Exo (0.0.x Python) used UDP broadcast on port 5678.
  Neither is browsed here.

So for those three servers the `_exo._tcp` / `_ollama._tcp` /
`_llamacpp._tcp` / `_llama-rpc._tcp` names only appear when a **Galion edge
wrapper** runs next to them and announces them (for example
`openmind-discovery announce --service ollama:desk:11434:caps=llm`), or the
operator runs `find --probe <local /24>` to find their default ports
(52415, 11434, 50052, 8080, 7787). Probe hits are marked `verified=true`
only when the HTTP fingerprint (`/models`, `/api/version`, `/health`)
answers 200 with JSON.

## Limits / open points

- The announcer does not do RFC 6762 name-conflict probing; it is an
  edge/test responder, not a host responder. IPv4 only.
- The provider registry (`openmind-provider`) has no "update endpoint"
  call, so a known device seen at a new address is reported as
  `seen_endpoint` and logged, not rewritten.
- `join` is a reachability check and registry health update; wiring an
  approved row into routing is the router's job (`openmind-provider::route`).
