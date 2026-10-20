<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 3000 Galion / galion-studio -->

# Sandbox policy presets

- DESK-ID: openmind.B31
- Written: 2026-10-19T16:08:48+02:00 (Europe/Zagreb, `TZ=Europe/Zagreb date -Iseconds`)
- Repo: /opt/galion/openmind
- Files: `deploy/sandbox/presets/{read-only-tool,build-tool,net-tool}.toml`
- Crate: `crates/openmind-sandbox` was not modified

## What a preset is

Each file is one `SandboxPolicy` for a later runner. It is data. It has no program, no arguments, and no interpreter. Reading the file does not start a process, write a unit, or change the host.

The caller does four things:

1. Read the TOML (schema 1, named in the comment header).
2. Replace every `{workdir}` with the absolute path of a directory that already exists. The literal `{workdir}` is not a directory, so `run` refuses it.
3. Build `SandboxPolicy` with the map below.
4. Pass an argv the operator chose. The preset never supplies one.

`crates/openmind-sandbox` has no TOML parser. This ticket does not add one and does not add a test. The map below is the contract a later parser should follow.

## Field map

Root keys are the policy. `name` and `schema` live in comments so a strict decode of the table does not see extra keys.

| TOML key | `SandboxPolicy` field | Rule |
| --- | --- | --- |
| `workdir` | `workdir` | String path after `{workdir}` substitution. Must already be a directory. |
| `env_allowlist` | `env_allowlist` | Names copied from the parent environment. Values are not stored in the file. |
| `env_set` | `env_set` | Array of `[name, value]` pairs, in order. Applied after the allowlist, so they override. Same shape as `Vec<(String, String)>`. |
| `wall_timeout_secs` | `wall_timeout` | Seconds. `Duration::from_secs`. |
| `cpu_secs` | `cpu_secs` | `RLIMIT_CPU` seconds. |
| `mem_bytes` | `mem_bytes` | `RLIMIT_AS` bytes. |
| `nofile` | `nofile` | `RLIMIT_NOFILE`. |
| `fsize_bytes` | `fsize_bytes` | `RLIMIT_FSIZE` bytes. Zero would leave the limit unchanged. No preset uses zero. |
| `output_cap` | `output_cap` | Bytes kept per stream. The rest is drained and dropped. |
| `net` | `net` | `false`: empty network namespace, or Landlock TCP deny when that is all the kernel has. `true`: network stays open and the outcome reports it as not enforced. |
| `require_net_isolation` | `require_net_isolation` | Matters only when `net` is false. The run fails closed if neither a network namespace nor Landlock TCP deny was applied. |
| `landlock` | `landlock` | Try Landlock. If the kernel has none, the crate reports that and still runs. |
| `fs_read` | `fs_read` | Read roots. The crate also adds the workdir. |
| `fs_write` | `fs_write` | Extra write roots. Empty in every preset. The crate still keeps the workdir writable. |
| `namespaces` | `namespaces` | Try an unprivileged user namespace, plus pid, mount, and (when `net` is false) an empty net namespace. |
| `ro_root` | `ro_root` | In a mount namespace, the tree is read-only except the workdir and `fs_write`. |
| `hide_paths` | `hide_paths` | Directories covered by an empty read-only tmpfs. Missing paths are skipped. A path that contains the workdir, a write root, a read root, or the program is kept visible and reported. |

`fs_read` matches `SandboxPolicy::new`: `/usr`, `/bin`, `/sbin`, `/lib`, `/lib64`, `/etc`, `/proc`, `/dev/zero`, `/dev/urandom`, `/dev/random`.

The runner should also append the real `HOME` to `hide_paths` when that directory exists, is not the workdir, and is not already under `/root` or `/home`. The file cannot know `HOME`.

## Shared rules

- No argv key and no command string. A preset cannot mutate the system because it cannot run anything.
- `fs_write` is empty. Host paths are not opened for writes.
- `ro_root` and `landlock` and `namespaces` are on. Ask for the closed tree even when this kernel can only apply part of it.
- `hide_paths` is `/root`, `/home`, and `/run/secrets`.
- `HOME` and `TMPDIR` are set to the workdir so tools do not use the real home or the host `/tmp`.
- Allowlists are names only. Nothing in these files is a credential, a key, or a proxy URL.
- These files are not referenced by a service unit. Nothing was installed or enabled.

"Read-only" has one limit in the current crate: the workdir stays writable, and `fs_write` cannot name a negative path. `fsize_bytes` caps a single file. It does not make the workdir immutable. Changing that would mean editing the crate, which this ticket does not do.

On a host where the mount namespace or Landlock did not apply, writes outside the workdir are not blocked. The outcome says so. `require_net_isolation = true` is the fail-closed case, and it covers network only.

## read-only-tool

File: `deploy/sandbox/presets/read-only-tool.toml`.

For a short inspection tool: list, read, or print inside the closed tree. Network is denied and must actually be isolated, or the run fails. Limits are tight: 15 s wall, 5 CPU seconds, 256 MiB address space, 64 open files, 1 MiB file size, 64 KiB of captured output per stream.

Environment copied from the parent: `PATH`, `LANG`, `LC_ALL`, `TZ`. Nothing else. `HOME` and `TMPDIR` point at the workdir.

Use it when the tool should not fetch and should not leave a large artifact. Do not point `fs_read` at a home directory to "make the tool work". That undoes `hide_paths`.

## build-tool

File: `deploy/sandbox/presets/build-tool.toml`.

For a compiler or similar tool whose outputs stay inside the workdir. Network is denied and must be isolated, so the run cannot fetch dependencies. Caches and vendored sources have to already sit inside the workdir (or on the `fs_read` list, which this file does not extend past the system paths).

Limits: 600 s wall, 300 CPU seconds, 1 GiB address space, 1024 open files, 512 MiB file size, 256 KiB captured output per stream. One GiB is a low-end cap. A caller who needs more changes the number for that run. The shipped file stays at 1 GiB.

Extra environment, still inside the workdir:

- `CARGO_HOME={workdir}/.cargo`
- `CARGO_TARGET_DIR={workdir}/target`

`TERM` is also copied when the parent has it. `RUSTUP_HOME` is not set and `/root` is hidden, so a toolchain that lives only under the real home is not visible. The caller may add that one toolchain directory to `fs_read` for a run. The preset does not.

`fs_write` stays empty on purpose. Object files belong under the workdir, which the crate already keeps writable. The preset does not name the compiler and does not start it.

## net-tool

File: `deploy/sandbox/presets/net-tool.toml`.

Same closed filesystem as `read-only-tool`, with the network left open. `net = true` does not take an empty network namespace and does not ask Landlock to deny TCP. There is no host allowlist field on `SandboxPolicy`, so this is not a filter of destinations. The operator picks the program. The preset still has no argv.

`require_net_isolation` is false because that flag only applies when `net` is false. Setting it true here would not close the network.

Limits: 30 s wall, 10 CPU seconds, 256 MiB, 64 open files, 32 MiB file size, 256 KiB captured output. The file-size cap is the download bound for a single file in the workdir.

Also copied when set: `SSL_CERT_FILE`, `SSL_CERT_DIR` (paths to certificate bundles, not bundles themselves). `/etc` stays on `fs_read` so resolver and certificate files remain readable. Proxy variables are not on the allowlist.

## What this host already measured

Ticket B26 recorded this LXC: user namespaces were usable, Landlock was not in the LSM list, cgroup v2 was not ours to edit. This ticket did not repeat those probes and did not spawn a sandboxed process. `landlock = true` still means "try". A missing Landlock layer is reported, not treated as success.

These presets are not wired into Hermes, omnode, or any unit under `deploy/`.
