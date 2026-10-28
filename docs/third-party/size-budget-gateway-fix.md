<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party: size-budget-gateway-fix (wave-4 slot 353) — 2026-10-28 (Europe/Zagreb CET +01:00)

No new dependency and no copied code. The merged slot 329 branch (hermes-gateway-2mib a0d099c2) brings
its own rows (docs/third-party/hermes-gateway-2mib.md: `rusqlite` dev-dependency of hermes-gateway, already
in Cargo.lock; `sha2` feature `force-soft-compact`, same crate). New code here: bash in the existing
scripts/size-budget.sh, a plain-text table config/release/size-budget.tsv and std-only Rust tests.
