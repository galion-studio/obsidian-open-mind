// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio
//! Sandbox policy: command-line parsing, validation and the human-readable plan.
//! Pure logic, no syscalls, so it is unit-tested on every target (also under qemu-aarch64).

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// UID/GID the payload sees inside the sandbox (an ordinary user, never 0).
pub const DEFAULT_INSIDE_ID: u32 = 1000;
pub const DEFAULT_HOSTNAME: &str = "galion-box";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetMode {
    /// New, empty network namespace; even loopback stays down (default-deny).
    None,
    /// New network namespace with only `lo` brought up (no route to the host or internet).
    Loopback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bind {
    pub src: PathBuf,
    pub dst: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub rootfs: PathBuf,
    pub net: NetMode,
    pub binds_ro: Vec<Bind>,
    pub ro_root: bool,
    pub require_proc: bool,
    pub inside_id: u32,
    pub hostname: String,
    pub timeout: Option<Duration>,
    pub nproc: Option<u64>,
    pub nofile: Option<u64>,
    pub as_mb: Option<u64>,
    pub nice: i32,
    pub env: Vec<(String, String)>,
    pub argv: Vec<String>,
}

impl Policy {
    pub fn new(rootfs: PathBuf, argv: Vec<String>) -> Self {
        Policy {
            rootfs,
            net: NetMode::None,
            binds_ro: Vec::new(),
            ro_root: false,
            require_proc: false,
            inside_id: DEFAULT_INSIDE_ID,
            hostname: DEFAULT_HOSTNAME.to_string(),
            timeout: None,
            nproc: None,
            nofile: Some(256),
            as_mb: None,
            nice: 10,
            env: Vec::new(),
            argv,
        }
    }

    /// Clean environment for the payload: nothing from the host leaks in.
    pub fn payload_env(&self, proc_state: &str) -> Vec<String> {
        let mut env = vec![
            "PATH=/bin:/usr/bin".to_string(),
            "HOME=/tmp".to_string(),
            "LANG=C.UTF-8".to_string(),
            format!("GALION_BOX_PROC={proc_state}"),
        ];
        for (k, v) in &self.env {
            env.push(format!("{k}={v}"));
        }
        env
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    Help,
    Version,
    Probe,
    Plan(Policy),
    Run(Policy),
    /// Payload helpers used by the real tests (run *inside* the sandbox).
    Inside(Vec<String>),
}

pub const USAGE: &str = "galion-box 0.1.0 - Galion OS sandbox launcher prototype (rootless)

USAGE:
  galion-box probe                       read-only capability report (no namespaces created)
  galion-box plan [OPTIONS] -- CMD...    validate the policy and print the steps (no namespaces created)
  galion-box run  [OPTIONS] -- CMD...    run CMD in a new user+mount+pid+net+uts+ipc namespace
  galion-box inside-check|spawn|alloc|sleep|exit ...   payload helpers for tests

OPTIONS:
  --rootfs DIR        new root (required; must not be /)
  --net none|lo       none = no interfaces up (default); lo = loopback only
  --bind-ro SRC:DST   read-only bind of a host path (repeatable)
  --ro-root           remount the new root read-only
  --require-proc      fail if a fresh /proc cannot be mounted (default: hide /proc instead)
  --uid N             uid/gid seen inside (default 1000, never 0)
  --hostname NAME     default galion-box
  --timeout SECS      kill the whole sandbox after SECS
  --nproc N           RLIMIT_NPROC for the payload
  --nofile N          RLIMIT_NOFILE (default 256)
  --as-mb N           RLIMIT_AS in MiB
  --nice N            niceness 0..19 (default 10, coexist with the host UI)
  --env K=V           extra payload env (repeatable; host env is never inherited)
";

fn parse_num<T: std::str::FromStr>(flag: &str, v: Option<String>) -> Result<T, String> {
    let v = v.ok_or_else(|| format!("{flag} needs a value"))?;
    v.parse::<T>()
        .map_err(|_| format!("{flag}: invalid number {v:?}"))
}

/// Parse argv (without the program name).
pub fn parse_args(args: &[String]) -> Result<Cmd, String> {
    let mut it = args.iter().cloned();
    let sub = match it.next() {
        None => return Ok(Cmd::Help),
        Some(s) => s,
    };
    match sub.as_str() {
        "-h" | "--help" | "help" => return Ok(Cmd::Help),
        "-V" | "--version" => return Ok(Cmd::Version),
        "probe" => return Ok(Cmd::Probe),
        "inside-check" | "spawn" | "alloc" | "sleep" | "exit" => {
            let mut v = vec![sub];
            v.extend(it);
            return Ok(Cmd::Inside(v));
        }
        "plan" | "run" => {}
        other => return Err(format!("unknown subcommand {other:?}\n\n{USAGE}")),
    }
    let mut rootfs: Option<PathBuf> = None;
    let mut p = Policy::new(PathBuf::new(), Vec::new());
    let mut argv = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--" => {
                argv.extend(it.by_ref());
                break;
            }
            "--rootfs" => rootfs = Some(PathBuf::from(it.next().ok_or("--rootfs needs a value")?)),
            "--net" => {
                p.net = match it.next().as_deref() {
                    Some("none") => NetMode::None,
                    Some("lo") => NetMode::Loopback,
                    o => return Err(format!("--net must be none or lo, got {o:?}")),
                }
            }
            "--bind-ro" => {
                let v = it.next().ok_or("--bind-ro needs SRC:DST")?;
                let (s, d) = v.split_once(':').ok_or("--bind-ro needs SRC:DST")?;
                p.binds_ro.push(Bind {
                    src: PathBuf::from(s),
                    dst: PathBuf::from(d),
                });
            }
            "--ro-root" => p.ro_root = true,
            "--require-proc" => p.require_proc = true,
            "--uid" => p.inside_id = parse_num("--uid", it.next())?,
            "--hostname" => p.hostname = it.next().ok_or("--hostname needs a value")?,
            "--timeout" => {
                let s: f64 = parse_num("--timeout", it.next())?;
                if !(s > 0.0 && s < 86_400.0) {
                    return Err("--timeout must be in (0, 86400)".into());
                }
                p.timeout = Some(Duration::from_secs_f64(s));
            }
            "--nproc" => p.nproc = Some(parse_num("--nproc", it.next())?),
            "--nofile" => p.nofile = Some(parse_num("--nofile", it.next())?),
            "--as-mb" => p.as_mb = Some(parse_num("--as-mb", it.next())?),
            "--nice" => p.nice = parse_num("--nice", it.next())?,
            "--env" => {
                let v = it.next().ok_or("--env needs K=V")?;
                let (k, val) = v.split_once('=').ok_or("--env needs K=V")?;
                p.env.push((k.to_string(), val.to_string()));
            }
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    p.rootfs = rootfs.ok_or("--rootfs is required")?;
    p.argv = argv;
    validate(&p)?;
    Ok(if sub == "plan" {
        Cmd::Plan(p)
    } else {
        Cmd::Run(p)
    })
}

/// A sandbox-relative path: absolute, no `..`, not `/` itself.
fn check_inside_path(p: &Path) -> Result<(), String> {
    if !p.is_absolute() {
        return Err(format!("{} must be absolute", p.display()));
    }
    if p.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("{} must not contain ..", p.display()));
    }
    if p.components().count() < 2 {
        return Err(format!("{} must not be /", p.display()));
    }
    for reserved in ["/proc", "/dev", "/tmp", "/.oldroot"] {
        if p.starts_with(reserved) {
            return Err(format!("{} is reserved by the launcher", p.display()));
        }
    }
    Ok(())
}

pub fn validate(p: &Policy) -> Result<(), String> {
    if p.argv.is_empty() {
        return Err("missing command after --".into());
    }
    if !p.rootfs.is_absolute() {
        return Err("--rootfs must be an absolute path".into());
    }
    if p.rootfs.components().count() < 2
        || p.rootfs
            .components()
            .any(|c| matches!(c, Component::ParentDir))
    {
        return Err("--rootfs must not be / and must not contain ..".into());
    }
    if p.inside_id == 0 {
        return Err("--uid 0 is refused: the payload never runs as root inside".into());
    }
    if p.hostname.is_empty()
        || p.hostname.len() > 63
        || !p
            .hostname
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("--hostname must be 1-63 chars of [A-Za-z0-9-]".into());
    }
    if !(0..=19).contains(&p.nice) {
        return Err("--nice must be 0..19 (never raise priority above the host UI)".into());
    }
    for b in &p.binds_ro {
        if !b.src.is_absolute() {
            return Err(format!("bind source {} must be absolute", b.src.display()));
        }
        check_inside_path(&b.dst)?;
    }
    for (k, _) in &p.env {
        if k.is_empty() || k.contains('=') || k == "GALION_BOX_PROC" {
            return Err(format!("bad --env key {k:?}"));
        }
    }
    Ok(())
}

/// Lines for /proc/self/uid_map and gid_map: one inside id mapped to the caller's own id.
pub fn id_map_line(inside: u32, outside: u32) -> String {
    format!("{inside} {outside} 1\n")
}

/// Human-readable steps `run` would take. Printed by `plan`; also documents the design.
pub fn plan_steps(p: &Policy, uid: u32, gid: u32) -> Vec<String> {
    let mut s = vec![
        "caller stays outside every namespace and supervises (timeout)".to_string(),
        "clone(CLONE_NEWUSER|NEWNS|NEWPID|NEWNET|NEWUTS|NEWIPC) -> init, pid 1 (no root, no setuid helper)".to_string(),
        format!(
            "write setgroups=deny, uid_map {:?}, gid_map {:?}",
            id_map_line(p.inside_id, uid).trim_end(),
            id_map_line(p.inside_id, gid).trim_end()
        ),
        "init: PR_SET_PDEATHSIG=SIGKILL, fail closed unless getpid() == 1".to_string(),
        "mount / rprivate (no mount events leak back to the host)".to_string(),
        format!("bind {} onto itself", p.rootfs.display()),
        format!(
            "mount fresh proc at /proc; on failure {}",
            if p.require_proc { "abort (--require-proc)" } else { "mount an empty read-only tmpfs (host /proc is never exposed)" }
        ),
        "mount tmpfs /tmp (64 MiB, nosuid,nodev) and tmpfs /dev with null zero full random urandom bound in".to_string(),
    ];
    for b in &p.binds_ro {
        s.push(format!(
            "bind {} -> {} read-only (non-recursive)",
            b.src.display(),
            b.dst.display()
        ));
    }
    s.push(format!("sethostname {:?}", p.hostname));
    s.push(match p.net {
        NetMode::None => "network: empty namespace, no interface up (default-deny)".to_string(),
        NetMode::Loopback => "network: empty namespace, only lo up".to_string(),
    });
    s.push("pivot_root into the new root, detach and remove the old root".to_string());
    if p.ro_root {
        s.push("remount / read-only".to_string());
    }
    let mut lim = vec![format!("nice {}", p.nice)];
    if let Some(n) = p.nproc {
        lim.push(format!("RLIMIT_NPROC {n}"));
    }
    if let Some(n) = p.nofile {
        lim.push(format!("RLIMIT_NOFILE {n}"));
    }
    if let Some(n) = p.as_mb {
        lim.push(format!("RLIMIT_AS {n} MiB"));
    }
    lim.push("RLIMIT_CORE 0".to_string());
    s.push(format!("payload limits: {}", lim.join(", ")));
    s.push("payload: drop capability bounding set + ambient, PR_SET_NO_NEW_PRIVS, clean env, execve (uid != 0 so all caps are dropped)".to_string());
    s.push(format!("exec {:?}", p.argv));
    if let Some(t) = p.timeout {
        s.push(format!(
            "supervisor kills the whole pid namespace after {:.1}s",
            t.as_secs_f64()
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_full_run() {
        let c = parse_args(&a(&[
            "run",
            "--rootfs",
            "/var/tmp/r",
            "--net",
            "lo",
            "--bind-ro",
            "/usr/lib:/usr/lib",
            "--ro-root",
            "--timeout",
            "2.5",
            "--nproc",
            "32",
            "--as-mb",
            "512",
            "--nice",
            "5",
            "--env",
            "A=b",
            "--",
            "/bin/x",
            "-y",
        ]))
        .unwrap();
        let Cmd::Run(p) = c else { panic!("not run") };
        assert_eq!(p.rootfs, PathBuf::from("/var/tmp/r"));
        assert_eq!(p.net, NetMode::Loopback);
        assert_eq!(p.binds_ro.len(), 1);
        assert!(p.ro_root);
        assert_eq!(p.timeout, Some(Duration::from_millis(2500)));
        assert_eq!(p.nproc, Some(32));
        assert_eq!(p.as_mb, Some(512));
        assert_eq!(p.nice, 5);
        assert_eq!(p.argv, a(&["/bin/x", "-y"]));
        assert!(p.payload_env("hidden").contains(&"A=b".to_string()));
    }

    #[test]
    fn defaults_are_default_deny() {
        let Cmd::Plan(p) = parse_args(&a(&["plan", "--rootfs", "/r/x", "--", "/bin/sh"])).unwrap()
        else {
            panic!()
        };
        assert_eq!(p.net, NetMode::None);
        assert_eq!(p.inside_id, 1000);
        assert_eq!(p.nice, 10);
        assert!(!p.require_proc);
        let env = p.payload_env("fresh");
        assert!(env
            .iter()
            .all(|e| !e.starts_with("SSH_") && !e.starts_with("USER=")));
    }

    #[test]
    fn rejects_dangerous_policies() {
        let bad: &[&[&str]] = &[
            &["run", "--rootfs", "/", "--", "/bin/sh"],
            &["run", "--rootfs", "rel/dir", "--", "/bin/sh"],
            &["run", "--rootfs", "/a/../b", "--", "/bin/sh"],
            &["run", "--rootfs", "/r/x"],
            &["run", "--rootfs", "/r/x", "--uid", "0", "--", "/x"],
            &["run", "--rootfs", "/r/x", "--nice", "-5", "--", "/x"],
            &["run", "--rootfs", "/r/x", "--bind-ro", "/etc:/", "--", "/x"],
            &[
                "run",
                "--rootfs",
                "/r/x",
                "--bind-ro",
                "/etc:/a/../../x",
                "--",
                "/x",
            ],
            &[
                "run",
                "--rootfs",
                "/r/x",
                "--bind-ro",
                "/etc:/proc/x",
                "--",
                "/x",
            ],
            &[
                "run",
                "--rootfs",
                "/r/x",
                "--bind-ro",
                "etc:/etc",
                "--",
                "/x",
            ],
            &[
                "run",
                "--rootfs",
                "/r/x",
                "--hostname",
                "bad name",
                "--",
                "/x",
            ],
            &["run", "--rootfs", "/r/x", "--net", "host", "--", "/x"],
            &["run", "--rootfs", "/r/x", "--timeout", "0", "--", "/x"],
            &[
                "run",
                "--rootfs",
                "/r/x",
                "--env",
                "GALION_BOX_PROC=x",
                "--",
                "/x",
            ],
            &["frobnicate"],
        ];
        for b in bad {
            assert!(parse_args(&a(b)).is_err(), "accepted {b:?}");
        }
    }

    #[test]
    fn id_map_maps_one_id() {
        assert_eq!(id_map_line(1000, 4242), "1000 4242 1\n");
    }

    #[test]
    fn plan_mentions_every_layer() {
        let mut p = Policy::new(PathBuf::from("/r/x"), a(&["/bin/sh"]));
        p.timeout = Some(Duration::from_secs(3));
        p.ro_root = true;
        let s = plan_steps(&p, 1001, 1001).join("\n");
        for needle in [
            "CLONE_NEWUSER",
            "NEWNET",
            "pivot_root",
            "PDEATHSIG",
            "NO_NEW_PRIVS",
            "default-deny",
            "read-only",
            "1000 1001 1",
            "3.0s",
        ] {
            assert!(s.contains(needle), "plan misses {needle}:\n{s}");
        }
    }

    #[test]
    fn inside_helpers_pass_through() {
        assert_eq!(
            parse_args(&a(&["spawn", "5"])).unwrap(),
            Cmd::Inside(a(&["spawn", "5"]))
        );
        assert_eq!(parse_args(&[]).unwrap(), Cmd::Help);
    }
}
