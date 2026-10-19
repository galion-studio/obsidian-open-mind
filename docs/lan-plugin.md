<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# LAN plug-in interface (design only)

- DESK-ID: openmind.B32
- Written: 2026-10-19T16:08:49+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Status: **Design only.** No code, no listener, no DNS change, no tunnel process, no unit, no registry file. Nothing is installed.
- Accept: the flow and the data model below match openmind-schema `Provider` (`id`, `kind`, `base_url`) and the Provider idea (`id`, `kind`, `endpoint`, `approval`).

Related: `docs/goals.md` (Local network instances), `docs/phone-instance.md`, `docs/hermes-enrollment.md`, `TEACHER/A-foundation.md` section 5, `crates/openmind-schema` (`Provider`, `ProviderKind`). Later tickets B34-B38 implement pieces of this page. This page does not.

## 1. Goal

A device on the home LAN (laptop, Mac, NVIDIA or AMD box, phone) can be added to the Obsidian Open Mind stack. It is found by mDNS or a restrained LAN look, then registered as one OpenMind Provider row, then reached from the VPS through a tunnel that the device dials.

The rule that gates every step: **no device joins without explicit approval.** Joining means a `Provider` row exists. Discovery does not join. A pending offer does not join. Only Slavik's approval writes the row.

The schema line this follows: adding a site is a new row, not a new program. The processor keeps the loop, the memory, and the key box. A provider is an adapter, not a secret.

## 2. Out of scope for this ticket

- No source file, crate, binary, or test.
- No socket and no listener. The omnode announce SRV port (7787) still has nothing bound. This design does not bind it.
- No DNS change. No new hostname, no zone edit, no tunnel route edit.
- No write to the schema, to Hermes, or to a registry file on disk.
- No production change. No `systemctl`. No inbound port on the VPS.
- No secret in this page or in the future row. `key_slot` is a name. The key box holds the secret, and the schema never does.
- No hard-coded host or address. Hosts, the enroll URL, and the loopback port come from config at the later tickets.

## 3. Who does what

The VPS is a data-centre container. It cannot see the home Wi-Fi. Discovery runs on a device that is already on that LAN (today: `omnode discover`). The VPS only stores a row after approval, and only calls a loopback endpoint.

| Actor | May do | May not do |
| --- | --- | --- |
| LAN device | Announce `_openmind._tcp`, or run its own compute server | Join by itself; open a path into the LAN for the VPS |
| LAN node (already on the Wi-Fi) | Browse, show candidates, offer one | Approve; write a Provider row |
| Slavik | Approve or reject an offer | (nobody else may) |
| VPS desk | After approval, call `127.0.0.1` only | Dial an RFC 1918 address; browse mDNS |

Heavy runtimes stay on the LAN host. They are never started on the VPS.

## 4. Discovery (mDNS / LAN)

Discovery builds a **candidate list**. A candidate is not a registry record and not a Provider row. `approval` is absent. `endpoint` is empty.

### 4.1 Passive browse (default)

- UDP 5353, group 224.0.0.251, DNS-SD, bounded (existing default 5 s, max 120 s).
- Service that matters: `_openmind._tcp.local.`
- TXT already advertised by omnode, and only those keys are interpreted: `v`, `id`, `fp`, `arch`, `cores`, `ram_mb`, `platform`, `caps`, and optional `name`. Unknown keys are kept as text and are not used for decisions.
- The network hash and the raw SSID are not advertised and are not stored.
- Other service types may be noted. Noting one does not offer it.

### 4.2 Active scan (off by default)

Matches the later `openmind-discovery` stub (B35). A static seed list plus a parser for mDNS-SD records. The active scan sits behind a flag and is off unless the owner passes it. Tests of that later crate use fixtures and do not open 5353.

The one-packet query (`_services._dns-sd._udp.local.` and `_openmind._tcp.local.`) stays opt-in, as `omnode discover --mdns-query` is today. It is not run on the VPS.

### 4.3 Probe (opt-in, not a join)

A TCP connect probe runs only if the owner asks, only against hosts already seen, only on the local /24, rate-limited, and only when that network is bound as trusted and the address is private. Ports of interest, already the omnode list: 11434 (ollama), 50052 (llama.cpp `rpc-server`), 52415 (exo). A hit is still only a candidate.

No default subnet sweep. No fetch of description URLs. No ARP or ICMP sweep. No browse on a network that is not the owner's.

Android may drop multicast when the process holds no multicast lock. A thin browse is acceptable. The design does not answer that by scanning harder.

### 4.4 Seed list

The owner may type a candidate in config (a name, a kind proposal of `local`, a LAN hint). The file ships with no addresses. A seed is offered and approved like any other candidate. It does not join because it was typed.

### 4.5 Candidate (local, not persisted as a Provider)

```json
{
  "id_hint": "",
  "kind": "local",
  "endpoint": "",
  "approval": null,
  "source": "mdns",
  "service_type": "_openmind._tcp.local.",
  "instance": "",
  "lan_hint": "",
  "txt": [],
  "seen_at": "2026-10-19T16:08:49+02:00"
}
```

`seen_at` is Europe/Zagreb RFC 3339 with an explicit offset, never `Z`. `approval: null` means "not offered". `lan_hint` is what the browse saw on the LAN. It is never copied into `endpoint`.

## 5. Approval

Three states, the B34 registry states. JSON is lowercase, the same `rename_all = "lowercase"` style as `ProviderKind`. Rust names are `Pending`, `Approved`, `Rejected`.

| approval | Meaning | Provider row | Tunnel |
| --- | --- | --- | --- |
| (no record) | Seen, or not seen. Not offered. | no | no |
| `pending` | Owner offered it. Slavik has not answered. | no | no |
| `approved` | Slavik approved. The row is the join. | yes | device may dial out |
| `rejected` | Slavik rejected or revoked. | no | dropped, and not redialed |

Rules:

1. mDNS presence does not create a record. The owner offers one candidate. That insert is `approval = pending`, `endpoint = ""`, `kind = local`.
2. Nothing auto-approves. No timeout, no "same LAN as last time", no second device vouching.
3. Approve and reject are explicit acts by Slavik. The console shows, for an `_openmind._tcp` peer, the fingerprint to compare with `identity show` on the device (same check as `docs/hermes-enrollment.md`). For a compute box that is not an omnode, it shows `id`, service, and `lan_hint`.
4. Pending expires after 24 h on the Zagreb clock and the record is dropped. Expiry is not a reject and not an approve.
5. Rejected stays rejected so the same candidate is not offered again by a later browse. A new join needs a new explicit offer from the owner.
6. Revoke is a transition from `approved` to `rejected`: delete the row, clear `endpoint`, drop the tunnel.
7. Two offers with the same `id` and a different fingerprint are refused. One device, one id.
8. Payment or money capabilities are never granted by this flow.

An offer is not a join. The desk must not call a pending or rejected record.

## 6. Data model

The plug-in record has four fields. These are the Provider idea. They are not a new type of program.

| Field | Values | Role |
| --- | --- | --- |
| `id` | string | Stable name. Becomes `Provider.id`. |
| `kind` | `local` or `remote` | Same tags as `ProviderKind`. |
| `endpoint` | string | URL the desk will call. Becomes `Provider.base_url`. Empty unless `approved`. |
| `approval` | `pending`, `approved`, `rejected` | Not a field on the schema struct. Lives on the registry beside the row. |

The schema row also has `key_slot` and `default_model`. They are filled only when the row is written. They are not a second approval channel.

| Idea field | openmind-schema | Rule |
| --- | --- | --- |
| `id` | `Provider.id` | Copied on approve. |
| `kind` | `Provider.kind` | Copied. LAN joins are `local`. |
| `endpoint` | `Provider.base_url` | Copied. Loopback URL on the VPS. |
| `approval` | (no field) | The row exists only while `approval` is `approved`. |
| (slot name) | `Provider.key_slot` | Name only. `KeySlot.present` stays false unless that slot already exists. The secret is not copied. |
| (model name) | `Provider.default_model` | Copied. Empty is allowed. |

Side fields sit next to the four in the registry JSON so a later health check can explain the row. They are not columns of `Provider`: `seen_at`, `source` (`mdns`, `seed`, `probe`), `service_type`, `instance`, `lan_hint`, `txt`, `capabilities`, `fingerprint`, `approved_at`, `rejected_at`. Times are Zagreb RFC 3339 with offset.

B34 persists that JSON at a configurable path. This ticket does not create the path and does not write the file.

### 6.1 `id`

- An `_openmind._tcp` peer uses the advertised `id` (`omn-` plus 12 hex). That is `Provider.id`. If the device also enrolls, it is the same id as in `docs/hermes-enrollment.md`.
- A compute box that is not an omnode (ollama, llama.cpp, exo) gets an id the owner types at offer time. Lowercase, no spaces, stable. The id is not derived from an address, so a DHCP change does not mint a second provider.

### 6.2 `kind`

- `local`: the desk reaches the device through a loopback tunnel. This is the same meaning as the laptop row in `Stack::cpu_desk`.
- `remote`: a hosted API that already has the card (the openrouter row). A device found by mDNS is never `remote`. This design does not add a public URL and does not create a DNS name to make one.

### 6.3 `endpoint`

- Empty while `pending` or `rejected`.
- On approve, a free `127.0.0.1` port is reserved from a configured range and stored as the endpoint. The range is config. It must not be a port the stack already uses. This page sets no default port and binds nothing.
- Shape: `http://127.0.0.1/<loopback-port>/<prefix>`. The prefix is the adapter's OpenAI-compatible prefix, `/v1` for ollama, llama-server, and exo. A llama.cpp `rpc-server` prefix is whatever the owner recorded, still on loopback.
- `lan_hint` is never copied into `base_url`. The VPS does not dial the LAN address.
- The existing laptop row uses `http://127.0.0.1:11434/v1`. A plugged-in device does not reuse that port.

### 6.4 Records

Pending offer (no schema row yet):

```json
{
  "id": "omn-example",
  "kind": "local",
  "endpoint": "",
  "approval": "pending"
}
```

Approved join. The registry record and the schema row are written in the same step:

```json
{
  "id": "omn-example",
  "kind": "local",
  "endpoint": "http://127.0.0.1/<loopback-port>/v1",
  "approval": "approved"
}
```

```json
{
  "id": "omn-example",
  "kind": "local",
  "base_url": "http://127.0.0.1/<loopback-port>/v1",
  "key_slot": "omn-example",
  "default_model": ""
}
```

If no free loopback port can be reserved, approval fails closed: the record stays `pending`, and no row is written.

## 7. What the row is allowed to front

The row is an adapter. The process behind it stays on the LAN host.

| LAN service | Later adapter | `kind` | Note |
| --- | --- | --- | --- |
| ollama | (existing laptop shape) | `local` | OpenAI-compatible `/v1`. Does not pool machines. |
| llama.cpp `rpc-server` / llama-server | B36 `LlamaCppProvider` | `local` | MIT. NVIDIA, AMD, Vulkan, Metal, CPU. Broad home GPU option. |
| exo | B37 `ExoProvider` | `local` | Apache-2.0. Apple Silicon via MLX. exo v1 GPU on Linux is in development (A-foundation.md section 5). Run it on the Mac, never on the VPS. |

The GPL CUDA fork of exo stays out of this MIT tree. It is not a `kind` and not an adapter here.

B34 `health()` and `capabilities()` read the loopback `endpoint` only. They do not read `lan_hint`. B38 `select_provider` sees only rows whose `approval` is `approved`. Order when several match: an approved local whose capabilities cover the need, then other approved LAN locals, then the CPU-only local path, then a `remote` hosted API. Pending, rejected, and empty-endpoint records are invisible to selection. A tunnel flap does not change `approval`; health fails and selection falls through.

## 8. Tunnel to the VPS

The device dials out. The VPS never reaches into the LAN. No new inbound port. No DNS change.

This is the rule, not a process. This ticket starts no tunnel.

1. The device may dial only after `approval` is `approved` and it has been told the reserved loopback port.
2. Preferred form, later: an SSH reverse forward. The device connects out. On the VPS the forward is `127.0.0.1:<reserved-port>` to the compute port on the device (`127.0.0.1` there). The credential stays in the existing admin key box. It is not written into the registry.
3. Also acceptable, still outbound only: a WebSocket dial-out from the device to the VPS. Same rule. The VPS does not connect inbound to the LAN.
4. The forward, when it exists, targets the compute port the owner named (ollama, llama.cpp, exo, or a future omnode API). It does not target a listener created by this ticket. Port 7787 stays empty.
5. Reject or revoke: the device drops the dial, the VPS drops the loopback forward, `endpoint` is cleared, the schema row is deleted, `approval = rejected`.
6. A flap leaves `approval = approved` and leaves the row. The device may redial the same reserved port. Until it does, health fails and B38 skips the row.
7. No hostname is chosen here. Bringing up a public name would be a DNS change, which this ticket forbids.

## 9. Flow

1. A LAN device may announce `_openmind._tcp` or just run ollama, llama.cpp, or exo. Announcing is not joining.
2. A node on that LAN browses. Passive is the default. The active scan flag stays off unless the owner passes it. The result is a candidate list. `endpoint` is empty. There is no registry record.
3. The owner offers one candidate. The registry gains `id`, `kind = local`, `endpoint = ""`, `approval = pending`. No schema row. No tunnel.
4. The console shows the fingerprint, or the service and `lan_hint`. It calls nothing.
5. Slavik rejects. `approval = rejected`. No row. No tunnel. Stop.
6. Slavik approves, and a loopback port is free. In one step: `approval = approved`, `endpoint` becomes that loopback `base_url`, and one `Provider` row is inserted (`id`, `kind`, `base_url`, `key_slot`, `default_model`). If the port cannot be reserved, stay `pending` and write no row.
7. The device dials out to that endpoint. The desk calls `127.0.0.1` only. Adding the device was a new row, not a new program.
8. Revoke sets `rejected`, deletes the row, and drops the tunnel.

```text
LAN device          LAN node (browse)         Slavik            VPS desk
    |                    |                       |                  |
    | announce / serve   |                       |                  |
    |------------------->| candidates only       |                  |
    |                    | offer                 |                  |
    |                    |---------------------->| pending, no row  |
    |                    |                       | reject -> stop   |
    |                    |                       | approve          |
    |                    |                       |----------------->| row + loopback endpoint
    | dial out (after approved)                   |                  |
    |-------------------------------------------------------------->| 127.0.0.1 only
```

## 10. Invariants

- No device joins without Slavik's explicit approval. Join = a `Provider` row.
- The VPS never dials the LAN. The device dials out, and only after approval.
- `endpoint` and `Provider.base_url` are the same string, and that string is a loopback URL.
- `kind` for a LAN join is `local`. `remote` is not used for mDNS finds.
- `approval` is not stored inside the schema struct. The row is present if and only if `approval` is `approved`.
- No secret in the row, the registry, the TXT record, or this page.
- No listener, no DNS change, and no code in this ticket.
- Timestamps are Europe/Zagreb RFC 3339 with an explicit offset (`+02:00` until 2026-10-25 03:00 local, then `+01:00`). Never `Z`.
