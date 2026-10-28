<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# D-421-1 option B prepared: post-publish batch as ONE replay commit (wave-4 slot 432), 2026-10-28 23:06-23:30 CET (+01:00)

Prep only. **The owner still decides D-421-1** (A merge vs B replay). main, main-staging and PRs
were NOT touched. No PR opened. Nothing deployed. VPS read-only.

## What this branch is

`swarm/post-publish-land-replay` = 2 commits on top of **2ce1a08f** (slot 421 rebuild of GitHub
main 7ba85b60, tree **a6f6c163101d2e8cedcf4919a6fbf25ef415cd81** = main's tree):

1. **d6976380** replay: `git diff --binary 2ce1a08f 96e24ce2` (the slot 415 test-only batch
   18d68395 as merged by slot 421) applied with `git apply --index --binary`, committed as ONE
   commit. 79 files, +3899 / -842. Patch sha256 `e24d8cb260ed0ae9...` (same bytes from
   `git diff 2ce1a08f 96e24ce2`, from `git show d6976380`, and from `git diff <main> d6976380`).
2. this note.

**Proof:** `git rev-parse d6976380^{tree}` = **19d5c9831cf5f6a01f10ec8b1b4e0031c49c9386** =
`96e24ce2^{tree}` (slot 421 merge result); `git diff --quiet d6976380 96e24ce2` exits 0.

## Where main came from

7ba85b60 is still not on the box (private repo, no credentials on the box). GitHub read through the
connector at 23:07 CET did not answer (two tries), so the base is the slot 421 rebuild 2ce1a08f
(every blob id checked against GitHub's compare in slot 421; main head 7ba85b60 last verified on
GitHub 21:42 CET by the refill run). **Before landing, the owner must confirm main is still
7ba85b60 / tree a6f6c163** (step 2 below checks it).

## Exact owner command (B, replay)

In any clone that can reach GitHub:

```
git fetch origin main swarm/post-publish-land-replay
git switch -c land/post-publish-batch origin/main
test "$(git rev-parse HEAD^{tree})" = a6f6c163101d2e8cedcf4919a6fbf25ef415cd81   # main unchanged since publish
git cherry-pick d6976380db60283f9bc29171eccec84018462622
test "$(git rev-parse HEAD^{tree})" = 19d5c9831cf5f6a01f10ec8b1b4e0031c49c9386   # same tree as slot 421 / this branch
git push origin land/post-publish-batch:main        # fast-forward of main by one commit (owner only; or open a PR from it)
```

History on main then gets exactly one new commit (the batch); the 2ce1a08f reconstruction commit
never reaches main (cherry-pick only copies d6976380's change). If step 3 fails, main has moved:
stop, and re-run this slot against the new main.

### Rehearsal (box, 23:12 CET, throwaway clone /tmp/rehearse432, removed)

Stand-in main = `git commit-tree a6f6c163 -p 13a296ce -p 83e573e2` (bfb974ea; main's tree, merge
shape like 7ba85b60). Three routes, all tree **19d5c983**:

| route | command | tree |
|---|---|---|
| B1 (owner command) | `git cherry-pick d6976380` | 19d5c983 |
| B2 (421 wording) | `git diff --binary 2ce1a08f 96e24ce2 \| git apply --index --binary` + commit | 19d5c983 |
| B3 (no 421 commits needed) | `git diff --binary <main> d6976380 \| git apply --index --binary` + commit | 19d5c983 |

## Gates on d6976380 (box, CET +01:00)

| gate | result |
|---|---|
| `cargo fmt --all --check` | ok (23:10) |
| `scripts/no-python.sh` | ok (23:10) |
| `scripts/spdx-scan.sh` | 817 scanned, 0 missing (23:10:06) |
| `cargo clippy --workspace --all-targets -j 2 -- -D warnings` | rc 0 (23:10:11-23:11:01) |
| `env -u GALION_REAL cargo test --workspace --no-fail-fast --exclude openmind-sandbox --exclude cgroup-detect --exclude hermes-tools` x3 | **424 suites, 2232 passed / 0 failed / 18 ignored, all 3 runs** (23:11:01-23:14:39, 23:14:39-23:16:16, 23:16:16-23:17:51) = slot 415/421 numbers exactly |

Sandbox crates excluded locally (sandbox_vps_only); the batch changes no sandbox code, so no VPS
sandbox run was needed. hermes-tools excluded as in slots 415/421. All via `cargo-slot`, `-j 2`,
own target dir (removed after).

## Real-life evidence

Box, `GALION_REAL=1`, `--release`, slot ports 23020-23029, loopback only, every process torn down by
the test itself (ports 23020-23029 checked free afterwards).

| time (CET) | test | env | result |
|---|---|---|---|
| 23:17:58-23:19:45 | `console-static --test real_landing` | `LANDING_REAL_PORT_BASE=23020` | 1/1 ok: `GET /console` 200 31,678 B, 0 `<script>`, 9 cards, 2 svg; base css 200 20,324 B (same bytes as 415/421) |
| 23:19:45-23:19:51 | `room --test real_room` | `ROOM_REAL_PORT=23022` | 2/2 ok: host session on :23022; netns session on :23023 invisible to the host |
| 23:19:51-23:22:08 | `openmind-nerve --test real_cross_target --test-threads=1` | `NERVE_XT_PORT_BASE=23025` | **8/8 ok, 100.3 s**: x86_64 musl 2,017,768 B sha256 ee9f33e9... (WARN above 1,887,435 B, headroom 79,384 B to 2 MiB); aarch64 1,666,696 B sha256 f207de74...; live recipe == solo (ee9f33e9); smoke :23025 / :23026 (qemu aarch64) / :23027 first cycle 0.10 / 0.21 / 0.10 s, /health wire order equal; `size_budget_table` ok. Binary hashes identical to slot 421 = the replay builds the same nerve. |

VPS read-only (ssh, GET + `systemctl show`), 23:14:52 CET: galion-nerve, gaia-cortex,
console-rust-primary, hermes-rs all active, NRestarts 0; nerve :8670 /health 200, /state cycle 24,
self_eval 68 D (top deduction: edge tunnel routes 12 hostnames to 7 closed origin ports, known,
slot 438); cortex :8674 200; console :8080 /console 200. No VPS write.

## Open / owner

- **D-421-1** (still open): land the batch by B with the command above (recommended, one clean
  commit) or by A (`git merge --no-ff swarm/post-publish-batch-main`, carries the duplicate
  2ce1a08f). Both give tree 19d5c983.
- **D-432-1**: after landing, delete superseded branches swarm/post-publish-batch-main and
  swarm/post-publish-land-replay (owner; recommended yes, once main shows tree 19d5c983).
- Stacking: 437 nerve-line-land-prep / 430 / 435 should rebase onto main AFTER this lands (their
  base is 7ba85b60/2ce1a08f; this commit touches 79 test/doc files only).
- Known, not caused by this batch: F-383-5, F-383-1..10, F-375-1; live nerve self_eval 68 D.
