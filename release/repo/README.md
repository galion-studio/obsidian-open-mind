# Galion OS, starting with Obsidian Open Mind

**Galion OS** is the full Galion operating system experience: one window, one gradient,
ultra-minimal, with Hermes at the centre. Obsidian Open Mind is its heart and its interface,
built in Rust so it stays light and lightning fast, and made to run on any processor: Linux,
Mac, Windows, Android, iPhone and ARM chips such as Snapdragon. Galion OS is where we are
heading, not a finished operating system you can install today; it starts here, with Open Mind
and Hermes. Open source under the MIT License: free for everybody to download and do anything
with.

Obsidian Open Mind ("Open Mind") is the stance and the code of the Obsidian stack by
Galion Studio: a lightweight, CPU-light Rust foundation where the CPU box owns the loop, the
memory and the keys. A GPU is not a special case in this design; it is one more `Provider`
row. License: MIT.

## Why a CPU-light Rust foundation

The processor runs the loop. The card only runs the model. That sentence is the whole
architecture ([`crates/openmind-schema/src/lib.rs`](crates/openmind-schema/src/lib.rs),
workspace root). Concretely:

- **The CPU box owns the loop, the memory and the keys.** `Machine::weights()` turns local
  weights on only when a card has at least 8 GiB free *and* the box has at least 8 GiB of
  memory; `Machine::mouth()` needs memory only, never a card. A CPU-only box is a
  first-class citizen, not a degraded one.
- **Adding a provider is a new row, not a new program.** `Provider { id, kind, base_url,
  key_slot, default_model }` covers a laptop Ollama and a remote site with the same shape.
  `ProviderKind` is `Local` or `Remote`. No per-backend code path.
- **Secrets never live in the schema.** A `KeySlot` records only `name`, `present` and a
  `hint`. The contract types contain no key material, and a test asserts the serialized
  stack never contains the string `sk-`.
- **Rust, std-first, small.** The workspace is a Cargo workspace with a release profile
  tuned for footprint (`opt-level = "z"`, `lto = true`, `codegen-units = 1`,
  `panic = "abort"`, `strip = true`), not for throughput. Measured release binaries at
  2026-10-18T19:14:44+02:00 (`ls -l target/release/`): `galion-one` 615624 B, `galion-desk`
  402720 B, `room` 344304 B, `desk-hermes` 1427472 B. `link-watch` had no binary then. The
  workspace is still being assembled — re-measure before quoting these anywhere.

Open Mind is **designed so that** accelerators from NVIDIA, AMD or Apple can be added the same
way as anything else: as provider rows that expose an OpenAI-compatible HTTP endpoint
(`A-foundation.md` — a design statement in the foundation study, not code). No accelerator
provider row exists in the workspace yet: **TODO(verify)**. No accelerator is required to run
the foundation.

## What is in the workspace today

`ls -1 crates/` read at **2026-10-18T19:14:44+02:00** (Europe/Zagreb) lists seven crates, and
`ls -l target/release/` in the same minute shows which of them have been built. The workspace
is being assembled by another worker while this kit is written, so the list is dated, not
permanent. "State" below means "what the build directory said at 19:16:54+02:00". Where this
table and the directory disagree, the directory is the newer fact. The description of each
crate is traceable to the file named in the last column; the exact line numbers live in
`FACTS.md`, because they move while the workspace is edited.

| Crate | State at 19:16:54 | What it is | Description sourced from |
|---|---|---|---|
| `openmind-schema` | built, v0.1.0 (`libopenmind_schema.rlib`) | The stack contract: `Machine`, `Provider`, `ProviderKind`, `KeySlot`, `Surface`, `Stack`, plus `Stack::cpu_desk()`. Re-exports `zagreb-time` as `openmind_schema::time`. | `crates/openmind-schema/Cargo.toml`, `src/lib.rs` |
| `zagreb-time` | built (`libzagreb_time.rlib`) | Europe/Zagreb time: RFC 3339 with an explicit offset, never `Z`. The timezone database is compiled in, so it works offline. | `crates/zagreb-time/Cargo.toml` |
| `galion-one` | built | Galion One — the box's own voice surface. | `crates/galion-one/Cargo.toml` |
| `galion-desk` | built | The operator's desk endpoint. Reads the pod, never writes it. | `crates/galion-desk/Cargo.toml` |
| `room` | built | A read-only view of the operator's files as one page. | `crates/room/Cargo.toml` |
| `desk-hermes` | built | Prints one compact JSON object with the same keys in the same order, and opens the Hermes state database read-only. | `crates/desk-hermes/Cargo.toml`, `src/main.rs` |
| `link-watch` | **`Cargo.toml` only; `src/` is empty; no binary** | Link watcher port. **TODO(verify)** before release. | `crates/link-watch/Cargo.toml` |

`galion-one`, `galion-desk` and `desk-hermes` are ports of programs that still exist elsewhere
on the box: behaviour-preserving copies, not rewrites — and two of them still spawn a Python
helper (see **Status / known gaps**).

Planned for the workspace, not present yet — named in the workspace `README.md` and the
foundation study, with no code in the tree:

- `openmind-http` — the std thread-per-connection HTTP/1 server and client, extracted from
  Galion One. **Planned.**
- `openmind-state` — typed state files and read-only SQLite access. **Planned.**

## Quick start

```sh
git clone <TODO-repo-url> openmind
cd openmind
nice -n 19 cargo build -j 2 --release --workspace
nice -n 19 cargo test -j 2 --workspace
```

`<TODO-repo-url>` is a placeholder: the repository does not exist yet and nothing has been
pushed. Builds run with `nice` and `-j 2` on purpose — the box is shared.

## Requirements

- A Rust toolchain with Cargo. The workspace does not pin a toolchain version
  (`rust-version` is not set in `Cargo.toml`), and there is no `rust-toolchain.toml`.
  **TODO(verify)** the minimum supported Rust version before release.
- The reference toolchain used to develop this workspace is rustc 1.99.0.
- No Python, no Node and no system services are required to **build or test** the workspace.

## Status / known gaps

The foundation is a Rust workspace. It is being ported from programs that still shell out to a
Python helper, so read "Rust foundation" as the shape of the foundation, not as a claim about
every process on the machine. The known gaps, all of them measured 2026-10-18 with
`TZ=Europe/Zagreb date -Iseconds` and all traceable in `FACTS.md`:

- **Python helper call sites.** The Rust binaries of `galion-one` and `galion-desk` still spawn
  a Python helper at runtime. `find crates -name '*.py'` finds nothing, so no crate holds a
  Python file; the removal is **TODO(verify)**.
- **The Hermes agent and the Node services are not part of this foundation.** Hermes is
  upstream software written in Python, and the Node services run alongside it on the same box.
  They are named here so that no reader takes "Rust foundation" to mean the whole machine
  (`A-foundation.md`, section 1).
- **No accelerator provider row exists.** The accelerator path is a design statement, not code.
- **`link-watch` has no source yet**, and `openmind-http` and `openmind-state` are planned only.
- **The repository is not published.** There is no remote, no push and no site deployment yet;
  those steps wait for the owner's approval.
- Milestone: **2026-10-31** — published, everything live, fact-checked and timestamped.
- The license holder line in the workspace root `LICENSE` is currently
  `Copyright (c) 2026 Slavik / Galion Studio`; see `FACTS.md` for the open question about
  matching it against the release copy.

## Timestamps

Every timestamp in this project is Europe/Zagreb time with an explicit UTC offset, never
server time and never `Z`:

- `2026-10-18T19:03:04+02:00` while CEST is in force (until 2026-10-25 03:00 local).
- `...T...+01:00` from 2026-10-25 03:00 local onward (CET).

Shell form: `TZ=Europe/Zagreb date -Iseconds`.

## License

MIT. See [LICENSE](LICENSE).

## Contributing and security

- [CONTRIBUTING.md](CONTRIBUTING.md) — build, test, style and sign-off rules.
- [SECURITY.md](SECURITY.md) — how to report a problem privately, and what never goes into
  an issue.
- [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) — Contributor Covenant 2.1.
- [RELEASE-CHECKLIST.md](RELEASE-CHECKLIST.md) — what must be true before 2026-10-31.
