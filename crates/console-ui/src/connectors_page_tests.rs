// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The page through `Ui::handle`: owner gate, CSRF, writes, no secret echo,
//! and the live Hermes servers file.

use super::*;
use crate::port4::HostFiles;
use crate::UiConfig;
use std::os::unix::fs::PermissionsExt;

const TOKEN: &str = "test-only-not-a-real-token-0123456789";

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        // Test scratch only (under the system temp dir).
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn tmp(tag: &str) -> Tmp {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "cn-test-{tag}-{}-{}",
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&d).unwrap();
    Tmp(d)
}

/// A logo folder with a fake PNG for every catalog logo (made once).
fn logo_dir() -> PathBuf {
    static D: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    D.get_or_init(|| {
        let d = std::env::temp_dir().join(format!(
            "cn-logo-fixture-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        for c in catalog().connectors {
            if !c.logo.is_empty() {
                fs::write(
                    d.join(format!("{}.png", c.logo)),
                    crate::logos::fake_png(&c.logo),
                )
                .unwrap();
            }
        }
        d
    })
    .clone()
}

fn ui(dir: Option<&Path>) -> Ui {
    Ui::new(UiConfig {
        upstream: "127.0.0.1:9".parse().unwrap(),
        pulse: None,
        extra_hosts: vec![],
        desk: None,
        files: HostFiles {
            connectors_dir: dir.map(Path::to_path_buf),
            connectors_logo_dir: Some(logo_dir()),
            ..HostFiles::default()
        },
    })
}

fn get<'a>(host: &'a str, loopback: bool, query: &'a str) -> Request<'a> {
    Request {
        method: "GET",
        path: PATH,
        query,
        host,
        cookie: None,
        origin: None,
        sec_fetch_site: None,
        forwarded_proto: None,
        peer_loopback: loopback,
        client_ip: if loopback {
            "127.0.0.1".into()
        } else {
            "203.0.113.9".into()
        },
        content_type: None,
        body: b"",
    }
}

fn post<'a>(cookie: Option<&'a str>, body: &'a [u8]) -> Request<'a> {
    Request {
        method: "POST",
        cookie,
        sec_fetch_site: Some("same-origin"),
        content_type: Some("application/x-www-form-urlencoded"),
        body,
        ..get("127.0.0.1:18080", true, "")
    }
}

fn csrf_of(r: &Reply) -> String {
    let c = r.header("Set-Cookie").expect("csrf cookie");
    assert!(c.contains("HttpOnly") && c.contains("SameSite=Strict"));
    c.split(';')
        .next()
        .unwrap()
        .split_once('=')
        .unwrap()
        .1
        .to_string()
}

fn body(r: &Reply) -> String {
    String::from_utf8(r.body.clone()).unwrap()
}

fn walk(d: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn renders_sections_for_the_owner_and_is_in_the_menu() {
    let t = tmp("render");
    let ui = ui(Some(&t.0));
    let r = ui.handle(&get("127.0.0.1:18080", true, ""));
    assert_eq!(r.status, 200);
    let b = body(&r);
    for s in [
        "Connectors let Hermes use external tools and data sources.",
        "placeholder=\"Search connectors...\"",
        "href=\"/console\" aria-label=\"Back",
        "Featured",
        "Productivity",
        "Finance",
        "Developer",
        "Microsoft 365",
        "Social Media",
        "Advertising",
        "Custom Connector",
        "Add your own custom connector",
        "GitHub",
        "Gmail",
        "Needs sign-in setup",
        "/_ui/connectors.",
        "data-theme=\"dark\"",
    ] {
        assert!(b.contains(s), "missing {s}");
    }
    // Section order follows the reference screenshots.
    let at = |s: &str| b.find(&format!("<h2 class=\"cn-lab\">{s}</h2>")).unwrap();
    assert!(at("Connected") < at("Featured") && at("Featured") < at("Productivity"));
    assert!(at("Finance") < at("Developer") && at("Advertising") < at("Custom Connector"));
    // Only really wired connectors are under Connected: OpenMind (built
    // in), not GitHub (nothing saved, client not live).
    let connected = &b[at("Connected")..at("Featured")];
    assert!(connected.contains("OpenMind") && !connected.contains("GitHub"));
    assert!(b[at("Developer")..].contains("GitHub"));
    // Real logos, served locally (never hotlinked).
    assert!(b.matches("<img ").count() >= 40);
    for src in b.split("src=\"").skip(1) {
        assert!(src.starts_with(crate::logos::PREFIX), "local logo only");
    }
    let src = b.split("src=\"").nth(1).unwrap();
    let src = &src[..src.find('"').unwrap()];
    let img = ui.handle(&Request {
        path: src,
        ..get("127.0.0.1:18080", true, "")
    });
    assert_eq!(img.status, 200);
    // Scope (t456u/t457u): Featured and GitHub connect; the rest say
    // "Coming soon" instead of Connect.
    let row_of = |id: &str| {
        let s = b.find(&format!("id=\"c-{id}\"")).unwrap();
        &b[s..s + b[s..].find("</summary>").unwrap()]
    };
    for id in ["gmail", "stripe", "wix", "github"] {
        assert!(
            row_of(id).contains(">Connect<") && !row_of(id).contains("Coming soon"),
            "{id}"
        );
    }
    for id in ["linear", "asana", "coinbase", "vercel", "hubspot", "x-ads"] {
        assert!(
            row_of(id).contains("Coming soon") && !row_of(id).contains(">Connect<"),
            "{id}"
        );
    }
    // Search filters on the server.
    let s = body(&ui.handle(&get("127.0.0.1:18080", true, "q=goo")));
    assert!(
        s.contains("Google Drive") && !s.contains(">Stripe<") && !s.contains("Custom Connector")
    );
    let s = body(&ui.handle(&get("127.0.0.1:18080", true, "q=zzzz")));
    assert!(s.contains("No connectors match."));
    assert!(!b.contains("<script"), "no script on /connectors");
    // The three-dash console menu lists the room.
    let m = ui.handle(&Request {
        path: "/console",
        ..get("127.0.0.1:18080", true, "")
    });
    assert!(body(&m).contains("href=\"/connectors\""));
    // Its stylesheet is served.
    let href = b.split("/_ui/connectors.").nth(1).unwrap();
    let href = format!("/_ui/connectors.{}", &href[..href.find('"').unwrap()]);
    let css = ui.handle(&Request {
        path: &href,
        ..get("127.0.0.1:18080", true, "")
    });
    assert_eq!(css.status, 200);
}

#[test]
fn refused_without_owner_auth() {
    let t = tmp("gate");
    // Public console host, no owner cookie, admin API unreachable: fail
    // closed. (The "not the owner" answer -> 401 is covered end to end in
    // console-static tests/ui.rs with its fake admin API.)
    let ui = ui(Some(&t.0));
    let r = ui.handle(&get("console.galion.app", false, ""));
    assert!(r.status >= 400, "{}", r.status);
    assert!(!body(&r).contains("Add your own custom connector"));
    let b = format!("op=connect&id=github&token={TOKEN}");
    let mut p = post(None, b.as_bytes());
    p.host = "console.galion.app";
    p.peer_loopback = false;
    assert!(ui.handle(&p).status >= 400);
    assert!(!t.0.join(SERVERS_FILE).exists());
}

#[test]
fn csrf_and_same_origin_are_required() {
    let t = tmp("csrf");
    let ui = ui(Some(&t.0));
    let tok = csrf_of(&ui.handle(&get("127.0.0.1:18080", true, "")));
    let cookie = format!("{CSRF_COOKIE}={tok}");
    let b = format!("csrf=wrong{}&op=connect&id=github&token={TOKEN}", &tok[5..]);
    assert_eq!(ui.handle(&post(Some(&cookie), b.as_bytes())).status, 403);
    let b = format!("op=connect&id=github&token={TOKEN}");
    assert_eq!(ui.handle(&post(Some(&cookie), b.as_bytes())).status, 403);
    let b = format!("csrf={tok}&op=connect&id=github&token={TOKEN}");
    let mut cross = post(Some(&cookie), b.as_bytes());
    cross.sec_fetch_site = Some("cross-site");
    assert_eq!(ui.handle(&cross).status, 403);
    assert!(!t.0.join("servers.d/github.yaml").exists());
}

#[test]
fn connect_writes_config_and_0600_secret_and_never_echoes_it() {
    let t = tmp("write");
    let ui = ui(Some(&t.0));
    let tok = csrf_of(&ui.handle(&get("127.0.0.1:18080", true, "")));
    let cookie = format!("{CSRF_COOKIE}={tok}");
    let b = format!("csrf={tok}&op=connect&id=github&token={TOKEN}");
    let r = ui.handle(&post(Some(&cookie), b.as_bytes()));
    assert_eq!(r.status, 303);
    let loc = r.header("Location").unwrap().to_string();
    assert!(!loc.contains(TOKEN) && !body(&r).contains(TOKEN));
    // The follow-up page: "saved", connected, no token.
    let (path, q) = loc.split_once('?').unwrap();
    assert_eq!(path, PATH);
    let mut g = get("127.0.0.1:18080", true, q);
    g.cookie = Some(&cookie);
    let page = ui.handle(&g);
    let pb = body(&page);
    assert!(pb.contains("GitHub: saved"));
    assert!(pb.contains("Saved, not live yet"));
    // Saved is not Connected until Hermes loads the file.
    let c0 = pb.find("<h2 class=\"cn-lab\">Connected</h2>").unwrap();
    let sec = pb.find("<h2 class=\"cn-lab\">Featured</h2>").unwrap();
    assert!(!pb[c0..sec].contains("GitHub"));
    assert!(!pb.contains(TOKEN));
    for (_, v) in &page.headers {
        assert!(!v.contains(TOKEN));
    }
    // Files: entry, 0600 secret, servers file.
    let secret = t.0.join("secrets/github.env");
    let mode = fs::metadata(&secret).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    assert!(fs::read_to_string(&secret).unwrap().contains(TOKEN));
    let servers = fs::read_to_string(t.0.join(SERVERS_FILE)).unwrap();
    assert_eq!(
        fs::metadata(t.0.join(SERVERS_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(servers.contains("\nmcp_servers:\n  github:\n"));
    assert!(servers.contains("url: \"https://api.githubcopilot.com/mcp/\""));
    assert!(servers.contains("Authorization: \"Bearer ${GITHUB_PERSONAL_ACCESS_TOKEN}\""));
    assert!(servers.contains("allow_writes: false"));
    // Only the secrets file holds the token.
    let mut all = Vec::new();
    walk(&t.0, &mut all);
    for p in all {
        if p != secret {
            let s = fs::read(&p).unwrap();
            assert!(
                !String::from_utf8_lossy(&s).contains(TOKEN),
                "token leaked into {}",
                p.display()
            );
        }
    }
    assert_eq!(
        fs::metadata(t.0.join("secrets"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn disconnect_renames_off_and_custom_add_validates() {
    let t = tmp("off");
    let ui = ui(Some(&t.0));
    let tok = csrf_of(&ui.handle(&get("127.0.0.1:18080", true, "")));
    let cookie = format!("{CSRF_COOKIE}={tok}");
    let send = |b: String| ui.handle(&post(Some(&cookie), b.as_bytes()));
    // Custom: http refused, unknown command dir refused.
    let r = send(format!(
        "csrf={tok}&op=add&name=Echo&target=http%3A%2F%2Fexample.com%2Fmcp&auth=none"
    ));
    assert_eq!(r.status, 400);
    let r = send(format!(
        "csrf={tok}&op=add&name=Echo&target=%2Fusr%2Fbin%2Fenv+id&auth=none"
    ));
    assert_eq!(r.status, 400);
    // Custom https, no auth.
    let r = send(format!(
        "csrf={tok}&op=add&name=Echo+Test&target=https%3A%2F%2Fexample.com%2Fmcp&auth=none"
    ));
    assert_eq!(r.status, 303, "{}", body(&r));
    let s = fs::read_to_string(t.0.join(SERVERS_FILE)).unwrap();
    assert!(s.contains("  echo-test:\n") && s.contains("allow_writes: false"));
    let page = ui.handle(&Request {
        cookie: Some(&cookie),
        ..get("127.0.0.1:18080", true, "")
    });
    assert!(body(&page).contains("Echo Test"));
    // Same name again: refused.
    let r = send(format!(
        "csrf={tok}&op=add&name=Echo+Test&target=https%3A%2F%2Fexample.com%2Fmcp&auth=none"
    ));
    assert_eq!(r.status, 400);
    // Disconnect: renamed .off, file regenerated empty.
    let r = send(format!("csrf={tok}&op=disconnect&id=echo-test"));
    assert_eq!(r.status, 303);
    assert!(t.0.join("servers.d/echo-test.yaml.off").exists());
    assert!(!t.0.join("servers.d/echo-test.yaml").exists());
    let s = fs::read_to_string(t.0.join(SERVERS_FILE)).unwrap();
    assert!(s.trim_end().ends_with("\nmcp_servers: {}"));
    // OAuth-only, unverified and unknown actions are refused, nothing faked.
    assert_eq!(
        send(format!("csrf={tok}&op=connect&id=coinbase")).status,
        400
    );
    let b = format!("csrf={tok}&op=connect&id=linear&token={TOKEN}");
    assert_eq!(send(b).status, 400, "not featured: coming soon");
    assert!(!t.0.join("servers.d/linear.yaml").exists());
    assert_eq!(send(format!("csrf={tok}&op=connect&id=gmail")).status, 400);
    assert!(!t.0.join("servers.d/gmail.yaml").exists());
    assert_eq!(send(format!("csrf={tok}&op=restart")).status, 400);
}

#[test]
fn without_a_dir_the_screen_is_read_only() {
    let ui = ui(None);
    let r = ui.handle(&get("127.0.0.1:18080", true, ""));
    assert_eq!(r.status, 200);
    let b = body(&r);
    assert!(b.contains("read-only") && !b.contains("name=\"target\""));
}

const HERMES_YAML: &str = r#"# Hermes MCP servers (hand-written header)
log_level: info
mcp_servers:
  openmind:
    command: openmind-mcp-stack
    class: read
  github:
    command: github-mcp-local
    env_file: @/mcp.d/github.env
    require_env: ["GITHUB_PERSONAL_ACCESS_TOKEN"]
    class: read
    allow_writes: false
    lazy: true
"#;

/// A fake /etc/hermes-rs: servers file (mode 0644) plus an mcp.d folder.
fn hermes_dir(t: &Tmp) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let h = t.0.join("hermes");
    fs::create_dir_all(h.join("mcp.d")).unwrap();
    fs::set_permissions(h.join("mcp.d"), fs::Permissions::from_mode(0o711)).unwrap();
    let f = h.join("mcp-servers.yaml");
    fs::write(&f, HERMES_YAML.replace('@', &h.display().to_string())).unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    f
}

fn by_id(id: &str) -> Connector {
    catalog()
        .connectors
        .into_iter()
        .find(|c| c.id == id)
        .unwrap()
}

#[test]
fn hand_entries_parse_names_env_file_and_vars() {
    let h = hand_entries(&HERMES_YAML.replace('@', "/h"));
    let names: Vec<&str> = h.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["openmind", "github"]);
    assert_eq!(h[1].env_file.as_deref(), Some("/h/mcp.d/github.env"));
    assert_eq!(h[1].require_env, ["GITHUB_PERSONAL_ACCESS_TOKEN"]);
    assert!(h[0].require_env.is_empty());
}

#[test]
fn live_file_github_token_only_touches_its_env_file() {
    use std::os::unix::fs::PermissionsExt;
    let t = tmp("hermes-gh");
    let f = hermes_dir(&t);
    let before = fs::read_to_string(&f).unwrap();
    let mut s = Store::new(t.0.join("console")).with_hermes_file(Some(f.clone()));
    s.hermes_started = None;
    let gh = by_id("github");
    assert!(s.hand_configured("github"));
    assert_eq!(s.state(&gh), State::Available, "no token yet: Connect");
    let secret = "test-token-not-real-123";
    s.connect(&gh, Some(secret)).unwrap();
    assert_eq!(fs::read_to_string(&f).unwrap(), before, "YAML untouched");
    let env = f.parent().unwrap().join("mcp.d/github.env");
    let m = fs::metadata(&env).unwrap().permissions().mode() & 0o777;
    assert_eq!(m, 0o600);
    assert!(fs::read_to_string(&env)
        .unwrap()
        .contains("GITHUB_PERSONAL_ACCESS_TOKEN=test-token"));
    let d = fs::metadata(f.parent().unwrap().join("mcp.d")).unwrap();
    assert_eq!(
        d.permissions().mode() & 0o777,
        0o711,
        "existing dir mode kept"
    );
    assert_eq!(s.state(&gh), State::Saved, "Hermes not seen: not live yet");
    s.hermes_started = Some(u64::MAX);
    assert_eq!(s.state(&gh), State::Connected, "token older than Hermes");
    s.hermes_started = Some(0);
    assert_eq!(s.state(&gh), State::Saved, "token newer than Hermes");
    // Replace keeps the old token as .bak; disconnect renames to .off.
    s.connect(&gh, Some(secret)).unwrap();
    assert!(env.with_extension("env.bak").exists());
    s.disconnect("github").unwrap();
    assert!(!env.exists() && env.with_extension("env.off").exists());
    assert_eq!(s.state(&gh), State::Available);
    assert_eq!(fs::read_to_string(&f).unwrap(), before);
}

#[test]
fn live_file_new_entry_goes_to_marked_block_and_back_out() {
    use std::os::unix::fs::PermissionsExt;
    let t = tmp("hermes-block");
    let f = hermes_dir(&t);
    let before = fs::read_to_string(&f).unwrap();
    let s = Store::new(t.0.join("console")).with_hermes_file(Some(f.clone()));
    let stripe = by_id("stripe");
    assert_eq!(s.state(&stripe), State::Available);
    s.connect(&stripe, Some("test-stripe-not-real-456"))
        .unwrap();
    let after = fs::read_to_string(&f).unwrap();
    assert!(
        after.starts_with(&before),
        "hand-written part kept byte for byte"
    );
    assert!(after.contains(BLOCK_BEGIN) && after.contains(BLOCK_END));
    assert!(after.contains("  stripe:\n") && after.contains("allow_writes: false"));
    assert!(
        !after.contains("test-stripe-not-real"),
        "no token in the YAML"
    );
    let names: Vec<String> = hand_entries(&after).into_iter().map(|h| h.name).collect();
    assert_eq!(
        names,
        ["openmind", "github"],
        "block entries are not hand-written"
    );
    assert_eq!(
        fs::metadata(&f).unwrap().permissions().mode() & 0o777,
        0o644
    );
    let baks = fs::read_dir(f.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(".bak-console-"))
        .count();
    assert_eq!(baks, 1, "old file copied aside first");
    assert!(f.parent().unwrap().join("mcp.d/stripe.env").exists());
    assert_eq!(s.state(&stripe), State::Saved);
    // Notion joins the same block; the block is rebuilt, not repeated.
    s.connect(&by_id("notion"), Some("test-notion-not-real-789"))
        .unwrap();
    let two = fs::read_to_string(&f).unwrap();
    assert_eq!(two.matches(BLOCK_BEGIN).count(), 1);
    assert!(two.contains("  notion:\n") && two.contains("  stripe:\n"));
    s.disconnect("stripe").unwrap();
    s.disconnect("notion").unwrap();
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        before,
        "back to the hand-written file"
    );
    assert!(f.parent().unwrap().join("mcp.d/stripe.env.off").exists());
    // Hermes-file names cannot be shadowed by a custom entry.
    assert!(splice_hermes_file(&f, "  github:\n    url: https://x.example/mcp\n").is_err());
}

#[test]
fn splice_refuses_when_mcp_servers_is_not_last() {
    let t = tmp("hermes-order");
    let f = t.0.join("m.yaml");
    fs::write(&f, "mcp_servers:\n  a:\n    command: x\nother: 1\n").unwrap();
    let before = fs::read_to_string(&f).unwrap();
    assert!(splice_hermes_file(&f, "  b:\n    command: y\n").is_err());
    assert_eq!(fs::read_to_string(&f).unwrap(), before);
}
