// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use galion_origin::{handle, percent_decode, route, safe_file, Config};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::time::Instant;

fn setup(name: &str) -> Config {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/galion-origin")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("root/docs/build-session/sub")).unwrap();
    std::fs::create_dir_all(d.join("root/state")).unwrap();
    std::fs::create_dir_all(d.join("grinder")).unwrap();
    std::fs::write(
        d.join("root/docs/build-session/00-summary.md"),
        "# Hi <b>\n",
    )
    .unwrap();
    std::fs::write(d.join("root/docs/build-session/sub/x.md"), "x").unwrap();
    std::fs::write(d.join("root/secret.md"), "nope").unwrap();
    std::fs::write(d.join("root/desk-admin.html"), "<p>desk admin</p>").unwrap();
    std::fs::write(d.join("grinder/index.html"), "<h1>grinder</h1>").unwrap();
    std::fs::write(d.join("grinder/grinder-50mm.stl"), "solid x").unwrap();
    Config {
        addr: "127.0.0.1:0".into(),
        root: d.join("root"),
        grinder_root: d.join("grinder"),
        console_url: None,
    }
}

#[test]
fn routes() {
    let mut cfg = setup("routes");
    let t = Instant::now();
    let r = route(&cfg, "GET", "/health", "", t);
    assert_eq!(r.code, 200);
    assert!(String::from_utf8_lossy(&r.body).contains("\"service\":\"galion-origin\""));
    let r = route(&cfg, "GET", "/grinder", "", t);
    assert_eq!(
        (r.code, r.body.as_slice()),
        (200, b"<h1>grinder</h1>".as_slice())
    );
    let r = route(&cfg, "GET", "/grinder/grinder-50mm.stl", "", t);
    assert_eq!(r.ctype, "model/stl");
    assert!(r
        .headers
        .iter()
        .any(|(k, v)| k == "content-disposition" && v.contains("grinder-50mm.stl")));
    assert_eq!(
        route(&cfg, "GET", "/grinder/../root/secret.md", "", t).code,
        404
    );
    assert_eq!(
        route(&cfg, "GET", "/grinder/%2e%2e/root/secret.md", "", t).code,
        404
    );
    let r = route(&cfg, "GET", "/desk", "", t);
    assert_eq!(r.body, b"<p>desk admin</p>");
    let body = String::from_utf8(route(&cfg, "GET", "/desk?n=00-summary.md", "", t).body).unwrap();
    assert!(body.contains("# Hi &lt;b&gt;") && body.contains("sub/x.md"));
    let body = String::from_utf8(route(&cfg, "GET", "/desk?n=..%2Fsecret.md", "", t).body).unwrap();
    assert!(body.contains("Missing.") && !body.contains("nope"));
    let r = route(&cfg, "GET", "/desk/live", "", t);
    assert!(String::from_utf8_lossy(&r.body).contains("\"laptop_port\":false"));
    std::fs::write(cfg.root.join("state/link.json"), "{\"at\":\"x\"}").unwrap();
    assert_eq!(
        route(&cfg, "GET", "/desk/live", "", t).body,
        b"{\"at\":\"x\"}"
    );
    let body = String::from_utf8(route(&cfg, "GET", "/", "evil<script>.com:80", t).body).unwrap();
    assert!(body.contains("evil&lt;script&gt;.com") && !body.contains("<script>"));
    assert_eq!(route(&cfg, "POST", "/desk", "", t).code, 405);
    cfg.console_url = Some("https://console.example/".into());
    let r = route(&cfg, "GET", "/api/v1/health?x=1", "", t);
    assert_eq!(r.code, 302);
    assert!(r
        .headers
        .iter()
        .any(|(k, v)| k == "location" && v == "https://console.example/api/v1/health?x=1"));
    assert!(safe_file(&cfg.root, "../grinder/index.html").is_none());
    assert_eq!(percent_decode("a%20b+c%zz%"), "a b c%zz%");
}

#[test]
fn serves_over_tcp() {
    let cfg = setup("tcp");
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    let h = std::thread::spawn(move || {
        let (s, _) = l.accept().unwrap();
        handle(&cfg, s, Instant::now());
    });
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.write_all(b"GET /health HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut s = String::new();
    c.read_to_string(&mut s).unwrap();
    h.join().unwrap();
    assert!(
        s.starts_with("HTTP/1.0 200 OK\r\n") && s.contains("healthy"),
        "{s}"
    );
}
