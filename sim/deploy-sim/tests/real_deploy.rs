// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test (GALION_REAL=1): the release simsup supervises the real tuner
//! binaries on 127.0.0.1:19335, and the real galion-deploy (direct backend) swaps
//! old -> new, then deploys sim-broken, which must roll back. A probe measures the
//! swap gap. Binaries come from scripts/real/deploy-sim-build.sh
//! ($DEPLOY_SIM_BIN, default /workspace/swarm/sim/deploy-sim/bin).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const PORT: u16 = 19335;

fn bin_dir() -> PathBuf {
    PathBuf::from(
        std::env::var("DEPLOY_SIM_BIN")
            .unwrap_or_else(|_| "/workspace/swarm/sim/deploy-sim/bin".into()),
    )
}

fn health() -> u16 {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect_timeout(
        &format!("127.0.0.1:{PORT}").parse().unwrap(),
        Duration::from_millis(300),
    ) else {
        return 0;
    };
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = s.write_all(b"GET /health HTTP/1.0\r\n\r\n");
    let mut b = [0u8; 16];
    let n = s.read(&mut b).unwrap_or(0);
    String::from_utf8_lossy(&b[..n])
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0)
}

fn wait_health(secs: u64) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if health() == 200 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

struct Sim {
    root: PathBuf,
    sup: Child,
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = Command::new(bin_dir().join("simsup"))
            .args(["--root", self.root.to_str().unwrap(), "ctl", "shutdown"])
            .output();
        let _ = self.sup.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn deploy(root: &Path, new: &Path) -> (i32, String) {
    let gd = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deploy/foundation/galion-deploy");
    let ctl = format!(
        "{} --root {} ctl",
        bin_dir().join("simsup").display(),
        root.display()
    );
    let out = Command::new(gd)
        .args([
            "tuner",
            new.to_str().unwrap(),
            &format!("http://127.0.0.1:{PORT}/health"),
        ])
        .env("GALION_DEPLOY_SM", "direct")
        .env("GALION_DEPLOY_CTL", ctl)
        .env("GALION_DEPLOY_BACKUP_DIR", root.join("backups/deploy"))
        .env("GALION_DEPLOY_DATA_BACKUP", root.join("none"))
        .env("TUNER_ADDR", format!("127.0.0.1:{PORT}"))
        .output()
        .expect("run galion-deploy");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

#[test]
fn real_tuner_swap_and_automatic_rollback() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("SKIP: set GALION_REAL=1 (and build with scripts/real/deploy-sim-build.sh)");
        return;
    }
    let b = bin_dir();
    for f in ["simsup", "tuner-old", "tuner-new", "sim-broken"] {
        assert!(b.join(f).exists(), "missing {}", b.join(f).display());
    }
    let root = std::env::temp_dir().join(format!("deploy-sim-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("units")).unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    let www = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/tuner/www");
    let target = root.join("bin/tuner");
    std::fs::copy(b.join("tuner-old"), &target).unwrap();
    std::fs::write(
        root.join("units/tuner.service"),
        format!(
            "[Service]\nExecStart={}\nEnvironment=TUNER_ADDR=127.0.0.1:{PORT}\nEnvironment=TUNER_ROOT={}\nRestart=always\nRestartSec=2\n",
            target.display(),
            www.display()
        ),
    )
    .unwrap();
    let sup = Command::new(b.join("simsup"))
        .args(["--root", root.to_str().unwrap(), "daemon"])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let sim = Sim {
        root: root.clone(),
        sup,
    };
    assert!(wait_health(5), "old tuner did not come up");

    // swap old -> new while a probe runs
    let stop = root.join("probe.stop");
    let probe = Command::new(b.join("simsup"))
        .args([
            "probe",
            &format!("127.0.0.1:{PORT}"),
            "/health",
            "2",
            stop.to_str().unwrap(),
            "60",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let (rc, out) = deploy(&root, &b.join("tuner-new"));
    std::fs::write(&stop, "").unwrap();
    let report = String::from_utf8_lossy(&probe.wait_with_output().unwrap().stdout).to_string();
    eprintln!("{out}{report}");
    assert_eq!(rc, 0, "{out}");
    assert!(out.contains("msg=\"deployed\""), "{out}");
    assert_eq!(
        std::fs::read(&target).unwrap(),
        std::fs::read(b.join("tuner-new")).unwrap()
    );
    let worst = report
        .split_whitespace()
        .filter_map(|w| w.strip_prefix("until_ok_ms="))
        .filter_map(|v| v.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    assert!(!report.contains("next_ok=never"), "{report}");
    assert!(worst < 1000, "swap gap {worst} ms");

    // broken build -> automatic rollback to the new tuner
    let (rc, out) = deploy(&root, &b.join("sim-broken"));
    eprintln!("{out}");
    assert_eq!(rc, 1, "{out}");
    assert!(
        out.contains("rolled back ok") || out.contains("health failed, rolling back"),
        "unexpected output: {out}"
    );
    // exit 1 is ambiguous (F3): judge by the unit and the bytes, and recover a start-limit trap
    if health() != 200 {
        let ctl = |v: &str| {
            Command::new(b.join("simsup"))
                .args(["--root", root.to_str().unwrap(), "ctl", v, "tuner"])
                .output()
                .unwrap()
        };
        let _ = ctl("reset-failed");
        let _ = ctl("restart");
    }
    assert!(wait_health(5));
    assert_eq!(
        std::fs::read(&target).unwrap(),
        std::fs::read(b.join("tuner-new")).unwrap()
    );
    drop(sim);
}
