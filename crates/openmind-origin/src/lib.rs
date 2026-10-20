// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! Tiny HTTP redirect builder.
//!
//! DESK-ID: openmind.B56
//!
//! [`redirect_response`] builds one HTTP/1.1 302 response. The Location header
//! comes from the argument. An empty argument, or one that would break the
//! header block, uses [`DEFAULT_LOCATION`] (`/`). The body is empty.
//!
//! This crate has no listener. Building a response does not bind a port,
//! dial out, or touch the network.

#![forbid(unsafe_code)]

use std::fmt;

/// Location used when the caller does not supply a usable one.
pub const DEFAULT_LOCATION: &str = "/";

/// Numeric status of every response this crate builds.
pub const STATUS_CODE: u16 = 302;

/// Status line of every response this crate builds, without the trailing CRLF.
pub const STATUS_LINE: &str = "HTTP/1.1 302 Found";

/// One HTTP redirect. Status is always 302. Body is always empty.
///
/// The value is bytes only. Building or displaying it does not touch the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedirectResponse {
    location: String,
}

impl RedirectResponse {
    /// Status code. Always [`STATUS_CODE`].
    #[must_use]
    pub fn status_code(&self) -> u16 {
        STATUS_CODE
    }

    /// Status line, without the trailing CRLF. Always [`STATUS_LINE`].
    #[must_use]
    pub fn status_line(&self) -> &'static str {
        STATUS_LINE
    }

    /// Location header value. Never empty.
    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Response body. Always empty.
    #[must_use]
    pub fn body(&self) -> &'static [u8] {
        b""
    }

    /// Always false. This value was built in memory and does not touch the network.
    #[must_use]
    pub fn touches_network(&self) -> bool {
        self.location.is_empty()
    }
}

impl fmt::Display for RedirectResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{STATUS_LINE}\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n",
            self.location
        )
    }
}

/// Build a 302 redirect.
///
/// `location` is the Location header. An empty string uses [`DEFAULT_LOCATION`].
/// A value containing CR, LF, or NUL also uses [`DEFAULT_LOCATION`], so the
/// status line stays a single line. The body is empty.
///
/// This function does not touch the network.
///
/// # Examples
///
/// ```
/// use openmind_origin::redirect_response;
///
/// let home = redirect_response("");
/// assert_eq!(home.status_line(), "HTTP/1.1 302 Found");
/// assert_eq!(home.location(), "/");
/// assert!(home.body().is_empty());
/// assert!(!home.touches_network());
///
/// let console = redirect_response("/console");
/// assert_eq!(console.location(), "/console");
/// assert_eq!(console.status_code(), 302);
/// ```
#[must_use]
pub fn redirect_response(location: &str) -> RedirectResponse {
    RedirectResponse {
        location: usable_location(location).to_owned(),
    }
}

fn usable_location(location: &str) -> &str {
    if location.is_empty() || has_header_break(location) {
        DEFAULT_LOCATION
    } else {
        location
    }
}

fn has_header_break(location: &str) -> bool {
    location
        .chars()
        .any(|ch| ch == '\r' || ch == '\n' || ch == '\0')
}
