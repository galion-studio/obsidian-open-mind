// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_guardian-tick");

fn tmp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join("guardian-tick-tests");
    std::fs::create_dir_all(&d).unwrap();
    d.join(name)
}

#[test]
fn fixture_parses_and_bad_file_exits_2() {
    let text = std::fs::read_to_string("tests/fixtures/probes.toml").unwrap();
    let v: toml::Value = toml::from_str(&text).unwrap();
    assert!(v["probe"].as_array().unwrap().len() >= 10);

    let bad = tmp("bad.toml");
    std::fs::write(&bad, "[[probe]]\nkind = \"bogus\"\n").unwrap();
    let o = Command::new(BIN)
        .arg("--probes")
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = Command::new(BIN)
        .args(["--probes", "/nonexistent/probes.toml"])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn mock_listener_ok_and_down() {
    let l = TcpListener::bind("127.0.0.1:18080").unwrap();
    let h = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 512];
            let _ = s.read(&mut b);
            let _ = s.write_all(b"HTTP/1.0 401 Unauthorized\r\n\r\n");
        }
    });
    let p = tmp("mock.toml");
    std::fs::write(
        &p,
        "[[probe]]\nkind=\"http\"\nname=\"mock\"\nport=18080\npath=\"/h\"\nok_codes=[200,401]\n\
         [[probe]]\nkind=\"http\"\nname=\"strict\"\nport=18080\npath=\"/h\"\nok_codes=[200]\n\
         [[probe]]\nkind=\"http\"\nname=\"closed\"\nport=18646\npath=\"/\"\n",
    )
    .unwrap();
    let out = tmp("mock-out/g.json");
    let o = Command::new(BIN)
        .args([
            "--probes",
            p.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--suggest",
        ])
        .output()
        .unwrap();
    h.join().unwrap();
    assert_eq!(o.status.code(), Some(0));
    let so = String::from_utf8_lossy(&o.stdout);
    assert!(
        so.contains("suggest: strict down: operator may restart strict"),
        "{so}"
    );
    assert!(so.contains("suggest: closed down"));
    assert!(!so.contains("suggest: mock"));
    let j: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(j["host_ok"], false);
    let r = j["results"].as_array().unwrap();
    assert_eq!(r.len(), 3);
    assert_eq!(r[0]["ok"], true);
    assert_eq!(r[1]["ok"], false);
    assert_eq!(r[2]["ok"], false);
    let ts = j["ts"].as_str().unwrap();
    assert!(ts.ends_with("+01:00") || ts.ends_with("+02:00"), "{ts}");
}
