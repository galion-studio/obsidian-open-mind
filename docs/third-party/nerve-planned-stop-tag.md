<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party record: swarm/nerve-planned-stop-tag (slot 310)

**No new Rust crates and no copied code.** `Cargo.lock` is unchanged. The
change uses only `std` and crates `openmind-nerve` already depends on
(`serde_json`).

Facts used (not code):

| fact | source | licence |
|---|---|---|
| journald MESSAGE_IDs: Stopping `de5b426a63be47a7b6ac3eaac82e2f6f`, process exit `98e322203f7a4ed290d09fe03c09fe15`, unit failed `d9b373ed55a64feb8242e02dbe79a49c`, Started `39f53479d3a045ac8e11786248231fbf` | systemd `catalog/systemd.catalog.in` and the real VPS journal (`journalctl -o json`, 2026-10-27) | LGPL-2.1-or-later (fact only, no code copied) |
| `INVOCATION_ID` is shared by the stop job, the exit and the result lines of one invocation; a new invocation gets a new ID at Started | `systemd.exec(5)` `$INVOCATION_ID`, checked on the VPS journal | fact only |

Fixtures `crates/openmind-nerve/tests/fixtures/planned-stop/*.jsonl` are real
VPS journal lines (no secrets; `_BOOT_ID`, `__CURSOR` and similar stripped).
