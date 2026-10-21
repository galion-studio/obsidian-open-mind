// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! galion-release: the Rust side of `.github/workflows/release.yml`.
//!
//! * [`package()`]: collect every release ELF executable of one target
//!   (workspace + omnode target dirs), refuse anything that is not static,
//!   not stripped, the wrong machine or over the 2 MiB universal-CPU budget,
//!   and write `<name>-<tag>-<target>.tar.gz` (deterministic, with a
//!   `MANIFEST` of per-binary sizes and SHA-256).
//! * [`write_sums`] / [`verify`]: `SHA256SUMS` in `sha256sum` format, and a
//!   verifier that also opens every archive and re-checks its manifest and
//!   binaries.
//! * [`rehearse`]: extract each archive and start every binary with
//!   `--version` (natively, or under `qemu-<arch>-static` for a foreign arch);
//!   the output must name the binary and the release version.
//!
//! Std only plus miniz_oxide (pure-Rust deflate). See docs/ci-release-artefacts.md.

pub mod elf;
pub mod gzip;
pub mod rehearsal;
pub mod sha256;
pub mod tar;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// UNIVERSAL-CPU.md / scripts/size-budget.sh: 2 MiB per binary.
pub const SIZE_BUDGET: u64 = 2 * 1024 * 1024;
/// Name of the checksum file, as release.yml publishes it.
pub const SUMS: &str = "SHA256SUMS";
/// Manifest file inside every archive.
pub const MANIFEST: &str = "MANIFEST";

/// Test-only helper binaries the workspace builds that are not product
/// binaries. Excluded by default (owner decision D-216-2 in the docs).
pub const DEFAULT_EXCLUDE: &[&str] = &["gaia-dummy", "galion-bench-mock", "hermes-mock-model"];

pub fn sha256_hex(b: &[u8]) -> String {
    let mut s = sha256::Sha256::new();
    s.update(b);
    sha256::hex(&s.finish())
}

/// `v0.1.1-rc.1` -> `0.1.1`: the version every binary must report.
pub fn version_of_tag(tag: &str) -> String {
    let t = tag.strip_prefix('v').unwrap_or(tag);
    t.split(['-', '+']).next().unwrap_or(t).to_string()
}

/// Archive base name (also the top directory inside it).
pub fn archive_stem(name: &str, tag: &str, target: &str) -> String {
    format!("{name}-{tag}-{target}")
}

/// One checked binary.
#[derive(Debug, Clone)]
pub struct Bin {
    pub name: String,
    pub src: PathBuf,
    pub size: u64,
    pub sha256: String,
}

/// Every executable ELF file directly inside `dir` (what cargo leaves in
/// `target/<triple>/release`): skips `.d`, `.rlib`, `.so`, `.a`, build dirs.
pub fn elf_executables(dir: &Path) -> Result<Vec<PathBuf>, String> {
    use std::os::unix::fs::PermissionsExt;
    let rd = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut v = Vec::new();
    for ent in rd {
        let ent = ent.map_err(|e| e.to_string())?;
        let p = ent.path();
        let md = match fs::metadata(&p) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !md.is_file() || md.permissions().mode() & 0o111 == 0 {
            continue;
        }
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if n.contains('.') {
            continue; // .d .rlib .so .a ...
        }
        let mut head = [0u8; 4];
        let ok = fs::File::open(&p)
            .and_then(|mut f| f.read_exact(&mut head))
            .is_ok();
        if ok && elf::is_elf(&head) {
            v.push(p);
        }
    }
    v.sort();
    Ok(v)
}

/// The release gate for one binary's bytes. Returns every problem found.
pub fn check_binary(name: &str, bytes: &[u8], target: &str, budget: u64) -> Vec<String> {
    let mut bad = Vec::new();
    let want = elf::machine_for_target(target);
    match elf::inspect(bytes) {
        Err(e) => bad.push(format!("{name}: {e}")),
        Ok(i) => {
            if Some(i.machine) != want {
                bad.push(format!(
                    "{name}: machine {} != {target}",
                    elf::machine_name(i.machine)
                ));
            }
            if i.has_interp {
                bad.push(format!("{name}: has PT_INTERP (dynamic loader)"));
            }
            if i.needed > 0 {
                bad.push(format!("{name}: {} DT_NEEDED shared libraries", i.needed));
            }
            if i.has_symtab {
                bad.push(format!("{name}: not stripped (.symtab present)"));
            }
        }
    }
    if bytes.len() as u64 > budget {
        bad.push(format!("{name}: {} bytes > budget {budget}", bytes.len()));
    }
    bad
}

/// Options for [`package()`].
#[derive(Debug, Clone)]
pub struct PackageOpts {
    pub name: String,
    pub tag: String,
    pub target: String,
    pub bin_dirs: Vec<PathBuf>,
    pub exclude: Vec<String>,
    /// Extra files copied to the archive root (LICENSE, README.md, ...).
    pub extras: Vec<PathBuf>,
    pub out: PathBuf,
    pub budget: u64,
    /// tar mtime (SOURCE_DATE_EPOCH of the release commit).
    pub mtime: u64,
}

/// Result of [`package()`].
#[derive(Debug, Clone)]
pub struct Packaged {
    pub archive: PathBuf,
    pub bins: Vec<Bin>,
    pub excluded: Vec<String>,
    pub bytes: u64,
}

fn manifest_text(o: &PackageOpts, bins: &[Bin]) -> String {
    let mut m = String::new();
    m.push_str(&format!("# {} release manifest\n", o.name));
    m.push_str(&format!("tag: {}\n", o.tag));
    m.push_str(&format!("version: {}\n", version_of_tag(&o.tag)));
    m.push_str(&format!("target: {}\n", o.target));
    m.push_str(&format!("binaries: {}\n", bins.len()));
    m.push_str("# sha256  size  name\n");
    for b in bins {
        m.push_str(&format!("{}  {}  {}\n", b.sha256, b.size, b.name));
    }
    m
}

/// Parsed archive manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manifest {
    pub tag: String,
    pub version: String,
    pub target: String,
    /// name -> (sha256, size)
    pub bins: BTreeMap<String, (String, u64)>,
}

pub fn parse_manifest(s: &str) -> Result<Manifest, String> {
    let mut m = Manifest::default();
    let mut count: Option<usize> = None;
    for l in s.lines() {
        if l.starts_with('#') || l.trim().is_empty() {
            continue;
        }
        if let Some(v) = l.strip_prefix("tag: ") {
            m.tag = v.into();
        } else if let Some(v) = l.strip_prefix("version: ") {
            m.version = v.into();
        } else if let Some(v) = l.strip_prefix("target: ") {
            m.target = v.into();
        } else if let Some(v) = l.strip_prefix("binaries: ") {
            count = Some(v.parse().map_err(|_| "manifest: bad binaries count")?);
        } else {
            let f: Vec<&str> = l.split("  ").collect();
            if f.len() != 3 || f[0].len() != 64 {
                return Err(format!("manifest: bad line {l:?}"));
            }
            let size = f[1]
                .parse()
                .map_err(|_| format!("manifest: bad size {l:?}"))?;
            if m.bins.insert(f[2].into(), (f[0].into(), size)).is_some() {
                return Err(format!("manifest: duplicate {}", f[2]));
            }
        }
    }
    if m.target.is_empty() || m.version.is_empty() {
        return Err("manifest: missing target/version".into());
    }
    if count != Some(m.bins.len()) {
        return Err("manifest: binaries count does not match the list".into());
    }
    Ok(m)
}

/// Check and package one target's binaries.
pub fn package(o: &PackageOpts) -> Result<Packaged, String> {
    if elf::machine_for_target(&o.target).is_none() {
        return Err(format!("unsupported target {}", o.target));
    }
    if o.tag.is_empty() || o.tag.contains('/') || o.name.contains('/') {
        return Err("tag/name must be non-empty and contain no '/'".into());
    }
    let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut excluded = Vec::new();
    for d in &o.bin_dirs {
        for p in elf_executables(d)? {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            if o.exclude.iter().any(|x| x == &n) {
                excluded.push(n);
                continue;
            }
            if let Some(prev) = found.insert(n.clone(), p.clone()) {
                return Err(format!(
                    "binary {n} found twice: {} and {}",
                    prev.display(),
                    p.display()
                ));
            }
        }
    }
    if found.is_empty() {
        return Err("no ELF executables found in the bin dirs".into());
    }
    excluded.sort();
    excluded.dedup();
    let mut bins = Vec::new();
    let mut problems = Vec::new();
    let mut data = Vec::new();
    for (n, p) in &found {
        let b = fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
        problems.extend(check_binary(n, &b, &o.target, o.budget));
        bins.push(Bin {
            name: n.clone(),
            src: p.clone(),
            size: b.len() as u64,
            sha256: sha256_hex(&b),
        });
        data.push(b);
    }
    if !problems.is_empty() {
        return Err(format!("release gate failed:\n  {}", problems.join("\n  ")));
    }
    let stem = archive_stem(&o.name, &o.tag, &o.target);
    let mut entries = vec![tar::Entry {
        path: format!("{stem}/"),
        mode: 0o755,
        data: vec![],
    }];
    for (b, bytes) in bins.iter().zip(data) {
        entries.push(tar::Entry {
            path: format!("{stem}/{}", b.name),
            mode: 0o755,
            data: bytes,
        });
    }
    let mut extra_names: Vec<(String, Vec<u8>)> = Vec::new();
    for x in &o.extras {
        let n = x
            .file_name()
            .ok_or_else(|| format!("bad extra {}", x.display()))?
            .to_string_lossy()
            .to_string();
        if n == MANIFEST || found.contains_key(&n) {
            return Err(format!("extra file {n} clashes with a packaged name"));
        }
        extra_names.push((n, fs::read(x).map_err(|e| format!("{}: {e}", x.display()))?));
    }
    extra_names.sort();
    for (n, d) in extra_names {
        entries.push(tar::Entry {
            path: format!("{stem}/{n}"),
            mode: 0o644,
            data: d,
        });
    }
    entries.push(tar::Entry {
        path: format!("{stem}/{MANIFEST}"),
        mode: 0o644,
        data: manifest_text(o, &bins).into_bytes(),
    });
    let raw = tar::write(&entries, o.mtime)?;
    let mut g = gzip::GzWriter::new(Vec::new(), 9).map_err(|e| e.to_string())?;
    g.write_all(&raw).map_err(|e| e.to_string())?;
    let gz = g.finish().map_err(|e| e.to_string())?;
    fs::create_dir_all(&o.out).map_err(|e| format!("{}: {e}", o.out.display()))?;
    let archive = o.out.join(format!("{stem}.tar.gz"));
    fs::write(&archive, &gz).map_err(|e| format!("{}: {e}", archive.display()))?;
    Ok(Packaged {
        archive,
        bins,
        excluded,
        bytes: gz.len() as u64,
    })
}

/// Files that SHA256SUMS covers: archives and SBOMs, not signatures.
fn summed(name: &str) -> bool {
    name.ends_with(".tar.gz") || name.ends_with(".cdx.json")
}

/// Write `dir/SHA256SUMS` (`<hex>  <name>`, sorted), like `sha256sum`.
pub fn write_sums(dir: &Path) -> Result<Vec<(String, String)>, String> {
    let mut v = Vec::new();
    let rd = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for ent in rd {
        let p = ent.map_err(|e| e.to_string())?.path();
        let n = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_file() && summed(&n) {
            let b = fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            v.push((n, sha256_hex(&b)));
        }
    }
    if v.is_empty() {
        return Err(format!("nothing to sum in {}", dir.display()));
    }
    v.sort();
    let body: String = v.iter().map(|(n, h)| format!("{h}  {n}\n")).collect();
    fs::write(dir.join(SUMS), body).map_err(|e| e.to_string())?;
    Ok(v)
}

/// Parse SHA256SUMS (also accepts `./name` and the binary-mode `*name`).
pub fn parse_sums(s: &str) -> Result<BTreeMap<String, String>, String> {
    let mut m = BTreeMap::new();
    for l in s.lines().filter(|l| !l.trim().is_empty()) {
        let (h, n) = l
            .split_once(' ')
            .ok_or_else(|| format!("SHA256SUMS: bad line {l:?}"))?;
        let n = n.trim_start_matches([' ', '*']);
        let n = n.strip_prefix("./").unwrap_or(n);
        if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_hexdigit()) || n.contains('/') {
            return Err(format!("SHA256SUMS: bad line {l:?}"));
        }
        if m.insert(n.to_string(), h.to_ascii_lowercase()).is_some() {
            return Err(format!("SHA256SUMS: duplicate {n}"));
        }
    }
    Ok(m)
}

/// An archive opened and checked by [`open_archive`].
#[derive(Debug, Clone)]
pub struct Opened {
    pub manifest: Manifest,
    pub stem: String,
    pub entries: Vec<tar::Entry>,
}

/// Gunzip + untar an archive and check it against its own manifest and the
/// release gate (static, stripped, machine, budget).
pub fn open_archive(path: &Path, budget: u64) -> Result<Opened, String> {
    let n = path.file_name().unwrap().to_string_lossy().to_string();
    let stem = n
        .strip_suffix(".tar.gz")
        .ok_or_else(|| format!("{n}: not a .tar.gz"))?
        .to_string();
    let gz = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let raw = gzip::gunzip(&gz).map_err(|e| format!("{n}: {e}"))?;
    let entries = tar::read(&raw).map_err(|e| format!("{n}: {e}"))?;
    let pre = format!("{stem}/");
    for e in &entries {
        if !e.path.starts_with(&pre) || e.path[pre.len()..].contains('/') {
            return Err(format!("{n}: member {} outside {pre}", e.path));
        }
    }
    let mf = entries
        .iter()
        .find(|e| e.path == format!("{pre}{MANIFEST}"))
        .ok_or_else(|| format!("{n}: no {MANIFEST}"))?;
    let manifest =
        parse_manifest(&String::from_utf8_lossy(&mf.data)).map_err(|e| format!("{n}: {e}"))?;
    if !stem.ends_with(&format!("-{}-{}", manifest.tag, manifest.target)) {
        return Err(format!("{n}: name does not match manifest tag/target"));
    }
    let mut problems = Vec::new();
    let mut seen = 0usize;
    for e in &entries {
        let base = &e.path[pre.len()..];
        if let Some((h, size)) = manifest.bins.get(base) {
            seen += 1;
            if &sha256_hex(&e.data) != h || e.data.len() as u64 != *size {
                problems.push(format!("{base}: bytes differ from MANIFEST"));
            }
            if e.mode & 0o111 == 0 {
                problems.push(format!("{base}: not executable in archive"));
            }
            problems.extend(check_binary(base, &e.data, &manifest.target, budget));
        } else if !e.data.is_empty() && elf::is_elf(&e.data) {
            problems.push(format!("{base}: ELF not listed in MANIFEST"));
        }
    }
    if seen != manifest.bins.len() {
        problems.push(format!(
            "{} MANIFEST binaries, {seen} in archive",
            manifest.bins.len()
        ));
    }
    if !problems.is_empty() {
        return Err(format!("{n}:\n  {}", problems.join("\n  ")));
    }
    Ok(Opened {
        manifest,
        stem,
        entries,
    })
}

/// Outcome of [`verify`].
#[derive(Debug, Clone)]
pub struct Verified {
    pub files: usize,
    pub archives: Vec<Opened>,
}

/// Re-hash every file in SHA256SUMS, require every archive/SBOM in `dir` to
/// be listed, and open + check every archive.
pub fn verify(dir: &Path, budget: u64) -> Result<Verified, String> {
    let s = fs::read_to_string(dir.join(SUMS)).map_err(|e| format!("{SUMS}: {e}"))?;
    let sums = parse_sums(&s)?;
    let mut problems = Vec::new();
    for (n, h) in &sums {
        match fs::read(dir.join(n)) {
            Ok(b) if &sha256_hex(&b) == h => {}
            Ok(_) => problems.push(format!("{n}: FAILED (sha256 mismatch)")),
            Err(e) => problems.push(format!("{n}: missing ({e})")),
        }
    }
    let rd = fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut archives_on_disk = Vec::new();
    for ent in rd {
        let n = ent.map_err(|e| e.to_string())?.file_name();
        let n = n.to_string_lossy().to_string();
        if summed(&n) && !sums.contains_key(&n) {
            problems.push(format!("{n}: not listed in {SUMS}"));
        }
        if n.ends_with(".tar.gz") {
            archives_on_disk.push(n);
        }
    }
    archives_on_disk.sort();
    let mut archives = Vec::new();
    if problems.is_empty() {
        for n in &archives_on_disk {
            match open_archive(&dir.join(n), budget) {
                Ok(o) => archives.push(o),
                Err(e) => problems.push(e),
            }
        }
    }
    if !problems.is_empty() {
        return Err(format!("verify failed:\n  {}", problems.join("\n  ")));
    }
    Ok(Verified {
        files: sums.len(),
        archives,
    })
}

/// One `--version` run.
#[derive(Debug, Clone)]
pub struct Run {
    pub target: String,
    pub name: String,
    pub size: u64,
    pub runner: String,
    pub ok: bool,
    pub line: String,
    pub millis: u128,
}

/// How to run a foreign-arch binary: `qemu-<arch>-static` on PATH (or in
/// `qemu_dir`); `None` means native.
pub fn runner_for(target: &str, qemu_dir: Option<&Path>) -> Result<Option<PathBuf>, String> {
    let arch = target.split('-').next().unwrap_or("");
    let host = std::env::consts::ARCH;
    let qarch = match arch {
        a if a == host => return Ok(None),
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        a if a.starts_with("armv7") || a == "arm" => "arm",
        a => return Err(format!("no runner for arch {a}")),
    };
    let exe = format!("qemu-{qarch}-static");
    let dirs: Vec<PathBuf> = match qemu_dir {
        Some(d) => vec![d.to_path_buf()],
        None => std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default(),
    };
    for d in dirs {
        let p = d.join(&exe);
        if p.is_file() {
            return Ok(Some(p));
        }
    }
    Err(format!("{exe} not found (apt: qemu-user-static)"))
}

/// Run `exe args` with an empty environment (PATH only), stdin closed and a
/// hard timeout. Returns (exit ok, stdout, stderr, elapsed).
pub fn run_bounded(
    runner: Option<&Path>,
    exe: &Path,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
) -> Result<(bool, String, String, Duration), String> {
    let mut c = match runner {
        Some(q) => {
            let mut c = Command::new(q);
            c.arg(exe);
            c
        }
        None => Command::new(exe),
    };
    c.args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let t0 = Instant::now();
    let mut ch = c.spawn().map_err(|e| format!("{}: {e}", exe.display()))?;
    let mut out = ch.stdout.take().unwrap();
    let mut err = ch.stderr.take().unwrap();
    let ro = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let re = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = err.read_to_end(&mut s);
        s
    });
    let status = loop {
        if let Some(st) = ch.try_wait().map_err(|e| e.to_string())? {
            break Some(st);
        }
        if t0.elapsed() > timeout {
            let _ = ch.kill();
            let _ = ch.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let el = t0.elapsed();
    let so = String::from_utf8_lossy(&ro.join().unwrap_or_default()).to_string();
    let se = String::from_utf8_lossy(&re.join().unwrap_or_default()).to_string();
    match status {
        Some(st) => Ok((st.success(), so, se, el)),
        None => Ok((false, so, format!("timeout after {timeout:?}\n{se}"), el)),
    }
}

/// Extract the binaries of an opened archive into `dir/<stem>/`.
pub fn extract(o: &Opened, dir: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::PermissionsExt;
    let root = dir.join(&o.stem);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for e in &o.entries {
        if e.is_dir() {
            continue;
        }
        let p = dir.join(&e.path);
        fs::write(&p, &e.data).map_err(|er| format!("{}: {er}", p.display()))?;
        fs::set_permissions(&p, fs::Permissions::from_mode(e.mode & 0o755))
            .map_err(|er| er.to_string())?;
    }
    Ok(root)
}

/// Start every packaged binary with `--version`; the first stdout line must
/// be `<name> <version>` optionally followed by ` (...)`.
pub fn rehearse(
    v: &Verified,
    work: &Path,
    expect_version: Option<&str>,
    qemu_dir: Option<&Path>,
    timeout: Duration,
) -> Result<Vec<Run>, String> {
    let mut runs = Vec::new();
    for o in &v.archives {
        let root = extract(o, work)?;
        let runner = runner_for(&o.manifest.target, qemu_dir)?;
        let want = expect_version.unwrap_or(&o.manifest.version);
        for (name, (_, size)) in &o.manifest.bins {
            let (ok, so, se, el) = run_bounded(
                runner.as_deref(),
                &root.join(name),
                &["--version"],
                &root,
                timeout,
            )?;
            let line = so.lines().next().unwrap_or("").trim().to_string();
            let good = ok && version_line_ok(&line, name, want);
            runs.push(Run {
                target: o.manifest.target.clone(),
                name: name.clone(),
                size: *size,
                runner: runner
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "native".into()),
                ok: good,
                line: if line.is_empty() {
                    format!(
                        "(no stdout; stderr: {})",
                        se.lines().next().unwrap_or("").trim()
                    )
                } else {
                    line
                },
                millis: el.as_millis(),
            });
        }
    }
    Ok(runs)
}

/// `omnode 0.1.0`, `tuner 0.1.0 (commit x, built y)` are fine for name
/// `tuner`, version `0.1.0`.
pub fn version_line_ok(line: &str, name: &str, version: &str) -> bool {
    let want = format!("{name} {version}");
    line == want || line.starts_with(&format!("{want} ("))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_version() {
        assert_eq!(version_of_tag("v0.1.1"), "0.1.1");
        assert_eq!(version_of_tag("v0.1.1-rc.1"), "0.1.1");
        assert_eq!(version_of_tag("0.2.0+build"), "0.2.0");
    }

    #[test]
    fn version_lines() {
        assert!(version_line_ok("tuner 0.1.0", "tuner", "0.1.0"));
        assert!(version_line_ok(
            "tuner 0.1.0 (commit abc, built t)",
            "tuner",
            "0.1.0"
        ));
        assert!(!version_line_ok("tuner 0.1.01", "tuner", "0.1.0"));
        assert!(!version_line_ok("tuner-x 0.1.0", "tuner", "0.1.0"));
        assert!(!version_line_ok("usage: tuner", "tuner", "0.1.0"));
    }

    #[test]
    fn sums_parse() {
        let h = "a".repeat(64);
        let m = parse_sums(&format!("{h}  x.tar.gz\n{h} *./y.cdx.json\n")).unwrap();
        assert_eq!(m.len(), 2);
        assert!(m.contains_key("y.cdx.json"));
        assert!(parse_sums(&format!("{h}  ../x\n")).is_err());
        assert!(parse_sums("abc  x\n").is_err());
        assert!(parse_sums(&format!("{h}  x\n{h}  x\n")).is_err());
    }

    #[test]
    fn manifest_roundtrip() {
        let o = PackageOpts {
            name: "obsidian-open-mind".into(),
            tag: "v0.1.0".into(),
            target: "aarch64-unknown-linux-musl".into(),
            bin_dirs: vec![],
            exclude: vec![],
            extras: vec![],
            out: PathBuf::new(),
            budget: SIZE_BUDGET,
            mtime: 0,
        };
        let bins = vec![Bin {
            name: "tuner".into(),
            src: PathBuf::new(),
            size: 7,
            sha256: "0".repeat(64),
        }];
        let m = parse_manifest(&manifest_text(&o, &bins)).unwrap();
        assert_eq!(m.version, "0.1.0");
        assert_eq!(m.bins["tuner"].1, 7);
        assert!(parse_manifest("target: x\nversion: 1\nbinaries: 2\n").is_err());
    }

    #[test]
    fn gate_rejects_host_binary() {
        let me = fs::read(std::env::current_exe().unwrap()).unwrap();
        let p = check_binary("me", &me, "aarch64-unknown-linux-musl", 1024);
        assert!(p.iter().any(|x| x.contains("machine")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("budget")), "{p:?}");
    }
}
