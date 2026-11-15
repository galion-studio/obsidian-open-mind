// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Unit tests for `connectors.rs` (catalog, store, YAML).

use super::*;

#[test]
fn shipped_catalog_parses_with_sections() {
    let c = catalog();
    let ids: Vec<&str> = c.connectors.iter().map(|x| x.id.as_str()).collect();
    // Mate's reference order (steering 2026-10-30 12:58 CET).
    let order = |sec: &str| -> Vec<&str> {
        c.connectors
            .iter()
            .filter(|x| x.section == sec)
            .map(|x| x.name.as_str())
            .collect()
    };
    assert_eq!(
        order("featured"),
        [
            "Gmail",
            "Google Calendar",
            "Google Drive",
            "Canva",
            "Stripe",
            "Notion",
            "Box",
            "Wix"
        ]
    );
    assert_eq!(order("productivity").len(), 11);
    assert_eq!(order("productivity")[0], "Asana");
    assert_eq!(order("finance").len(), 11);
    assert_eq!(order("finance")[10], "etoro");
    assert_eq!(
        order("developer"),
        [
            "GitHub",
            "Datadog",
            "Excalidraw",
            "Mixpanel",
            "Netlify",
            "Vercel"
        ]
    );
    assert_eq!(
        order("microsoft"),
        ["Outlook", "Outlook Calendar", "Microsoft Teams"]
    );
    assert_eq!(order("sales"), ["HubSpot", "Meltwater"]);
    assert_eq!(order("social"), ["Buffer"]);
    assert_eq!(order("analytics"), ["Google Cloud BigQuery"]);
    assert_eq!(order("advertising"), ["X Ads"]);
    let titles: Vec<&str> = c.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(
        &titles[..9],
        [
            "Featured",
            "Productivity",
            "Finance",
            "Developer",
            "Microsoft 365",
            "Sales",
            "Social Media",
            "Analytics",
            "Advertising"
        ]
    );
    assert!(ids.contains(&"openmind"));
    for x in &c.connectors {
        if x.verified {
            assert!(x.source.starts_with("https://") || x.auth == Auth::Builtin);
            assert!(x.url.is_some() || x.command.is_some());
        } else {
            assert_eq!(x.auth, Auth::Unverified, "{}", x.id);
            assert!(x.url.is_none() && x.command.is_none());
        }
        assert!(!x.initials.is_empty() && x.initials.len() <= 3);
    }
    let oauth: Vec<&str> = c
        .connectors
        .iter()
        .filter(|x| x.auth == Auth::OAuth)
        .map(|x| x.id.as_str())
        .collect();
    for g in ["gmail", "google-calendar", "google-drive"] {
        assert!(oauth.contains(&g), "{g} is OAuth-only");
    }
}

#[test]
fn catalog_rejects_bad_entries() {
    let base = |c: &str| format!(r#"{{"sections":[{{"id":"p","title":"P"}}],"connectors":[{c}]}}"#);
    assert!(parse_catalog(&base(
        r#"{"id":"x","section":"p","auth":"none","url":"http://x"}"#
    ))
    .is_err());
    assert!(parse_catalog(&base(
        r#"{"id":"x","section":"q","auth":"none","url":"https://x"}"#
    ))
    .is_err());
    assert!(parse_catalog(&base(
        r#"{"id":"x","section":"p","auth":"token","url":"https://x"}"#
    ))
    .is_err());
    assert!(parse_catalog(&base(
        r#"{"id":"x","section":"p","auth":"none","command":"/usr/lib/x-mcp","verified":true,"source":"https://s"}"#
    ))
    .is_err());
    assert!(
        parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"none","url":"https://x/mcp"}"#
        ))
        .is_err(),
        "verified entries need a source"
    );
    assert!(
        parse_catalog(&base(
            r#"{"id":"x","section":"p","auth":"unverified","url":"https://x/mcp"}"#
        ))
        .is_err(),
        "unverified entries carry no endpoint"
    );
    assert!(parse_catalog(&base(
        r#"{"id":"x","section":"p","auth":"unverified","verified":true}"#
    ))
    .is_err());
    assert!(parse_catalog(&base(r#"{"id":"x","section":"p","auth":"unverified"}"#)).is_ok());
    assert!(parse_catalog(&base(
        r#"{"id":"x","section":"p","auth":"none","url":"https://x/mcp","verified":true,"source":"https://s"}"#
    ))
    .is_ok());
}

#[test]
fn validation() {
    assert!(check_url("https://mcp.stripe.com").is_ok());
    for bad in [
        "http://x",
        "file:///etc/passwd",
        "https://",
        "https://u:p@x",
        "https://x/\"y",
        "https://x/${A}",
        "https://x #c",
        "javascript:alert(1)",
    ] {
        assert!(check_url(bad).is_err(), "{bad}");
    }
    let dir = Some("/srv/mcp-bin/");
    assert!(check_command("/srv/mcp-bin/openmind-mcp-stack", dir).is_ok());
    assert!(check_command("/srv/mcp-bin/openmind-mcp-stack", None).is_err());
    for bad in [
        "/usr/bin/env",
        "/srv/mcp-bin/console-serve",
        "/srv/mcp-bin/../../../usr/bin/mcp",
        "/srv/mcp-bin/sub/mcp",
        "npx",
    ] {
        assert!(check_command(bad, dir).is_err(), "{bad}");
    }
    assert!(check_args(&["--vault".into(), "/a/b".into()]).is_ok());
    assert!(check_args(&["$(id)".into()]).is_err());
    assert!(check_token("short").is_err());
    assert!(check_token("has space in it").is_err());
    assert!(check_token("abc_defghijklmnop1").is_ok());
    assert_eq!(slug("  My Server 2 "), "my-server-2");
    assert!(id_ok("my-server") && !id_ok("1x") && !id_ok("A"));
    assert!(csrf_ok(&new_csrf().unwrap()));
    assert!(ct_eq("ab", "ab") && !ct_eq("ab", "ac") && !ct_eq("a", "ab"));
}

#[test]
fn entry_yaml_has_no_secret_and_writes_off() {
    let c = catalog();
    let gh = c.connectors.iter().find(|x| x.id == "github").unwrap();
    let y = entry_yaml(gh, Some(Path::new("/d/secrets/github.env")));
    assert!(y.starts_with("  github:\n"));
    assert!(y.contains("url: \"https://api.githubcopilot.com/mcp/\""));
    assert!(y.contains("headers: {Authorization: \"Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}\"}"));
    assert!(y.contains("allow_writes: false") && y.contains("class: \"read\""));
    assert!(y.contains("env_file: \"/d/secrets/github.env\""));
}
