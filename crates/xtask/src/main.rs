// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// xtask: the CI task runner. Every step in .github/workflows calls
// `cargo xtask <task>` (alias in .cargo/config.toml), so the YAML stays a thin
// list of checkouts, toolchains and task names, and every check can be run
// locally with the same command. See docs/ci.md for the step -> task map.

mod checks;
mod run;
mod sbom;
mod tasks;
mod tools;

use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <task> [args]

gates (self-eval.yml):
  fmt                     cargo fmt --check (workspace + edge/openmind-node)
  clippy                  cargo clippy --workspace --all-targets -D warnings
  test                    cargo test --workspace --locked
  omnode                  clippy + test of edge/openmind-node (own workspace)
  ucpu [--zig] STAGE...   scripts/ucpu-gate.sh STAGE... (--zig: install pinned zig first)
  ucpu-qemu [--apt-qemu]  pinned zig, run scripts/real/ucpu.sh (qemu-user; --apt-qemu installs it)
  self-eval               build repo-check (static musl), write self-eval.json, print report
  no-python               fail on tracked *.py/*.pyc/*.pyw or python shebangs
  cjk                     fail on Han/Kana/Hangul characters in tracked text
  zagreb-ts               warn on UTC 'Z' timestamps (always exits 0)
  license                 LICENSE is MIT, workspace MIT, crate copies identical
  gitleaks                install pinned gitleaks, scan full history (redacted)
  ci                      local run of fmt clippy test no-python cjk license zagreb-ts

other workflows:
  realtest [--out DIR] [ARGS]  release stack + galion-realtest stack ARGS (realtest.yml)
  launch-gates [ARGS]     install pinned scanners, cargo fetch, scripts/public-export.sh ARGS
  sbom [--check FILE]     cargo metadata licence SBOM -> dist/sbom-cargo.json (sbom.yml)
  fetch                   cargo fetch --locked (workspace + edge/openmind-node)
  install TOOL...         install pinned, sha256-verified tools; prints the bin dir
                          (gitleaks cargo-deny cargo-cyclonedx comply zig)
  package                 release tarball + SHA256SUMS in dist/ (release.yml)
  sign                    cosign keyless sign-blob of dist/ artifacts (release.yml)
  gh-release              gh release create from dist/ (release.yml)
  list                    print the workflow-step -> task inventory
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((task, rest)) = args.split_first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match task.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}");
            Ok(())
        }
        "fmt" => tasks::fmt(),
        "clippy" => tasks::clippy(),
        "test" => tasks::test(),
        "omnode" => tasks::omnode(),
        "ucpu" => tasks::ucpu(rest),
        "ucpu-qemu" => tasks::ucpu_qemu(rest),
        "self-eval" => tasks::self_eval(),
        "no-python" => checks::no_python(&run::repo_root()),
        "cjk" => checks::cjk(&run::repo_root()),
        "zagreb-ts" => checks::zagreb_ts(&run::repo_root()),
        "license" => checks::license(&run::repo_root()),
        "gitleaks" => tasks::gitleaks(),
        "ci" => tasks::ci(),
        "realtest" => tasks::realtest(rest),
        "launch-gates" => tasks::launch_gates(rest),
        "sbom" => sbom::run(rest),
        "fetch" => tasks::fetch(),
        "install" => tools::install_cmd(rest),
        "package" => tasks::package(),
        "sign" => tasks::sign(),
        "gh-release" => tasks::gh_release(),
        "list" => {
            print!("{}", tasks::INVENTORY);
            Ok(())
        }
        other => Err(format!("unknown task '{other}' (try: cargo xtask help)")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask {task}: {e}");
            ExitCode::FAILURE
        }
    }
}
