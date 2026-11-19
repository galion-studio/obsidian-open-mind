// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! The default hermes-gateway build leaves `mcp-http` off. A `url` server
//! is reported at discovery and does not dial. `cfg(test)` is not set on
//! the hermes-rs dependency of this crate, so this is the build that ships.

use hermes_rs::config::McpServerCfg;
use hermes_rs::mcp::discover;

#[test]
fn default_feature_line_does_not_enable_mcp_http() {
    let toml = include_str!("../Cargo.toml");
    let defaults = toml
        .lines()
        .find(|line| line.starts_with("default = "))
        .expect("default features");
    assert!(
        !defaults.contains("mcp-http"),
        "mcp-http must stay out of the default gateway build: {defaults}"
    );
}

#[cfg(not(feature = "mcp-http"))]
#[test]
fn url_server_is_reported_without_dialing() {
    let cfg = McpServerCfg::http("lab", "https://api.githubcopilot.com/mcp/x/all/readonly");
    let (tools, errors) = discover(&[cfg]);
    assert!(tools.is_empty());
    assert_eq!(
        errors,
        vec!["lab: streamable HTTP needs the mcp-http feature".to_string()]
    );
}
