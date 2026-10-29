# Hermes deny list (hardened 2026-10-29)

Code: `crates/hermes-rs/src/denylist/` (`mod.rs` checks, `rules.rs` tables,
`shell.rs` lexer, `ere.rs` dependency-free regex engine, `prior/` the prior
list verbatim). Tests: `crates/hermes-rs/tests/denylist_bypass.rs` with the
payload corpus in `crates/hermes-rs/tests/denylist/`.

## Where it runs

`Registry::add` wraps every tool in `Guarded`. The boss agent, delegated
children (their registry is the parent's, minus `delegate_task`), cron runs,
MCP tools and the coding tools (`Registry::add_in_workspace`, resolved in the
coding worktree) all go through `denylist::check_tool_call` before the tool
runs. A refusal returns `blocked by the Hermes deny list (<rule>): <reason>`
and logs one line (`hermes-rs denylist: refused tool=.. rule=..`). It fails
closed and has no off switch (the prior `guard.json {"on": false}` switch and
the prior plugin's fail-open on exceptions are gone).

## Layers

| Layer | What it does |
|---|---|
| P | Prior list (55 Galion rules + 2 Ondrej rules) and prior 12-rule floor, verbatim, on the raw text, the prior normalisation and the de-quoted command with the command name reduced to its basename. Secret/guard floor rules (6, 6b, 8) run on every string of every tool. |
| S1 | Splits on `; & && \|\| \| \|&` and newlines and checks every part. Refuses subshells, `$( )`, backticks, `<( )`, here-docs, `if/for/while/case/function`, `{ }`. |
| S2 | Removes quotes and escapes like the shell. Refuses invisible / look-alike unicode, globs in command names, globs in absolute, `~` or `..` paths and in write targets. |
| S3 | Refuses decoders (base64, base32, xxd ...), `printf` with escapes, `echo -e`, `$'..'`. Decodes embedded base64 / hex / `\x` text and checks it again. |
| S4 | Refuses interpreters, shells and wrappers (`env`, `nice`, `timeout`, `xargs`, `busybox`, `sudo` ...), running files by path, `find -exec/-delete`, unsafe `sed`, tar/zip program hooks, `rg --pre`, `sort --compress-program`. |
| S5 | `NAME=value` prefixes only from a short allow list; refuses `$VAR` / `${}` expansion, env dumpers (`env`, `printenv`, `set`, `jq env`), `/proc/*/environ`, and tool `env` objects with other keys. |
| S6 | Resolves every path (`~`, `..`, `//`, relative to the tracked cwd, symlinks on the longest existing prefix). Refuses secrets for reads; writes only inside the workspace and never to agent / hook / guard files; `/etc` and `/boot` on the resolved path; `/proc/self/root`, `/dev/fd`, `/dev/tcp` aliases; `cd` and tool `cwd` outside the workspace; `rm` of the workspace root or above. |
| S7 | Refuses network clients (curl/wget only with `HERMES_RS_GUARD_NET_ALLOW`, never uploads), loopback / private / metadata hosts and non-http(s) URL schemes in any tool. |
| S8 | Refuses service, process, package, mount, firewall and clock changes; `git` only with listed subcommands and no `-c`, `--exec-path`, `-C` outside the workspace; `cargo` only listed subcommands with `--offline`/`--frozen`. |
| S9 | Refuses code-execution tools, shell tools with no inspectable command, vault writes through any tool (name with vault/note + write verb, or a path inside a vault root), delegated tasks that ask to write to the vault or a protected path, oversize or deeply nested arguments. |

## Settings (environment)

- `HERMES_RS_GUARD_WORKSPACE` (else `HERMES_RS_CODING_WORKTREE`): the cwd and only writable root. Unset = nothing is writable.
- `HERMES_RS_GUARD_MODE=allowlist`: default-deny terminal (only the research command words in `RESEARCH_ALLOW`).
- `HERMES_RS_GUARD_NET_ALLOW=host,host`: hosts curl/wget may fetch from.
- `HERMES_RS_GUARD_VAULTS=/a:/b`: extra read-only vault roots.

## Known limits

- A `build.rs` or test written into the workspace runs when `cargo test` runs; that is left to the coding sandbox (seccomp / namespaces / caps).
- Web tools may still send non-secret data to public hosts; DNS names that resolve to loopback are not resolved by the guard.
- tar/unzip extraction is limited to the workspace, but archive members are not inspected.
