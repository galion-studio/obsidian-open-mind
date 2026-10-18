# Universal CPU constraint (all Rust workstreams) - added 2026-10-18 21:02 CEST (+02:00)
Every binary must run on consumer-grade CPUs: phones, laptops, Raspberry Pi-class boards.
- Tier 1 (must cross-compile + pass `cargo check`/build): x86_64-unknown-linux-musl, aarch64-unknown-linux-musl.
- Tier 2 (cross-check where feasible, report results): aarch64-linux-android (Termux), armv7-unknown-linux-musleabihf (Pi Zero 2/older ARM).
- Portable Rust only: no inline asm, no `target-cpu=native`, no `target-feature` flags, no arch-specific SIMD intrinsics; gate any optional arch path behind cfg with a portable fallback.
- Minimal, lightweight deps: prefer std; avoid C/C++ build deps (no openssl-sys: use rustls with ring or no TLS; rusqlite only with `bundled`); default-features = false; no tokio "full".
- Release profile: opt-level="z" or "s", lto, codegen-units=1, panic="abort", strip.
- Report per binary: size per target, RSS on x86_64, and which tier-2 targets built.
- Linking: rust-lld (bundled with rustup) for musl targets; Android check with `cargo check --target aarch64-linux-android` (no NDK on this box).
