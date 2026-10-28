// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Port bases for real tests that used to hard-code their ports (slot 354,
//! real-toolchain-aarch64-ports, 2026-10-28 CET +01:00).
//!
//! A swarm slot may only bind inside its assigned range, so a test with fixed
//! ports outside that range could not be re-run by a later slot. Each such
//! test now reads an env var naming its first port; unset keeps the old
//! default, so existing runs and docs stay valid. A bad value is an error
//! (panic in the test), never a silent fall-back to the default range.

/// Parse a port-base value: `span` ports `base..base+span` must all fit in
/// 1024..=65535. `None` (unset) gives `default`.
pub fn parse_base(raw: Option<&str>, default: u16, span: u16) -> Result<u16, String> {
    let Some(raw) = raw else {
        return Ok(default);
    };
    let s = raw.trim();
    let base: u16 = s
        .parse()
        .map_err(|_| format!("port base {s:?} is not a number 1024-65535"))?;
    if base < 1024 {
        return Err(format!("port base {base} is below 1024"));
    }
    if span == 0 || u32::from(base) + u32::from(span) - 1 > 65535 {
        return Err(format!("port base {base} + {span} ports passes 65535"));
    }
    Ok(base)
}

/// `parse_base` on env var `var`; panics with the var name on a bad value.
pub fn base_from_env(var: &str, default: u16, span: u16) -> u16 {
    let raw = std::env::var(var).ok();
    parse_base(raw.as_deref(), default, span).unwrap_or_else(|e| panic!("{var}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::parse_base;

    #[test]
    fn unset_keeps_default() {
        assert_eq!(parse_base(None, 20150, 4), Ok(20150));
    }

    #[test]
    fn valid_value_used() {
        assert_eq!(parse_base(Some("22240"), 20150, 4), Ok(22240));
        assert_eq!(parse_base(Some(" 22240\n"), 20150, 4), Ok(22240));
        assert_eq!(parse_base(Some("65532"), 20150, 4), Ok(65532));
    }

    #[test]
    fn bad_values_rejected() {
        for bad in ["", "abc", "-1", "70000", "22240x", "1023", "0"] {
            assert!(parse_base(Some(bad), 20150, 4).is_err(), "{bad:?}");
        }
        assert!(parse_base(Some("65533"), 20150, 4).is_err());
        assert!(parse_base(Some("22240"), 20150, 0).is_err());
    }
}
