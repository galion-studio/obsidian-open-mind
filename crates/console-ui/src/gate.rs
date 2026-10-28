// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The phone/host gate for pages, ported from console-ide `src/start.ts`
//! (phone middleware) and `src/lib/host.ts` (live tree, 2026-10-20).
//!
//! Pages are decided here, before any data is fetched:
//!
//! 1. a host that is not a console host gets the quiet "Soon" page
//!    (`developer.galion.app` gets the links page);
//! 2. `/` on a console host redirects to `/console`;
//! 3. a loopback host is let through **only when the TCP peer is loopback
//!    too** (the TSX trusted the Host header alone);
//! 4. any other console host must present the admin cookie; the cookie is
//!    checked by the API upstream (`GET /api/v1/admin` -> `{"you":true}`),
//!    so this process never reads the device store under /root;
//! 5. anything else gets the pairing page. Upstream errors fail closed.

/// Hosts where the console renders (console-ide `CONSOLE_HOSTS`). The TSX list
/// also carried the two server IPs literally; here they come from
/// `--console-host` (the unit passes them), so no public IP is in the code.
/// `console.galion.studio` / `dashboard.galion.studio` were dropped (D-414-1 A,
/// slot 433): NXDOMAIN, never in the tunnel, never served; see
/// [`RETIRED_HOSTS`].
pub const CONSOLE_HOSTS: &[&str] = &[
    "console.galion.app",
    "ip51.vs5.titanaxe.com",
    "ip50.vp3.titanaxe.com",
];

/// Old console-ide aliases that were never live (NXDOMAIN, not in the tunnel;
/// D-414-1 A). They would still match the `console.` / `dashboard.` prefix
/// rules, so they are refused explicitly and get the unknown-host ("Soon")
/// answer. An operator who really wants one back passes it with
/// `--console-host` (an explicit `extra` entry wins) after the DNS + tunnel
/// work of D-414-1 B.
pub const RETIRED_HOSTS: &[&str] = &["console.galion.studio", "dashboard.galion.studio"];

/// `normalizeHost`: drop the port, trim, lower-case, drop `www.` and a
/// trailing root dot (`host.` is the same DNS name; slot 433, so a dotted
/// retired name cannot slip past [`RETIRED_HOSTS`] via the prefix rules).
pub fn normalize_host(host: &str) -> String {
    let h = host
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    h.strip_prefix("www.").map(str::to_string).unwrap_or(h)
}

pub fn is_loopback_host(bare: &str) -> bool {
    bare == "localhost" || bare.starts_with("127.")
}

/// `isConsoleHost`. `extra` is the machine's public IP when known (the TSX
/// read `/workspace/galion-light/state/machine.json`; here it is a flag).
pub fn is_console_host(host: &str, extra: &[String]) -> bool {
    let h = normalize_host(host);
    if h.is_empty() {
        return false;
    }
    if extra.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
        return true;
    }
    if RETIRED_HOSTS.contains(&h.as_str()) {
        return false;
    }
    CONSOLE_HOSTS.contains(&h.as_str())
        || h.starts_with("console.")
        || h.starts_with("dashboard.")
        || h.contains("runpod.net")
        || is_loopback_host(&h)
        || h.ends_with(".local")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Not a console host: quiet page (`links` for developer.galion.app).
    Soon { links: bool },
    /// `/` on a console host.
    RedirectConsole,
    /// Render the page.
    Allow,
    /// Need the admin cookie; ask the upstream.
    AskUpstream,
}

/// First, local part of the decision (no I/O).
pub fn decide(host: &str, path: &str, peer_loopback: bool, extra: &[String]) -> Decision {
    if !is_console_host(host, extra) {
        return Decision::Soon {
            links: normalize_host(host) == "developer.galion.app",
        };
    }
    if path == "/" || path.is_empty() {
        return Decision::RedirectConsole;
    }
    if is_loopback_host(&normalize_host(host)) && peer_loopback {
        return Decision::Allow;
    }
    Decision::AskUpstream
}

/// Only the admin cookie is forwarded to the upstream for gate checks and
/// data calls; other cookies (analytics, auth experiments) stay here.
pub fn admin_cookie(cookie_header: Option<&str>) -> Option<String> {
    let h = cookie_header?;
    h.split(';')
        .map(str::trim)
        .find(|p| p.starts_with("galion_admin="))
        .map(str::to_string)
}

/// Read a cookie value by name.
pub fn cookie(cookie_header: Option<&str>, name: &str) -> Option<String> {
    cookie_header?.split(';').map(str::trim).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        (k == name).then(|| v.to_string())
    })
}

/// `galion_device` values are base64url secrets from POST /api/v1/admin.
/// Reject anything else before it is echoed into JSON or a Set-Cookie.
pub fn device_token_ok(raw: &str) -> bool {
    let b = raw.as_bytes();
    (16..=128).contains(&b.len())
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_')
}

/// True when the request carries a non-empty `galion_admin` cookie pair.
pub fn has_admin_cookie(cookie_header: Option<&str>) -> bool {
    admin_cookie(cookie_header)
        .map(|c| c.len() > "galion_admin=".len())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(
            normalize_host("WWW.Console.Galion.App:443"),
            "console.galion.app"
        );
        for h in [
            "console.galion.app",
            "console.anything.example",
            "dashboard.galion.app",
            "console.galion.app.",
            "127.0.0.1:19370",
            "localhost",
            "pod-1.proxy.runpod.net",
            "box.local",
        ] {
            assert!(is_console_host(h, &[]), "{h}");
        }
        for h in ["galion.app", "developer.galion.app", "", "evil.com"] {
            assert!(!is_console_host(h, &[]), "{h}");
        }
        assert!(is_console_host("203.0.113.7", &["203.0.113.7".into()]));
        assert!(!is_console_host("203.0.113.7", &[]));
        // D-414-1 A: the two never-live .studio aliases are unknown hosts now,
        // in any case / port / www. form, unless passed explicitly.
        for h in [
            "console.galion.studio",
            "dashboard.galion.studio",
            "WWW.Console.Galion.Studio:443",
            "dashboard.galion.studio.",
        ] {
            assert!(!is_console_host(h, &[]), "{h}");
            assert_eq!(
                decide(h, "/console", true, &[]),
                Decision::Soon { links: false },
                "{h}"
            );
        }
        assert!(is_console_host(
            "console.galion.studio",
            &["console.galion.studio".into()]
        ));
        // Live names keep working.
        for h in CONSOLE_HOSTS {
            assert!(is_console_host(h, &[]), "{h}");
        }
        assert!(!CONSOLE_HOSTS.iter().any(|h| RETIRED_HOSTS.contains(h)));
    }

    #[test]
    fn decisions() {
        assert_eq!(
            decide("galion.app", "/status", true, &[]),
            Decision::Soon { links: false }
        );
        assert_eq!(
            decide("developer.galion.app", "/x", true, &[]),
            Decision::Soon { links: true }
        );
        assert_eq!(
            decide("console.galion.app", "/", false, &[]),
            Decision::RedirectConsole
        );
        assert_eq!(
            decide("127.0.0.1:8080", "/agents", true, &[]),
            Decision::Allow
        );
        // Spoofed loopback Host from a non-loopback peer must still pair.
        assert_eq!(
            decide("127.0.0.1", "/agents", false, &[]),
            Decision::AskUpstream
        );
        // cloudflared connects from loopback but carries the public Host.
        assert_eq!(
            decide("console.galion.app", "/agents", true, &[]),
            Decision::AskUpstream
        );
    }

    #[test]
    fn device_token_shape() {
        assert!(device_token_ok("abcdefghijklmnopqrstuv"));
        assert!(device_token_ok(&"a".repeat(16)));
        assert!(!device_token_ok("short"));
        assert!(!device_token_ok("bad token with spaces!!!!"));
        assert!(!device_token_ok(&"a".repeat(129)));
        assert!(has_admin_cookie(Some("galion_admin=tok")));
        assert!(!has_admin_cookie(Some("galion_admin=")));
        assert!(!has_admin_cookie(None));
    }

    #[test]
    fn cookies() {
        let h = Some("a=1; galion_admin=tok%2B; galion_theme=light");
        assert_eq!(admin_cookie(h).as_deref(), Some("galion_admin=tok%2B"));
        assert_eq!(cookie(h, "galion_theme").as_deref(), Some("light"));
        assert_eq!(admin_cookie(Some("x_galion_admin=1")), None);
        assert_eq!(cookie(None, "a"), None);
    }
}
