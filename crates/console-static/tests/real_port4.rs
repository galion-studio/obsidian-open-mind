// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real-life test for the console-port-4 rooms (`/md/$name`, `/memory`,
//! `/mesh`) against the **live VPS**, read-only.
//!
//! - One `ssh` read-only probe (find/stat/date/head, nothing written on the
//!   VPS) lists Hermes's memory tree and counts it with GNU `find` (an
//!   independent implementation of the memory.tsx rules), stats `USER.md` and
//!   `persona.md`, formats their mtimes with the VPS's own
//!   `TZ=Europe/Zagreb date`, reads the model name and the first 1600 bytes
//!   of `persona.md`.
//! - The tree is rebuilt on the box as an **anonymised** mirror in a 0700
//!   temp folder (every name replaced by `nN` keeping only what the rules look
//!   at: `.md`, `refs`, `canvas.mmd`, the fixed folder names; empty files;
//!   `USER.md` is a placeholder of the same size and mtime, its text is never
//!   copied). The mirror is removed at the end.
//! - One `ssh -N` forward: box 127.0.0.1:20081 -> VPS 8080 (console API),
//!   20082 -> 8670 (galion-nerve). The real `galion-console-static --ui rust`
//!   runs on 127.0.0.1:20080 (slot 138 ports 20080-20089).
//! - Checks: `/memory` numbers = VPS `find` numbers, model = VPS file,
//!   persona excerpt = first 400 UTF-16 units of the real head; `/md/*` size
//!   and Zagreb time = VPS `stat`/`date`; `/mesh` = live `GET /api/mesh`;
//!   the gate asks the live `/api/v1/admin` for a public host (pairing page,
//!   no data). Nerve `/state` before/after. Only GET requests reach the VPS.
//!   Persona and file names are never printed.
//!
//! Gated: `GALION_REAL=1 cargo test --release -p console-static --test real_port4 -- --ignored --nocapture`.
//! `PORT4_VPS` names the ssh target (default `slavik-vps`, an ssh config alias).

use console_ui::html::esc;
use console_ui::json::{self, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, UNIX_EPOCH};

const BIND: &str = "127.0.0.1:20080";
const API: &str = "127.0.0.1:20081";
const NERVE: &str = "127.0.0.1:20082";

/// The read-only probe run on the VPS (no redirection to files, no writes).
const PROBE: &str = r#"set -u
R=/var/lib/memory-tencentdb
U=/opt/galion/hermes/memories/USER.md
M=/opt/galion/galion-light/one/model
cd "$R" || exit 3
echo =list
find -L pyramid scene_blocks offload -printf '%y\t%s\t%p\n' 2>/dev/null
echo =ref
for l in l0 l1 l2; do echo "$l $(find -L pyramid/$l -type f 2>/dev/null | wc -l)"; done
echo "l3 $(stat -Lc %s pyramid/l3/persona.md 2>/dev/null || echo 0)"
echo "scenes $(find -L scene_blocks -mindepth 1 -maxdepth 1 -type f -name '*.md' 2>/dev/null | wc -l)"
echo "refs $(find -L offload -mindepth 1 -type d -name refs -exec find -L {} -mindepth 1 -maxdepth 1 -type f -name '*.md' \; 2>/dev/null | wc -l)"
echo "canvases $(find -L offload -type f -name canvas.mmd 2>/dev/null | wc -l)"
echo =stat
echo "persona $(stat -Lc '%s %Y' persona.md)"
echo "user $(stat -Lc '%s %Y' "$U")"
echo "zpersona $(TZ=Europe/Zagreb date -d @$(stat -Lc %Y persona.md) '+%Y-%m-%d %H:%M %Z (%:z)')"
echo "zuser $(TZ=Europe/Zagreb date -d @$(stat -Lc %Y "$U") '+%Y-%m-%d %H:%M %Z (%:z)')"
echo "model $(head -c 256 "$M" | tr -d '\r\n')"
echo "head $(head -c 1600 persona.md | od -An -tx1 -v | tr -d ' \n')"
echo =end
"#;

struct Kill(Child, &'static str);
impl Drop for Kill {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
        eprintln!("real: stopped {}", self.1);
    }
}

struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        eprintln!("real: removed the mirror ({})", !self.0.exists());
    }
}

fn get_host(at: &str, path: &str, host: &str) -> (u16, String) {
    get_accept(at, path, host, "application/json")
}

fn get_accept(at: &str, path: &str, host: &str, accept: &str) -> (u16, String) {
    let Ok(mut s) = TcpStream::connect(at) else {
        return (0, String::new());
    };
    s.set_read_timeout(Some(Duration::from_secs(25))).unwrap();
    let _ = write!(
        s,
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nAccept: {accept}\r\nConnection: close\r\n\r\n"
    );
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).to_string();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let code = head
        .split(' ')
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        String::from_utf8_lossy(&console_ui::client::dechunk(body.as_bytes()).unwrap_or_default())
            .to_string()
    } else {
        body.to_string()
    };
    (code, body)
}

fn get(at: &str, path: &str) -> (u16, String) {
    get_host(at, path, "127.0.0.1")
}

fn wait_for(what: &str, secs: u64, mut ok: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !ok() {
        assert!(
            t0.elapsed() < Duration::from_secs(secs),
            "{what} did not come up"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn nerve_watch(tag: &str) -> Vec<String> {
    let (c, b) = get(NERVE, "/state");
    assert_eq!(c, 200, "nerve /state");
    let v = json::parse(b.as_bytes()).unwrap_or(Value::Null);
    let mut out = Vec::new();
    for comp in v.get("components").as_array() {
        let port = comp.get("port").as_f64().unwrap_or(0.0) as u16;
        if [8080u16, 8642, 8646, 8091].contains(&port) {
            out.push(format!(
                "{}:{} {}",
                comp.get("name").as_str().unwrap_or("?"),
                port,
                comp.get("status").as_str().unwrap_or("?")
            ));
        }
    }
    eprintln!(
        "real: nerve {tag}: cycle {} mode {} components {} -> {:?}",
        v.get("cycle").to_json(),
        v.get("mode").as_str().unwrap_or("?"),
        v.get("components").as_array().len(),
        out
    );
    out
}

fn hex(s: &str) -> Vec<u8> {
    s.as_bytes()
        .chunks(2)
        .filter_map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

/// Names the counting rules look at stay; everything else becomes `nN`.
fn anon(name: &str, n: &mut usize) -> String {
    const KEEP: &[&str] = &[
        "pyramid",
        "l0",
        "l1",
        "l2",
        "l3",
        "persona.md",
        "scene_blocks",
        "offload",
        "refs",
        "canvas.mmd",
    ];
    if KEEP.contains(&name) {
        return name.to_string();
    }
    *n += 1;
    if name.ends_with(".md") {
        format!("n{n}.md")
    } else {
        format!("n{n}")
    }
}

fn set_mtime(p: &Path, unix: i64) {
    let f = fs::OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(unix as u64))
        .unwrap();
}

/// Same rule as JS `slice(0, 400)`, written separately from the crate's.
fn first_units(s: &str, n: usize) -> String {
    let mut out = String::new();
    let mut u = 0;
    for c in s.chars() {
        u += c.len_utf16();
        if u > n {
            break;
        }
        out.push(c);
    }
    out
}

fn row(v: &str, label: &str) -> String {
    format!("<span class=\"p4-n\">{v}</span><span class=\"ell\">{label}</span>")
}

#[test]
#[ignore = "real-life: needs GALION_REAL=1, ssh to the VPS and ports 20080-20082"]
fn real_port4_rooms_against_live_vps() {
    if std::env::var("GALION_REAL").as_deref() != Ok("1") {
        eprintln!("skipped: set GALION_REAL=1");
        return;
    }
    let vps = std::env::var("PORT4_VPS").unwrap_or_else(|_| "slavik-vps".into());

    // 1. Read-only probe.
    let t = Instant::now();
    let out = Command::new("ssh")
        .args(["-o", "BatchMode=yes", &vps, PROBE])
        .stdin(Stdio::null())
        .output()
        .expect("ssh probe");
    assert!(out.status.success(), "probe failed: {:?}", out.status);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let section = |name: &str| -> Vec<&str> {
        text.split(&format!("={name}\n"))
            .nth(1)
            .unwrap_or("")
            .split("\n=")
            .next()
            .unwrap_or("")
            .lines()
            .collect()
    };
    let kv: HashMap<&str, &str> = section("ref")
        .into_iter()
        .chain(section("stat"))
        .filter_map(|l| l.split_once(' '))
        .collect();
    let list = section("list");
    eprintln!(
        "real: VPS probe {} ms: {} tree entries; find counts l0 {} l1 {} l2 {} l3 {} B scenes {} refs {} canvases {}",
        t.elapsed().as_millis(),
        list.len(),
        kv["l0"],
        kv["l1"],
        kv["l2"],
        kv["l3"],
        kv["scenes"],
        kv["refs"],
        kv["canvases"]
    );
    assert!(list.len() > 10, "tree listed");

    // 2. Anonymised mirror on the box.
    let root = Tmp(std::env::temp_dir().join(format!("real-port4-{}", std::process::id())));
    let _ = fs::remove_dir_all(&root.0);
    fs::create_dir_all(&root.0).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mem = root.0.join("mem");
    let mut map: HashMap<String, PathBuf> = HashMap::new();
    let mut n = 0usize;
    let (mut dirs, mut files) = (0, 0);
    for line in &list {
        let mut it = line.splitn(3, '\t');
        let (Some(kind), Some(size), Some(path)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let (parent, name) = path.rsplit_once('/').unwrap_or(("", path));
        let base = if parent.is_empty() {
            mem.clone()
        } else {
            map.get(parent).cloned().expect("parent listed first")
        };
        let dest = base.join(anon(name, &mut n));
        match kind {
            "d" => {
                fs::create_dir_all(&dest).unwrap();
                dirs += 1;
            }
            "f" => {
                let f = fs::File::create(&dest).unwrap();
                if path == "pyramid/l3/persona.md" {
                    f.set_len(size.parse().unwrap()).unwrap();
                }
                files += 1;
            }
            _ => {}
        }
        map.insert(path.to_string(), dest);
    }
    let (psize, ptime) = kv["persona"].split_once(' ').unwrap();
    let (usize_, utime) = kv["user"].split_once(' ').unwrap();
    let (psize, ptime): (u64, i64) = (psize.parse().unwrap(), ptime.parse().unwrap());
    let (usize_, utime): (u64, i64) = (usize_.parse().unwrap(), utime.parse().unwrap());
    let head = hex(kv.get("head").copied().unwrap_or(""));
    let mut persona = head.clone();
    persona.resize(psize.max(head.len() as u64) as usize, b' ');
    fs::write(mem.join("persona.md"), &persona).unwrap();
    set_mtime(&mem.join("persona.md"), ptime);
    fs::write(root.0.join("USER.md"), vec![b'x'; usize_ as usize]).unwrap();
    set_mtime(&root.0.join("USER.md"), utime);
    let model = kv.get("model").copied().unwrap_or("").trim().to_string();
    fs::write(root.0.join("model"), format!("{model}\n")).unwrap();
    eprintln!(
        "real: mirror built: {dirs} dirs, {files} files (names anonymised), persona {psize} B head {} B, USER.md placeholder {usize_} B",
        head.len()
    );

    // 3. Forward + binary.
    let ssh = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ExitOnForwardFailure=yes",
            "-N",
        ])
        .args(["-L", &format!("{API}:127.0.0.1:8080")])
        .args(["-L", &format!("{NERVE}:127.0.0.1:8670")])
        .arg(&vps)
        .stdin(Stdio::null())
        .spawn()
        .expect("ssh");
    let _ssh = Kill(ssh, "ssh forward");
    wait_for("ssh forward", 25, || get(API, "/api/v1/health").0 == 200);
    eprintln!("real: forward {API}->8080 {NERVE}->8670 (ssh -N, GET only)");
    let before = nerve_watch("before");

    let bin = env!("CARGO_BIN_EXE_galion-console-static");
    let child = Command::new(bin)
        .args(["--ui", "rust", "--upstream", API, "--bind", BIND, "--quiet"])
        .arg("--memory-root")
        .arg(&mem)
        .arg("--model-file")
        .arg(root.0.join("model"))
        .arg("--user-md")
        .arg(root.0.join("USER.md"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start binary");
    let _srv = Kill(child, "galion-console-static");
    wait_for("binary", 10, || TcpStream::connect(BIND).is_ok());

    // /memory vs VPS find.
    let t = Instant::now();
    let (c, page) = get(BIND, "/memory");
    eprintln!(
        "real: /memory {c} {} B in {} ms",
        page.len(),
        t.elapsed().as_millis()
    );
    assert_eq!(c, 200);
    for (k, label) in [
        ("l0", "L0 Conversation"),
        ("l1", "L1 Atom"),
        ("l2", "L2 Scenario"),
        ("l3", "L3 Persona"),
        ("scenes", "scenes"),
        ("refs", "raw tool refs"),
        ("canvases", "mermaid canvases"),
    ] {
        assert!(
            page.contains(&row(kv[k].trim(), label)),
            "{label}: VPS find says {}",
            kv[k]
        );
    }
    assert!(page.contains(&format!("<p class=\"p4-small\">{}</p>", esc(&model))));
    let want = first_units(&String::from_utf8_lossy(&head), 400);
    assert!(!want.is_empty());
    assert!(page.contains(&esc(&want)), "persona excerpt (not printed)");
    assert!(!page.contains("p4-bad"), "no error note");
    assert!(!page.contains(&*root.0.to_string_lossy()), "no host path");
    eprintln!(
        "real: /memory = VPS find on all 7 rows; model {model:?}; excerpt {} UTF-16 units, fnv {:016x} (text not printed)",
        want.encode_utf16().count(),
        console_ui::css::fnv64(want.as_bytes())
    );

    // /md/* vs VPS stat/date.
    for (name, title, size, zone_key) in [
        ("persona", "persona.md", psize, "zpersona"),
        ("user", "USER.md", usize_, "zuser"),
    ] {
        let (c, page) = get(BIND, &format!("/md/{name}"));
        assert_eq!(c, 200, "/md/{name}");
        let want = format!("{title} · {size} bytes · {}", kv[zone_key].trim());
        assert!(page.contains(&want), "/md/{name}: want {want}");
        assert!(!page.to_ascii_lowercase().contains("<script"));
        eprintln!("real: /md/{name} shows \"{want}\" (= VPS stat + TZ=Europe/Zagreb date)");
    }
    let (c, _) = get(BIND, "/md/other");
    assert_eq!(c, 404);
    let (lc, lpage) = get_accept(API, "/md/user", "127.0.0.1", "text/html");
    assert_eq!(lc, 200, "live Node /md/user");
    eprintln!(
        "real: live Node /md/user on a loopback Host: {lc} {} B, {} (the TSX stub route; the middleware skips the file on loopback)",
        lpage.len(),
        if lpage.contains("bytes ·") { "file page" } else { "SPA shell, no file" }
    );

    // /mesh vs live GET /api/mesh.
    let (mc, mb) = get(API, "/api/mesh");
    assert_eq!(mc, 200, "live /api/mesh");
    let board = json::parse(mb.as_bytes()).unwrap();
    let lines: Vec<&str> = board
        .get("score")
        .as_array()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let phase = board.get("phase").as_str().unwrap_or("");
    let t = Instant::now();
    let (c, page) = get(BIND, "/mesh");
    eprintln!(
        "real: /mesh {c} {} B in {} ms; live phase {phase:?}, {} score line(s)",
        page.len(),
        t.elapsed().as_millis(),
        lines.len()
    );
    assert_eq!(c, 200);
    assert!(!page.contains("did not load"), "{page}");
    for l in &lines {
        assert!(page.contains(&format!("<span class=\"p4-line\">{}</span>", esc(l))));
    }
    if lines.is_empty() {
        assert!(page.contains("No live line yet."));
    }
    let want = match phase {
        "idle" => "Cold. Nothing is spending.".to_string(),
        "" => "Cold.".to_string(),
        p => p.to_string(),
    };
    assert!(page.contains(&format!("<p class=\"note\">{}</p>", esc(&want))));
    eprintln!("real: /mesh lines + footer {want:?} = live");

    // Gate: public host, no cookie -> live /api/v1/admin says not paired.
    for p in ["/memory", "/mesh", "/md/user"] {
        let (c, page) = get_host(BIND, p, "console.galion.app");
        assert_eq!(c, 200, "{p}");
        assert!(page.contains("/_ui/pair"), "{p}: pairing page");
        assert!(!page.contains("p4-n") && !page.contains("bytes ·"), "{p}");
    }
    eprintln!("real: public host unpaired -> pairing page for all 3 rooms (live /api/v1/admin)");

    for p in ["/memory", "/mesh", "/md/user", "/md/persona"] {
        let (_, b) = get(BIND, p);
        assert!(!b.to_ascii_lowercase().contains("<script"), "{p}");
    }
    let after = nerve_watch("after");
    assert_eq!(before.len(), after.len());
    for a in &after {
        assert!(!a.ends_with(" down"), "component down after the run: {a}");
    }
}
