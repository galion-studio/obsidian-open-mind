// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//
// xtask: the CI task runner. CI steps call `cargo xtask <task>` (alias in
// .cargo/config.toml), so the YAML stays a thin list of checkouts,
// toolchains and task names, and every check runs locally with the same
// command. Same shape as swarm/ci-workflow-rust 38a26cb (the full runner):
// merging the two adds `mod replay_corpus;` and one match arm there.

mod release_recipe;
mod replay_corpus;

use std::process::ExitCode;

const USAGE: &str = "\
usage: cargo xtask <task> [args]

  replay-corpus [--bless] [--require-live] [--release]
                          cortex T5.3: replay every capture in
                          crates/openmind-nerve/tests/replay-corpus/corpus.json,
                          fail on verdict drift or a flapping verdict over the
                          capture plus 20 hold cycles (self-eval.yml)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((task, rest)) = args.split_first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    let result = match task.as_str() {
        "-h" | "--help" | "help" => {
            print!("{USAGE}{}", release_recipe::USAGE);
            Ok(())
        }
        "replay-corpus" => replay_corpus::run(rest),
        "release-recipe" => release_recipe::run(rest),
        // Internal: the C compiler shim `release-recipe build --cc zig` puts in CC_<triple>.
        "zig-cc" => release_recipe::zig_cc(rest),
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
