// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

use galion_boot::secrets::{self, Live, Tools};
use galion_boot::supervise::{parse_env_file, Supervisor};
use galion_boot::{config, hardware, units};
use openmind_survival::Runner;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn out(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-out/galion-boot")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn staged() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/staged")
}

#[test]
fn staged_units_match_renderer() {
    let f = config::parse(&std::fs::read_to_string(staged().join("boot.toml")).unwrap()).unwrap();
    let all = units::render_all(&f.service, false);
    assert!(all.len() >= 6);
    for (name, body) in all {
        let disk = std::fs::read_to_string(staged().join(&name)).unwrap_or_default();
        assert_eq!(disk, body, "{name} is stale: re-run render-units");
        assert!(
            !body.contains("[Install]"),
            "{name}: staged units have no [Install]"
        );
        assert!(!body.to_lowercase().contains("lock-secrets") && !body.contains("--token"));
    }
    let with = units::render_all(&f.service, true);
    assert!(with
        .iter()
        .any(|(n, b)| n == "galion-guardian.service" && b.contains("WantedBy=multi-user.target")));
    assert!(with
        .iter()
        .any(|(n, b)| n == "galion-fire-loops.timer" && b.contains("WantedBy=timers.target")));
    config::parse(&std::fs::read_to_string(staged().join("boot-device.toml")).unwrap()).unwrap();
}

#[test]
fn config_refusals_and_quoting() {
    let svc = |argv: &str| format!("[[service]]\nname=\"t\"\ndescription=\"d\"\nargv={argv}\n");
    assert!(config::parse(&svc(
        "[\"/usr/bin/cloudflared\",\"tunnel\",\"run\",\"--token\",\"x\"]"
    ))
    .is_err());
    assert!(config::parse(&svc("[\"/bin/bash\",\"guard/lock-secrets.sh\"]")).is_err());
    assert!(config::parse(&svc("[\"relative\"]")).is_err());
    assert!(config::parse(&svc("[]")).is_err());
    assert!(config::parse(&(svc("[\"/bin/true\"]") + "restart=\"sometimes\"\n")).is_err());
    assert!(config::parse(&(svc("[\"/bin/true\"]") + &svc("[\"/bin/true\"]"))).is_err());
    assert_eq!(units::quote("a b"), "\"a b\"");
    assert_eq!(units::quote("x%y"), "\"x%%y\"");
    assert_eq!(units::quote("/opt/x"), "/opt/x");
}

struct Smi(Option<&'static str>);
impl Runner for Smi {
    fn run(&self, p: &str, _a: &[&str]) -> Result<String, String> {
        assert_eq!(p, "nvidia-smi");
        self.0.map(str::to_string).ok_or("not found".into())
    }
}

#[test]
fn hardware_profiles() {
    let d = out("hw");
    let proc_root = d.join("proc");
    std::fs::create_dir_all(&proc_root).unwrap();
    std::fs::write(
        proc_root.join("meminfo"),
        "MemTotal: 16384000 kB\nMemAvailable: 8192000 kB\n",
    )
    .unwrap();
    let sys = d.join("sys");
    let p = |r: &'static Smi, lan, wan| hardware::Probe {
        runner: r,
        proc_root: &proc_root,
        sys_root: &sys,
        lan_ok: lan,
        wan_ok: wan,
    };
    static CPU: Smi = Smi(None);
    static BIG: Smi = Smi(Some("RTX 4090, 24564, 1000\n"));
    let hw = hardware::hardware(&p(&CPU, false, true));
    assert_eq!(
        hw["line"],
        "CPU only. Hermes stays on the laptop model. Weights stay off."
    );
    assert_eq!(
        (hw["weights"].as_bool(), hw["mouth"].as_bool()),
        (Some(false), Some(true))
    );
    assert!(hw["gpu"].is_null());
    assert_eq!(hw["cap"], 15);
    let hw = hardware::hardware(&p(&BIG, false, true));
    assert_eq!(hw["weights"], true);
    assert_eq!(hw["gpu"]["free_mib"], 23564);
    std::fs::create_dir_all(sys.join("class/drm/card0")).unwrap();
    let hw = hardware::hardware(&p(&CPU, true, false));
    assert_eq!(hw["llm"], "lan");
    let m = hardware::machine(&hw, "box", None);
    assert_eq!(
        (
            m["provider"].as_str(),
            m["publicIp"].as_str(),
            m["gpu"].as_str()
        ),
        (Some("cpu"), Some(""), Some("drm"))
    );
    assert_eq!(hardware::parse_nvidia("bad"), None);
}

/// A stand-in for `age -d -i ID BUNDLE`: the "bundle" is a plain tar.
fn fake_age(dir: &Path) -> String {
    let p = dir.join("fake-age");
    std::fs::write(&p, "#!/bin/sh\n[ \"$1\" = -d ] && [ \"$2\" = -i ] && [ -f \"$3\" ] || exit 1\nexec cat \"$4\"\n").unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p.to_string_lossy().into_owned()
}

#[test]
fn secrets_restore_rules() {
    let d = out("secrets");
    let src = d.join("src/persist");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("hermes.key"), "fake-hermes-key-for-test\n").unwrap();
    std::fs::write(src.join("github.token"), "fake-gh-token\n").unwrap();
    std::fs::write(src.join("stub.key"), "SEALED\n").unwrap();
    let bundle = d.join("b.tar.age");
    let st = std::process::Command::new("tar")
        .args(["-c", "-f"])
        .arg(&bundle)
        .arg("-C")
        .arg(d.join("src"))
        .arg("persist")
        .status()
        .unwrap();
    assert!(st.success());
    let id = d.join("id.agekey");
    std::fs::write(&id, "AGE-SECRET-KEY-FAKE").unwrap();
    let live = d.join("live");
    std::fs::create_dir_all(&live).unwrap();
    std::fs::write(live.join("github.token"), "real-live-token\n").unwrap();
    std::fs::write(live.join("hermes.key"), "SEALED\n").unwrap();
    let age = fake_age(&d);
    let t = Tools {
        age: &age,
        tar: "tar",
    };
    let names: Vec<String> = ["hermes.key", "github.token", "stub.key", "absent"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // dry-run writes nothing
    let steps = secrets::restore(&t, &bundle, &id, &live, &names, false).unwrap();
    let acts: Vec<&str> = steps.iter().map(|s| s.action).collect();
    assert_eq!(
        acts,
        [
            "install (replaces SEALED stub)",
            "keep (live file is real)",
            "install",
            "missing from bundle"
        ]
    );
    assert_eq!(secrets::live_state(&live.join("hermes.key")), Live::Stub);
    // apply
    let steps = secrets::restore(&t, &bundle, &id, &live, &names, true).unwrap();
    let acts: Vec<&str> = steps.iter().map(|s| s.action).collect();
    assert_eq!(
        acts,
        [
            "installed",
            "keep (live file is real)",
            "install failed",
            "missing from bundle"
        ]
    );
    assert_eq!(
        std::fs::read_to_string(live.join("hermes.key")).unwrap(),
        "fake-hermes-key-for-test\n"
    );
    assert_eq!(
        std::fs::metadata(live.join("hermes.key"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o400
    );
    assert_eq!(
        std::fs::read_to_string(live.join("github.token")).unwrap(),
        "real-live-token\n"
    );
    assert!(
        !live.join("stub.key").exists(),
        "a SEALED member is never installed"
    );
    assert!(std::fs::read_dir(&live).unwrap().all(|e| !e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with(".restore")));
    assert!(secrets::restore(&t, &bundle, &id, &live, &["../x".into()], false).is_err());
    let bad = Tools {
        age: &age,
        tar: "tar",
    };
    assert!(secrets::restore(&bad, &bundle, &d.join("missing-id"), &live, &names, false).is_err());
}

#[test]
fn tunnel_env_file() {
    let d = out("tunnel");
    let tok = d.join("galion-app.token");
    std::fs::write(&tok, "eyFAKEtokenFAKEtokenFAKEtoken\n").unwrap();
    let envf = d.join("cloudflared.env");
    let r = secrets::tunnel_env(&tok, &envf, false).unwrap();
    assert!(r.starts_with("would write") && !r.contains("eyFAKE"));
    assert!(!envf.exists());
    let r = secrets::tunnel_env(&tok, &envf, true).unwrap();
    assert!(!r.contains("eyFAKE"));
    assert_eq!(
        std::fs::read_to_string(&envf).unwrap(),
        "TUNNEL_TOKEN=eyFAKEtokenFAKEtokenFAKEtoken\n"
    );
    assert_eq!(
        std::fs::metadata(&envf).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::write(&tok, "SEALED\n").unwrap();
    assert!(secrets::tunnel_env(&tok, &envf, true).is_err());
}

#[test]
fn env_file_parse() {
    let v = parse_env_file("# c\nexport A=1\nB=\"two words\"\nC='x'\nbad line\n=nokey\n");
    assert_eq!(
        v,
        vec![
            ("A".into(), "1".into()),
            ("B".into(), "two words".into()),
            ("C".into(), "x".into())
        ]
    );
}

#[test]
fn supervisor_restarts_and_requests() {
    let d = out("sup");
    std::fs::write(d.join("e.env"), "GREETING=hi\n").unwrap();
    let t = format!(
        "[boot]\nlog_dir=\"{0}/logs\"\nsupervisor_dir=\"{0}/sup\"\n\
         [[service]]\nname=\"quick\"\ndescription=\"q\"\nargv=[\"/bin/sh\",\"-c\",\"echo $GREETING\"]\nenv_files=[\"{0}/e.env\"]\nrestart_sec=0\n\
         [[service]]\nname=\"once\"\ndescription=\"o\"\nargv=[\"/bin/true\"]\nrestart=\"on-failure\"\n\
         [[service]]\nname=\"long\"\ndescription=\"l\"\nargv=[\"/bin/sleep\",\"30\"]\n",
        d.display()
    );
    let f = config::parse(&t).unwrap();
    let mut s = Supervisor::new(&f);
    let t0 = Instant::now();
    s.tick(t0);
    assert_eq!(s.events.iter().filter(|e| e.contains("started")).count(), 3);
    std::thread::sleep(Duration::from_millis(400));
    s.tick(t0 + Duration::from_secs(1));
    s.tick(t0 + Duration::from_secs(2));
    let quick_starts = s
        .events
        .iter()
        .filter(|e| e.contains("quick started"))
        .count();
    assert!(quick_starts >= 2, "{:?}", s.events);
    assert_eq!(
        s.events
            .iter()
            .filter(|e| e.contains("once started"))
            .count(),
        1,
        "on-failure: no restart after exit 0"
    );
    let pid1 = std::fs::read_to_string(d.join("sup/pids/long")).unwrap();
    std::fs::create_dir_all(d.join("sup/restart")).unwrap();
    std::fs::write(d.join("sup/restart/long"), "x").unwrap();
    s.tick(t0 + Duration::from_secs(3));
    let pid2 = std::fs::read_to_string(d.join("sup/pids/long")).unwrap();
    assert_ne!(pid1, pid2);
    assert!(!d.join("sup/restart/long").exists());
    s.stop_all();
    assert!(std::fs::read_to_string(d.join("logs/quick.log"))
        .unwrap()
        .contains("hi"));
    // start limit: a crash loop pauses after 10 starts in the window
    for i in 0..30u64 {
        std::thread::sleep(Duration::from_millis(20));
        s.tick(t0 + Duration::from_secs(4 + i));
    }
    s.stop_all();
    assert!(
        s.events.iter().any(|e| e.contains("quick start limit hit")),
        "{:?}",
        s.events
    );
}
