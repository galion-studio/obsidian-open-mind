// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Account sign-in: encoders, provider config checks, token parsing, and the
//! whole flow through `Ui::handle` against a local mock OAuth server
//! (start, callback, state/binding/replay/expiry refused, PKCE checked by
//! the mock, code exchange, refresh with rotation, disconnect + revoke,
//! Hermes env file + waiting entries, no token in any page or header).
//! Scratch dirs are left under the system temp dir (never removed).

use super::*;
use crate::connectors::{catalog, Store, PATH};
use crate::port4::HostFiles;
use crate::{Request, Ui, UiConfig};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

fn scratch(tag: &str) -> PathBuf {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "cn-oauth-{tag}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir_all(&d).unwrap();
    d
}

fn write600(p: &Path, s: &str) {
    fs::write(p, s).unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o600)).unwrap();
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn base64_and_pkce_vectors() {
    assert_eq!(b64(b"", false), "");
    assert_eq!(b64(b"f", false), "Zg==");
    assert_eq!(b64(b"fo", false), "Zm8=");
    assert_eq!(b64(b"foobar", false), "Zm9vYmFy");
    assert_eq!(b64(&[0xfb, 0xff], false), "+/8=");
    assert_eq!(b64(&[0xfb, 0xff], true), "-_8");
    // RFC 7636 appendix B.
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let a = random_token().unwrap();
    let b = random_token().unwrap();
    assert_eq!(a.len(), 43);
    assert!(token_shape_ok(&a) && a != b);
    assert!(!token_shape_ok("short") && !token_shape_ok(&format!("{}!", &a[..42])));
}

#[test]
fn urls_and_provider_config_checks() {
    assert!(url_ok("https://accounts.google.com/o/oauth2/v2/auth"));
    assert!(url_ok("http://127.0.0.1:9/token") && url_ok("http://localhost:9/t"));
    assert!(!url_ok("http://example.com/token"));
    assert!(!url_ok("ftp://x") && !url_ok("https://") && !url_ok("https://a b"));

    let d = scratch("cfg");
    let g = provider_of("gmail").unwrap();
    assert_eq!(g.key, "google");
    assert!(matches!(load_provider(&d, g), ProviderState::Missing));
    let f = d.join("google.env");
    fs::write(&f, "CLIENT_ID=abc\nCLIENT_SECRET=s3cret-value\n").unwrap();
    fs::set_permissions(&f, fs::Permissions::from_mode(0o644)).unwrap();
    match load_provider(&d, g) {
        ProviderState::Bad(e) => assert!(e.contains("0600"), "{e}"),
        _ => panic!("0644 accepted"),
    }
    write600(&f, "CLIENT_SECRET=x\n");
    assert!(matches!(load_provider(&d, g), ProviderState::Bad(_)));
    write600(&f, "CLIENT_ID=abc\nTOKEN_URL=http://example.com/t\n");
    assert!(matches!(load_provider(&d, g), ProviderState::Bad(_)));
    write600(
        &f,
        "CLIENT_ID=abc\nREDIRECT_URI=https://console.galion.app/elsewhere\n",
    );
    assert!(matches!(load_provider(&d, g), ProviderState::Bad(_)));
    write600(
        &f,
        "# Google app\nCLIENT_ID=abc.apps.googleusercontent.com\nCLIENT_SECRET=s3cret-value\n",
    );
    let ProviderState::Ready(c) = load_provider(&d, g) else {
        panic!("good config refused")
    };
    assert_eq!(c.redirect_uri, DEFAULT_REDIRECT);
    assert_eq!(c.token_url, "https://oauth2.googleapis.com/token");
    assert!(!format!("{c:?}").contains("s3cret-value"), "Debug redacts");
    // Read-only scopes by default.
    for id in ["gmail", "google-calendar", "google-drive"] {
        let s = c.scopes_for(id);
        assert!(s.contains("readonly"), "{id}: {s}");
        assert!(
            !s.contains("gmail.modify") && !s.contains("gmail.send") && !s.ends_with("/auth/drive")
        );
    }
    let u = authorize_url(&c, "gmail", "STATE", "CHAL");
    for p in [
        "response_type=code",
        "code_challenge=CHAL",
        "code_challenge_method=S256",
        "state=STATE",
        "access_type=offline",
        "redirect_uri=https%3A//console.galion.app/connectors/oauth/callback",
    ] {
        assert!(u.contains(p), "{p} in {u}");
    }
    assert!(!u.contains("s3cret"));
    // Symlinks are refused.
    let d2 = scratch("cfg-link");
    std::os::unix::fs::symlink(&f, d2.join("google.env")).unwrap();
    assert!(matches!(load_provider(&d2, g), ProviderState::Bad(_)));
    // Every featured OAuth connector has a provider and read-only scopes.
    for id in [
        "gmail",
        "google-calendar",
        "google-drive",
        "canva",
        "box",
        "wix",
    ] {
        assert!(provider_of(id).is_some(), "{id}");
        let s = default_scopes(id).unwrap();
        assert!(!s.contains("write") && !s.contains("manage"), "{id}: {s}");
    }
    assert_eq!(token_env("google-calendar"), "GOOGLE_CALENDAR_ACCESS_TOKEN");
    for id in ["github", "stripe", "notion"] {
        assert!(provider_of(id).is_none(), "{id} stays token-based");
    }
}

#[test]
fn token_answers() {
    let t = parse_tokens(
        200,
        br#"{"access_token":"at-1-long-enough","token_type":"Bearer","expires_in":3599,"refresh_token":"rt-1-long-enough","scope":"a b"}"#,
        1000,
    )
    .unwrap();
    assert_eq!(
        (t.access.as_str(), t.refresh.as_deref(), t.expires_at),
        ("at-1-long-enough", Some("rt-1-long-enough"), Some(4599))
    );
    assert!(!format!("{t:?}").contains("at-1-long-enough"));
    let t = parse_tokens(
        200,
        br#"{"access_token":"at-2-long-enough","token_type":"bearer","expires_in":"60"}"#,
        0,
    )
    .unwrap();
    assert_eq!((t.refresh, t.expires_at), (None, Some(60)));
    assert!(parse_tokens(200, br#"{"access_token":"x","token_type":"mac"}"#, 0).is_err());
    let e = parse_tokens(
        400,
        br#"{"error":"invalid_grant","error_description":"secret stuff at-9"}"#,
        0,
    )
    .unwrap_err();
    assert!(e.contains("invalid_grant") && !e.contains("at-9"), "{e}");
    let e = parse_tokens(500, b"<html>at-9</html>", 0).unwrap_err();
    assert!(!e.contains("at-9"));
}

// ---- mock OAuth server ----

#[derive(Default)]
struct Mock {
    /// code -> (code_challenge, connector redirect_uri)
    codes: Vec<(String, String, String)>,
    /// Bodies of every POST (for checks); tokens issued.
    posts: Vec<(String, String)>,
    issued: u32,
    /// Current valid refresh token.
    refresh: String,
    revoked: Vec<String>,
}

fn form_map(s: &str) -> Vec<(String, String)> {
    html::form_pairs(s)
        .into_iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}
fn fget<'a>(m: &'a [(String, String)], k: &str) -> &'a str {
    m.iter()
        .find(|(a, _)| a == k)
        .map(|(_, b)| b.as_str())
        .unwrap_or("")
}

fn mock_server(m: Arc<Mutex<Mock>>) -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            r.read_line(&mut line).unwrap();
            let path = line.split(' ').nth(1).unwrap_or("").to_string();
            let mut len = 0usize;
            let mut auth = String::new();
            loop {
                let mut h = String::new();
                r.read_line(&mut h).unwrap();
                let h = h.trim_end();
                if h.is_empty() {
                    break;
                }
                let (k, v) = h.split_once(':').unwrap();
                if k.eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap();
                }
                if k.eq_ignore_ascii_case("authorization") {
                    auth = v.trim().to_string();
                }
            }
            let mut body = vec![0; len];
            r.read_exact(&mut body).unwrap();
            let body = String::from_utf8(body).unwrap();
            let f = form_map(&body);
            let mut g = m.lock().unwrap();
            g.posts.push((path.clone(), body.clone()));
            let client_ok = (fget(&f, "client_id") == "mock-client"
                && fget(&f, "client_secret") == "mock-secret-xyz")
                || auth == format!("Basic {}", b64(b"mock-client:mock-secret-xyz", false));
            let (code, out) = if !client_ok {
                (401, r#"{"error":"invalid_client"}"#.to_string())
            } else if path == "/token" && fget(&f, "grant_type") == "authorization_code" {
                let c = fget(&f, "code").to_string();
                match g.codes.iter().position(|x| x.0 == c) {
                    Some(i) => {
                        let (_, chal, redir) = g.codes.remove(i);
                        if pkce_challenge(fget(&f, "code_verifier")) != chal {
                            (400, r#"{"error":"invalid_grant","error_description":"PKCE verification failed"}"#.into())
                        } else if fget(&f, "redirect_uri") != redir {
                            (400, r#"{"error":"invalid_grant"}"#.into())
                        } else {
                            g.issued += 1;
                            g.refresh = format!("mock-rt-{}-SECRETREFRESH", g.issued);
                            (
                                200,
                                format!(
                                    r#"{{"access_token":"mock-at-{}-SECRETACCESS","token_type":"Bearer","expires_in":3600,"refresh_token":"{}","scope":"read"}}"#,
                                    g.issued, g.refresh
                                ),
                            )
                        }
                    }
                    None => (400, r#"{"error":"invalid_grant"}"#.into()),
                }
            } else if path == "/token" && fget(&f, "grant_type") == "refresh_token" {
                if fget(&f, "refresh_token") == g.refresh && !g.refresh.is_empty() {
                    g.issued += 1;
                    // Rotating refresh tokens (Box style).
                    g.refresh = format!("mock-rt-{}-SECRETREFRESH", g.issued);
                    (
                        200,
                        format!(
                            r#"{{"access_token":"mock-at-{}-SECRETACCESS","token_type":"Bearer","expires_in":3600,"refresh_token":"{}"}}"#,
                            g.issued, g.refresh
                        ),
                    )
                } else {
                    (400, r#"{"error":"invalid_grant"}"#.into())
                }
            } else if path == "/revoke" {
                let t = fget(&f, "token").to_string();
                if t == g.refresh {
                    g.refresh.clear();
                }
                g.revoked.push(t);
                (200, String::new())
            } else {
                (404, "{}".into())
            };
            drop(g);
            let mut s = s;
            let _ = write!(
                s,
                "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}",
                out.len()
            );
        }
    });
    port
}

struct Env {
    ui: Ui,
    st: Store,
    root: PathBuf,
    hermes_file: PathBuf,
    mock: Arc<Mutex<Mock>>,
}

fn env(tag: &str, providers: &[&str]) -> Env {
    let root = scratch(tag);
    let cn = root.join("connectors");
    let cfg = root.join("oauth-cfg");
    let hdir = root.join("hermes");
    fs::create_dir_all(&cn).unwrap();
    fs::create_dir_all(&cfg).unwrap();
    fs::create_dir_all(hdir.join("mcp.d")).unwrap();
    let hermes_file = hdir.join("mcp-servers.yaml");
    write600(
        &hermes_file,
        "mcp_servers:\n  openmind:\n    url: \"http://127.0.0.1:1/mcp\"\n    transport: \"http\"\n",
    );
    let mock = Arc::new(Mutex::new(Mock::default()));
    let port = mock_server(mock.clone());
    for p in providers {
        let auth = if *p == "canva" { "basic" } else { "post" };
        write600(
            &cfg.join(format!("{p}.env")),
            &format!(
                "CLIENT_ID=mock-client\nCLIENT_SECRET=mock-secret-xyz\nAUTH_URL=http://127.0.0.1:{port}/authorize\n\
TOKEN_URL=http://127.0.0.1:{port}/token\nREVOKE_URL=http://127.0.0.1:{port}/revoke\nTOKEN_AUTH={auth}\n\
REDIRECT_URI=http://127.0.0.1:18080/connectors/oauth/callback\n"
            ),
        );
    }
    let files = HostFiles {
        connectors_dir: Some(cn.clone()),
        connectors_hermes_file: Some(hermes_file.clone()),
        connectors_oauth_dir: Some(cfg.clone()),
        ..HostFiles::default()
    };
    let ui = Ui::new(UiConfig {
        upstream: "127.0.0.1:9".parse().unwrap(),
        pulse: None,
        extra_hosts: vec![],
        desk: None,
        files,
    });
    let st = Store::new(&cn)
        .with_hermes_file(Some(hermes_file.clone()))
        .with_oauth_dir(Some(cfg));
    Env {
        ui,
        st,
        root,
        hermes_file,
        mock,
    }
}

fn req<'a>(
    method: &'a str,
    path: &'a str,
    query: &'a str,
    cookie: Option<&'a str>,
    body: &'a [u8],
) -> Request<'a> {
    Request {
        method,
        path,
        query,
        host: "127.0.0.1:18080",
        cookie,
        origin: None,
        sec_fetch_site: Some("same-origin"),
        forwarded_proto: None,
        peer_loopback: true,
        client_ip: "127.0.0.1".into(),
        content_type: if method == "POST" {
            Some("application/x-www-form-urlencoded")
        } else {
            None
        },
        body,
    }
}

fn cookie_val(r: &crate::Reply, name: &str) -> Option<String> {
    r.headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .find_map(|(_, v)| {
            v.strip_prefix(&format!("{name}="))
                .map(|x| x.split(';').next().unwrap().to_string())
        })
}

/// Everything a browser could see from a reply.
fn visible(r: &crate::Reply) -> String {
    let mut s = String::from_utf8_lossy(&r.body).to_string();
    for (k, v) in &r.headers {
        s.push_str(&format!("\n{k}: {v}"));
    }
    s
}

fn no_secret(s: &str) {
    for bad in ["SECRETACCESS", "SECRETREFRESH", "mock-secret-xyz"] {
        assert!(!s.contains(bad), "{bad} leaked: {s}");
    }
}

impl Env {
    fn page(&self, q: &str) -> (crate::Reply, String) {
        let r = self.ui.handle(&req("GET", PATH, q, None, b""));
        assert_eq!(r.status, 200);
        no_secret(&visible(&r));
        let csrf = cookie_val(&r, "galion_cn_csrf").expect("csrf");
        (r, csrf)
    }
    fn post(&self, csrf: &str, op: &str, id: &str) -> crate::Reply {
        let cookie = format!("galion_cn_csrf={csrf}");
        let body = format!("csrf={csrf}&op={op}&id={id}");
        let r = self
            .ui
            .handle(&req("POST", PATH, "", Some(&cookie), body.as_bytes()));
        no_secret(&visible(&r));
        r
    }
    /// Start; returns (state, challenge, bind cookie, provider URL).
    fn start(&self, id: &str) -> (String, String, String, String) {
        let (_, csrf) = self.page("");
        let r = self.post(&csrf, "oauth-start", id);
        assert_eq!(r.status, 303, "{}", String::from_utf8_lossy(&r.body));
        let to = r.header("Location").unwrap().to_string();
        let bind_hdr = r
            .headers
            .iter()
            .find(|(k, v)| k == "Set-Cookie" && v.starts_with(BIND_COOKIE))
            .unwrap()
            .1
            .clone();
        assert!(
            bind_hdr.contains("HttpOnly")
                && bind_hdr.contains("SameSite=Lax")
                && bind_hdr.contains("Path=/connectors/oauth")
        );
        let bind = cookie_val(&r, BIND_COOKIE).unwrap();
        let q = form_map(to.split_once('?').unwrap().1);
        assert_eq!(fget(&q, "response_type"), "code");
        assert_eq!(fget(&q, "client_id"), "mock-client");
        assert_eq!(fget(&q, "code_challenge_method"), "S256");
        assert_eq!(
            fget(&q, "redirect_uri"),
            "http://127.0.0.1:18080/connectors/oauth/callback"
        );
        assert_eq!(fget(&q, "scope"), default_scopes(id).unwrap());
        assert!(
            !to.contains("mock-secret-xyz"),
            "no client secret in the browser"
        );
        (
            fget(&q, "state").into(),
            fget(&q, "code_challenge").into(),
            bind,
            to,
        )
    }
    /// The provider approves: issue a code bound to the challenge.
    fn approve(&self, chal: &str) -> String {
        let code = format!("code-{}", random_token().unwrap());
        self.mock.lock().unwrap().codes.push((
            code.clone(),
            chal.into(),
            "http://127.0.0.1:18080/connectors/oauth/callback".into(),
        ));
        code
    }
    fn callback(&self, query: &str, bind: Option<&str>) -> crate::Reply {
        let c = bind.map(|b| format!("{BIND_COOKIE}={b}"));
        let r = self
            .ui
            .handle(&req("GET", CALLBACK_PATH, query, c.as_deref(), b""));
        no_secret(&visible(&r));
        r
    }
}

fn row<'a>(page: &'a str, id: &str) -> &'a str {
    let s = page.find(&format!("id=\"c-{id}\"")).unwrap();
    &page[s..s + page[s..].find("</details>").unwrap()]
}

#[test]
fn needs_app_credentials_and_waiting_entries() {
    let e = env("nocreds", &[]);
    assert!(e.st.sync_oauth_entries().unwrap(), "first sync writes");
    assert!(
        !e.st.sync_oauth_entries().unwrap(),
        "second sync is a no-op"
    );
    let (r, _) = e.page("");
    let b = String::from_utf8(r.body).unwrap();
    for id in [
        "gmail",
        "google-calendar",
        "google-drive",
        "canva",
        "box",
        "wix",
    ] {
        let row = row(&b, id);
        assert!(row.contains("Needs app credentials"), "{id}");
        assert!(
            row.contains(DEFAULT_REDIRECT),
            "{id} shows the redirect URI"
        );
        assert!(row.contains("disabled"), "{id}");
    }
    // Token connectors are untouched by the sign-in work.
    assert!(!row(&b, "stripe").contains("Needs app credentials"));
    assert!(!row(&b, "stripe").contains("Sign in with"));
    // Hermes file: the six entries, waiting on their env files, writes off.
    let h = fs::read_to_string(&e.hermes_file).unwrap();
    assert!(h.contains("  openmind:\n"), "hand entries kept");
    for id in [
        "gmail",
        "google-calendar",
        "google-drive",
        "canva",
        "box",
        "wix",
    ] {
        let env_file = e.st.secret_path(id);
        assert!(h.contains(&format!("\n  {id}:\n")), "{id} entry");
        assert!(h.contains(&format!("env_file: \"{}\"", env_file.display())));
        assert!(h.contains(&format!("require_env: [\"{}\"]", token_env(id))));
        assert!(!env_file.exists(), "no token file yet: nerve says waiting");
    }
    assert!(!h.contains("stripe:") && !h.contains("allow_writes: true"));
    assert!(fs::read_dir(e.root.join("hermes"))
        .unwrap()
        .flatten()
        .any(|f| f.file_name().to_string_lossy().contains(".bak-console-")));
}

#[test]
fn full_flow_with_mock_provider() {
    let e = env("flow", &["google", "box", "canva"]);
    e.st.sync_oauth_entries().unwrap();
    let (r, _) = e.page("");
    let csp = r.header("Content-Security-Policy").unwrap().to_string();
    assert!(
        csp.contains("form-action 'self' http://127.0.0.1:"),
        "{csp}"
    );
    let b = String::from_utf8(r.body).unwrap();
    assert!(row(&b, "gmail").contains("Sign in with Google"));
    assert!(
        row(&b, "wix").contains("Needs app credentials"),
        "no wix.env"
    );

    // 1. State mismatch: unknown state, wrong/missing binding, replay.
    let (state, chal, bind, _) = e.start("gmail");
    let code = e.approve(&chal);
    let bogus = random_token().unwrap();
    let r = e.callback(&format!("state={bogus}&code={code}"), Some(&bind));
    assert_eq!(r.status, 400);
    assert!(String::from_utf8_lossy(&r.body).contains("state check failed"));
    assert!(
        !String::from_utf8_lossy(&r.body).contains(&code),
        "code never echoed"
    );
    let other = random_token().unwrap();
    let r = e.callback(&format!("state={state}&code={code}"), Some(&other));
    assert_eq!(r.status, 400, "another browser's binding");
    let r = e.callback(&format!("state={state}&code={code}"), None);
    assert_eq!(r.status, 400, "no binding cookie");
    assert!(
        e.mock.lock().unwrap().posts.is_empty(),
        "nothing exchanged on refusal"
    );

    // 2. Success.
    let r = e.callback(&format!("state={state}&code={code}"), Some(&bind));
    assert_eq!(r.status, 303);
    assert_eq!(
        r.header("Location"),
        Some("/connectors?done=signed-in&id=gmail")
    );
    assert!(
        visible(&r).contains(&format!("{BIND_COOKIE}=;")),
        "binding cleared"
    );
    {
        let m = e.mock.lock().unwrap();
        let (path, body) = &m.posts[0];
        assert_eq!(path, "/token");
        let f = form_map(body);
        assert_eq!(fget(&f, "grant_type"), "authorization_code");
        assert_eq!(
            pkce_challenge(fget(&f, "code_verifier")),
            chal,
            "PKCE verifier matches"
        );
    }
    // 3. Replay of the same state is refused (single use).
    let r = e.callback(&format!("state={state}&code={code}"), Some(&bind));
    assert_eq!(r.status, 400);

    // Stored server-side, 0600; Hermes env holds only the access token.
    let rec = e.st.dir.join("oauth").join("gmail.json");
    assert_eq!(mode(&rec), 0o600);
    assert_eq!(mode(&e.st.dir.join("oauth")), 0o700);
    let envf = e.st.secret_path("gmail");
    assert_eq!(mode(&envf), 0o600);
    let env_text = fs::read_to_string(&envf).unwrap();
    assert!(env_text.contains("GMAIL_ACCESS_TOKEN=mock-at-1-SECRETACCESS"));
    assert!(
        !env_text.contains("REFRESH"),
        "refresh token never handed to Hermes"
    );
    let (r, _) = e.page("done=signed-in&id=gmail");
    let b = String::from_utf8(r.body).unwrap();
    assert!(b.contains("Gmail: signed in."));
    assert!(
        row(&b, "gmail").contains("Signed in, not live yet"),
        "Hermes started before"
    );
    assert!(row(&b, "gmail").contains("Refresh now") && row(&b, "gmail").contains("Disconnect"));

    // 4. Refresh (forced from the screen) rotates both tokens.
    let (_, csrf) = e.page("");
    let r = e.post(&csrf, "oauth-refresh", "gmail");
    assert_eq!(r.status, 303);
    assert!(r.header("Location").unwrap().contains("done=refreshed"));
    let env_text = fs::read_to_string(&envf).unwrap();
    assert!(env_text.contains("mock-at-2-SECRETACCESS") && !env_text.contains("mock-at-1"));
    let rec_text = fs::read_to_string(&rec).unwrap();
    assert!(
        rec_text.contains("mock-rt-2-SECRETREFRESH"),
        "rotated refresh kept"
    );
    // Not due yet: the background tick does nothing.
    assert_eq!(tick(&e.st), 0);
    // Due: the tick refreshes.
    let mut r0 = read_record(&e.st, "gmail").unwrap();
    r0.expires_at = now_unix() + 10;
    write_record(&e.st, "gmail", &r0).unwrap();
    assert_eq!(tick(&e.st), 1);
    assert!(fs::read_to_string(&envf)
        .unwrap()
        .contains("mock-at-3-SECRETACCESS"));

    // 5. A refused refresh (invalid_grant) shows Error + Sign in again.
    e.mock.lock().unwrap().refresh = "something-else".into();
    assert!(refresh(&e.st, "gmail", 0, true).is_err());
    let (r, _) = e.page("");
    let b = String::from_utf8(r.body).unwrap();
    assert!(row(&b, "gmail").contains("cn-err") && row(&b, "gmail").contains("Sign in again"));
    assert!(row(&b, "gmail").contains("invalid_grant"));

    // 6. Sign in again, then disconnect: revoke hit, files token-free + .off.
    let (state, chal, bind, _) = e.start("gmail");
    let code = e.approve(&chal);
    assert_eq!(
        e.callback(&format!("state={state}&code={code}"), Some(&bind))
            .status,
        303
    );
    let rt = e.mock.lock().unwrap().refresh.clone();
    let (_, csrf) = e.page("");
    let r = e.post(&csrf, "disconnect", "gmail");
    assert_eq!(
        r.header("Location"),
        Some("/connectors?done=signed-out&id=gmail")
    );
    assert_eq!(e.mock.lock().unwrap().revoked, vec![rt]);
    assert!(!rec.exists() && !envf.exists());
    let offs: Vec<PathBuf> = [e.st.dir.join("oauth"), envf.parent().unwrap().to_path_buf()]
        .iter()
        .flat_map(|d| fs::read_dir(d).unwrap().flatten().map(|x| x.path()))
        .filter(|p| p.to_string_lossy().contains(".off"))
        .collect();
    assert!(offs.len() >= 2);
    for p in &offs {
        no_secret(&fs::read_to_string(p).unwrap());
    }
    // The Hermes entry stays (waiting), and the row is back to Sign in.
    assert!(fs::read_to_string(&e.hermes_file)
        .unwrap()
        .contains("\n  gmail:\n"));
    let (r, _) = e.page("");
    assert!(row(&String::from_utf8(r.body).unwrap(), "gmail").contains("Sign in with Google"));

    // 7. Box (post auth) and Canva (Basic auth) also sign in.
    for id in ["box", "canva"] {
        let (state, chal, bind, _) = e.start(id);
        let code = e.approve(&chal);
        let r = e.callback(&format!("state={state}&code={code}"), Some(&bind));
        assert_eq!(
            r.header("Location").unwrap(),
            format!("/connectors?done=signed-in&id={id}")
        );
        assert!(fs::read_to_string(e.st.secret_path(id))
            .unwrap()
            .contains(&format!("{}=mock-at-", token_env(id))));
    }

    // 8. The provider says no: Error shown, reason from our own record.
    let (state, _, bind, _) = e.start("google-drive");
    let r = e.callback(&format!("state={state}&error=access_denied"), Some(&bind));
    assert_eq!(
        r.header("Location"),
        Some("/connectors?done=oauth-error&id=google-drive")
    );
    let (r, _) = e.page("done=oauth-error&id=google-drive");
    let b = String::from_utf8(r.body).unwrap();
    assert!(b.contains("the provider refused the sign-in (access_denied)"));

    // 9. A wrong PKCE verifier is refused by the provider -> Error.
    let (state, _, bind, _) = e.start("google-calendar");
    let code = e.approve("not-the-challenge");
    let r = e.callback(&format!("state={state}&code={code}"), Some(&bind));
    assert_eq!(
        r.header("Location"),
        Some("/connectors?done=oauth-error&id=google-calendar")
    );
    assert!(!e.st.secret_path("google-calendar").exists());
}

#[test]
fn expired_sign_in_and_csrf_and_gate() {
    let e = env("expiry", &["google"]);
    let (state, _, bind, _) = e.start("gmail");
    let p =
        e.st.dir
            .join("oauth")
            .join("pending")
            .join(format!("{}.json", sha256::hex(state.as_bytes())));
    let t = fs::read_to_string(&p).unwrap();
    assert!(!t.contains(&bind), "only a hash of the binding is stored");
    let old = now_unix() - PENDING_TTL_S - 5;
    let v = json::parse(t.as_bytes()).unwrap();
    let t2 = t.replace(v.get("created").as_str().unwrap(), &old.to_string());
    write600(&p, &t2);
    let r = e.callback(&format!("state={state}&code=x"), Some(&bind));
    assert_eq!(r.status, 400);
    assert!(String::from_utf8_lossy(&r.body).contains("10 minutes"));
    let used = fs::read_to_string(format!("{}.used", p.display())).unwrap();
    assert!(!used.contains("verifier"), "used marker keeps no verifier");
    // oauth-start without the CSRF cookie is refused.
    let r = e.ui.handle(&req(
        "POST",
        PATH,
        "",
        None,
        b"csrf=x&op=oauth-start&id=gmail",
    ));
    assert_ne!(r.status, 303);
    // A non-owner (not loopback, no admin cookie) cannot reach the callback.
    let mut q = req("GET", CALLBACK_PATH, "state=x&code=y", None, b"");
    q.peer_loopback = false;
    q.client_ip = "203.0.113.9".into();
    q.host = "console.galion.app";
    let r = e.ui.handle(&q);
    // Refused (401 from the gate, or 503 when the admin check is unreachable).
    assert!(matches!(r.status, 401 | 403 | 502 | 503), "{}", r.status);
    assert!(!e.st.dir.join("oauth").join("gmail.json").exists());
    // Token connectors cannot be pushed through the sign-in.
    let (_, csrf) = e.page("");
    let r = e.post(&csrf, "oauth-start", "stripe");
    assert_ne!(r.status, 303);
    let _ = catalog();
}

#[test]
fn off_without_the_flag() {
    let root = scratch("noflag");
    let st = Store::new(&root);
    assert_eq!(status(&st, "gmail"), Status::NotEnabled);
    assert!(!st.sync_oauth_entries().unwrap());
    assert!(!root.join("servers.d").exists());
    let _ = std::io::stdout().flush();
}
