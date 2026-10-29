# hermes-gate: Hermes output goes through the sandbox before anything live

Crate `crates/hermes-gate` (MIT). v2 on branch `swarm/hermes-sandbox-gate-v2`
(based on main 7ba85b60); the first prototype stays on `swarm/hermes-sandbox-gate`
(e2b68411). Nothing here pushes, deploys, commits, or touches a live root.

## CLI

```
hermes-gate keygen  SEED_OUT PUB_OUT
hermes-gate propose --id ID --summary S [--tier T] --out P.json (--worktree W | --base-commit SHA --patch FILE)
hermes-gate gate    --repo R --scratch D --proposal P.json --plan PLAN.json --key SEED --out REC.json
                    [--allow-tree crates/,docs/] [--allow net,spawn,unsafe]
hermes-gate verify  --record REC.json --sandbox-pub HEX|FILE --proposal P.json
hermes-gate approve --owner-key SEED --record REC.json --proposal P.json --target-head SHA [--ttl-s N] [--label TEXT] --out A.json
hermes-gate apply   --source R --staging-root D [--ledger F] --record REC.json --sandbox-pub HEX|FILE
                    --owner-pub HEX|FILE --proposal P.json --approval A.json
```

## Flow

1. **Propose**: a `Proposal` is a unified diff + its base commit + a sha256 over the patch
   (Hermes writes text only; `--patch` takes the diff as extracted from a chat reply).
2. **Gate: 7 static checks** (`checks.rs`), all must pass or nothing is built:
   `applies_to_base` (`git apply --cached --check` against the base tree in a temp index),
   `allowed_tree` (paths under `--allow-tree`, default `crates/`, `docs/`; no absolute,
   `..`, `.git`), `protected_targets` (CI, deploy, scripts, unit/conf files, Cargo.toml/lock,
   the gate, the sandbox, hades-gate, galion-guard/-deploy, one-defence, repo-check, key and
   secret files; live paths, systemctl, ssh, git push in added lines), `no_secrets` (key
   blocks, token shapes, public IPv4 literals; values are never echoed), `no_new_capabilities`
   (new `unsafe`, process spawning, network; only unlocked by the operator `--allow` flag,
   never by the proposal), `size_limits` (256 KiB, 32 files, 2000 added lines),
   `text_only` (no binary, mode change, symlink, long blob-like lines).
3. **Sandbox deploy** (`stage.rs`): a throwaway `git worktree --detach` at the base,
   `git apply --index`, `write-tree`; the worktree's `.git` link and the source repo's git
   dir are removed/hidden; then the plan's stages (build, affected crate tests, live-style
   deploy) each run in `openmind_sandbox::run` with: **seccomp** (BPF deny-list, required),
   user/pid/mount/net namespaces (net = `lo` only), read-only root, rlimits, cleared env, no
   capabilities, no_new_privs, and live roots / homes / `/var/lib/*` / key dirs hidden.
   **Landlock is never used** (VPS kernel 6.8.12-pve LXC has none: error 95); a plan
   that asks for it is refused, and so is one that drops seccomp, net or no_new_privs.
   A deploy stage may list `loopback_ports`. SECURITY REVIEW: without Landlock the port
   list is not kernel-enforced; the boundary is the private network namespace (only `lo`).
4. **Pass record**: Ed25519-signed with the *sandbox* key (`/var/lib/galion-gate/keys`,
   mode 600, owned by `galion-gate`; Hermes never holds it): proposal sha, patch sha, plan
   sha, the 7 check results, stage results with per-layer enforcement, result tree, verdict.
5. **Owner approval**: a separate owner key (kept off the server) signs
   `proposal sha | record sha | target commit | expiry | label` (`--label` e.g. `DEMO approval`).
6. **Apply** (`apply.rs`): refuses live paths for source, staging root and ledger
   (`/opt/galion/openmind`, `/opt/galion/rust/src`, the other VPS live roots) *before any
   command runs* (no git on a refused path). Then: replay ledger (an approval works once),
   both signatures (sandbox key must differ from owner key), pass verdict, proposal and
   patch checksums, target commit == tested base == approved commit, expiry. It then makes
   a fresh staging copy (`git worktree add --detach <staging-root>/<id>-<rand>`) and applies
   there with `git apply --index` (no commit). If the resulting tree differs from the tested
   tree it is reversed and the apply fails. The source tree is never modified.

## Plans

`plans/hermes-gateway-live-style.example.json` (box) and
`plans/hermes-gateway-vps.example.json` (VPS, toolchain under `/var/lib/galion-gate`) build
`hermes-gateway`, run the `hermes-rs` lib tests, then boot the built gateway with `--mock`
(no provider calls, no spend) on loopback in the sandbox and probe `/health`, a chat
completion (200), and `/v1/models` and chat without the key (401)
(`plans/hermes-gateway-deploy-smoke.sh`).

## Model tiers (`tier.rs`)

Propose / ReviewFailure → high (`boss`), Summarize → low (`worker`), Gate → no model.
A call is refused when remaining budget minus the estimate is below the tier's floor.
