<!-- SPDX-License-Identifier: MIT -->
<!-- SPDX-FileCopyrightText: 2026 Slavik / Galion Studio -->

# What to do next

- Written: 2026-10-19T22:22:11+02:00 (Europe/Zagreb)
- Private only. Do not push. Do not add a public remote.
- Do not systemctl restart, and do not copy a new binary over a live pid, until Slavik says so.

The library wave is far enough. More "does not dial" tests will not make Hermes autonomous. The next work is one shadow path at a time.

## Do not do these yet

- Do not free port 8080. Vite stays until the console switch is approved.
- Do not point memory clients at a new process on 8420. The live gateway stays.
- Do not install deploy/boot/rendered/*.service.
- Do not bind port 443. openmind-origin is a 302 builder with no listener.
- Do not treat REUSE as done. Headers are in. REUSE.toml is not.

## Order

1. Keep this repo private. Commit locally. No push.
2. One offline cargo metadata, or one cargo generate-lockfile, as a single job, then replace the working-tree Cargo.lock only if the diff is the package list and nothing else. Until that job, the lock in git is the dirty working copy from parallel builds.
3. Shadow memory. Run the fixture server on 127.0.0.1:0, never 8420. Compare health, v1 search, and v2 search to the live gateway with a field projection. A miss on a fixture phrase is expected. Do not switch the client.
4. Dry-run route inside a test only. An approved CPU-local row may be chosen. Do not dial it from galion-one.
5. After Slavik approves one binary, replace one service, not four. Suggested first candidate is not the live One. Tuner or room is smaller, and room must stay without --listen until asked.
6. Only then talk about wiring origin, the console, and the provider socket.

## Live check after any future switch

One read each, no retry loop: 127.0.0.1:8091/health should stay the word ok, 8420/health should stay JSON, 8080/health should stay 404 until the console is actually switched.
