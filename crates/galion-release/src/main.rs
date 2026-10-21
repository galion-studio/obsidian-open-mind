// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio
//! galion-release CLI (release.yml calls this; no shell packaging logic).

use galion_release as r;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

const USAGE: &str = "usage:
  galion-release package --target TRIPLE --tag vX.Y.Z --out DIR --bin-dir DIR [--bin-dir DIR]...
                         [--name NAME] [--extra FILE]... [--exclude BIN]... [--no-default-exclude]
                         [--budget BYTES] [--mtime SECS]
  galion-release sums DIR            write DIR/SHA256SUMS (sha256sum format)
  galion-release verify DIR          re-hash SHA256SUMS and open + check every archive
  galion-release rehearse DIR --work DIR [--expect-version X.Y.Z] [--qemu-dir DIR] [--timeout SECS]
                                     verify, extract, run every binary with --version
  galion-release --version
defaults: --name obsidian-open-mind, --budget 2097152, --mtime $SOURCE_DATE_EPOCH or 0,
          excludes test helpers gaia-dummy galion-bench-mock hermes-mock-model.
exit: 0 ok, 1 gate/verify/rehearse failure, 2 usage";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, msg)) => {
            eprintln!("galion-release: {msg}");
            ExitCode::from(code)
        }
    }
}

type Res = Result<(), (u8, String)>;

fn usage(m: &str) -> (u8, String) {
    (2, format!("{m}\n{USAGE}"))
}

struct Args {
    pos: Vec<String>,
    opts: Vec<(String, String)>,
    flags: Vec<String>,
}

fn parse(a: &[String], with_value: &[&str], flags: &[&str]) -> Result<Args, (u8, String)> {
    let mut o = Args {
        pos: vec![],
        opts: vec![],
        flags: vec![],
    };
    let mut i = 0;
    while i < a.len() {
        let k = a[i].as_str();
        if with_value.contains(&k) {
            let v = a
                .get(i + 1)
                .ok_or_else(|| usage(&format!("{k} needs a value")))?;
            o.opts.push((k.into(), v.clone()));
            i += 2;
        } else if flags.contains(&k) {
            o.flags.push(k.into());
            i += 1;
        } else if k.starts_with("--") {
            return Err(usage(&format!("unknown option {k}")));
        } else {
            o.pos.push(k.into());
            i += 1;
        }
    }
    Ok(o)
}

impl Args {
    fn one(&self, k: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(a, _)| a == k)
            .map(|(_, v)| v.as_str())
    }
    fn all(&self, k: &str) -> Vec<String> {
        self.opts
            .iter()
            .filter(|(a, _)| a == k)
            .map(|(_, v)| v.clone())
            .collect()
    }
    fn need(&self, k: &str) -> Result<&str, (u8, String)> {
        self.one(k)
            .ok_or_else(|| usage(&format!("{k} is required")))
    }
    fn num(&self, k: &str, d: u64) -> Result<u64, (u8, String)> {
        match self.one(k) {
            None => Ok(d),
            Some(v) => v.parse().map_err(|_| usage(&format!("{k}: not a number"))),
        }
    }
    fn dir(&self) -> Result<PathBuf, (u8, String)> {
        match self.pos.as_slice() {
            [d] => Ok(PathBuf::from(d)),
            _ => Err(usage("exactly one DIR expected")),
        }
    }
}

fn run(a: &[String]) -> Res {
    let (cmd, rest) = match a.split_first() {
        Some((c, r)) => (c.as_str(), r),
        None => return Err(usage("missing command")),
    };
    match cmd {
        "--version" | "-V" => {
            println!("galion-release {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "--help" | "-h" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        "package" => package(rest),
        "sums" => {
            let p = parse(rest, &[], &[])?;
            let d = p.dir()?;
            let v = r::write_sums(&d).map_err(|e| (1, e))?;
            for (n, h) in &v {
                println!("{h}  {n}");
            }
            Ok(())
        }
        "verify" => {
            let p = parse(rest, &["--budget"], &[])?;
            let d = p.dir()?;
            let v = r::verify(&d, p.num("--budget", r::SIZE_BUDGET)?).map_err(|e| (1, e))?;
            for o in &v.archives {
                println!(
                    "OK {}.tar.gz  target {}  version {}  {} binaries",
                    o.stem,
                    o.manifest.target,
                    o.manifest.version,
                    o.manifest.bins.len()
                );
            }
            println!("verify: {} files in {} OK", v.files, r::SUMS);
            Ok(())
        }
        "rehearse" => {
            let p = parse(
                rest,
                &[
                    "--work",
                    "--expect-version",
                    "--qemu-dir",
                    "--timeout",
                    "--budget",
                ],
                &[],
            )?;
            let d = p.dir()?;
            let work = PathBuf::from(p.need("--work")?);
            let v = r::verify(&d, p.num("--budget", r::SIZE_BUDGET)?).map_err(|e| (1, e))?;
            let q = p.one("--qemu-dir").map(PathBuf::from);
            let t = Duration::from_secs(p.num("--timeout", 30)?);
            let runs = r::rehearse(&v, &work, p.one("--expect-version"), q.as_deref(), t)
                .map_err(|e| (1, e))?;
            println!("result\ttarget\tbinary\tbytes\tbudget%\trunner\tms\t--version");
            let mut bad = 0;
            for x in &runs {
                if !x.ok {
                    bad += 1;
                }
                println!(
                    "{}\t{}\t{}\t{}\t{:.1}\t{}\t{}\t{}",
                    if x.ok { "PASS" } else { "FAIL" },
                    x.target,
                    x.name,
                    x.size,
                    x.size as f64 * 100.0 / r::SIZE_BUDGET as f64,
                    x.runner,
                    x.millis,
                    x.line
                );
            }
            println!("rehearse: {} runs, {} failed", runs.len(), bad);
            if bad > 0 {
                return Err((1, format!("{bad} binaries failed --version")));
            }
            Ok(())
        }
        other => Err(usage(&format!("unknown command {other}"))),
    }
}

fn package(rest: &[String]) -> Res {
    let p = parse(
        rest,
        &[
            "--target",
            "--tag",
            "--out",
            "--bin-dir",
            "--name",
            "--extra",
            "--exclude",
            "--budget",
            "--mtime",
        ],
        &["--no-default-exclude"],
    )?;
    if !p.pos.is_empty() {
        return Err(usage("package takes no positional arguments"));
    }
    let mut exclude = p.all("--exclude");
    if !p.flags.iter().any(|f| f == "--no-default-exclude") {
        exclude.extend(r::DEFAULT_EXCLUDE.iter().map(|s| s.to_string()));
    }
    let mtime_default = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let bin_dirs: Vec<PathBuf> = p.all("--bin-dir").into_iter().map(PathBuf::from).collect();
    if bin_dirs.is_empty() {
        return Err(usage("--bin-dir is required"));
    }
    let o = r::PackageOpts {
        name: p.one("--name").unwrap_or("obsidian-open-mind").into(),
        tag: p.need("--tag")?.into(),
        target: p.need("--target")?.into(),
        bin_dirs,
        exclude,
        extras: p.all("--extra").into_iter().map(PathBuf::from).collect(),
        out: PathBuf::from(p.need("--out")?),
        budget: p.num("--budget", r::SIZE_BUDGET)?,
        mtime: p.num("--mtime", mtime_default)?,
    };
    let k = r::package(&o).map_err(|e| (1, e))?;
    for b in &k.bins {
        println!("{}  {}  {}", b.sha256, b.size, b.name);
    }
    println!(
        "package: {} ({} bytes, {} binaries, excluded: {})",
        k.archive.display(),
        k.bytes,
        k.bins.len(),
        if k.excluded.is_empty() {
            "none".into()
        } else {
            k.excluded.join(" ")
        }
    );
    Ok(())
}
