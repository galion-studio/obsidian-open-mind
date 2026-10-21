// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// Workflow tasks. Each function is one former YAML `run:` block.

use crate::run::{append_github, cargo, cmd, output, repo_root, status, target_dir, Res};
use crate::{checks, tools};

const OMNODE: &str = "edge/openmind-node/Cargo.toml";

/// The workflow-step -> task map (also in docs/ci.md).
pub const INVENTORY: &str = "\
workflow          job / step                          task
self-eval.yml     fmt                                 cargo xtask fmt
self-eval.yml     clippy                              cargo xtask clippy
self-eval.yml     test                                cargo xtask test
self-eval.yml     omnode                              cargo xtask omnode
self-eval.yml     ucpu-tier1 (matrix)                 cargo xtask ucpu --zig grep build static size clippy
self-eval.yml     ucpu-tier2 (matrix)                 cargo xtask ucpu --zig check
self-eval.yml     ucpu-qemu                           cargo xtask ucpu-qemu --apt-qemu
self-eval.yml     repo-check                          cargo xtask self-eval
self-eval.yml     no-python                           cargo xtask no-python
self-eval.yml     cjk-scan                            cargo xtask cjk
self-eval.yml     zagreb-timestamps                   cargo xtask zagreb-ts
self-eval.yml     gitleaks                            cargo xtask gitleaks
self-eval.yml     license                             cargo xtask license
realtest.yml      box-stack                           cargo xtask realtest --out target/realtest/ci
launch-gates.yml  public-export                       cargo xtask launch-gates --out $RUNNER_TEMP/public --online
sbom.yml          cargo-sbom                          cargo xtask sbom
release.yml       release: package                    cargo xtask package
release.yml       release: sign                       cargo xtask sign
release.yml       release: GitHub Release             cargo xtask gh-release
release.yml       release: SBOM                       anchore/sbom-action (pinned action, unchanged)
scorecard.yml     analysis                            ossf/scorecard-action (pinned action, unchanged)
";

pub fn fmt() -> Res {
    cargo(&["fmt", "--all", "--", "--check"])?;
    cargo(&["fmt", "--manifest-path", OMNODE, "--all", "--", "--check"])
}

pub fn clippy() -> Res {
    cargo(&[
        "clippy",
        "--workspace",
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings",
    ])
}

pub fn test() -> Res {
    cargo(&["test", "--workspace", "--locked"])
}

pub fn omnode() -> Res {
    cargo(&[
        "clippy",
        "--manifest-path",
        OMNODE,
        "--all-targets",
        "--locked",
        "--",
        "-D",
        "warnings",
    ])?;
    cargo(&["test", "--manifest-path", OMNODE, "--locked"])
}

pub fn fetch() -> Res {
    cargo(&["fetch", "--locked"])?;
    cargo(&["fetch", "--locked", "--manifest-path", OMNODE])
}

/// `ucpu [--zig] STAGE...`: scripts/ucpu-gate.sh with the pinned zig.
pub fn ucpu(args: &[String]) -> Res {
    let (want_zig, stages) = match args.split_first() {
        Some((f, rest)) if f == "--zig" => (true, rest),
        _ => (false, args),
    };
    let mut c = cmd("bash", &["scripts/ucpu-gate.sh"]);
    c.args(stages);
    if want_zig {
        c.env("ZIG", tools::install("zig")?);
    }
    status(c)
}

/// qemu-user real run (scripts/real/ucpu.sh) with the pinned zig.
/// `--apt-qemu`: on a CI runner without qemu-aarch64, apt-install
/// qemu-user-static first (sudo; the runner image allows it).
pub fn ucpu_qemu(args: &[String]) -> Res {
    match args {
        [] => {}
        [f] if f == "--apt-qemu" => {
            if !on_path("qemu-aarch64") && !on_path("qemu-aarch64-static") {
                status(cmd("sudo", &["apt-get", "update", "-q"]))?;
                status(cmd(
                    "sudo",
                    &["apt-get", "install", "-y", "-q", "qemu-user-static"],
                ))?;
            }
        }
        _ => return Err("usage: cargo xtask ucpu-qemu [--apt-qemu]".into()),
    }
    let mut c = cmd("bash", &["scripts/real/ucpu.sh"]);
    c.env("ZIG", tools::install("zig")?);
    status(c)
}

/// repo-check self-evaluation: portable static musl build, JSON artifact
/// (never fails), then the human report (fails on findings).
pub fn self_eval() -> Res {
    let target = "x86_64-unknown-linux-musl";
    let mut c = cmd(
        &crate::run::cargo_bin(),
        &[
            "build",
            "--release",
            "--locked",
            "-p",
            "repo-check",
            "--target",
            target,
        ],
    );
    c.env("CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER", "rust-lld");
    status(c)?;
    let bin = target_dir().join(target).join("release").join("repo-check");
    let b = bin.to_string_lossy();
    let copyright = "3000 Galion / galion-studio";
    let json = cmd(&b, &["--json", "--copyright", copyright])
        .output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    std::fs::write(repo_root().join("self-eval.json"), json).map_err(|e| e.to_string())?;
    status(cmd(&b, &["--copyright", copyright]))
}

/// True when `prog` is an executable file in a $PATH directory.
fn on_path(prog: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false)
}

pub fn gitleaks() -> Res {
    let g = tools::install("gitleaks")?;
    status(cmd(
        &g.to_string_lossy(),
        &["git", "--redact", "--no-banner", "--verbose", "."],
    ))
}

type Step = (&'static str, fn() -> Res);
type Gate = (&'static str, fn(&std::path::Path) -> Res);

/// Local convenience: the cheap self-eval gates in one go.
pub fn ci() -> Res {
    let root = repo_root();
    let mut failed = Vec::new();
    let steps: [Step; 3] = [("fmt", fmt), ("clippy", clippy), ("test", test)];
    for (name, f) in steps {
        if let Err(e) = f() {
            eprintln!("xtask ci: {name}: {e}");
            failed.push(name);
        }
    }
    let gates: [Gate; 4] = [
        ("no-python", checks::no_python),
        ("cjk", checks::cjk),
        ("license", checks::license),
        ("zagreb-ts", checks::zagreb_ts),
    ];
    for (name, f) in gates {
        if let Err(e) = f(&root) {
            eprintln!("xtask ci: {name}: {e}");
            failed.push(name);
        }
    }
    if failed.is_empty() {
        println!("xtask ci: all green");
        Ok(())
    } else {
        Err(format!("failed: {}", failed.join(", ")))
    }
}

pub const REALTEST_PKGS: [&str; 9] = [
    "galion-realtest",
    "galion-boot",
    "galion-one",
    "openmind-memory",
    "tuner",
    "galion-desk",
    "link-watch",
    "guardian-tick",
    "persona-plex-relay",
];

/// Release build of the stack, then galion-realtest scenarios. The report is
/// appended to the job summary even when scenarios fail.
pub fn realtest(args: &[String]) -> Res {
    // --out DIR is ours; anything else (e.g. --base-port N) goes to
    // `galion-realtest stack` unchanged.
    let mut out = "target/realtest/ci".to_string();
    let mut extra: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--out" {
            out = it.next().ok_or("--out needs a directory")?.clone();
        } else {
            extra.push(a);
        }
    }
    let mut build = vec!["build", "--release", "--locked"];
    for p in REALTEST_PKGS {
        build.extend(["-p", p]);
    }
    cargo(&build)?;
    let rel = target_dir().join("release");
    let bin = rel.join("galion-realtest");
    let mut c = cmd(&bin.to_string_lossy(), &["stack", "--bin-dir"]);
    c.arg(&rel).args(["--out", &out]).args(&extra);
    let r = status(c);
    let report = repo_root().join(&out).join("report.md");
    if let Ok(md) = std::fs::read_to_string(&report) {
        append_github("GITHUB_STEP_SUMMARY", &md)?;
    }
    r
}

/// Install the pinned scanners, fill the cargo cache (both workspaces), run
/// scripts/public-export.sh with GALION_TOOLS_BIN pointing at them.
pub fn launch_gates(args: &[String]) -> Res {
    for t in ["gitleaks", "cargo-deny", "cargo-cyclonedx", "comply"] {
        tools::install(t)?;
    }
    fetch()?;
    let mut c = cmd("bash", &["scripts/public-export.sh"]);
    c.args(args).env("GALION_TOOLS_BIN", tools::bin_dir());
    status(c)
}

pub const PACKAGED_BINS: [&str; 5] = [
    "galion-one",
    "galion-desk",
    "room",
    "desk-hermes",
    "link-watch",
];

/// Release archive name for a tag, e.g. obsidian-open-mind-v1.0.0-x86_64-linux.
pub fn archive_name(tag: &str) -> String {
    format!("obsidian-open-mind-{tag}-x86_64-linux")
}

fn ref_name() -> Result<String, String> {
    std::env::var("GITHUB_REF_NAME").map_err(|_| "GITHUB_REF_NAME not set (release tag)".into())
}

/// Workspace release build, then dist/<name>.tar.gz + dist/SHA256SUMS.
pub fn package() -> Res {
    let tag = ref_name()?;
    cargo(&["build", "--release", "--workspace", "--locked"])?;
    let root = repo_root();
    let name = archive_name(&tag);
    let stage = root.join("dist").join(&name);
    std::fs::create_dir_all(&stage).map_err(|e| e.to_string())?;
    let rel = target_dir().join("release");
    for b in PACKAGED_BINS {
        let src = rel.join(b);
        if src.is_file() {
            std::fs::copy(&src, stage.join(b)).map_err(|e| format!("{b}: {e}"))?;
        }
    }
    for f in ["LICENSE", "README.md"] {
        std::fs::copy(root.join(f), stage.join(f)).map_err(|e| format!("{f}: {e}"))?;
    }
    let tgz = format!("{name}.tar.gz");
    status(cmd(
        "tar",
        &["-C", "dist", "-czf", &format!("dist/{tgz}"), &name],
    ))?;
    std::fs::remove_dir_all(&stage).map_err(|e| e.to_string())?;
    let mut c = cmd("sha256sum", &[&format!("./{tgz}")]);
    c.current_dir(root.join("dist"));
    let sums = output(c)?;
    std::fs::write(root.join("dist/SHA256SUMS"), sums).map_err(|e| e.to_string())?;
    status(cmd("ls", &["-l", "dist"]))
}

fn dist_artifacts() -> Result<Vec<String>, String> {
    let mut v: Vec<String> = std::fs::read_dir(repo_root().join("dist"))
        .map_err(|e| format!("dist/: {e}"))?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".tar.gz") || n.ends_with(".cdx.json") || n == "SHA256SUMS")
        .collect();
    v.sort();
    Ok(v)
}

/// cosign keyless signature bundle next to every artifact.
pub fn sign() -> Res {
    let dist = repo_root().join("dist");
    for f in dist_artifacts()? {
        let mut c = cmd(
            "cosign",
            &[
                "sign-blob",
                "--yes",
                "--bundle",
                &format!("{f}.sigstore.json"),
                &f,
            ],
        );
        c.current_dir(&dist);
        status(c)?;
    }
    status(cmd("ls", &["-l", "dist"]))
}

pub fn gh_release() -> Res {
    let tag = ref_name()?;
    let notes = "Built by release.yml after self-eval passed. Verify with: cosign verify-blob --bundle <file>.sigstore.json --certificate-identity-regexp 'https://github.com/galion-studio/obsidian-open-mind/' --certificate-oidc-issuer https://token.actions.githubusercontent.com <file>";
    let mut files: Vec<String> = std::fs::read_dir(repo_root().join("dist"))
        .map_err(|e| format!("dist/: {e}"))?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .map(|n| format!("dist/{n}"))
        .collect();
    files.sort();
    let mut c = cmd("gh", &["release", "create", &tag]);
    c.args(&files).args(["--title", &tag, "--notes", notes]);
    status(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_covers_every_workflow() {
        for wf in [
            "self-eval.yml",
            "realtest.yml",
            "launch-gates.yml",
            "sbom.yml",
            "release.yml",
            "scorecard.yml",
        ] {
            assert!(INVENTORY.contains(wf), "{wf}");
        }
        assert_eq!(
            archive_name("v1.2.3"),
            "obsidian-open-mind-v1.2.3-x86_64-linux"
        );
        assert!(REALTEST_PKGS.contains(&"galion-realtest"));
    }

    /// Every `run:` line in .github/workflows must call `cargo xtask` (the
    /// whole point of the change): no inline shell, no direct script calls.
    #[test]
    fn workflows_only_call_xtask() {
        let dir = repo_root().join(".github/workflows");
        let mut bad = Vec::new();
        let mut runs = 0;
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            let path = e.path();
            let text = std::fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                let t = line.trim_start().trim_start_matches("- ");
                if let Some(cmdline) = t.strip_prefix("run:") {
                    runs += 1;
                    let c = cmdline.trim();
                    if !c.starts_with("cargo xtask ") {
                        bad.push(format!("{}:{}: {c}", path.display(), n + 1));
                    }
                }
            }
        }
        assert!(runs >= 15, "only {runs} run: steps found");
        assert!(bad.is_empty(), "non-xtask run steps:\n{}", bad.join("\n"));
    }

    /// Every task named in the inventory exists in main.rs's dispatcher.
    #[test]
    fn inventory_tasks_are_dispatched() {
        let main = include_str!("main.rs");
        for line in INVENTORY.lines().skip(1) {
            if let Some(rest) = line.split("cargo xtask ").nth(1) {
                let task = rest.split_whitespace().next().unwrap();
                assert!(main.contains(&format!("\"{task}\" =>")), "{task}");
            }
        }
    }
}
