// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Pinned CI tools: one table of versions and sha256 sums (previously spread
// over launch-gates.yml env, self-eval.yml and scripts/install-zig.sh).
// Download with curl, verify with `sha256sum -c`, unpack with tar. Nothing is
// installed system-wide: binaries land in tools_root()/bin.

use crate::run::{append_github, cmd, status, tools_root, Res};
use std::path::{Path, PathBuf};

pub struct Pin {
    pub name: &'static str,
    pub version: &'static str,
    /// Download URL; `{v}` is replaced by `version`.
    pub url: &'static str,
    /// sha256 of the downloaded archive; empty for `cargo install` tools.
    pub sha256: &'static str,
}

pub const PINS: [Pin; 5] = [
    Pin {
        name: "gitleaks",
        version: "8.30.1",
        url: "https://github.com/gitleaks/gitleaks/releases/download/v{v}/gitleaks_{v}_linux_x64.tar.gz",
        sha256: "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb",
    },
    Pin {
        name: "cargo-deny",
        version: "0.20.2",
        url: "https://github.com/EmbarkStudios/cargo-deny/releases/download/{v}/cargo-deny-{v}-x86_64-unknown-linux-musl.tar.gz",
        sha256: "9f12ed4c49936e09b48bf862b595cde2fe64fcbd9d74dfacac6131ca824c8d5f",
    },
    Pin {
        name: "cargo-cyclonedx",
        version: "0.5.9",
        url: "https://github.com/CycloneDX/cyclonedx-rust-cargo/releases/download/cargo-cyclonedx-{v}/cargo-cyclonedx-x86_64-unknown-linux-gnu.tar.xz",
        sha256: "fb8dbee9f182173e062a64a387b21a0badc6fab8b2abf9294973f012972bf6d8",
    },
    // comply (Rust REUSE linter) ships no binaries: built from crates.io with
    // its own Cargo.lock (`cargo install --locked`).
    Pin {
        name: "comply",
        version: "0.1.1",
        url: "",
        sha256: "",
    },
    // zig cc: the C cross compiler for bundled SQLite (ucpu gate).
    Pin {
        name: "zig",
        version: "0.15.2",
        url: "https://ziglang.org/download/{v}/zig-x86_64-linux-{v}.tar.xz",
        sha256: "02aa270f183da276e5b5920b1dac44a63f1a49e55050ebde3aecc9eb82f93239",
    },
];

pub fn pin(name: &str) -> Option<&'static Pin> {
    PINS.iter().find(|p| p.name == name)
}

pub fn bin_dir() -> PathBuf {
    tools_root().join("bin")
}

pub fn url_of(p: &Pin) -> String {
    p.url.replace("{v}", p.version)
}

/// Install one pinned tool (idempotent) and return the path of its binary.
pub fn install(name: &str) -> Result<PathBuf, String> {
    let p = pin(name).ok_or_else(|| format!("no pin for tool '{name}'"))?;
    let bin = bin_dir();
    std::fs::create_dir_all(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    let exe = if p.name == "zig" {
        tools_root().join("zig").join("zig")
    } else {
        bin.join(p.name)
    };
    if exe.is_file() {
        return Ok(exe);
    }
    if p.url.is_empty() {
        let root = tools_root();
        let r = root.to_string_lossy();
        status(cmd(
            &crate::run::cargo_bin(),
            &[
                "install",
                "--locked",
                p.name,
                "--version",
                p.version,
                "--root",
                &r,
            ],
        ))?;
        return Ok(exe);
    }
    let work = tools_root().join(format!("dl-{}", p.name));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let url = url_of(p);
    let file = url.rsplit('/').next().unwrap_or("download");
    let mut c = cmd("curl", &["-sSfL", "-o", file, &url]);
    c.current_dir(&work);
    status(c)?;
    let mut c = cmd("sha256sum", &["-c", "-"]);
    c.current_dir(&work).stdin(std::process::Stdio::piped());
    verify_sha(c, &format!("{}  {file}\n", p.sha256))?;
    if p.name == "zig" {
        let dest = tools_root().join("zig");
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        let d = dest.to_string_lossy();
        let mut c = cmd("tar", &["-C", &d, "--strip-components=1", "-xJf", file]);
        c.current_dir(&work);
        status(c)?;
    } else {
        let flag = if file.ends_with(".xz") {
            "-xJf"
        } else {
            "-xzf"
        };
        let mut c = cmd("tar", &[flag, file]);
        c.current_dir(&work);
        status(c)?;
        let found = find_file(&work, p.name).ok_or_else(|| format!("{} not in archive", p.name))?;
        std::fs::rename(&found, &exe).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(&work);
    Ok(exe)
}

fn verify_sha(mut c: std::process::Command, line: &str) -> Res {
    use std::io::Write;
    let mut child = c.spawn().map_err(|e| format!("sha256sum: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("sha256sum: no stdin")?
        .write_all(line.as_bytes())
        .map_err(|e| e.to_string())?;
    let st = child.wait().map_err(|e| e.to_string())?;
    if st.success() {
        Ok(())
    } else {
        Err("sha256 mismatch: refusing to use the download".into())
    }
}

fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        if p.is_dir() {
            if let Some(f) = find_file(&p, name) {
                return Some(f);
            }
        } else if p.file_name().is_some_and(|n| n == name) {
            return Some(p);
        }
    }
    None
}

/// `cargo xtask install TOOL...`: install, export GALION_TOOLS_BIN (and ZIG)
/// to $GITHUB_ENV when on a runner, print the bin dir.
pub fn install_cmd(names: &[String]) -> Res {
    if names.is_empty() {
        let all: Vec<&str> = PINS.iter().map(|p| p.name).collect();
        return Err(format!("name one or more tools: {}", all.join(" ")));
    }
    for n in names {
        let exe = install(n)?;
        if n == "zig" {
            append_github("GITHUB_ENV", &format!("ZIG={}\n", exe.display()))?;
        }
        eprintln!("installed {n} -> {}", exe.display());
    }
    append_github(
        "GITHUB_ENV",
        &format!("GALION_TOOLS_BIN={}\n", bin_dir().display()),
    )?;
    println!("{}", bin_dir().display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_are_complete() {
        for p in &PINS {
            assert!(!p.version.is_empty(), "{}", p.name);
            if p.url.is_empty() {
                assert!(
                    p.sha256.is_empty(),
                    "{}: cargo-install pin has a sha",
                    p.name
                );
            } else {
                assert!(url_of(p).starts_with("https://"), "{}", p.name);
                assert!(!url_of(p).contains("{v}"), "{}", p.name);
                assert_eq!(p.sha256.len(), 64, "{}", p.name);
                assert!(
                    p.sha256.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{}",
                    p.name
                );
            }
        }
        assert_eq!(
            url_of(pin("zig").unwrap()),
            "https://ziglang.org/download/0.15.2/zig-x86_64-linux-0.15.2.tar.xz"
        );
        assert!(pin("nope").is_none());
    }
}
