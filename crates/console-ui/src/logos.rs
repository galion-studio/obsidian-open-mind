// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Connector logos (owner t455u, 2026-10-30): each brand's own icon,
//! downloaded once from its official site (sources in
//! `assets/logos/SOURCES.md`) and served by the console from
//! `/_ui/cn-logo/`. Never hotlinked.
//!
//! The images are data, not source: they are not in the repository (the
//! sandbox gate keeps encoded blobs out of patches). The console reads them
//! once at start from `--connectors-logo-dir` (`<name>.png` files). Only
//! real PNG files with a plain name and at most 64 KiB are served; anything
//! else is skipped and the row falls back to a monogram. Logos are
//! trademarks of their owners.

use std::fs;
use std::path::Path;

/// URL prefix of the logo files.
pub const PREFIX: &str = "/_ui/cn-logo/";
/// Largest logo file served.
pub const MAX_BYTES: u64 = 64 * 1024;
/// Most logos read from the folder.
pub const MAX_LOGOS: usize = 128;
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// One logo, read once at start.
#[derive(Debug, Clone)]
pub struct Logo {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Vec<u8>,
    /// `/_ui/cn-logo/<name>.<hash>.png` (immutable).
    pub href: String,
    pub etag: String,
}

fn name_ok(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 40
        && n.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Every valid `<name>.png` in `dir`, sorted by name. `None` = no logos.
pub fn load(dir: Option<&Path>) -> Vec<Logo> {
    let Some(rd) = dir.and_then(|d| fs::read_dir(d).ok()) else {
        return Vec::new();
    };
    let mut out: Vec<Logo> = Vec::new();
    for e in rd.flatten() {
        let file = e.file_name().to_string_lossy().to_string();
        let Some(name) = file.strip_suffix(".png").filter(|n| name_ok(n)) else {
            continue;
        };
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() || m.len() > MAX_BYTES {
            continue;
        }
        let Ok(bytes) = fs::read(e.path()) else {
            continue;
        };
        if !bytes.starts_with(PNG_MAGIC) {
            continue;
        }
        let h = crate::css::fnv64(&bytes);
        out.push(Logo {
            name: name.to_string(),
            mime: "image/png",
            href: format!("{PREFIX}{name}.{:08x}.png", h as u32),
            etag: format!("\"{h:016x}\""),
            bytes,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.truncate(MAX_LOGOS);
    out
}

/// The URL of a logo by name.
pub fn href<'a>(logos: &'a [Logo], name: &str) -> Option<&'a str> {
    logos
        .iter()
        .find(|l| l.name == name)
        .map(|l| l.href.as_str())
}

/// The logo served at `path`, if any.
pub fn by_path<'a>(logos: &'a [Logo], path: &str) -> Option<&'a Logo> {
    logos.iter().find(|l| l.href == path)
}

/// Test helper: a tiny file that starts like a PNG.
#[cfg(test)]
pub fn fake_png(seed: &str) -> Vec<u8> {
    let mut v = PNG_MAGIC.to_vec();
    v.extend_from_slice(seed.as_bytes());
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_only_small_named_pngs() {
        let d = std::env::temp_dir().join(format!(
            "cn-logos-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("gmail.png"), fake_png("g")).unwrap();
        fs::write(d.join("box.png"), fake_png("b")).unwrap();
        fs::write(d.join("not-png.png"), b"<svg onload=x>").unwrap();
        fs::write(d.join("Bad Name.png"), fake_png("x")).unwrap();
        fs::write(d.join("note.txt"), fake_png("t")).unwrap();
        let mut big = fake_png("big");
        big.resize(MAX_BYTES as usize + 1, 0);
        fs::write(d.join("big.png"), big).unwrap();
        let l = load(Some(&d));
        let names: Vec<&str> = l.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["box", "gmail"]);
        let h = href(&l, "gmail").unwrap();
        assert!(h.starts_with(PREFIX) && h.ends_with(".png"));
        assert_eq!(by_path(&l, h).unwrap().bytes, fake_png("g"));
        assert!(href(&l, "canva").is_none());
        assert!(load(None).is_empty());
        assert!(load(Some(&d.join("missing"))).is_empty());
        // Test scratch only (under the system temp dir).
        let _ = fs::remove_dir_all(&d);
    }
}
