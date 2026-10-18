## What this changes

<!-- One or two sentences. Link the issue it closes. -->

Closes #

## How it was tested

```
```

## Checklist

- [ ] `nice -n 19 cargo fmt --all -- --check` passes.
- [ ] `nice -n 19 cargo clippy --workspace --all-targets -- -D warnings` passes.
- [ ] `nice -n 19 cargo test --workspace` passes.
- [ ] No Python file was added, and nothing shells out to `python3`.
- [ ] No secrets, keys, tokens or `.env` content in the change, the tests or the logs.
- [ ] Every timestamp is Europe/Zagreb RFC 3339 with an offset (`+02:00` CEST / `+01:00`
      CET). No server time, no `Z`.
- [ ] CJK scan is clean (no Han, Hiragana, Katakana or Hangul characters).
- [ ] No money-rail integration and no profit or return claims were introduced.
- [ ] New factual claims in docs are traced to a file and line, or marked `TODO(verify)`.
- [ ] Commits carry a `Signed-off-by` line (`git commit -s`).
- [ ] The change is small and does one thing.
