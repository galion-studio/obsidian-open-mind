// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Slot 393 (F-377-1): the real `openmind-nerve` binary of this build prints
//! its build stamp in `--version` / `-V`, in the startup log line
//! (`commit=<short>`) and in `GET /health` (`commit` + `build`). Ungated,
//! loopback only, ephemeral port (no fixed port, so parallel copies are fine).

use openmind_nerve::build_info;
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_openmind-nerve");

#[test]
fn version_and_short_flag_print_build_stamp() {
    for flag in ["--version", "-V"] {
        let out = Command::new(BIN).arg(flag).output().unwrap();
        assert!(out.status.success(), "{flag}");
        let v = String::from_utf8(out.stdout).unwrap();
        assert_eq!(v.trim_end(), build_info::version_line(), "{flag}");
        assert!(v.contains(" (commit "), "{v}");
        assert!(!v.contains('Z'), "{v}");
    }
}

struct Kill(Child);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    let code = out.split_whitespace().nth(1)?.parse().ok()?;
    Some((code, out.split_once("\r\n\r\n")?.1.to_string()))
}

#[test]
fn startup_line_and_health_carry_build_stamp() {
    let dir = std::env::temp_dir().join(format!("nerve-stamp-it-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("data")).unwrap();
    // One unreachable component (port 1 is never listened on as non-root).
    std::fs::write(
        dir.join("components.json"),
        r#"[{"name":"stamp-it-dead","port":1,"path":"/health","gaia":"DEMETER","system":"tuner"}]"#,
    )
    .unwrap();
    // Ephemeral port: bind :0, release, hand it to the nerve; retry on a bind race.
    let mut last = String::new();
    for attempt in 0..5 {
        let port = TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut child = Command::new(BIN)
            .args(["serve", "--listen", &format!("127.0.0.1:{port}")])
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("NERVE_DATA_DIR", dir.join("data"))
            .env("NERVE_COMPONENTS", dir.join("components.json"))
            .env("NERVE_DRY_RUN", "1")
            .env("NERVE_SYSTEMD", "0")
            .env("NERVE_JOURNAL", "0")
            .env("NERVE_GALION_HEALTH_PORT", "0")
            .env("NERVE_INTERVAL", "3600")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = child.stderr.take().unwrap();
        let k = Kill(child);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for l in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx.send(l);
            }
        });
        let t = Instant::now();
        let mut line = None;
        while t.elapsed() < Duration::from_secs(30) {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(l) if l.contains(" listening on ") => {
                    line = Some(l);
                    break;
                }
                Ok(l) => last = l,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break, // nerve exited (bind race) -> retry
            }
        }
        let Some(line) = line else {
            eprintln!("attempt {attempt}: no listening line on {port}: {last}");
            drop(k);
            continue;
        };
        assert!(
            line.ends_with(&format!(" commit={}", build_info::short_commit())),
            "{line}"
        );
        assert!(
            line.contains(&format!("listening on 127.0.0.1:{port}")),
            "{line}"
        );
        let t = Instant::now();
        let h = loop {
            if let Some((200, b)) = get(port, "/health") {
                break b;
            }
            assert!(
                t.elapsed() < Duration::from_secs(30),
                "no /health on {port}"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        let h: Value = serde_json::from_str(&h).unwrap();
        assert_eq!(h["status"], "ok");
        assert_eq!(h["service"], "openmind-nerve");
        assert_eq!(h["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(h["commit"], build_info::COMMIT);
        assert_eq!(h["build"], build_info::json());
        // Old keys stay (consumers: gaia-live `healthy`, deploy health table).
        for k in ["appended", "ts", "system", "score", "cycle"] {
            assert!(h.get(k).is_some(), "/health lost {k}: {h}");
        }
        assert!(!h["build"]["date"].as_str().unwrap().ends_with('Z'));
        drop(k);
        let _ = std::fs::remove_dir_all(&dir);
        return;
    }
    panic!("nerve never came up after 5 attempts: {last}");
}
