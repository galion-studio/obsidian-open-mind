<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->

# Third-party code: bank-openrouter-budget (wave 4 slot 366)

No new dependencies and no copied code. `hermes-bank` and `hermes_rs::bank` use only crates
hermes-rs already had: `serde_json` (MIT OR Apache-2.0) and, behind the default `sqlite` feature,
`rusqlite` 0.32 with bundled SQLite (MIT; SQLite is public domain). `Cargo.lock` unchanged.

Reference read (not copied): OpenRouter API docs, Limits page
<https://openrouter.ai/docs/api/reference/limits>, for the `budgets` / `effective_budget` reply
shape (read 2026-10-28 05:50 CET). The logic ported is Galion's own console-ide `src/lib/bank.ts`.
