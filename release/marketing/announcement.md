# Obsidian Open Mind — launch announcement (draft)

**STATUS: DRAFT — NOT POSTED.** Target date: **2026-10-31**.
Drafted 2026-10-18T19:05:52+02:00; Status paragraph added and body de-numbered
2026-10-18T19:25:00+02:00 (Europe/Zagreb).
Every factual sentence carries a footnote id; each id maps to a source in `FACTS.md`
and is listed in `FACT-CHECK.md`.

---

Obsidian Open Mind ("Open Mind") is the stance and the code of the Obsidian stack by
Galion Studio.[^1] It is a lightweight, CPU-light Rust foundation where the CPU box owns the
loop, the memory and the keys.[^2] The whole architecture fits in one sentence: the processor
runs this, and the card only runs the model.[^3]

That stance has a practical consequence: a graphics card is not a special case. It is one
more `Provider` row.[^4] The same shape describes a laptop model server and a remote API site,
so adding a provider is a new row, not a new program.[^5] A provider is an adapter, not a
secret.[^6] Providers are either local or remote.[^7]

Secrets never live in the contract. A `KeySlot` records only a name, whether it is present,
and a hint; the types carry no key material.[^8]

The foundation is small on purpose. The workspace release profile is tuned for footprint, not
throughput.[^9] Release binaries measured at 2026-10-18T19:16:54+02:00: `galion-one` 615624 B,
`galion-desk` 402720 B, `room` 344304 B, `desk-hermes` 1427472 B.[^16] Open Mind is designed so
that accelerators from NVIDIA, AMD or Apple **can** be added the same way as any other provider:
as a row that exposes an OpenAI-compatible HTTP endpoint.[^10]

The project is released under the MIT license.[^11] Every timestamp in the project is
Europe/Zagreb time with an explicit offset.[^12]

## Status

Read "Rust foundation" as the shape of the foundation, not as a claim about every process on
the machine.[^2] Two Rust binaries, `galion-one` and `galion-desk`, still spawn a Python helper
at runtime, and removing those calls is **TODO(verify)**.[^17] The Hermes agent (upstream,
Python) and the Node services on the box are not part of this foundation.[^18] No accelerator
provider row exists in the workspace yet; that path is a design statement, not code.[^10][^17]

The milestone is 2026-10-31, when the stack is published and fact-checked.[^13] The repository
is not published yet: there is no remote and nothing has been pushed.[^14] This announcement
itself is a draft and has not been posted.[^15]

---

## Footnotes

[^1]: `FACTS.md` "The Open Mind stance" — stack `Obsidian`, stance `Open Mind`, studio `Galion Studio`: `crates/openmind-schema/src/lib.rs:89-91` (re-read 2026-10-18T19:17:58+02:00).
[^2]: `crates/openmind-schema/src/lib.rs:3` for the stance line. Qualified by the Python evidence: `rg -n -i 'python' crates/` hits `crates/galion-one/src/main.rs:1136,1238,1830` and `crates/galion-desk/src/main.rs:236`; `find crates -name '*.py'` finds nothing; read 2026-10-18T19:14:44+02:00.
[^3]: `crates/openmind-schema/src/lib.rs:4`.
[^4]: `crates/openmind-schema/src/lib.rs:42`; `Provider` row shape at `crates/openmind-schema/src/lib.rs:51-58`.
[^5]: `crates/openmind-schema/src/lib.rs:49`.
[^6]: `crates/openmind-schema/src/lib.rs:4`.
[^7]: `crates/openmind-schema/src/lib.rs:42-47` (`ProviderKind` is `Remote` or `Local`).
[^8]: `crates/openmind-schema/src/lib.rs:62-66`; the crate test asserts no `sk-` in a serialised stack (`:151`).
[^9]: `/opt/galion/openmind/Cargo.toml:19-28` (release profile `opt-level = "z"`, `lto`, `codegen-units = 1`, `panic = "abort"`, `strip`).
[^10]: `contrib/grokbot/docs/study/A-foundation.md:76` — a design statement in the foundation study, not code.
[^11]: `/opt/galion/openmind/Cargo.toml:10`; `contrib/grokbot/docs/main.md:26`; `/opt/galion/openmind/LICENSE:1`.
[^12]: `contrib/grokbot/docs/main.md:52-55`; `crates/zagreb-time/src/lib.rs:29-37`.
[^13]: `contrib/grokbot/docs/main.md:48`.
[^14]: `git remote -v` in `/opt/galion/openmind` empty (read 2026-10-18T19:05 CEST); `release/repo/README.md`.
[^15]: ticket/status, not a factual claim about the project; recorded in `FACT-CHECK.md`.
[^16]: `ls -l /opt/galion/openmind/target/release/` read 2026-10-18T19:16:54+02:00 (Europe/Zagreb). `link-watch` had no binary then.
[^17]: No accelerator `Provider` row exists in the workspace. What does exist is hardware probing, not a provider: `crates/galion-one/src/main.rs:3219` runs `nvidia-smi`, and a mock fixture at `:1447` names `"device":"cuda"`. The contract itself has only `ProviderKind::Remote` and `ProviderKind::Local` (`crates/openmind-schema/src/lib.rs:42-47`), and `Stack::cpu_desk()` builds one `Local` row (`:86-132`). Read 2026-10-18T19:18:14+02:00.
[^18]: `contrib/grokbot/docs/study/A-foundation.md` section 1 (what is and is not part of the foundation).
