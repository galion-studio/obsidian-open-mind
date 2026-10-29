# hermes-gate: Hermes output goes through the sandbox before anything live

Prototype (branch `swarm/hermes-sandbox-gate`, crate `crates/hermes-gate`, MIT).
Nothing here pushes, deploys or touches a live root.

## Flow

1. **Propose**: Hermes writes only a `Proposal` (`hermes-gate propose`):
   id, base commit, unified diff, summary, model tier, and a sha256 over the patch.
2. **Static security checks** (`checks.rs`): size, text only, no mode changes, path deny
   (`.git/`, `deploy/`, unit files, the gate and the sandbox themselves, keys),
   no live paths / systemctl / git push / ssh / key material in added lines, no host
   addresses, no new `unsafe`. Fail here = no deploy.
3. **Sandbox deploy** (`stage.rs`): throwaway `git worktree --detach` at the base,
   `git apply --index`, `write-tree`, then the plan's stages (build, test, live-style
   deploy) each in `openmind_sandbox::run` with Landlock, userns, an empty netns, rlimits,
   a cleared env, and live dirs hidden. It fails closed if a required layer is missing. A deploy stage may list
   `loopback_ports`: the service boots inside the private netns (`lo` only) and
   is smoke-tested over loopback; no other port, no outside network.
4. **Pass record**: `record.rs` Ed25519-signed by the *sandbox* key (not held by Hermes):
   proposal sha, plan sha, check results, stage results, result tree, verdict.
5. **Human approval**: owner Ed25519 signature over
   `proposal sha | record sha | target HEAD | expiry` (`hermes-gate approve`).
6. **Apply** (`apply.rs`): requires different sandbox/owner keys, a valid record, a pass verdict,
   matching proposal and base, target HEAD == base, and a fresh approval for this head. It applies with
   `git apply --index` (no commit) and reverts unless the tree equals the record's result tree.
   Live roots are refused outright in this prototype, and the hardened git helper never runs
   in a live root.

## Live-style example (box)

`plans/hermes-gateway-live-style.example.json` builds `hermes-gateway`, runs the
`hermes-rs` lib tests, then boots the built gateway with `--mock` (no provider
calls, no spend) on loopback in the sandbox and probes `/health`, a chat
completion, and a 401 without the key (`plans/hermes-gateway-deploy-smoke.sh`).

## Model tiers (`tier.rs`)

Propose / ReviewFailure → high (`boss`), Summarize → low (`worker`), Gate → no model.
A call is refused when remaining budget minus the estimate is below the tier's floor.
