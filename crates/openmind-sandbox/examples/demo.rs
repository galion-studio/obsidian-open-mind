// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio
use openmind_sandbox::{run, SandboxPolicy};

fn main() {
    let dir = std::env::temp_dir();
    let policy = SandboxPolicy::new(dir);
    let out = run(&["/bin/echo".to_string(), "hi".to_string()], &policy);
    print!("stdout: {}", out.stdout_str());
    println!(
        "status: {:?} timed_out: {} wall: {:?}",
        out.status, out.timed_out, out.wall
    );
    for (layer, e) in &out.enforced {
        println!("{layer}: {e:?}");
    }
}
