// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Real test of `galion-admin image verify` (GALION_REAL=1, box only; no device, no network).
//!
//! Builds a gold image from REAL artifacts (the release galion-admin and gold_image binaries
//! themselves, plus every regular file under $GALION_IMAGE_EXTRA, e.g. cross-built aarch64 /
//! armv7 binaries and a hades-gate build), signs it with a key made by the real hades-gate
//! `keygen` when $GALION_HADES_GATE points at it (else gold_image keygen, same format), then
//! drives the real binary through the tamper matrix.
//!
//! Optional: $GALION_ADMIN_BIN (verify with another build, e.g. aarch64-musl) and
//! $GALION_IMAGE_RUNNER (e.g. `qemu-aarch64`), $GALION_IMAGE_BIG_MB (default 512) for the
//! throughput run. Slot 204 ports 20740-20749 are reserved but unused: nothing listens.

mod common;
use common::{fresh_dir, stamp};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

fn on() -> bool {
    let ok = std::env::var("GALION_REAL").as_deref() == Ok("1");
    if !ok {
        eprintln!("skipped: set GALION_REAL=1");
    }
    ok
}

struct Env {
    admin: PathBuf,
    runner: Option<String>,
    tool: PathBuf,
    base: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let own = PathBuf::from(env!("CARGO_BIN_EXE_galion-admin"));
        let tool = own.parent().unwrap().join("examples/gold_image");
        assert!(tool.is_file(), "build examples first: {}", tool.display());
        Env {
            admin: std::env::var_os("GALION_ADMIN_BIN")
                .map(PathBuf::from)
                .unwrap_or(own),
            runner: std::env::var("GALION_IMAGE_RUNNER").ok().filter(|s| !s.is_empty()),
            tool,
            base: fresh_dir(
                &PathBuf::from(env!("CARGO_TARGET_TMPDIR")),
                &format!("real-image-{name}"),
            ),
        }
    }
    fn admin(&self, args: &[&str]) -> (i32, String, String, u128) {
        let mut c = match &self.runner {
            Some(r) => {
                let mut c = Command::new(r);
                c.arg(&self.admin);
                c
            }
            None => Command::new(&self.admin),
        };
        c.env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("TZ", "Europe/Zagreb")
            .arg("--state")
            .arg(self.base.join("state"))
            .args(args);
        let t = Instant::now();
        let o = c.output().expect("run galion-admin");
        (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            t.elapsed().as_millis(),
        )
    }
    fn tool(&self, args: &[&str]) -> String {
        let o = Command::new(&self.tool)
            .env("TZ", "Europe/Zagreb")
            .args(args)
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "gold_image {args:?}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    /// Key pair in hades-gate format. Prefers the real hades-gate binary.
    fn keygen(&self, tag: &str) -> (PathBuf, PathBuf, String) {
        let seed = self.base.join(format!("{tag}.seed"));
        let public = self.base.join(format!("{tag}.pub"));
        let how = match std::env::var_os("GALION_HADES_GATE") {
            Some(h) => {
                let o = Command::new(&h)
                    .args(["keygen", "--seed"])
                    .arg(&seed)
                    .arg("--pub")
                    .arg(&public)
                    .output()
                    .unwrap();
                assert!(o.status.success(), "hades-gate keygen failed");
                "hades-gate keygen".to_string()
            }
            None => {
                self.tool(&[
                    "keygen",
                    "--seed",
                    seed.to_str().unwrap(),
                    "--pub",
                    public.to_str().unwrap(),
                ]);
                "gold_image keygen".to_string()
            }
        };
        let mode = std::fs::metadata(&seed).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "seed file mode");
        (seed, public, how)
    }
}

fn copy(src: &Path, dst: &Path, mode: u32) {
    std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
    std::fs::copy(src, dst).unwrap();
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Real-artifact image: returns the image dir and a short description.
fn real_image(e: &Env, seed: &Path) -> (PathBuf, String) {
    let img = e.base.join("image");
    let own = PathBuf::from(env!("CARGO_BIN_EXE_galion-admin"));
    copy(&own, &img.join("bin/galion-admin"), 0o755);
    copy(&e.tool, &img.join("tools/gold_image"), 0o755);
    let mut extra = 0;
    if let Some(x) = std::env::var_os("GALION_IMAGE_EXTRA") {
        let x = PathBuf::from(x);
        let mut stack = vec![x.clone()];
        while let Some(d) = stack.pop() {
            for ent in std::fs::read_dir(&d).unwrap().flatten() {
                let ft = ent.file_type().unwrap();
                let p = ent.path();
                if ft.is_dir() {
                    stack.push(p);
                } else if ft.is_file() {
                    let rel = p.strip_prefix(&x).unwrap();
                    copy(&p, &img.join("extra").join(rel), 0o755);
                    extra += 1;
                }
            }
        }
    }
    std::fs::create_dir_all(img.join("etc/galion")).unwrap();
    std::fs::write(
        img.join("etc/galion/os.toml"),
        "# SPDX-License-Identifier: MIT\n[os]\nname = \"galion-os\"\nchannel = \"gold\"\n",
    )
    .unwrap();
    std::fs::write(img.join("etc/galion/empty.conf"), "").unwrap();
    std::fs::set_permissions(
        img.join("etc/galion/os.toml"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    std::fs::set_permissions(
        img.join("etc/galion/empty.conf"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let m = e.tool(&[
        "manifest",
        img.to_str().unwrap(),
        "--name",
        "galion-os-gold",
        "--version",
        "0.1.0-s204",
        "--arch",
        std::env::consts::ARCH,
        "--commit",
        "bf33cba",
    ]);
    let s = e.tool(&["sign", img.to_str().unwrap(), "--seed", seed.to_str().unwrap()]);
    (img, format!("{m}; {s}; extra files {extra}"))
}

/// Every file's bytes and mode, to prove verify writes nothing.
fn snapshot(dir: &Path) -> Vec<(String, u32, Vec<u8>)> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for ent in std::fs::read_dir(&d).unwrap().flatten() {
            let md = std::fs::symlink_metadata(ent.path()).unwrap();
            if md.is_dir() {
                stack.push(ent.path());
            } else {
                v.push((
                    ent.path().display().to_string(),
                    md.permissions().mode(),
                    std::fs::read(ent.path()).unwrap_or_default(),
                ));
            }
        }
    }
    v.sort();
    v
}

#[test]
fn i01_real_image_ok_and_tamper_matrix() {
    if !on() {
        return;
    }
    let e = Env::new("i01");
    let (seed, public, how) = e.keygen("admin");
    let (_, other_pub, _) = e.keygen("other");
    let (img, desc) = real_image(&e, &seed);
    let key = public.to_str().unwrap();
    let dir = img.to_str().unwrap();
    println!("[{}] built: {desc}; key by {how}", stamp());

    let before = snapshot(&img);
    let (c, out, err, ms) = e.admin(&["image", "verify", dir, "--key", key]);
    println!("[{}] ok run: exit {c} {ms} ms: {}", stamp(), out.trim());
    assert_eq!(c, 0, "{out}{err}");
    assert!(out.starts_with("image ok: galion-os-gold 0.1.0-s204"), "{out}");
    assert_eq!(snapshot(&img), before, "verify must not write into the image");

    let (c, out, _, _) = e.admin(&["image", "verify", dir, "--key", key, "--json"]);
    assert_eq!(c, 0);
    assert!(out.starts_with("{\"ok\":true,") && out.contains("\"problems\":[]"), "{out}");

    let bin = img.join("bin/galion-admin");
    let orig = std::fs::read(&bin).unwrap();
    let len = orig.len();
    let mut rows: Vec<(String, i32, String)> = Vec::new();
    let mut case = |name: &str, want: &str, f: &mut dyn FnMut(), undo: &mut dyn FnMut()| {
        f();
        let (c, out, err, ms) = e.admin(&["image", "verify", dir, "--key", key]);
        undo();
        let first = out.lines().take(2).collect::<Vec<_>>().join(" | ");
        println!("[{}] {name}: exit {c} {ms} ms: {first}", stamp());
        assert_eq!(c, 4, "{name}: {out}{err}");
        assert!(out.contains(want), "{name}: want {want:?} in {out}");
        rows.push((name.to_string(), c, first));
        // Restored image verifies again.
        assert_eq!(e.admin(&["image", "verify", dir, "--key", key]).0, 0, "{name} undo");
    };
    let b2 = bin.clone();
    let o2 = orig.clone();
    for pos in [0, 1, len / 2, len - 1] {
        let (b, o) = (b2.clone(), o2.clone());
        case(
            &format!("bit-flip byte {pos} of {len}"),
            "sha256       bin/galion-admin",
            &mut || {
                let mut d = o.clone();
                d[pos] ^= 0x80;
                std::fs::write(&b, d).unwrap();
            },
            &mut || std::fs::write(&b, &o).unwrap(),
        );
    }
    for cut in [1usize, 4096, len] {
        let (b, o) = (b2.clone(), o2.clone());
        case(
            &format!("truncate by {cut}"),
            "truncated    bin/galion-admin",
            &mut || std::fs::write(&b, &o[..len - cut]).unwrap(),
            &mut || std::fs::write(&b, &o).unwrap(),
        );
    }
    {
        let (b, o) = (b2.clone(), o2.clone());
        case(
            "append 1 byte",
            "grown        bin/galion-admin",
            &mut || {
                let mut d = o.clone();
                d.push(0);
                std::fs::write(&b, d).unwrap();
            },
            &mut || std::fs::write(&b, &o).unwrap(),
        );
    }
    let cfg = img.join("etc/galion/os.toml");
    let cfg_o = std::fs::read(&cfg).unwrap();
    case(
        "missing file",
        "missing      etc/galion/os.toml",
        &mut || std::fs::remove_file(&cfg).unwrap(),
        &mut || {
            std::fs::write(&cfg, &cfg_o).unwrap();
            std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap();
        },
    );
    let extra = img.join("bin/implant");
    case(
        "unlisted file added",
        "unlisted     bin/implant",
        &mut || std::fs::write(&extra, b"#!/bin/sh\n").unwrap(),
        &mut || std::fs::remove_file(&extra).unwrap(),
    );
    let xdir = img.join("var");
    case(
        "unlisted empty dir",
        "unlisted-dir var",
        &mut || std::fs::create_dir(&xdir).unwrap(),
        &mut || std::fs::remove_dir(&xdir).unwrap(),
    );
    let outside = e.base.join("os.toml.outside");
    case(
        "symlink swap (same content)",
        "not-regular  etc/galion/os.toml  (symlink)",
        &mut || {
            std::fs::write(&outside, &cfg_o).unwrap();
            std::fs::remove_file(&cfg).unwrap();
            std::os::unix::fs::symlink(&outside, &cfg).unwrap();
        },
        &mut || {
            std::fs::remove_file(&cfg).unwrap();
            std::fs::write(&cfg, &cfg_o).unwrap();
            std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap();
        },
    );
    case(
        "mode change 0644 -> 0666",
        "mode         etc/galion/os.toml",
        &mut || std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o666)).unwrap(),
        &mut || std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o644)).unwrap(),
    );
    let man = img.join("galion-image.manifest");
    let man_o = std::fs::read_to_string(&man).unwrap();
    case(
        "manifest edit (version)",
        "REJECTED: bad signature",
        &mut || std::fs::write(&man, man_o.replace("0.1.0-s204", "0.1.1-s204")).unwrap(),
        &mut || std::fs::write(&man, &man_o).unwrap(),
    );
    case(
        "manifest truncated (end line cut)",
        "REJECTED: bad signature",
        &mut || std::fs::write(&man, &man_o[..man_o.len() - 4]).unwrap(),
        &mut || std::fs::write(&man, &man_o).unwrap(),
    );
    let sig = img.join("galion-image.sig");
    let sig_o = std::fs::read_to_string(&sig).unwrap();
    case(
        "signature hex char changed",
        "REJECTED: bad signature",
        &mut || {
            let i = sig_o.rfind(' ').unwrap() + 10;
            let mut s = sig_o.clone().into_bytes();
            s[i] = if s[i] == b'0' { b'1' } else { b'0' };
            std::fs::write(&sig, s).unwrap();
        },
        &mut || std::fs::write(&sig, &sig_o).unwrap(),
    );
    case(
        "signature file deleted",
        "REJECTED: metadata: galion-image.sig",
        &mut || std::fs::remove_file(&sig).unwrap(),
        &mut || std::fs::write(&sig, &sig_o).unwrap(),
    );
    // Wrong key: a second real key pair.
    let (c, out, _, _) = e.admin(&[
        "image",
        "verify",
        dir,
        "--key",
        other_pub.to_str().unwrap(),
    ]);
    println!("[{}] wrong key: exit {c}: {}", stamp(), out.trim());
    assert_eq!(c, 4);
    assert!(out.contains("REJECTED: wrong key"), "{out}");
    // Seed passed as --key: refused as not a public key, exit 1, no key bytes printed.
    let (c, out, err, _) = e.admin(&["image", "verify", dir, "--key", seed.to_str().unwrap()]);
    assert_eq!(c, 1, "{out}{err}");
    assert!(err.contains("not a ed25519-pub file"), "{err}");

    // No key material in any output or the audit log.
    let seed_hex = std::fs::read_to_string(&seed).unwrap();
    let seed_hex = seed_hex.trim().rsplit(' ').next().unwrap().to_string();
    let pub_hex = std::fs::read_to_string(&public).unwrap();
    let pub_hex = pub_hex.trim().rsplit(' ').next().unwrap().to_string();
    let audit = std::fs::read_to_string(e.base.join("state/audit.jsonl")).unwrap();
    for (n, _, line) in &rows {
        assert!(!line.contains(&seed_hex) && !line.contains(&pub_hex), "{n}");
    }
    assert!(!audit.contains(&seed_hex) && !audit.contains(&pub_hex));
    let n_rec = audit.lines().filter(|l| l.contains("image.verify")).count();
    let (c, out, _, _) = e.admin(&["audit", "verify"]);
    println!("[{}] audit: {n_rec} image.verify records; {}", stamp(), out.trim());
    assert_eq!(c, 0);
    assert!(n_rec >= 2 * rows.len() + 3, "{n_rec}");
    assert_eq!(snapshot(&img), before, "image restored byte-identical");
    println!("[{}] tamper cases: {}", stamp(), rows.len());
}

#[test]
fn i02_throughput_big_file() {
    if !on() {
        return;
    }
    let e = Env::new("i02");
    let (seed, public, _) = e.keygen("big");
    let mb: usize = std::env::var("GALION_IMAGE_BIG_MB")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(512);
    let img = e.base.join("image");
    std::fs::create_dir_all(img.join("data")).unwrap();
    // Pseudo-random (xorshift64*) so nothing compresses or dedups below the hash.
    {
        use std::io::Write;
        let mut f = std::io::BufWriter::new(
            std::fs::File::create(img.join("data/rootfs.img")).unwrap(),
        );
        let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut buf = vec![0u8; 1 << 20];
        for _ in 0..mb {
            for c in buf.chunks_mut(8) {
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                c.copy_from_slice(&x.wrapping_mul(0x2545_f491_4f6c_dd1d).to_le_bytes());
            }
            f.write_all(&buf).unwrap();
        }
        f.flush().unwrap();
    }
    std::fs::set_permissions(
        img.join("data/rootfs.img"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let t = Instant::now();
    e.tool(&[
        "manifest",
        img.to_str().unwrap(),
        "--name",
        "galion-os-big",
        "--version",
        "0.1.0",
        "--arch",
        std::env::consts::ARCH,
        "--commit",
        "bf33cba",
    ]);
    let build_ms = t.elapsed().as_millis();
    e.tool(&["sign", img.to_str().unwrap(), "--seed", seed.to_str().unwrap()]);
    let mut runs = Vec::new();
    for _ in 0..3 {
        let (c, out, err, ms) =
            e.admin(&["image", "verify", img.to_str().unwrap(), "--key", public.to_str().unwrap()]);
        assert_eq!(c, 0, "{out}{err}");
        runs.push(ms);
    }
    runs.sort();
    let med = runs[1].max(1);
    println!(
        "[{}] big image {mb} MiB: manifest build {build_ms} ms; verify runs {runs:?} ms, median {med} ms = {:.0} MiB/s{}",
        stamp(),
        mb as f64 * 1000.0 / med as f64,
        e.runner.as_deref().map(|r| format!(" (under {r})")).unwrap_or_default()
    );
    // Bit flip in the last MiB of the big file is found.
    {
        use std::io::{Read, Seek, SeekFrom, Write};
        let mut f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(img.join("data/rootfs.img"))
            .unwrap();
        let off = (mb as u64) * (1 << 20) - 12_345;
        let mut b = [0u8; 1];
        f.seek(SeekFrom::Start(off)).unwrap();
        f.read_exact(&mut b).unwrap();
        b[0] ^= 1;
        f.seek(SeekFrom::Start(off)).unwrap();
        f.write_all(&b).unwrap();
    }
    let (c, out, _, ms) =
        e.admin(&["image", "verify", img.to_str().unwrap(), "--key", public.to_str().unwrap()]);
    println!("[{}] big image 1-bit flip: exit {c} {ms} ms: {}", stamp(), out.lines().nth(1).unwrap_or(""));
    assert_eq!(c, 4);
    assert!(out.contains("sha256       data/rootfs.img"));
    let _ = std::fs::remove_dir_all(&e.base);
}
