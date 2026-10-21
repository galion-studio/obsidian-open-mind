// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The few POSIX calls std does not wrap: signal(2), umask(2), kill(2). Declared
//! here instead of pulling in the `libc` crate; std already links the C library on
//! every target we build (x86_64/aarch64 musl, aarch64-android, armv7 musl), and the
//! signal numbers below are the same on all Linux architectures we target.

use std::sync::atomic::{AtomicI32, Ordering};

pub const SIGHUP: i32 = 1;
pub const SIGINT: i32 = 2;
pub const SIGKILL: i32 = 9;
pub const SIGTERM: i32 = 15;

extern "C" {
    fn signal(signum: i32, handler: usize) -> usize;
    fn umask(mask: u32) -> u32;
    fn kill(pid: i32, sig: i32) -> i32;
}

static PENDING: AtomicI32 = AtomicI32::new(0);

extern "C" fn on_signal(sig: i32) {
    // Async-signal-safe: one atomic store, nothing else.
    PENDING.store(sig, Ordering::SeqCst);
}

/// Catch SIGINT, SIGTERM and SIGHUP (Ctrl-C, `timeout`/`kill`, a dropped ssh session):
/// the deploy loop polls `pending()` and rolls back instead of dying mid-swap (F6).
pub fn install_handlers() {
    let h = on_signal as extern "C" fn(i32) as usize;
    for s in [SIGHUP, SIGINT, SIGTERM] {
        // SAFETY: plain signal(2) with a handler that only stores an atomic.
        unsafe {
            signal(s, h);
        }
    }
}

/// The last caught signal, if any (0 = none).
pub fn pending() -> i32 {
    PENDING.load(Ordering::SeqCst)
}

/// Forget a caught signal (used once a rollback has started: it must finish).
pub fn clear() -> i32 {
    PENDING.swap(0, Ordering::SeqCst)
}

#[cfg(test)]
pub fn raise_for_test(sig: i32) {
    PENDING.store(sig, Ordering::SeqCst);
}

pub fn name(sig: i32) -> &'static str {
    match sig {
        SIGHUP => "SIGHUP",
        SIGINT => "SIGINT",
        SIGTERM => "SIGTERM",
        _ => "signal",
    }
}

pub fn set_umask(mask: u32) {
    // SAFETY: umask(2) cannot fail.
    unsafe {
        umask(mask);
    }
}

/// kill(2); a negative pid addresses a process group.
pub fn send(pid: i32, sig: i32) -> bool {
    // SAFETY: kill(2) with plain integer arguments.
    unsafe { kill(pid, sig) == 0 }
}

/// True if `pid` exists and its command line mentions `needle`.
pub fn alive_with(pid: u32, needle: &str) -> bool {
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(raw) => String::from_utf8_lossy(&raw).contains(needle),
        Err(_) => false,
    }
}
