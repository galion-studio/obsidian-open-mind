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

#[test]
fn deploy_survival_probes_toml_parses() {
    let rel = "../../deploy/survival/probes.toml";
    let text = std::fs::read_to_string(rel).unwrap_or_else(|e| panic!("{rel}: {e}"));
    let v: toml::Value = toml::from_str(&text).expect("toml");
    let probes = v["probe"].as_array().expect("[[probe]] table");
    assert!(!probes.is_empty(), "zero [[probe]] entries");

    let mut saw_disk = false;
    let mut saw_mem = false;
    let mut saw_load = false;
    let mut saw_cloudflared = false;
    let mut http = Vec::new();
    for p in probes {
        let kind = p["kind"].as_str().expect("kind");
        match kind {
            "http" => http.push(p["name"].as_str().expect("http name")),
            "disk" => saw_disk = true,
            "mem" => saw_mem = true,
            "load" => saw_load = true,
            "cloudflared" => saw_cloudflared = true,
            "unit" | "fresh" => {}
            other => panic!("unknown probe kind {other}"),
        }
    }
    assert!(saw_disk, "missing disk");
    assert!(saw_mem, "missing mem");
    assert!(saw_load, "missing load");
    assert!(saw_cloudflared, "missing cloudflared");
    for name in [
        "hermes", "one", "whisper", "vision", "voice", "console", "origin",
    ] {
        assert!(http.contains(&name), "missing {name}");
    }

    let out = tmp("survival-probes-out.json");
    let o = Command::new(BIN)
        .args(["--probes", rel, "--out", out.to_str().unwrap()])
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
}

fn local_probes(dir: &std::path::Path) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("probes.toml");
    std::fs::write(
        &p,
        "[[probe]]\nkind=\"mem\"\n[[probe]]\nkind=\"load\"\n[[probe]]\nkind=\"cloudflared\"\n",
    )
    .unwrap();
    p
}

fn dir_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn default_run_writes_no_jsonl() {
    let dir = tmp("b25-no-jsonl");
    let _ = std::fs::remove_dir_all(&dir);
    let probes = local_probes(&dir);
    let out = dir.join("report.json");
    let o = Command::new(BIN)
        .args([
            "--probes",
            probes.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .env_remove("GUARDIAN_JSONL")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    assert_eq!(
        dir_names(&dir),
        vec!["probes.toml".to_string(), "report.json".to_string()]
    );
    assert!(!dir.join("report.jsonl").exists());
}

#[test]
fn jsonl_flag_writes_one_line_per_probe() {
    let dir = tmp("b25-jsonl");
    let _ = std::fs::remove_dir_all(&dir);
    let probes = local_probes(&dir);
    let out = dir.join("report.json");
    let jsonl = dir.join("events.jsonl");
    let o = Command::new(BIN)
        .args([
            "--probes",
            probes.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--jsonl",
            jsonl.to_str().unwrap(),
        ])
        .env_remove("GUARDIAN_JSONL")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    let text = std::fs::read_to_string(&jsonl).unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 3, "{text}");
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let results = report["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    for (row, line) in results.iter().zip(lines.iter()) {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(&v["name"], &row["name"]);
        assert_eq!(&v["ok"], &row["ok"]);
        assert_eq!(&v["detail"], &row["detail"]);
        assert_eq!(&v["ts"], &row["ts"]);
        assert_eq!(v.as_object().unwrap().len(), 4, "{v}");
        let ts = v["ts"].as_str().unwrap();
        assert!(ts.ends_with("+01:00") || ts.ends_with("+02:00"), "{ts}");
        assert!(!ts.ends_with('Z'), "{ts}");
        assert!(!ts.contains("+00:00"), "{ts}");
    }
}

#[test]
fn jsonl_env_writes_one_line_per_probe() {
    let dir = tmp("b25-jsonl-env");
    let _ = std::fs::remove_dir_all(&dir);
    let probes = local_probes(&dir);
    let out = dir.join("report.json");
    let jsonl = dir.join("env.jsonl");
    let o = Command::new(BIN)
        .args([
            "--probes",
            probes.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .env("GUARDIAN_JSONL", jsonl.as_os_str())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(0), "{err}");
    let n = std::fs::read_to_string(&jsonl)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .count();
    assert_eq!(n, 3);
}

fn proc_starttime(pid: u32) -> String {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
    let rest = stat.rsplit_once(')').unwrap().1;
    rest.split_whitespace().nth(19).unwrap().to_string()
}

#[test]
fn tick_result_is_report_only() {
    let dir = tmp("report-only");
    let _ = std::fs::remove_dir_all(&dir);
    let probes = local_probes(&dir);
    let out = dir.join("report.json");

    let mut sentinel = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let pid = sentinel.id();
    let started = proc_starttime(pid);

    let o = Command::new(BIN)
        .args([
            "--probes",
            probes.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .env_remove("GUARDIAN_JSONL")
        .env_remove("GUARDIAN_PROBES")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr).into_owned();
    assert_eq!(o.status.code(), Some(0), "{stderr}");

    assert!(
        sentinel.try_wait().unwrap().is_none(),
        "tick killed pid {pid}"
    );
    assert_eq!(proc_starttime(pid), started, "tick restarted pid {pid}");

    let stdout = String::from_utf8_lossy(&o.stdout).into_owned();
    let report = std::fs::read_to_string(&out).unwrap();
    for (label, text) in [
        ("stdout", &stdout),
        ("stderr", &stderr),
        ("report", &report),
    ] {
        assert!(
            !text.contains("systemctl"),
            "{label} contains systemctl: {text}"
        );
        assert!(!text.contains("kill"), "{label} contains kill: {text}");
        assert!(
            !text.contains("restart"),
            "{label} contains restart: {text}"
        );
    }

    let j: serde_json::Value = serde_json::from_str(&report).unwrap();
    let obj = j.as_object().unwrap();
    assert_eq!(obj.len(), 3, "{j}");
    assert!(obj.contains_key("ts"), "{j}");
    assert!(obj.contains_key("host_ok"), "{j}");
    assert!(obj.contains_key("results"), "{j}");
    assert!(j["host_ok"].is_boolean(), "{j}");
    assert!(obj.get("restart").is_none(), "{j}");
    assert!(obj.get("kill").is_none(), "{j}");
    let rows = j["results"].as_array().unwrap();
    assert_eq!(rows.len(), 3, "{j}");
    for row in rows {
        let row = row.as_object().unwrap();
        assert_eq!(row.len(), 4, "{row:?}");
        assert!(row.contains_key("name"), "{row:?}");
        assert!(row.contains_key("ok"), "{row:?}");
        assert!(row.contains_key("detail"), "{row:?}");
        assert!(row.contains_key("ts"), "{row:?}");
        let detail = row["detail"].as_str().unwrap();
        assert!(!detail.contains("systemctl"), "{detail}");
        assert!(!detail.contains("kill"), "{detail}");
        assert!(!detail.contains("restart"), "{detail}");
    }
}
