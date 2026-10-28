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

/// A loopback port block for one real-test file (slot 375,
/// real-ports-batch-2, 2026-10-28 CET +01:00): `127.0.0.1:base+off`, with
/// `base` read once from env `var` (unset = `default`, bad value = panic
/// naming the var). Meant to sit in a `static` so call sites get
/// `&'static str` addresses without leaking per call:
///
/// ```
/// use galion_realtest::ports::Block;
/// static PORTS: Block = Block::new("MY_TEST_PORT_BASE", 20060, 10);
/// assert_eq!(PORTS.port(3), PORTS.base() + 3);
/// assert!(PORTS.addr(3).starts_with("127.0.0.1:"));
/// ```
pub struct Block {
    var: &'static str,
    default: u16,
    span: u16,
    cell: std::sync::OnceLock<(u16, Vec<String>)>,
}

impl Block {
    /// `span` ports from `base`; nothing is read until first use.
    pub const fn new(var: &'static str, default: u16, span: u16) -> Self {
        Block {
            var,
            default,
            span,
            cell: std::sync::OnceLock::new(),
        }
    }

    fn get(&self) -> &(u16, Vec<String>) {
        self.cell.get_or_init(|| {
            let base = base_from_env(self.var, self.default, self.span);
            let addrs = (0..self.span)
                .map(|o| format!("127.0.0.1:{}", base + o))
                .collect();
            (base, addrs)
        })
    }

    /// First port of the block.
    pub fn base(&self) -> u16 {
        self.get().0
    }

    /// `base + off`; panics if `off` is outside the block.
    pub fn port(&self, off: u16) -> u16 {
        assert!(
            off < self.span,
            "{}: offset {off} >= span {}",
            self.var,
            self.span
        );
        self.base() + off
    }

    /// `"127.0.0.1:<base+off>"`; panics if `off` is outside the block.
    pub fn addr(&self, off: u16) -> &str {
        assert!(
            off < self.span,
            "{}: offset {off} >= span {}",
            self.var,
            self.span
        );
        &self.get().1[usize::from(off)]
    }

    /// `"<base>-<base+span-1>"` (e.g. for `--allow-ports`).
    pub fn range(&self) -> String {
        format!("{}-{}", self.base(), self.base() + self.span - 1)
    }

    /// The env var name (for messages).
    pub fn var(&self) -> &'static str {
        self.var
    }
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

    #[test]
    fn block_default_addresses() {
        static B: super::Block = super::Block::new("GALION_PORTS_TEST_UNSET_375", 20060, 4);
        assert_eq!(B.base(), 20060);
        assert_eq!(B.port(3), 20063);
        assert_eq!(B.addr(0), "127.0.0.1:20060");
        assert_eq!(B.addr(3), "127.0.0.1:20063");
        assert_eq!(B.range(), "20060-20063");
        assert_eq!(B.var(), "GALION_PORTS_TEST_UNSET_375");
    }

    #[test]
    #[should_panic(expected = "offset 4 >= span 4")]
    fn block_offset_outside_span_panics() {
        static B: super::Block = super::Block::new("GALION_PORTS_TEST_UNSET_375B", 20060, 4);
        let _ = B.addr(4);
    }
}
