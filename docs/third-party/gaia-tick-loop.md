<!-- SPDX-License-Identifier: MIT -->
<!-- Copyright (c) 3000 Galion / galion-studio -->
# Third-party code: gaia-tick-loop (Gaia S3.1)

**No new dependencies.** Cargo.lock is unchanged. The loop uses only std plus crates already in the workspace: `gaia-live`, `hades-gate` (for `keys::os_random`, i.e. /dev/urandom), `serde_json` and `zagreb-time`.

| Item | Origin | Licence | Note |
|---|---|---|---|
| splitmix64 mixer (`heal_loop::Jitter`) | Algorithm by Sebastiano Vigna (public-domain reference at https://prng.di.unimi.it/splitmix64.c), constants 0x9E3779B97F4A7C15, 0xBF58476D1CE4E5B9, 0x94D049BB133111EB | Public domain (algorithm; our code is written from scratch, MIT like the repo) | Spreads tick times only, never used for anything secret. Writing it out (5 lines) avoids pulling in `rand`. |
| `File::lock` (flock) | Rust std, stable since 1.89 (box toolchain 1.99.1) | MIT OR Apache-2.0 (std) | Locks the global ledger around check-and-record. |
