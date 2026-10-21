// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! Real-binary auth test (Gaia S4.1, slot 205), gated by `GALION_REAL=1`.
//! Runs the release `openmind-queue` (from `POSEIDON_BIN`, default
//! `$CARGO_TARGET_DIR/release/openmind-queue`) on the box only:
//!
//! * refusals: a LAN bind without `--token-file`, with a 0644 token file,
//!   and `--token-on-loopback` without a file all exit non-zero, bind
//!   nothing and never print the secret;
//! * loopback 127.0.0.1:20753 with a token file: the CLI with no token works
//!   exactly as before (submit, lease, ack, worker);
//! * non-loopback test bind <box LAN IP>:20754 with `--allow-remote
//!   --token-file`: the CLI with no / wrong token gets 401 (exit 1), with
//!   `--token-file` or `POSEIDON_TOKEN_FILE` it works, a CLI worker drains
//!   jobs over the LAN address, a 600-request wrong-token burst is all 401
//!   and the server stays healthy; the stats counters match.
//!
//! No VPS contact, no LAN bind on the VPS; processes and files are removed.

use openmind_queue::http;
use serde_json::Value;
use std::net::{IpAddr, TcpStream, UdpSocket};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const SECRET: &str = "s205-real-test-only-ZmFrZS1zZWNyZXQtZm9yLWEtdGVzdA-not-real";

fn bin() -> String {
    std::env::var("POSEIDON_BIN").unwrap_or_else(|_| {
        let t = std::env::var("CARGO_TARGET_DIR")
            .unwrap_or_else(|_| "/workspace/swarm/target/poseidon-auth".into());
        format!("{t}/release/openmind-queue")
    })
}

fn lan_ip() -> Option<IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("192.0.2.1:9").ok()?;
    let ip = s.local_addr().ok()?.ip();
    (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
}

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn serve(addr: &str, data: &Path, extra: &[&str]) -> Server {
    let child = Command::new(bin())
        .args(["serve", "--bind", addr, "--data"])
        .arg(data)
        .args(extra)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start release binary (build it first)");
    let s = Server(child);
    for _ in 0..100 {
        if TcpStream::connect(addr).is_ok() {
            return s;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not come up on {addr}");
}

fn cli(args: &[&str], env_token: Option<&Path>) -> Output {
    let mut c = Command::new(bin());
    c.args(args).env_remove("POSEIDON_TOKEN_FILE");
    if let Some(p) = env_token {
        c.env("POSEIDON_TOKEN_FILE", p);
    }
    let o = c.output().unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(!all.contains(SECRET), "CLI output leaked the secret");
    o
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn stats(addr: &str) -> Value {
    let (c, b) = http::request_auth(
        addr,
        "GET",
        "/stats",
        None,
        Some(SECRET.as_bytes()),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(c, 200);
    serde_json::from_str(&b).unwrap()
}

fn log(line: &str) {
    let t = zagreb_time::from_unix(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    );
    println!("{t} {line}");
}

#[test]
fn real_auth() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1 to run the real auth test");
        return;
    }
    let ip = lan_ip().expect("box needs a non-loopback IPv4 for the test bind");
    let lan = format!("{ip}:20754");
    let lo = "127.0.0.1:20753".to_string();
    let dir: PathBuf =
        std::env::temp_dir().join(format!("poseidon-real-auth-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let tok = dir.join("token");
    std::fs::write(&tok, format!("{SECRET}\n")).unwrap();
    std::fs::set_permissions(&tok, std::fs::Permissions::from_mode(0o600)).unwrap();
    let wrong = dir.join("wrong");
    std::fs::write(
        &wrong,
        "this-is-the-wrong-token-but-long-enough-0123456789\n",
    )
    .unwrap();
    std::fs::set_permissions(&wrong, std::fs::Permissions::from_mode(0o600)).unwrap();
    let loose = dir.join("loose");
    std::fs::write(&loose, format!("{SECRET}\n")).unwrap();
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).unwrap();
    let ts = tok.to_str().unwrap();
    log(&format!("bin {} lan {lan} lo {lo}", bin()));

    // --- refusals ---------------------------------------------------------
    let data = dir.join("refused");
    let ds = data.to_str().unwrap();
    for (what, args) in [
        (
            "lan without --token-file",
            vec!["serve", "--bind", &lan, "--data", ds, "--allow-remote"],
        ),
        (
            "lan without --allow-remote",
            vec!["serve", "--bind", &lan, "--data", ds, "--token-file", ts],
        ),
        (
            "0644 token file",
            vec![
                "serve",
                "--bind",
                &lan,
                "--data",
                ds,
                "--allow-remote",
                "--token-file",
                loose.to_str().unwrap(),
            ],
        ),
        (
            "--token-on-loopback without file",
            vec!["serve", "--bind", &lo, "--data", ds, "--token-on-loopback"],
        ),
    ] {
        let o = cli(&args, None);
        let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
        assert!(!o.status.success(), "{what}: started");
        assert!(TcpStream::connect(&lan).is_err() && TcpStream::connect(&lo).is_err());
        log(&format!(
            "refused [{what}]: exit {:?}: {err}",
            o.status.code()
        ));
    }
    // A CLI pointed at a loose token file refuses too.
    let o = cli(
        &[
            "get",
            "/healthz",
            "--addr",
            &lo,
            "--token-file",
            loose.to_str().unwrap(),
        ],
        None,
    );
    assert_eq!(o.status.code(), Some(2));

    // --- loopback: unchanged behaviour -----------------------------------
    {
        let _s = serve(&lo, &dir.join("lo"), &["--token-file", ts]);
        let o = cli(
            &["submit", "--addr", &lo, "--kind", "echo", "--payload", "hi"],
            None,
        );
        assert!(o.status.success(), "{}", out(&o));
        let o = cli(
            &["worker", "--addr", &lo, "--worker", "lw", "--max-jobs", "1"],
            None,
        );
        assert!(o.status.success(), "{}", out(&o));
        let st = stats(&lo);
        assert_eq!(st["stats"]["acked"], 1, "{st}");
        assert_eq!(st["auth"]["mode"], "token-for-remote");
        log(&format!(
            "loopback no-token CLI submit+worker ok; auth {}",
            st["auth"]
        ));
    }

    // --- non-loopback test bind ------------------------------------------
    let _s = serve(
        &lan,
        &dir.join("lan"),
        &["--allow-remote", "--token-file", ts],
    );
    assert!(TcpStream::connect("127.0.0.1:20754").is_err());
    let o = cli(&["get", "/healthz", "--addr", &lan], None);
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).starts_with("401 "), "{}", out(&o));
    log(&format!("lan no token: exit 1 `{}`", out(&o)));
    let o = cli(&["submit", "--addr", &lan, "--kind", "echo"], Some(&wrong));
    assert_eq!(o.status.code(), Some(1));
    assert!(out(&o).starts_with("401 "), "{}", out(&o));
    log(&format!("lan wrong token: exit 1 `{}`", out(&o)));
    let o = cli(
        &["get", "/healthz", "--addr", &lan, "--token-file", ts],
        None,
    );
    assert!(
        o.status.success() && out(&o).starts_with("200 "),
        "{}",
        out(&o)
    );
    for i in 0..5 {
        let p = i.to_string();
        let o = cli(
            &["submit", "--addr", &lan, "--kind", "echo", "--payload", &p],
            Some(&tok),
        );
        assert!(out(&o).starts_with("201 "), "{}", out(&o));
    }
    let o = cli(
        &[
            "worker",
            "--addr",
            &lan,
            "--worker",
            "lanw",
            "--max-jobs",
            "5",
        ],
        Some(&tok),
    );
    assert!(o.status.success(), "{}", out(&o));
    log(&format!(
        "lan token CLI: 5 submits 201, worker drained {} lines",
        out(&o).lines().count()
    ));
    // A worker without the token cannot lease (fails, nothing leased).
    let o = cli(
        &[
            "worker",
            "--addr",
            &lan,
            "--worker",
            "evil",
            "--max-jobs",
            "1",
        ],
        None,
    );
    assert!(!o.status.success());

    // Wrong-token burst: all 401, server still healthy.
    let t0 = Instant::now();
    let mut n401 = 0;
    for i in 0..600 {
        let t = format!("guess-{i:06}-{}", &SECRET[..20]);
        let (c, _) = http::request_auth(
            &lan,
            "POST",
            "/jobs",
            Some(r#"{"kind":"echo"}"#),
            Some(t.as_bytes()),
            Duration::from_secs(5),
        )
        .unwrap();
        if c == 401 {
            n401 += 1;
        }
    }
    let burst = t0.elapsed();
    assert_eq!(n401, 600);
    let st = stats(&lan);
    assert_eq!(st["stats"]["acked"], 5, "{st}");
    assert_eq!(st["depth"]["ready"], 0, "refused submits leaked: {st}");
    assert!(!st.to_string().contains(SECRET));
    log(&format!(
        "burst 600 wrong tokens: 401 x{n401} in {} ms; stats auth {}",
        burst.as_millis(),
        st["auth"]
    ));
    drop(_s);
    assert!(TcpStream::connect(&lan).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
    log("cleanup: servers killed, temp dir removed");
}
