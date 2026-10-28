// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! Real test (GALION_REAL=1) of the release `galion-report-push` binary
//! against a SIMULATED root on the box: `<sim>/opt/galion/openmind` is a real
//! git work tree laid out like the VPS one (checked out on a swarm/* branch,
//! F-391-3), `<sim>/srv/origin.git` stands in for GitHub. Every attack the
//! guard must stop is run through the binary, then the sim live tree is
//! fingerprinted (every file under it, .git included: path, size, mtime,
//! bytes hash) and must be identical to before. The real /opt/galion/openmind
//! is never touched (only the pure-path `check` subcommand names it).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_galion-report-push");

fn real() -> bool {
    std::env::var("GALION_REAL").as_deref() == Ok("1")
}

fn git(dir: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_AUTHOR_NAME", "sim")
        .env("GIT_AUTHOR_EMAIL", "sim@localhost")
        .env("GIT_COMMITTER_NAME", "sim")
        .env("GIT_COMMITTER_EMAIL", "sim@localhost")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// path -> (len, mtime ns, content hash) for every file and dir under `root`.
fn fingerprint(root: &Path) -> BTreeMap<String, (u64, i128, u64)> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let p = e.path();
            let md = std::fs::symlink_metadata(&p).unwrap();
            let mt = md
                .modified()
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as i128;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if md.is_file() {
                std::fs::read(&p).unwrap().hash(&mut h);
            }
            out.insert(
                p.strip_prefix(root).unwrap().display().to_string(),
                (md.len(), mt, h.finish()),
            );
            if md.is_dir() {
                stack.push(p);
            }
        }
    }
    out
}

/// (name, cwd, extra env, extra args)
type Attack<'a> = (&'a str, PathBuf, Vec<(&'a str, String)>, Vec<String>);

struct Sim {
    root: PathBuf,
}

impl Sim {
    fn live(&self) -> PathBuf {
        self.root.join("opt/galion/openmind")
    }
    fn origin(&self) -> String {
        self.root.join("srv/origin.git").display().to_string()
    }
    fn run(&self, cwd: &Path, env: &[(&str, &str)], args: &[&str]) -> Output {
        let mut c = Command::new(BIN);
        c.args(args)
            .current_dir(cwd)
            .env("GALION_SIM_ROOT", &self.root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
        for (k, v) in env {
            c.env(k, v);
        }
        c.output().unwrap()
    }
    fn publish(&self, cwd: &Path, env: &[(&str, &str)], extra: &[&str]) -> Output {
        let origin = self.origin();
        let report = self.root.join("work/report.md").display().to_string();
        let file = format!("{report}:swarm/reports/overnight-loop-log.md");
        let mut a = vec![
            "publish",
            "--remote",
            origin.as_str(),
            "--branch",
            "swarm/overnight-loop-sim",
            "--file",
            file.as_str(),
            "--message",
            "overnight: loop log (slot 419 sim)",
        ];
        a.extend_from_slice(extra);
        self.run(cwd, env, &a)
    }
}

fn code(o: &Output) -> i32 {
    o.status.code().unwrap_or(-1)
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn setup() -> Sim {
    let root =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("sim-419-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for d in ["srv", "opt/galion", "work", "seed"] {
        std::fs::create_dir_all(root.join(d)).unwrap();
    }
    let sim = Sim { root };
    // origin with one commit on main
    git(
        &sim.root.join("srv"),
        &["init", "-q", "--bare", "-b", "main", "origin.git"],
    );
    let seed = sim.root.join("seed");
    git(&seed, &["init", "-q", "-b", "main"]);
    std::fs::write(seed.join("README.md"), "sim main\n").unwrap();
    git(&seed, &["add", "README.md"]);
    git(&seed, &["commit", "-q", "-m", "sim main 13a296ce stand-in"]);
    git(&seed, &["push", "-q", &sim.origin(), "main"]);
    // sim live tree: a clone checked out on a swarm/* branch (as F-391-3)
    git(
        &sim.root.join("opt/galion"),
        &["clone", "-q", &sim.origin(), "openmind"],
    );
    git(
        &sim.live(),
        &["checkout", "-q", "-b", "swarm/overnight-loop-20261027"],
    );
    std::fs::create_dir_all(sim.live().join("docs")).unwrap();
    // attack fixtures that must be set up BEFORE the fingerprint
    git(
        &sim.live(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            &sim.root.join("linked-wt").display().to_string(),
        ],
    );
    git(
        &sim.root,
        &[
            "clone",
            "-q",
            "--shared",
            &sim.live().display().to_string(),
            "alt-clone",
        ],
    );
    std::fs::create_dir_all(sim.root.join("gitfile-clone")).unwrap();
    std::fs::write(
        sim.root.join("gitfile-clone/.git"),
        format!("gitdir: {}/.git\n", sim.live().display()),
    )
    .unwrap();
    std::os::unix::fs::symlink(sim.live(), sim.root.join("link-to-live")).unwrap();
    std::fs::write(sim.live().join("docs/report-in-live.md"), "x\n").unwrap();
    std::fs::write(
        sim.root.join("work/report.md"),
        "# loop log\n- 18:00 CET run 1\n",
    )
    .unwrap();
    sim
}

#[test]
fn real_openmind_write_guard_sim_root() {
    if !real() {
        eprintln!("skip: GALION_REAL=1 not set");
        return;
    }
    let sim = setup();
    let live = sim.live();
    let before = fingerprint(&live);
    let head_before = std::fs::read_to_string(live.join(".git/HEAD")).unwrap();
    let work = sim.root.join("work");
    let s = |p: &Path| p.display().to_string();
    let live_s = s(&live);
    let live_git = s(&live.join(".git"));
    let in_live_report = format!("{}:x.md", s(&live.join("docs/report-in-live.md")));

    // ---- attacks: every one must exit 3 (refused) -----------------------
    let mut refused = 0;
    let attacks: Vec<Attack> = vec![
        (
            "clone = live tree",
            work.clone(),
            vec![],
            vec!["--clone".into(), live_s.clone()],
        ),
        (
            "clone = live subdir",
            work.clone(),
            vec![],
            vec!["--clone".into(), format!("{live_s}/docs")],
        ),
        (
            "clone = live/../openmind",
            work.clone(),
            vec![],
            vec!["--clone".into(), format!("{live_s}/../openmind")],
        ),
        (
            "clone = symlink to live",
            work.clone(),
            vec![],
            vec!["--clone".into(), s(&sim.root.join("link-to-live"))],
        ),
        (
            "clone = new dir under symlink",
            work.clone(),
            vec![],
            vec![
                "--clone".into(),
                format!("{}/new-clone", s(&sim.root.join("link-to-live"))),
            ],
        ),
        (
            "clone = linked worktree of live",
            work.clone(),
            vec![],
            vec!["--clone".into(), s(&sim.root.join("linked-wt"))],
        ),
        (
            "clone = alternates into live",
            work.clone(),
            vec![],
            vec!["--clone".into(), s(&sim.root.join("alt-clone"))],
        ),
        (
            "clone = gitfile into live",
            work.clone(),
            vec![],
            vec!["--clone".into(), s(&sim.root.join("gitfile-clone"))],
        ),
        (
            "GIT_DIR into live",
            work.clone(),
            vec![("GIT_DIR", live_git.clone())],
            vec![],
        ),
        (
            "GIT_WORK_TREE into live",
            work.clone(),
            vec![("GIT_WORK_TREE", live_s.clone())],
            vec![],
        ),
        (
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            work.clone(),
            vec![(
                "GIT_ALTERNATE_OBJECT_DIRECTORIES",
                format!("/tmp/x:{live_git}/objects"),
            )],
            vec![],
        ),
        ("cwd inside live", live.join("docs"), vec![], vec![]),
        (
            "remote = live tree",
            work.clone(),
            vec![],
            vec!["--remote".into(), live_s.clone()],
        ),
        (
            "remote = file:// live",
            work.clone(),
            vec![],
            vec!["--remote".into(), format!("file://{live_git}")],
        ),
    ];
    for (name, cwd, env, extra) in &attacks {
        let env: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        // later --remote overrides the default one
        let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
        let o = sim.publish(cwd, &env, &extra);
        eprintln!(
            "attack {name:<34} exit {} | {}",
            code(&o),
            text(&o).lines().next().unwrap_or("")
        );
        assert_eq!(code(&o), 3, "{name}: {}", text(&o));
        assert!(text(&o).contains("refused"), "{name}");
        refused += 1;
    }
    // report source inside live
    let o = sim.run(
        &work,
        &[],
        &[
            "publish",
            "--remote",
            &sim.origin(),
            "--branch",
            "swarm/x",
            "--file",
            &in_live_report,
            "--message",
            "m",
        ],
    );
    eprintln!("attack {:<34} exit {}", "report source in live", code(&o));
    assert_eq!(code(&o), 3);
    refused += 1;
    // the real live path, pure path check (no filesystem access needed)
    let o = sim.run(
        &work,
        &[],
        &[
            "check",
            "/opt/galion/openmind",
            "/opt/galion/openmind/.git",
            "/opt/galion/swarm-reports-git",
        ],
    );
    eprintln!("check real paths exit {}:\n{}", code(&o), text(&o));
    assert_eq!(code(&o), 3);
    assert!(text(&o).contains("ok       /opt/galion/swarm-reports-git"));
    refused += 1;
    // the env var can only ADD: the real path stays protected with a sim root
    let o = sim.run(&work, &[], &["protected"]);
    assert!(
        text(&o).lines().any(|l| l == "/opt/galion/openmind"),
        "{}",
        text(&o)
    );
    assert!(text(&o).contains(&live_s));

    // ---- usage refusals (exit 2) ----------------------------------------
    let report = format!("{}:r.md", s(&work.join("report.md")));
    for br in ["main", "refs/heads/main", "swarm/a..b"] {
        let o = sim.run(
            &work,
            &[],
            &[
                "publish",
                "--remote",
                &sim.origin(),
                "--branch",
                br,
                "--file",
                &report,
                "--message",
                "m",
            ],
        );
        assert_eq!(code(&o), 2, "branch {br}");
    }
    for f in [
        format!("{}:../x", s(&work.join("report.md"))),
        format!("{}:.git/hooks/pre-commit", s(&work.join("report.md"))),
    ] {
        let o = sim.run(
            &work,
            &[],
            &[
                "publish",
                "--remote",
                &sim.origin(),
                "--branch",
                "swarm/x",
                "--file",
                &f,
                "--message",
                "m",
            ],
        );
        assert_eq!(code(&o), 2, "dest {f}");
    }

    // ---- happy path: default own clone under the sim root ---------------
    let own = sim.root.join("opt/galion/swarm-reports-git");
    let t0 = std::time::Instant::now();
    let o = sim.publish(&work, &[], &["--push"]);
    let ms1 = t0.elapsed().as_millis();
    eprintln!("publish #1 exit {} {ms1} ms: {}", code(&o), text(&o).trim());
    assert_eq!(code(&o), 0, "{}", text(&o));
    assert!(own.join(".git").is_dir());
    let origin = PathBuf::from(sim.origin());
    let tip = git(
        &origin,
        &["rev-parse", "refs/heads/swarm/overnight-loop-sim"],
    );
    assert!(text(&o).contains(&tip));
    let msg = git(&origin, &["log", "-1", "--format=%an <%ae>|%s", &tip]);
    eprintln!("origin tip {tip}: {msg}");
    assert!(msg
        .starts_with("Grok Bot <grokbot@galion.studio>|overnight: loop log (slot 419 sim) (2026-"));
    assert!(
        msg.ends_with("+01:00)") || msg.ends_with("+02:00)"),
        "{msg}"
    );
    assert_eq!(
        git(
            &origin,
            &[
                "show",
                &format!("{tip}:swarm/reports/overnight-loop-log.md")
            ]
        ),
        "# loop log\n- 18:00 CET run 1"
    );
    // same content again: nothing to commit
    let o = sim.publish(&work, &[], &["--push"]);
    assert_eq!(code(&o), 0);
    assert!(text(&o).contains("nothing to commit"), "{}", text(&o));
    // someone else pushes to the branch from another clone; we build on top
    let other = sim.root.join("other");
    git(
        &sim.root,
        &[
            "clone",
            "-q",
            "-b",
            "swarm/overnight-loop-sim",
            &sim.origin(),
            "other",
        ],
    );
    std::fs::write(other.join("other.md"), "from another clone\n").unwrap();
    git(&other, &["add", "other.md"]);
    git(&other, &["commit", "-q", "-m", "other clone"]);
    git(
        &other,
        &["push", "-q", "origin", "swarm/overnight-loop-sim"],
    );
    // leave the own clone dirty as if a run had aborted
    std::fs::write(own.join("stray.txt"), "dirty\n").unwrap();
    std::fs::write(own.join("README.md"), "modified\n").unwrap();
    std::fs::write(
        work.join("report.md"),
        "# loop log\n- 18:00 CET run 1\n- 18:15 CET run 2\n",
    )
    .unwrap();
    let o = sim.publish(&work, &[], &["--push"]);
    eprintln!("publish #3 exit {}: {}", code(&o), text(&o).trim());
    assert_eq!(code(&o), 0, "{}", text(&o));
    let tip2 = git(
        &origin,
        &["rev-parse", "refs/heads/swarm/overnight-loop-sim"],
    );
    let files = git(&origin, &["show", "--name-only", "--format=", &tip2]);
    assert_eq!(
        files, "swarm/reports/overnight-loop-log.md",
        "README/stray must not leak"
    );
    let chain = git(&origin, &["rev-list", "--count", &tip2]);
    assert_eq!(
        chain, "4",
        "main + run1 + other + run2 (fast-forward, nothing lost)"
    );
    // commit without push
    std::fs::write(work.join("report.md"), "# loop log\nrun 3\n").unwrap();
    let o = sim.publish(&work, &[], &[]);
    assert_eq!(code(&o), 0);
    assert!(text(&o).contains("(not pushed)"));
    assert_eq!(
        git(
            &origin,
            &["rev-parse", "refs/heads/swarm/overnight-loop-sim"]
        ),
        tip2
    );
    // origin main untouched
    assert_eq!(git(&origin, &["rev-list", "--count", "main"]), "1");

    // ---- the sim live tree is byte-for-byte untouched -------------------
    let after = fingerprint(&live);
    assert_eq!(before.len(), after.len());
    let changed: Vec<_> = before
        .iter()
        .filter(|(k, v)| after.get(*k) != Some(v))
        .map(|(k, _)| k.clone())
        .collect();
    assert!(changed.is_empty(), "sim live tree changed: {changed:?}");
    assert_eq!(
        std::fs::read_to_string(live.join(".git/HEAD")).unwrap(),
        head_before
    );
    eprintln!(
        "RESULT refused {refused} attacks; sim live tree {} entries unchanged; HEAD {}; publish #1 {ms1} ms; own clone {}",
        after.len(),
        head_before.trim(),
        own.display()
    );
    let _ = std::fs::remove_dir_all(&sim.root);
}
