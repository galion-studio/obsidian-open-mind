# Contributing to Obsidian Open Mind

Thanks for wanting to help. This project is deliberately small and CPU-light; the rules
below exist to keep it that way.

## Before you start

- Open an issue (bug report or feature request template) before a large change. Small
  pull requests are welcome without ceremony.
- One change per pull request. Small PRs get reviewed; large ones sit.

## Build and test

The development box is shared, so builds run cold:

```sh
export PATH=/root/.cargo/bin:$PATH
nice -n 19 cargo build -j 2 --release --workspace
nice -n 19 cargo test -j 2 --workspace
```

## Style gate

All four must pass before a pull request is ready to review:

```sh
nice -n 19 cargo fmt --all -- --check
nice -n 19 cargo clippy --workspace --all-targets -- -D warnings
nice -n 19 cargo test --workspace
```

`clippy` runs with `-D warnings`: a warning fails the build, it is not a suggestion.

## Sign-off (DCO)

This project uses the **Developer Certificate of Origin, sign-off only** — no separate CLA.
Every commit must carry a sign-off line, added with `git commit -s`:

```
Signed-off-by: Your Name <your@email>
```

The sign-off is your statement that you wrote the change, or that you have the right to
submit it under the MIT license (see `LICENSE`).

## Hard rules

1. **No Python.** Never add, run or require a Python file, and never shell out to
   `python3`. The CI fails on any tracked `.py` file. Use Rust.
2. **Rust only for new code.** No Node, no shell-out to other runtimes, no new heavy
   dependencies. Justify any new crate in the pull request; the standard is a small
   `std`-first change.
3. **Timestamps are Europe/Zagreb RFC 3339 with an explicit offset** — `+02:00` (CEST)
   until 2026-10-25 03:00 local, `+01:00` (CET) after. Never the server clock, never `Z`.
   Shell form: `TZ=Europe/Zagreb date -Iseconds`.
4. **No secrets, ever.** No keys, tokens, `.env` files or credentials in code, tests,
   fixtures, logs or issues. Key material stays in the key box; the schema holds a
   `KeySlot` name, never the secret.
5. **No stray CJK characters** (Chinese, Japanese, Korean) in any file. CI scans for them.
6. **No money rails.** Connecting a real-money account is not approved for this project and
   must not appear in code, docs, issues or roadmap.
7. **CPU-light by default.** Prefix heavy commands with `nice -n 19`, build with `-j 2`,
   and avoid fast polling loops.
8. **Facts only.** Any number, size or capability claim in docs, README or comments must be
   traced to a file and line, or explicitly marked `TODO(verify)`. "Planned" is not
   "implemented".
9. **MIT.** By contributing you agree your work is licensed under the repository `LICENSE`.

## Review

A pull request is ready when: `fmt`, `clippy -D warnings` and `test` pass; the checklist in
the pull request template is filled in; and no rule above is broken.
