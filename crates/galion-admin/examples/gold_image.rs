// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Builder-side helper for gold images (NOT shipped on the device; the device only verifies).
//!
//!   gold_image keygen   --seed FILE --pub FILE          (hades-gate format; 0600 / 0644; never overwrites)
//!   gold_image manifest DIR --name N --version V --arch A --commit C [--rustc R] [--time T]
//!   gold_image sign     DIR --seed FILE
//!
//! Key material is never printed; only the 16-hex key fingerprint.

use galion_admin::image::{self, MANIFEST_NAME, PUB_TAG, SEED_TAG, SIG_NAME};
use galion_admin::timefmt;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

fn flags(a: &[String]) -> Result<(Vec<String>, HashMap<String, String>), String> {
    let (mut pos, mut map) = (Vec::new(), HashMap::new());
    let mut it = a.iter();
    while let Some(x) = it.next() {
        if let Some(k) = x.strip_prefix("--") {
            let v = it.next().ok_or_else(|| format!("--{k} needs a value"))?;
            if map.insert(k.to_string(), v.clone()).is_some() {
                return Err(format!("--{k} given twice"));
            }
        } else {
            pos.push(x.clone());
        }
    }
    Ok((pos, map))
}

fn write_new(p: &Path, text: &str, mode: u32) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(p)
        .map_err(|e| format!("{}: {}", p.display(), e.kind()))?;
    f.write_all(text.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(|e| format!("{}: {}", p.display(), e.kind()))
}

fn write_replace(p: &Path, text: &str) -> Result<(), String> {
    let tmp = p.with_extension("tmp");
    let _ = std::fs::remove_file(&tmp);
    write_new(&tmp, text, 0o644)?;
    std::fs::rename(&tmp, p).map_err(|e| format!("{}: {}", p.display(), e.kind()))
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn run(a: &[String]) -> Result<(), String> {
    let (pos, f) = flags(a.get(1..).unwrap_or_default())?;
    let need = |k: &str| f.get(k).cloned().ok_or(format!("--{k} is required"));
    match a.first().map(String::as_str) {
        Some("keygen") => {
            let (seed_p, pub_p) = (PathBuf::from(need("seed")?), PathBuf::from(need("pub")?));
            let mut seed = Zeroizing::new([0u8; 32]);
            std::fs::File::open("/dev/urandom")
                .and_then(|mut r| r.read_exact(seed.as_mut()))
                .map_err(|e| format!("/dev/urandom: {}", e.kind()))?;
            let sk = ed25519_dalek::SigningKey::from_bytes(&seed);
            let line = Zeroizing::new(format!("{SEED_TAG} {}\n", image::to_hex(seed.as_ref())));
            write_new(&seed_p, &line, 0o600)?;
            let vk = sk.verifying_key();
            write_new(
                &pub_p,
                &format!("{PUB_TAG} {}\n", image::to_hex(vk.as_bytes())),
                0o644,
            )?;
            println!("key {}", image::fingerprint(&vk));
        }
        Some("manifest") => {
            let dir = PathBuf::from(pos.first().ok_or("manifest needs DIR")?);
            let header = [
                need("name")?,
                need("version")?,
                need("arch")?,
                need("commit")?,
                f.get("rustc").cloned().unwrap_or_else(rustc_version),
                f.get("time").cloned().unwrap_or_else(timefmt::now_local),
            ];
            let m = image::build(&dir, header)?;
            write_replace(&dir.join(MANIFEST_NAME), &m.to_text())?;
            println!(
                "manifest {} files, {} bytes, sha256 {}",
                m.files.len(),
                m.total_bytes(),
                image::to_hex(&image::sha256(m.to_text().as_bytes()))
            );
        }
        Some("sign") => {
            let dir = PathBuf::from(pos.first().ok_or("sign needs DIR")?);
            let sk = image::load_seed(Path::new(&need("seed")?))?;
            let man = std::fs::read(dir.join(MANIFEST_NAME))
                .map_err(|e| format!("{MANIFEST_NAME}: {}", e.kind()))?;
            image::Manifest::parse(&man).map_err(|e| format!("refusing to sign: {e}"))?;
            write_replace(&dir.join(SIG_NAME), &image::sign(&man, &sk))?;
            println!("signed by key {}", image::fingerprint(&sk.verifying_key()));
        }
        _ => return Err("usage: gold_image keygen|manifest|sign (see source header)".into()),
    }
    Ok(())
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&a) {
        eprintln!("gold_image: {e}");
        std::process::exit(1);
    }
}
