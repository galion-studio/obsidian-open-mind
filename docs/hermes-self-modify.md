<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Hermes self-modify through the sandbox gate

Hermes may change its own code to adapt to the user, but only along one path:
Hermes writes, the sandbox checks and signs, Slavik approves, a person deploys.

| Step | Who | Key | Code |
|---|---|---|---|
| propose | Hermes (`propose_self_change` tool) | public keys only | `self_modify::SelfModifyTool` |
| gate | gate user (`hermes-self-modify gate`) | gate signing key | `self_modify::gate_entry` |
| approve | Slavik (`hermes-self-modify approve`) | owner key (never on the server) | `self_modify::approve` |
| deploy / rollback | root, by a person | none (verifies both) | `self_modify::deploy`, `rollback` |

1. **propose**: the tool writes the files (full new content) into a fresh git
   worktree under `<queue>/<id>/worktree` (never the running source), makes
   `change.patch` + `proposal.json`, and runs the deny-list checks for quick
   feedback. If `HERMES_RS_SELF_MODIFY_GATE_CMD` is set it then calls the gate.
2. **gate**: re-checks the patch, applies it to the base in the gate's OWN
   clone, runs the plan's stages (build, tests, clippy, deny-list tests; any
   wrapper such as a user+net namespace), copies the built binary and signs
   `gate/record.json` (pass or fail). A failing stage is a signed FAIL.
3. **pending**: a signed PASS waits; `status`/`list` show "pending Slavik's approval".
4. **approve**: signs `approval.json` bound to the proposal, the record and
   the binary's sha256, with an expiry. Refused for "hermes", for the gate
   key, for a FAIL, and inside a Hermes environment.
5. **deploy**: verifies both signatures and every sha again, installs
   `hermes-gateway-<id>-<sha12>` (create-new, `.sha256` sidecar), saves the
   current drop-in to `<entry>/rollback/` and writes the new drop-in. It does
   not reload or restart; that stays a manual step.
6. **rollback**: puts the saved drop-in back (or moves ours aside). Nothing is deleted.

Never changeable this way: SOUL.md, config.yaml, Cargo.toml/Cargo.lock,
build scripts, unit/conf/env/key files, the gate and sandbox crates, the deny
list, the guards, and this self-modify module, CLI and tests. Only
`crates/hermes-rs`, `crates/hermes-gateway`, `crates/hermes-tools` and `docs/`.
No deletes, no binaries, only 644 files.

Hermes cannot sign or approve: the tool has no seed, refuses
approve/sign/deploy/rollback actions, and refuses to start if its process can
read the gate signing key (`HERMES_RS_SELF_MODIFY_SANDBOX_KEY`) or if the gate
and owner public keys are equal. Records signed by any other key do not verify.

Env (tool off unless REPO is set): `HERMES_RS_SELF_MODIFY_REPO`, `_QUEUE`,
`_BASE` (default HEAD), `_SANDBOX_PUB`, `_OWNER_PUB` (files with hex keys),
`_SANDBOX_KEY` (path Hermes must NOT be able to read), `_GATE_CMD` (optional).
When hermes-rs runs as root: `_RUN_AS=uid:gid` + `_CLI` make the propose work run as that
unprivileged user via `hermes-self-modify propose`; the key check is done as that user too.
No `_OWNER_PUB` file yet = nothing can be approved.

Tests: `cargo test -p hermes-rs --test self_modify` (fake Hermes repo, real
cargo build/test/clippy in the gate copy, real CLI as the gate runner).
