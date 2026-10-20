<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Nerve align (not wired)

- DESK-ID: openmind.NERVE-ALIGN
- Written: 2026-10-19T18:11:54+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Boot: `crates/openmind-boot` (`to_nerve`)
- Schema: `crates/openmind-schema` (`NerveEvent`)

## They are not wired together

`openmind-boot` and `openmind-schema` are not wired together.

`crates/openmind-boot/Cargo.toml` depends on `serde`, `serde_json`, and `zagreb-time`. It does not depend on `openmind-schema`.

`crates/openmind-schema/Cargo.toml` depends on `openmind-config`, `serde`, `serde_json`, `toml`, and `zagreb-time`. It does not depend on `openmind-boot`.

`to_nerve` builds a private `NerveBody` and pretty-prints it. It does not construct `openmind_schema::NerveEvent`. `NerveEvent` is not referenced from `openmind-boot`. `to_nerve` is not referenced from `openmind-schema`. No other crate calls both. This file does not wire them. Neither crate was edited.

Both crates format time with `zagreb-time` on their own. Sharing that helper is not a wire between the nerve object and the nerve event.

## Envelope

`to_nerve` emits one JSON object. The keys, in order, are `ts`, `source`, `kind`, `severity`, `data`. `NerveEvent` has the same five names, in that order. Schema `data` is `serde_json::Value`. The schema does not name `cpu`, `ram`, or `gpu`.

Boot constants inside `to_nerve`:

- `source` is `boot`
- `kind` is `hardware`
- `severity` is `info`

A missing GPU stays `absent` on `data.gpu.status`. It is not an error and it does not change `severity`.

`ts` is `Hardware.probed_at`. Boot `check_timestamp` accepts a string of length at least 25 that ends in `+01:00` or `+02:00` and does not contain `Z` or `+00:00`. Schema `is_zagreb_rfc3339` accepts only length 25, seconds precision, digit fields, a calendar range, and those two offsets. A stamp from `zagreb-time` fits both. A longer string that only passes the boot check does not fit the schema.

## Field table

| field | boot value | schema name | match or gap |
| --- | --- | --- | --- |
| ts | `Hardware.probed_at` (Europe/Zagreb RFC 3339) | ts | gap: same name, boot check is looser than the schema |
| source | boot | source | match |
| kind | hardware | kind | match |
| severity | info | severity | match |
| data | object of cpu, ram, and gpu | data | gap: name matches, schema does not name the keys |
| data.cpu.processors | processors (u32) | none | gap |
| data.cpu.model | cpu_model (string, may be empty) | none | gap |
| data.ram.total_mib | ram_total_mib (u64) | none | gap |
| data.ram.free_mib | ram_free_mib (u64) | none | gap |
| data.gpu.status | absent or present | none | gap |
| data.gpu.count | count (u32, 0 when absent) | none | gap |
| data.gpu.name | optional string, omitted when none | none | gap |
| data.gpu.vendor | optional string, omitted when none | none | gap |
| data.gpu.source | optional proc or sys, omitted when none | none | gap |
| data.gpu.total_mib | optional u64, omitted when none | none | gap |
| data.gpu.used_mib | optional u64, omitted when none | none | gap |
| data.gpu.free_mib | optional u64, omitted when none | none | gap |

`data.gpu.source` is not the envelope `source`. The envelope `source` is always `boot`. The schema field named `source` is only the envelope. Schema `source`, `kind`, and `severity` are free strings. `boot`, `hardware`, and `info` are not an enum in the schema. They match because any string is accepted.

## Boot fields left out of the nerve object

`to_nerve` does not copy these `Hardware` fields: `worker_mib`, `cap`, `weights`, `line`. The schema nerve event does not name them either. `probed_at` is copied only as `ts`.

## Absent GPU object (boot golden)

```json
{
  "ts": "2026-10-19T16:45:00+02:00",
  "source": "boot",
  "kind": "hardware",
  "severity": "info",
  "data": {
    "cpu": {
      "processors": 4,
      "model": "Test CPU"
    },
    "ram": {
      "total_mib": 8192,
      "free_mib": 4096
    },
    "gpu": {
      "status": "absent",
      "count": 0
    }
  }
}
```

That `ts` is 25 characters and would pass `is_zagreb_rfc3339`. The schema still has no type for `data.cpu`, `data.ram`, or `data.gpu`.

## What was not done

Neither crate was edited. No dependency was added. No caller was added. They are not wired together.
