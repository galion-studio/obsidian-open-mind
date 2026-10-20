<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Clock

- DESK-ID: openmind.CLOCK-DOC
- Written: 2026-10-19T21:06:22+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Source: crates/zagreb-time/src/lib.rs only. The crate was not edited.

The stamp offset is numeric. It is not a trailing UTC designator.

## Source

`require_rfc3339` in crates/zagreb-time/src/lib.rs refuses a timestamp that ends in Z or z. The refusal is StampReject::Zulu. The function comment states the rule in those words: a timestamp ending in Z is rejected.

`now_rfc3339` returns the current instant through `rfc3339`. That helper formats the instant in Tz::Europe__Zagreb with to_rfc3339_opts(SecondsFormat::Secs, false). The false flag keeps an explicit numeric offset. The tests now_is_zagreb_rfc3339_seconds_with_explicit_offset and timestamp_ending_in_z_is_rejected_and_now_uses_offset both require the same ending: now_rfc3339 ends in +01:00 or +02:00.

Accepted stamps are seconds precision. Summer is +02:00. Winter is +01:00.

## Not done

This note only records the crate source. crates/zagreb-time was not edited. The host clock was not set. Nothing was committed. Nothing was pushed. cargo was not run. systemctl was not used.
