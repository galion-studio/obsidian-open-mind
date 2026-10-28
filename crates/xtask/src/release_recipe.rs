// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// `cargo xtask release-recipe`: build a release artefact from a git commit
// with a pinned, recorded recipe, and prove later that the recipe still
// reproduces the bytes (wave-4 slot 434, D-416-1 / D-416-3).
//
// Every artefact gets two files next to it:
//   <artefact>.sha256  `sha256sum -c` format
//   <artefact>.recipe  key=value text: commit, package, bin, target, C
//                      compiler choice, exact cargo argv, the environment
//                      the build saw, toolchain versions, size, sha256
//
// The build runs in a cleared environment (only PATH, HOME, CARGO_HOME,
// RUSTUP_HOME, RUSTUP_TOOLCHAIN, TMPDIR and ZIG_GLOBAL_CACHE_DIR pass), from a
// `git archive` of the commit (never the working tree), into a fresh target
// directory. So CFLAGS, CRATE_CC_NO_DEFAULTS, RUSTFLAGS or a stray CC in the
// caller's shell cannot change the bytes (the cause of F-416-1).
//
// std only (+ the std-only galion-common for SHA-256 and Zagreb time).
// Shells out to git, tar, cargo, rustc and the C compiler.

use galion_common::sha256;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const FORMAT: &str = "galion-release-recipe/1";
const DEFAULT_PACKAGE: &str = "hermes-gateway";
const DEFAULT_TARGET: &str = "x86_64-unknown-linux-musl";
const PASS_ENV: [&str; 7] = [
    "PATH",
    "HOME",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "RUSTUP_TOOLCHAIN",
    "TMPDIR",
    "ZIG_GLOBAL_CACHE_DIR",
];
/// Recipe keys that pin the toolchain; any drift makes `verify` fail before it builds.
pub const TOOLCHAIN_KEYS: [&str; 6] = ["rustc", "rustc_commit", "llvm", "cargo", "cc_version", "lld"];

pub const USAGE: &str = "\
  release-recipe build --commit REV --out DIR [--cc gcc|zig] [-p PKG] [--bin BIN]
                       [--target TRIPLE] [--features F] [--jobs N] [--work DIR]
                       [--source-date-epoch N] [--keep]
                          build PKG (default hermes-gateway) from `git archive REV`
                          in a cleared env + fresh target dir; write
                          DIR/<bin>-<rev8>[-zig] + .sha256 + .recipe; fail if over
                          the config/release/size-budget.tsv budget
  release-recipe verify ARTEFACT [--work DIR] [--keep]
                          artefact sha == recipe sha, toolchain == recorded, then
                          rebuild from the recipe in another path; fail unless the
                          sha256 is identical
  release-recipe check ARTEFACT
                          no build: artefact sha/size == recipe, within budget
  (env XTASK_CARGO: cargo program to run, default `cargo`)
";

/// One parsed recipe. Insertion order of `keys` is the file order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Recipe {
    pub map: BTreeMap<String, String>,
}

impl Recipe {
    pub fn get(&self, k: &str) -> Result<&str, String> {
        self.map
            .get(k)
            .map(|s| s.as_str())
            .ok_or_else(|| format!("recipe has no '{k}'"))
    }

    pub fn parse(text: &str) -> Result<Recipe, String> {
        let mut map = BTreeMap::new();
        for (n, line) in text.lines().enumerate() {
            let line = line.trim_end();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| format!("recipe line {}: no '='", n + 1))?;
            if map.insert(k.trim().to_string(), v.to_string()).is_some() {
                return Err(format!("recipe line {}: duplicate key '{}'", n + 1, k.trim()));
            }
        }
        let r = Recipe { map };
        if r.get("format")? != FORMAT {
            return Err(format!("recipe format is not {FORMAT}"));
        }
        for k in [
            "commit", "package", "bin", "target", "cc", "source_date_epoch", "cargo_argv",
            "sha256", "size",
        ] {
            r.get(k)?;
        }
        Ok(r)
    }

    pub fn render(&self) -> String {
        let mut s = String::from(
            "# SPDX-License-Identifier: MIT\n# Copyright (c) 3000 Galion / galion-studio\n\
             # Release recipe written by `cargo xtask release-recipe build`; verify with\n\
             # `cargo xtask release-recipe verify <artefact>`.\n",
        );
        // Stable, readable order: identity, inputs, toolchain, outputs.
        let order = [
            "format", "artefact", "commit", "package", "bin", "target", "features", "cc",
            "source_date_epoch", "cargo_argv", "env", "rustc", "rustc_commit", "llvm",
            "cargo", "cc_path", "cc_version", "ar", "lld", "size", "budget", "sha256",
            "built_at",
        ];
        for k in order {
            if let Some(v) = self.map.get(k) {
                s.push_str(&format!("{k}={v}\n"));
            }
        }
        for (k, v) in &self.map {
            if !order.contains(&k.as_str()) {
                s.push_str(&format!("{k}={v}\n"));
            }
        }
        s
    }
}

/// Inputs that decide the bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub commit: String,
    pub package: String,
    pub bin: String,
    pub target: String,
    pub features: String,
    pub cc: String,
    pub source_date_epoch: String,
    pub jobs: String,
}

impl Spec {
    /// cargo argv (after the program name). `-j` is not part of the recipe:
    /// parallelism does not change the bytes (proved by slots 312/325).
    pub fn cargo_argv(&self) -> Vec<String> {
        let mut a: Vec<String> = ["build", "--release", "--locked", "--offline", "-p"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        a.push(self.package.clone());
        a.push("--bin".into());
        a.push(self.bin.clone());
        a.push("--target".into());
        a.push(self.target.clone());
        if !self.features.is_empty() {
            a.push("--features".into());
            a.push(self.features.clone());
        }
        a
    }

    fn from_recipe(r: &Recipe) -> Result<Spec, String> {
        Ok(Spec {
            commit: r.get("commit")?.into(),
            package: r.get("package")?.into(),
            bin: r.get("bin")?.into(),
            target: r.get("target")?.into(),
            features: r.map.get("features").cloned().unwrap_or_default(),
            cc: r.get("cc")?.into(),
            source_date_epoch: r.get("source_date_epoch")?.into(),
            jobs: "2".into(),
        })
    }
}

/// Rust triple -> (env suffix, gcc driver, zig triple).
fn triple_tools(target: &str) -> Result<(String, &'static str, &'static str), String> {
    let (gcc, zig) = match target {
        "x86_64-unknown-linux-musl" => ("x86_64-linux-musl-gcc", "x86_64-linux-musl"),
        "aarch64-unknown-linux-musl" => ("aarch64-linux-musl-gcc", "aarch64-linux-musl"),
        "armv7-unknown-linux-musleabihf" => ("arm-linux-musleabihf-gcc", "arm-linux-musleabihf"),
        other => return Err(format!("unsupported target '{other}' (musl targets only)")),
    };
    Ok((target.replace('-', "_"), gcc, zig))
}

fn run_out(cmd: &mut Command) -> Result<String, String> {
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run {:?}: {e}", cmd.get_program()))?;
    if !out.status.success() {
        return Err(format!(
            "{:?} failed ({}): {}",
            cmd.get_program(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn which(prog: &str) -> Result<PathBuf, String> {
    if prog.contains('/') {
        return Ok(PathBuf::from(prog));
    }
    let path = std::env::var("PATH").unwrap_or_default();
    for d in path.split(':').filter(|d| !d.is_empty()) {
        let p = Path::new(d).join(prog);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(format!("'{prog}' not found on PATH"))
}

/// A Command with the cleared, recorded environment.
fn clean_cmd(prog: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut c = Command::new(prog);
    c.env_clear();
    for k in PASS_ENV {
        if let Ok(v) = std::env::var(k) {
            c.env(k, v);
        }
    }
    c.env("LC_ALL", "C");
    c
}

/// (env pairs for the cargo build, cc path, ar description).
fn cc_env(spec: &Spec) -> Result<(Vec<(String, String)>, String, String), String> {
    let (suffix, gcc, zig_triple) = triple_tools(&spec.target)?;
    match spec.cc.as_str() {
        "gcc" => {
            let cc = which(gcc)?.to_string_lossy().to_string();
            let ar = "ar".to_string();
            Ok((
                vec![(format!("CC_{suffix}"), cc.clone()), (format!("AR_{suffix}"), ar.clone())],
                cc,
                ar,
            ))
        }
        "zig" => {
            // cc-rs splits CC on whitespace: program + args. The program is this
            // xtask binary, which drops cc-rs's `--target=<rust triple>` (zig
            // rejects it) and execs `zig cc -target <zig triple>`: the same
            // command line as the scripts/ucpu-gate.sh wrappers, no shell script.
            let me = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
            let me = me.to_string_lossy().to_string();
            if me.contains(char::is_whitespace) {
                return Err(format!("xtask path has whitespace: {me}"));
            }
            let zig = which("zig")?.to_string_lossy().to_string();
            let cc = format!("{me} zig-cc {zig} {zig_triple}");
            let ar = format!("{zig} ar");
            Ok((
                vec![(format!("CC_{suffix}"), cc.clone()), (format!("AR_{suffix}"), ar.clone())],
                cc,
                ar,
            ))
        }
        other => Err(format!("--cc must be gcc or zig, not '{other}'")),
    }
}

/// Toolchain facts recorded in (and compared against) the recipe.
fn toolchain(spec: &Spec) -> Result<BTreeMap<String, String>, String> {
    let mut m = BTreeMap::new();
    let vv = run_out(clean_cmd("rustc").arg("-vV"))?;
    for line in vv.lines() {
        if let Some(v) = line.strip_prefix("release: ") {
            m.insert("rustc".into(), format!("rustc {v}"));
        } else if let Some(v) = line.strip_prefix("commit-hash: ") {
            m.insert("rustc_commit".into(), v.into());
        } else if let Some(v) = line.strip_prefix("LLVM version: ") {
            m.insert("llvm".into(), v.into());
        }
    }
    // Always the real cargo for the version, even when XTASK_CARGO is a queue wrapper.
    m.insert("cargo".into(), run_out(clean_cmd("cargo").arg("-V"))?);
    let (_, cc_path, ar) = cc_env(spec)?;
    let cc_version = match spec.cc.as_str() {
        "zig" => format!(
            "zig {}",
            run_out(clean_cmd(which("zig")?).arg("version"))?
        ),
        _ => run_out(clean_cmd(&cc_path).arg("--version"))?
            .lines()
            .next()
            .unwrap_or("")
            .to_string(),
    };
    m.insert("cc_version".into(), cc_version);
    m.insert(
        "cc_path".into(),
        if spec.cc == "zig" {
            "xtask zig-cc <zig> <zig triple> (drops --target=)".into()
        } else {
            cc_path
        },
    );
    m.insert(
        "ar".into(),
        if spec.cc == "zig" { "zig ar".into() } else { ar },
    );
    let sysroot = run_out(clean_cmd("rustc").args(["--print", "sysroot"]))?;
    let host = vv
        .lines()
        .find_map(|l| l.strip_prefix("host: "))
        .unwrap_or("x86_64-unknown-linux-gnu")
        .to_string();
    let lld = Path::new(&sysroot).join(format!("lib/rustlib/{host}/bin/rust-lld"));
    let lld_v = run_out(clean_cmd(&lld).args(["-flavor", "gnu", "--version"]))
        .unwrap_or_else(|_| "unknown".into());
    m.insert("lld".into(), lld_v.lines().next().unwrap_or("").to_string());
    Ok(m)
}

fn cargo_prog() -> String {
    std::env::var("XTASK_CARGO")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "cargo".into())
}

fn repo_root() -> PathBuf {
    let dir = std::env::var("CARGO_MANIFEST_DIR")
        .ok()
        .filter(|s| s.ends_with("crates/xtask"))
        .unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").to_string());
    PathBuf::from(dir).join("../..")
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(8)]
}

pub fn artefact_name(spec: &Spec) -> String {
    let base = format!("{}-{}", spec.bin, short(&spec.commit));
    if spec.cc == "gcc" {
        base
    } else {
        format!("{base}-{}", spec.cc)
    }
}

/// Budget for `bin` from a size-budget.tsv text (`default` row as fallback).
pub fn budget_for(tsv: &str, bin: &str) -> Option<u64> {
    let mut default = None;
    for line in tsv.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(name), Some(n)) = (it.next(), it.next()) else {
            continue;
        };
        let Ok(n) = n.parse::<u64>() else { continue };
        if name == bin {
            return Some(n);
        }
        if name == "default" {
            default = Some(n);
        }
    }
    default
}

/// Export `commit` into `src` (must not exist) with git archive + tar.
fn export_source(commit: &str, src: &Path) -> Result<(), String> {
    fs::create_dir_all(src).map_err(|e| format!("mkdir {src:?}: {e}"))?;
    let tar = src.with_extension("tar");
    run_out(
        Command::new("git")
            .arg("-C")
            .arg(repo_root())
            .args(["archive", "--format=tar", "-o"])
            .arg(&tar)
            .arg(commit),
    )?;
    run_out(Command::new("tar").arg("-xf").arg(&tar).arg("-C").arg(src))?;
    let _ = fs::remove_file(&tar);
    Ok(())
}

pub struct Built {
    pub path: PathBuf,
    pub sha256: String,
    pub size: u64,
    pub budget: Option<u64>,
    pub recipe: Recipe,
}

/// Build `spec` under `work` (fresh) and return the binary + its recipe.
fn build_in(spec: &Spec, work: &Path) -> Result<Built, String> {
    if work.exists() {
        return Err(format!("work dir {work:?} exists; it must be fresh"));
    }
    let src = work.join(format!("src-{}", short(&spec.commit)));
    let target_dir = work.join("target");
    export_source(&spec.commit, &src)?;
    let tools = toolchain(spec)?;
    let (cc_pairs, _, _) = cc_env(spec)?;
    let mut env: Vec<(String, String)> = vec![
        ("GALION_BUILD_COMMIT".into(), spec.commit.clone()),
        ("SOURCE_DATE_EPOCH".into(), spec.source_date_epoch.clone()),
        ("CARGO_INCREMENTAL".into(), "0".into()),
    ];
    env.extend(cc_pairs);
    let argv = spec.cargo_argv();
    let mut cmd = clean_cmd(cargo_prog());
    cmd.current_dir(&src)
        .env("CARGO_TARGET_DIR", &target_dir)
        .args(&argv[..1])
        .arg("-j")
        .arg(&spec.jobs)
        .args(&argv[1..]);
    for (k, v) in &env {
        cmd.env(k, v);
    }
    eprintln!(
        "release-recipe: building {} {} cc={} in {:?}",
        spec.package,
        short(&spec.commit),
        spec.cc,
        src
    );
    let status = cmd
        .status()
        .map_err(|e| format!("cannot run {}: {e}", cargo_prog()))?;
    if !status.success() {
        return Err(format!("cargo build failed ({status})"));
    }
    let path = target_dir.join(&spec.target).join("release").join(&spec.bin);
    let bytes = fs::read(&path).map_err(|e| format!("read {path:?}: {e}"))?;
    let tsv = fs::read_to_string(src.join("config/release/size-budget.tsv")).unwrap_or_default();
    let budget = budget_for(&tsv, &spec.bin);
    let sha = sha256::hex(&bytes);
    let mut map = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        map.insert(k.to_string(), v);
    };
    put("format", FORMAT.into());
    put("artefact", artefact_name(spec));
    put("commit", spec.commit.clone());
    put("package", spec.package.clone());
    put("bin", spec.bin.clone());
    put("target", spec.target.clone());
    put("features", spec.features.clone());
    put("cc", spec.cc.clone());
    put("source_date_epoch", spec.source_date_epoch.clone());
    put("cargo_argv", format!("cargo {}", argv.join(" ")));
    put(
        "env",
        format!(
            "cleared except {}; LC_ALL=C {}",
            PASS_ENV.join(","),
            env.iter()
                .map(|(k, v)| {
                    let v = if k.starts_with("CC_") && spec.cc == "zig" {
                        "<xtask> zig-cc <zig> <zig triple>".to_string()
                    } else {
                        v.clone()
                    };
                    format!("{k}={}", v.replace(' ', "\\ "))
                })
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );
    for (k, v) in tools {
        put(&k, v);
    }
    put("size", bytes.len().to_string());
    put(
        "budget",
        budget.map(|b| b.to_string()).unwrap_or_else(|| "none".into()),
    );
    put("sha256", sha.clone());
    put("built_at", galion_common::time::now_rfc3339());
    Ok(Built {
        path,
        sha256: sha,
        size: bytes.len() as u64,
        budget,
        recipe: Recipe { map },
    })
}

/// Differences between the recorded toolchain and `now` (key, recorded, now).
pub fn toolchain_drift(
    recipe: &Recipe,
    now: &BTreeMap<String, String>,
) -> Vec<(String, String, String)> {
    let mut d = vec![];
    for k in TOOLCHAIN_KEYS {
        let rec = recipe.map.get(k).cloned().unwrap_or_default();
        let cur = now.get(k).cloned().unwrap_or_default();
        if rec != cur {
            d.push((k.to_string(), rec, cur));
        }
    }
    d
}

/// The rebuild check itself: Ok when the rebuilt bytes are the recorded bytes.
pub fn compare_rebuild(recorded_sha: &str, rebuilt_sha: &str) -> Result<(), String> {
    if recorded_sha == rebuilt_sha {
        Ok(())
    } else {
        Err(format!(
            "recipe does NOT reproduce the artefact: recorded sha256 {recorded_sha}, rebuilt {rebuilt_sha}"
        ))
    }
}

fn sha_of(path: &Path) -> Result<(String, u64), String> {
    let b = fs::read(path).map_err(|e| format!("read {path:?}: {e}"))?;
    Ok((sha256::hex(&b), b.len() as u64))
}

fn recipe_path(artefact: &Path) -> PathBuf {
    let mut s = artefact.as_os_str().to_owned();
    s.push(".recipe");
    PathBuf::from(s)
}

fn load(artefact: &Path) -> Result<Recipe, String> {
    let p = recipe_path(artefact);
    let text = fs::read_to_string(&p).map_err(|e| format!("read {p:?}: {e}"))?;
    Recipe::parse(&text)
}

/// Artefact bytes == recipe; size within the recorded budget.
pub fn check(artefact: &Path) -> Result<Recipe, String> {
    let r = load(artefact)?;
    let (sha, size) = sha_of(artefact)?;
    if sha != r.get("sha256")? {
        return Err(format!(
            "artefact sha256 {sha} != recipe sha256 {}",
            r.get("sha256")?
        ));
    }
    if size.to_string() != r.get("size")? {
        return Err(format!("artefact size {size} != recipe size {}", r.get("size")?));
    }
    if let Some(Ok(b)) = r.map.get("budget").map(|b| b.parse::<u64>()) {
        if size > b {
            return Err(format!("artefact {size} B is over its budget {b} B"));
        }
    }
    let spec = Spec::from_recipe(&r)?;
    let want = format!("cargo {}", spec.cargo_argv().join(" "));
    if r.get("cargo_argv")? != want {
        return Err(format!(
            "recipe cargo_argv was edited or comes from another tool version:\n  recipe: {}\n  tool:   {want}",
            r.get("cargo_argv")?
        ));
    }
    Ok(r)
}

fn take(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

pub fn run(args: &[String]) -> Result<(), String> {
    let Some((sub, rest)) = args.split_first() else {
        return Err(format!("missing subcommand\n{USAGE}"));
    };
    match sub.as_str() {
        "build" => cmd_build(rest),
        "verify" => cmd_verify(rest),
        "check" => {
            let a = rest.first().ok_or("check needs ARTEFACT")?;
            let r = check(Path::new(a))?;
            println!(
                "release-recipe check OK: {a} {} B sha256 {} (budget {})",
                r.get("size")?,
                r.get("sha256")?,
                r.map.get("budget").map(String::as_str).unwrap_or("none")
            );
            Ok(())
        }
        other => Err(format!("unknown release-recipe subcommand '{other}'\n{USAGE}")),
    }
}

fn cmd_build(args: &[String]) -> Result<(), String> {
    let (mut commit, mut out, mut work) = (None, None, None);
    let mut spec = Spec {
        commit: String::new(),
        package: DEFAULT_PACKAGE.into(),
        bin: String::new(),
        target: DEFAULT_TARGET.into(),
        features: String::new(),
        cc: "gcc".into(),
        source_date_epoch: String::new(),
        jobs: "2".into(),
    };
    let mut keep = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--commit" => commit = Some(take(args, &mut i, a)?),
            "--out" => out = Some(PathBuf::from(take(args, &mut i, a)?)),
            "--work" => work = Some(PathBuf::from(take(args, &mut i, a)?)),
            "--cc" => spec.cc = take(args, &mut i, a)?,
            "-p" | "--package" => spec.package = take(args, &mut i, a)?,
            "--bin" => spec.bin = take(args, &mut i, a)?,
            "--target" => spec.target = take(args, &mut i, a)?,
            "--features" => spec.features = take(args, &mut i, a)?,
            "--jobs" | "-j" => spec.jobs = take(args, &mut i, a)?,
            "--source-date-epoch" => spec.source_date_epoch = take(args, &mut i, a)?,
            "--keep" => keep = true,
            other => return Err(format!("unknown build flag '{other}'")),
        }
        i += 1;
    }
    let rev = commit.ok_or("--commit REV is required")?;
    let out = out.ok_or("--out DIR is required")?;
    spec.commit = run_out(
        Command::new("git")
            .arg("-C")
            .arg(repo_root())
            .args(["rev-parse", "--verify"])
            .arg(format!("{rev}^{{commit}}")),
    )?;
    if spec.bin.is_empty() {
        spec.bin = spec.package.clone();
    }
    if spec.source_date_epoch.is_empty() {
        spec.source_date_epoch = run_out(
            Command::new("git")
                .arg("-C")
                .arg(repo_root())
                .args(["show", "-s", "--format=%ct"])
                .arg(&spec.commit),
        )?;
    }
    fs::create_dir_all(&out).map_err(|e| format!("mkdir {out:?}: {e}"))?;
    let work = work.unwrap_or_else(|| out.join(format!(".work-{}", std::process::id())));
    let res = build_in(&spec, &work);
    let result = res.and_then(|b| {
        let name = artefact_name(&spec);
        let dest = out.join(&name);
        fs::copy(&b.path, &dest).map_err(|e| format!("copy to {dest:?}: {e}"))?;
        fs::write(out.join(format!("{name}.sha256")), format!("{}  {name}\n", b.sha256))
            .map_err(|e| format!("write sha256: {e}"))?;
        fs::write(recipe_path(&dest), b.recipe.render())
            .map_err(|e| format!("write recipe: {e}"))?;
        println!("release-recipe build: {} {} B sha256 {}", dest.display(), b.size, b.sha256);
        match b.budget {
            Some(lim) if b.size > lim => Err(format!(
                "{} is {} B, over its budget {lim} B",
                dest.display(),
                b.size
            )),
            Some(lim) => {
                println!("release-recipe build: within budget {lim} B (headroom {} B)", lim - b.size);
                Ok(())
            }
            None => Ok(()),
        }
    });
    if !keep {
        let _ = fs::remove_dir_all(&work);
    }
    result
}

fn cmd_verify(args: &[String]) -> Result<(), String> {
    let mut artefact = None;
    let mut work = None;
    let mut keep = false;
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "--work" => work = Some(PathBuf::from(take(args, &mut i, a)?)),
            "--keep" => keep = true,
            other if !other.starts_with('-') && artefact.is_none() => {
                artefact = Some(PathBuf::from(other))
            }
            other => return Err(format!("unknown verify argument '{other}'")),
        }
        i += 1;
    }
    let artefact = artefact.ok_or("verify needs ARTEFACT")?;
    let r = check(&artefact)?;
    let spec = Spec::from_recipe(&r)?;
    let now = toolchain(&spec)?;
    let drift = toolchain_drift(&r, &now);
    if !drift.is_empty() {
        let lines: Vec<String> = drift
            .iter()
            .map(|(k, a, b)| format!("  {k}: recorded '{a}', now '{b}'"))
            .collect();
        return Err(format!(
            "toolchain differs from the recipe, a rebuild cannot prove anything:\n{}",
            lines.join("\n")
        ));
    }
    let parent = artefact
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let work = work.unwrap_or_else(|| parent.join(format!(".verify-{}", std::process::id())));
    let res = build_in(&spec, &work);
    if !keep {
        let _ = fs::remove_dir_all(&work);
    }
    let b = res?;
    println!(
        "release-recipe verify: rebuilt {} B sha256 {} (recorded {} B {})",
        b.size,
        b.sha256,
        r.get("size")?,
        r.get("sha256")?
    );
    compare_rebuild(r.get("sha256")?, &b.sha256)?;
    println!("release-recipe verify OK: {} reproduces from its recipe", artefact.display());
    Ok(())
}

/// `xtask zig-cc <zig> <zig triple> <cc args...>`: cc-rs C compiler shim.
pub fn zig_cc(args: &[String]) -> Result<(), String> {
    let (zig, rest) = args.split_first().ok_or("zig-cc needs <zig> <triple>")?;
    let (triple, cc_args) = rest.split_first().ok_or("zig-cc needs <triple>")?;
    let filtered: Vec<&String> = cc_args.iter().filter(|a| !a.starts_with("--target=")).collect();
    let status = Command::new(zig)
        .args(["cc", "-target", triple])
        .args(filtered)
        .status()
        .map_err(|e| format!("cannot run {zig}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Recipe {
        let spec = Spec {
            commit: "989958bed31f18f92dafc99059cd0eb384898c5e".into(),
            package: "hermes-gateway".into(),
            bin: "hermes-gateway".into(),
            target: DEFAULT_TARGET.into(),
            features: String::new(),
            cc: "gcc".into(),
            source_date_epoch: "1793206972".into(),
            jobs: "2".into(),
        };
        let mut map = BTreeMap::new();
        for (k, v) in [
            ("format", FORMAT.to_string()),
            ("commit", spec.commit.clone()),
            ("package", spec.package.clone()),
            ("bin", spec.bin.clone()),
            ("target", spec.target.clone()),
            ("features", String::new()),
            ("cc", "gcc".into()),
            ("source_date_epoch", spec.source_date_epoch.clone()),
            ("cargo_argv", format!("cargo {}", spec.cargo_argv().join(" "))),
            ("rustc", "rustc 1.99.1".into()),
            ("rustc_commit", "58cf8f9c35b355199727627d07ae34a4e2846d67".into()),
            ("llvm", "23.1.1".into()),
            ("cargo", "cargo 1.99.1".into()),
            ("cc_version", "x86_64-linux-gnu-gcc (Debian 14.2.0-19) 14.2.0".into()),
            ("lld", "LLD 23.1.1".into()),
            ("size", "1952592".into()),
            ("budget", "2097152".into()),
            (
                "sha256",
                "0f7bb47123628ff6b76231844991cd3bc577d5cd4eb541a0b8e29df158e38ac8".into(),
            ),
        ] {
            map.insert(k.to_string(), v);
        }
        Recipe { map }
    }

    #[test]
    fn render_parse_roundtrip() {
        let r = sample();
        let text = r.render();
        assert!(text.starts_with("# SPDX-License-Identifier: MIT\n"));
        assert_eq!(Recipe::parse(&text).unwrap(), r);
    }

    #[test]
    fn parse_rejects_bad_recipes() {
        assert!(Recipe::parse("format=other/1\n").is_err());
        let text = sample().render().replace("cc=gcc\n", "");
        assert!(Recipe::parse(&text).unwrap_err().contains("'cc'"));
        let dup = format!("{}cc=zig\n", sample().render());
        assert!(Recipe::parse(&dup).unwrap_err().contains("duplicate"));
        assert!(Recipe::parse("format\n").is_err());
    }

    #[test]
    fn rebuild_mismatch_fails() {
        let r = sample();
        let rec = r.get("sha256").unwrap();
        assert!(compare_rebuild(rec, rec).is_ok());
        let e = compare_rebuild(
            rec,
            "47d3897a00000000000000000000000000000000000000000000000000000000",
        )
        .unwrap_err();
        assert!(e.contains("does NOT reproduce"), "{e}");
    }

    #[test]
    fn toolchain_drift_is_reported() {
        let r = sample();
        let mut now: BTreeMap<String, String> = TOOLCHAIN_KEYS
            .iter()
            .map(|k| (k.to_string(), r.map[*k].clone()))
            .collect();
        assert!(toolchain_drift(&r, &now).is_empty());
        now.insert("cc_version".into(), "zig 0.15.2".into());
        let d = toolchain_drift(&r, &now);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].0, "cc_version");
    }

    #[test]
    fn cargo_argv_is_pinned() {
        let spec = Spec::from_recipe(&sample()).unwrap();
        assert_eq!(
            spec.cargo_argv().join(" "),
            "build --release --locked --offline -p hermes-gateway --bin hermes-gateway \
             --target x86_64-unknown-linux-musl"
        );
        assert_eq!(artefact_name(&spec), "hermes-gateway-989958be");
        let zig = Spec { cc: "zig".into(), ..spec };
        assert_eq!(artefact_name(&zig), "hermes-gateway-989958be-zig");
    }

    #[test]
    fn budget_lookup() {
        let tsv = "# c\ndefault 2097152\nhermes-gateway   2000000\n";
        assert_eq!(budget_for(tsv, "hermes-gateway"), Some(2_000_000));
        assert_eq!(budget_for(tsv, "openmind-nerve"), Some(2_097_152));
        assert_eq!(budget_for("", "x"), None);
    }

    #[test]
    fn triples() {
        assert!(triple_tools("x86_64-unknown-linux-gnu").is_err());
        let (s, g, z) = triple_tools("aarch64-unknown-linux-musl").unwrap();
        assert_eq!(s, "aarch64_unknown_linux_musl");
        assert_eq!(g, "aarch64-linux-musl-gcc");
        assert_eq!(z, "aarch64-linux-musl");
    }
}
