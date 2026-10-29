<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes teaching: sandbox gate first

Written 2026-10-29 (+01:00) Europe/Zagreb on `swarm/hermes-teaching-sandbox-first`.
The pre-change backup is on `swarm/hermes-teaching-backup-20261029`, under `backups/hermes-teaching/`.

## What changed

Two notes taught Hermes the old way (write notes into the vault, a change is "deployed" right
after testing). They now say: **sandbox gate first, Hermes never changes live code.**

1. `SOUL.md`, section `## Vault`:
   - The vault path is now `/opt/galion/galion-light/docs/build-session`.
   - The vault is read-only, read through the `vault_*` tools of the Rust `openmind` MCP server.
   - Durable memory goes through `memory_save` and is found again with `memory_search`,
     `memory_recall` or `memory_conversations`.
   - New section `## Changes`: Hermes never changes live code, units, drop-ins, configs or
     binaries. Every change is built on a `swarm/*` branch, tested and passed through the
     sandbox gate before it touches live. Hermes proposes and waits.
2. `skills/galion/cortex-escalation/SKILL.md`, step 4: the deploy step now reads "passed
   through the sandbox gate (seccomp, non-root, no network), and only then deployed".

In the copies here, public IP addresses are `<IP>` and email addresses are `<email>`. The live
file keeps its original lines. Only the two sections above are swapped in place.

## Note: what hermes-rs loads

`hermes-rs` (crates/hermes-rs/src/persona.rs) reads only `SOUL.md` from `HERMES_HOME`
(and `channels/<c>.md`). It does **not** load `skills/`. The skills folder was used by the old
Python hermes-agent. The cortex-escalation edit is still made, so the note is not wrong when
someone reads it.

## Python in skills: port status

Not ported in this change. Each one calls a Python program that lives outside the skill, or is
bundled upstream with the Python agent. Porting means a new Rust crate per tool. Listed so the
work can be planned:

| Skill | Python it runs | Port path |
|---|---|---|
| galion/alpha, galion/platform | `galion-light/loop/loop.py` | Rust loop runner (new crate) |
| galion/mesh | `galion-light/mesh/mesh.py` | Rust mesh claim/act/verify (new crate) |
| galion/grade | `galion-light/grade/grade.py` | Rust grader (new crate) |
| galion/mark | `galion-light/one/trace.py` | Rust image trace (needs an image crate) |
| galion/one-voice | `galion-light/one/talk.py` | Part of the console voice path; port with the console |
| galion/eyes, web/playwright-see | `vision/look.py` (venv) | Depends on a Python vision stack; not feasible now |
| galion/tv | `scripts/vidaa-bridge.py` (runs at home, not on the VPS) | Out of scope for the VPS |
| research/arxiv | `scripts/search_arxiv.py`, `python -c` XML parsing | Feasible: curl + a small Rust Atom parser |
| research/grounded-citations | `scripts/sources.py` (stdlib only) | Feasible: small Rust CLI |
| web/blocked-page-recovery | `scripts/recover_page.py` | Feasible: Rust fetch + readability |
| software-development/github | `scripts/git-credential-token.py` | Use `gh auth git-credential` instead; no port needed |

## Deploy (sandbox first)

1. Mock load check in the sandbox:
   - Run `hermes-gateway --mock` with `HERMES_HOME` set to a copy that contains the new
     `SOUL.md`.
   - Run it as a transient systemd unit: user nobody, `PrivateNetwork=yes`, seccomp
     `@system-service`, `ProtectSystem=strict`, with `/opt/galion` and `/root` made
     inaccessible.
   - Expect a 200 on `/health` and a mock reply.
2. Live:
   - Back up `SOUL.md` and the cortex-escalation `SKILL.md`.
   - Swap in the two sections (`cat` into place, so the file mode is kept).
   - Run `systemctl restart hermes-rs` and check `/health`.
3. Rollback: `cat` the backups back into place, then run `systemctl restart hermes-rs`.
