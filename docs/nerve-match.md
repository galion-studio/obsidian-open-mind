<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Nerve match

- DESK-ID: openmind.NERVE-MATCH
- Written: 2026-10-19T19:55:36+02:00 (Europe/Zagreb, TZ=Europe/Zagreb date -Iseconds)
- Repo: /opt/galion/openmind
- Boot: crates/openmind-boot to_nerve
- Schema: crates/openmind-schema NerveEvent

## Result

Match: yes.

ts, source, kind, severity, and data match. to_nerve emits those five JSON fields. NerveEvent accepts those five JSON fields. The names are the same and the order is the same.

| field | to_nerve emits | NerveEvent accepts | match |
| --- | --- | --- | --- |
| ts | string, the probe stamp | string, Zagreb RFC 3339 | yes |
| source | boot | string | yes |
| kind | hardware | string | yes |
| severity | info | string | yes |
| data | object with cpu, ram, and gpu | JSON value | yes |

## Boot emit

File: crates/openmind-boot/src/lib.rs. Function: to_nerve. Struct: NerveBody.

NerveBody is the only object to_nerve prints. The fields, in order, are ts, source, kind, severity, and data. None of the five is renamed. None of the five is skipped.

- ts is Hardware.probed_at
- source is the constant boot (NERVE_SOURCE)
- kind is the constant hardware (NERVE_KIND)
- severity is the constant info (NERVE_SEVERITY)
- data is one object: cpu, ram, and gpu

cpu has processors (number) and model (string). ram has total_mib and free_mib (numbers). gpu has status and count. Optional gpu fields are name, vendor, source, total_mib, used_mib, and free_mib. An optional gpu field is left out when it is empty. A missing GPU is status absent and count 0. Severity stays info.

to_nerve does not write a file. It does not create hardware.json. It does not create nerve.json.

The text is pretty JSON: two-space indent and a trailing newline. Key order follows the struct.

check_timestamp runs before the object is built. It allows a string of length at least 25 that ends in +01:00 or +02:00. It refuses a UTC designator and it refuses +00:00.

probe sets probed_at from zagreb_time::now_rfc3339. That stamp is Europe/Zagreb RFC 3339 at seconds precision, with an explicit numeric offset.

## Schema accept

File: crates/openmind-schema/src/lib.rs. Struct: NerveEvent.

NerveEvent fields, in order, are ts, source, kind, severity, and data. All five are required. None is optional. The struct has no deny_unknown_fields and no rename.

- ts is a string. deserialize_zagreb_ts keeps it only when is_zagreb_rfc3339 is true. That check wants length 25, the shape YYYY-MM-DDTHH:MM:SS, offset +01:00 or +02:00, digits in the date and time fields, month 1 through 12, day 1 through 31, hour 0 through 23, minute 0 through 59, and second 0 through 59. A UTC designator fails. +00:00 fails.
- source is a plain string. boot is accepted.
- kind is a plain string. hardware is accepted.
- severity is a plain string. info is accepted.
- data is serde_json::Value. An object is a JSON value, so cpu, ram, and gpu are accepted. The schema does not declare those inner keys.

## Why each field matches

ts matches. Both sides use the name ts. A zagreb-time stamp is 25 characters and ends in +01:00 or +02:00, so the schema checker accepts it. The boot golden stamp is 2026-10-19T16:45:00+02:00. The schema test nerve_event_parses_boot_hardware reads that same object.

source matches. Boot always emits boot. The schema stores a string and does not limit the token.

kind matches. Boot always emits hardware. The schema stores a string and does not limit the token.

severity matches. Boot always emits info. A missing GPU does not change it. The schema stores a string and does not limit the token.

data matches. Boot emits one JSON object. NerveEvent.data is a JSON value, and an object is a value. The schema test reads cpu.processors, cpu.model, ram.total_mib, ram.free_mib, gpu.status absent, and gpu.count 0 from that object.

## Note on the ts gate

The field name matches. The two checkers are not the same width.

Boot check_timestamp allows any string of length at least 25 that ends in +01:00 or +02:00 and does not contain a UTC designator or +00:00. It does not require the digit layout.

Schema is_zagreb_rfc3339 allows only the 25-character form.

probe and the golden use the strict form, so the object to_nerve emits on that path is accepted. A hand-built Hardware value can pass the boot gate with a longer stamp, or a stamp that is not digits, and NerveEvent will then refuse ts. That is a value-gate gap. It is not a different field name. This page does not change either checker.

## Left out of the nerve object

to_nerve does not emit worker_mib, cap, weights, or line. NerveEvent does not have those fields. probed_at is emitted only as ts.

data.gpu.source is the GPU probe origin (proc or sys) when a card is present. It is not the envelope field source. The envelope source stays boot.

## What was not done

Neither crate was edited. No hardware.json was written. No nerve.json was written. cargo was not run. No unit was started, stopped, or restarted. Nothing was committed. Nothing was pushed.
