// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `--owner-routes` (wave 4 slot 395, D-384-1 option A prep, 2026-10-28 CET
//! +01:00): the Node-era console API paths that the Rust console does not
//! answer itself.
//!
//! * [`OWNER_ROUTES`]: rows whose Rust owner is another live loopback service
//!   (galion-one `--pulse`, hermes-rs `--hermes`). With `--owner-routes` the
//!   console forwards an exact-path GET/HEAD there (path rewritten, no body,
//!   no `Cookie` / `Authorization` sent on). Same API gate as every other
//!   `/api/*` route (it ran before this).
//! * [`RETIRED`]: rows with no Rust provider anywhere. With `--owner-routes`
//!   they answer an explicit `410 Gone` JSON instead of a bare 404, so a test
//!   or a page can tell "retired" from "missing".
//!
//! Not proxied on purpose: `/api/hermes/models` and `/api/hermes/history`
//! (hermes-rs `/v1/models`, `/api/sessions` need the bearer key server-side;
//! owner decision D-384-3). They stay 404 like on live.
//!
//! Keep both lists in step with `galion_realtest::console_routes::MAP`
//! (checked by `tests/owner_routes.rs`).

use std::net::SocketAddr;

/// Which loopback service owns a forwarded row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    /// galion-one (`--pulse`, live :8091).
    Pulse,
    /// hermes-rs (`--hermes`, live :8642), unauthenticated paths only.
    Hermes,
}

/// One forwarded row: Node-era console path -> owner path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnerRoute {
    pub node: &'static str,
    pub owner: Owner,
    pub target: &'static str,
}

const fn o(node: &'static str, owner: Owner, target: &'static str) -> OwnerRoute {
    OwnerRoute {
        node,
        owner,
        target,
    }
}

/// Rows forwarded to their Rust owner.
pub const OWNER_ROUTES: &[OwnerRoute] = &[
    o("/api/v1/analytics", Owner::Pulse, "/api/v1/analytics"),
    o("/api/v1/awake", Owner::Pulse, "/api/v1/awake"),
    o("/api/hermes/health", Owner::Hermes, "/health"),
];

/// Node-era rows with no Rust provider (D-384-2 lists them for the owner).
/// `/api/v1/bank` is here too. With `--owner-routes` the default
/// `--data-path` list leaves it out (F-395-3, slot 413), so it answers 410;
/// only an explicit `--data-path /api/v1/bank` (the owner's D-384-2 call)
/// sends it to `--data-upstream` (the data proxy runs before this module).
pub const RETIRED: &[&str] = &[
    "/api/v1/bank",
    "/api/v1/instances",
    "/api/v1/laptop",
    "/api/v1/keys",
    "/api/v1/security",
    "/api/v3/guardian/status",
    "/api/v3/guardian/incidents",
    "/api/mesh",
    "/api/v1/cloudflare",
    "/api/v1/pack",
    "/api/hermes/guard",
    "/api/hermes/modes",
];

/// Headers never sent on to an owner (credentials stay with the console).
pub const DROP_HEADERS: &[&str] = &["cookie", "authorization"];

fn bare(path: &str) -> &str {
    path.split('?').next().unwrap_or(path)
}

/// The forwarded row for `path` (query ignored).
pub fn owner_route(path: &str) -> Option<&'static OwnerRoute> {
    let p = bare(path);
    OWNER_ROUTES.iter().find(|r| r.node == p)
}

/// True for a retired row (query ignored).
pub fn is_retired(path: &str) -> bool {
    RETIRED.contains(&bare(path))
}

/// Owner address for `r`, if that owner is configured.
pub fn addr(
    r: &OwnerRoute,
    pulse: Option<SocketAddr>,
    hermes: Option<SocketAddr>,
) -> Option<SocketAddr> {
    match r.owner {
        Owner::Pulse => pulse,
        Owner::Hermes => hermes,
    }
}

/// Upstream target for a forwarded request: owner path plus the original
/// query string (`query` without the `?`; empty = none).
pub fn target(r: &OwnerRoute, query: &str) -> String {
    if query.is_empty() {
        r.target.to_string()
    } else {
        format!("{}?{query}", r.target)
    }
}

/// Body of the explicit `410 Gone` answer for a retired row.
pub fn retired_body(path: &str) -> String {
    let mut b = String::from(r#"{"error":"retired","route":"#);
    console_ui::json::write_str(&mut b, bare(path));
    b.push_str(
        r#","note":"Node-era console route with no Rust provider (slot 384 MAP; owner decision D-384-2)."}"#,
    );
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_disjoint_and_exact() {
        for r in OWNER_ROUTES {
            assert!(r.node.starts_with("/api/") && r.target.starts_with('/'));
            assert!(!is_retired(r.node), "{}", r.node);
        }
        for (i, a) in RETIRED.iter().enumerate() {
            assert!(a.starts_with("/api/") && !a.contains('?'));
            assert!(!RETIRED[i + 1..].contains(a), "duplicate {a}");
        }
    }

    #[test]
    fn lookup_ignores_query() {
        assert_eq!(
            owner_route("/api/hermes/health?x=1").unwrap().target,
            "/health"
        );
        assert!(
            owner_route("/api/hermes/history").is_none(),
            "D-384-3: not proxied"
        );
        assert!(
            owner_route("/api/hermes/models").is_none(),
            "D-384-3: not proxied"
        );
        assert!(is_retired("/api/mesh?x=1"));
        assert!(!is_retired("/api/v1/health"));
        assert!(!is_retired("/api/v1/markers"));
    }

    #[test]
    fn target_keeps_query() {
        let r = owner_route("/api/v1/analytics").unwrap();
        assert_eq!(target(r, ""), "/api/v1/analytics");
        assert_eq!(target(r, "board=x"), "/api/v1/analytics?board=x");
        let h = owner_route("/api/hermes/health").unwrap();
        assert_eq!(target(h, ""), "/health");
    }

    #[test]
    fn addr_by_owner() {
        let p: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let h: SocketAddr = "127.0.0.1:2".parse().unwrap();
        assert_eq!(
            addr(owner_route("/api/v1/awake").unwrap(), Some(p), Some(h)),
            Some(p)
        );
        assert_eq!(
            addr(owner_route("/api/hermes/health").unwrap(), Some(p), Some(h)),
            Some(h)
        );
        assert_eq!(
            addr(owner_route("/api/hermes/health").unwrap(), Some(p), None),
            None
        );
    }

    #[test]
    fn retired_body_is_json_and_names_route() {
        let b = retired_body("/api/mesh?x=\"1\"");
        let v = console_ui::json::parse(b.as_bytes()).unwrap();
        assert_eq!(v.get("error").as_str(), Some("retired"));
        assert_eq!(v.get("route").as_str(), Some("/api/mesh"));
    }
}
