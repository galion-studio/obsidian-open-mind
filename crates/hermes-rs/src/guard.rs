// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Turn a panic into an error string for one request, session or tool.
//!
//! `catch_unwind` only stops the process from dying when the binary is built
//! with `panic = "unwind"` (profile `release-unwind` for hermes-gateway).
//! Under `panic = "abort"` the process still ends; the text helpers in
//! [`crate::utf8`] are what keep ordinary multilingual input from panicking
//! in the first place.

use std::any::Any;
use std::panic::AssertUnwindSafe;

pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".into())
}

/// Log the panic and return a short, redacted error. The process keeps running.
pub fn report(what: &str, payload: Box<dyn Any + Send>) -> String {
    let msg: String = panic_message(payload.as_ref()).chars().take(300).collect();
    let safe = crate::redact(&msg);
    eprintln!("hermes-rs {what}: PANIC caught, gateway keeps running: {safe}");
    format!("{what} failed internally (bug, caught; the gateway keeps running): {safe}")
}

/// Run `f`. On panic, log it and return a 500 whose body names `what`.
pub fn isolate(
    what: &'static str,
    f: impl FnOnce() -> crate::http::Response,
) -> crate::http::Response {
    match std::panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(p) => crate::http::error(
            500,
            &report(what, p),
            "internal_error",
            Some("session_panic"),
        ),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_unwind_profile_overrides_abort() {
        let toml = include_str!("../../../Cargo.toml");
        let unwind = toml
            .split("[profile.release-unwind]")
            .nth(1)
            .expect("profile release-unwind");
        let body = unwind.split("\n[").next().unwrap_or("");
        assert!(
            body.contains("panic = \"unwind\""),
            "hermes-gateway must be buildable with panic=unwind ({body})"
        );
    }
}
