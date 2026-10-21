// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! omnode: phone/LAN-side node of the Obsidian Open Mind Stack (MIT).
//! Rust only. See docs/phone-instance.md.

mod announce;
mod boot;
mod config;
mod discover;
mod dns;
mod doctor;
mod enroll;
mod identity;
mod pair;
mod sandbox;
mod seccomp;
mod sysinfo;
mod tunnel;
mod util;
mod wifi;
mod zagreb;

use identity::Identity;
use std::io::Read as _;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "omnode - Obsidian Open Mind Stack phone/LAN node

USAGE: omnode [--config FILE] <command>

  identity init [--no-bind]  create device key (~/.config/openmind/node.key, 600); binds this network
                           if its gateway is private (home LAN)
  identity show            print key fingerprint and device id only
  identity bind [--force]  trust the current network (only networks you own)
  identity network         show current network hash and whether it is trusted
  discover [--seconds N] [--dry-run] [--mdns-query] [--no-msearch] [--probe] [--probe-subnet]
                           polite LAN discovery, JSON on stdout
  announce [--seconds N] [--dry-run]
                           advertise _openmind._tcp over mDNS (0 = until stopped)
  pair URL [--state DIR] [--insecure-http] [--via-curl] [--no-qr] [--no-tunnel] [--wait S]
           [--poll-ms MS] [--tunnel-secs S] [--verbose]
                           enroll this device: creates/loads the key, sends the signed request, prints a
                           QR + short CODE, waits for the owner (exit 4 refused, 5 timed out), stores the
                           credential (600), then keeps the outbound tunnel up (dials out, never listens)
  pair [URL] --dry-run | --offline [--no-qr] [--state DIR] [--now UNIX]
                           print only: --dry-run the HTTP plan, --offline the signed payload + QR
                           (--now backdates it: Hermes must refuse an expired payload)
  tunnel [--state DIR] [--insecure-http] [--via-curl] [--secs S] [--verbose]
                           (re)open the outbound tunnel with the stored credential (exit 2 if refused)
  enroll request URL [--state DIR] [--insecure-http]
                           send the signed payload to Hermes (POST URL); exit 3 = waiting for owner
  enroll claim [--state DIR] [--insecure-http]
                           after approval: signed claim, stores the scoped credential (600, never printed)
  enroll status [--state DIR] [--scope S] [--insecure-http]
                           does Hermes accept this node (and scope S) now? exit 2 if not
  pair verify FILE --url URL [--now UNIX]
                           fail-closed check of a pairing payload (what Hermes runs); exit 2 if refused
  boot [--state DIR] [--listen IP:PORT] [--allow-lan] [--code-ttl S] [--code-file F]
       [--new-code] [--require-seccomp]
                           edge boot path: first-boot identity, seccomp self-test, one-time pairing
                           code, loopback HTTP (/health, /v1/node, POST /v1/pair, /v1/whoami)
  sandbox run [--wall S] [--cpu S] [--mem MB] [--allow-net] [--require-isolation]
              [--no-landlock] [--no-seccomp] [--no-namespaces] [--keep] -- CMD [ARGS...]
  sandbox check            report which isolation this platform provides
  sandbox selftest         install the seccomp deny-list in a child and try denied syscalls
  doctor                   what this device can do (SSID, namespaces, landlock, CPU)
  version

Set OMNODE_PRINT_RSS=1 to print peak RSS (VmHWM) to stderr on exit.";

fn die(msg: impl std::fmt::Display) -> ExitCode {
    eprintln!("omnode: {msg}");
    ExitCode::from(2)
}

fn print_json(v: &impl serde::Serialize) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

struct Args(Vec<String>);
impl Args {
    fn flag(&mut self, f: &str) -> bool {
        if let Some(i) = self.0.iter().position(|a| a == f) {
            self.0.remove(i);
            true
        } else {
            false
        }
    }
    fn value(&mut self, f: &str) -> Result<Option<String>, String> {
        match self.0.iter().position(|a| a == f) {
            Some(i) if i + 1 < self.0.len() => {
                let v = self.0.remove(i + 1);
                self.0.remove(i);
                Ok(Some(v))
            }
            Some(_) => Err(format!("{f} needs a value")),
            None => Ok(None),
        }
    }
    fn num(&mut self, f: &str) -> Result<Option<u64>, String> {
        self.value(f)?
            .map(|v| v.parse().map_err(|_| format!("{f}: not a number: {v}")))
            .transpose()
    }
    fn done(&self) -> Result<(), String> {
        match self.0.first() {
            Some(a) => Err(format!("unexpected argument: {a}")),
            None => Ok(()),
        }
    }
}

fn main() -> ExitCode {
    let code = real_main();
    if std::env::var_os("OMNODE_PRINT_RSS").is_some() {
        if let Some(kb) = util::vm_hwm_kb() {
            eprintln!("omnode: peak RSS (VmHWM) {kb} KiB");
        }
    }
    code
}

fn real_main() -> ExitCode {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    // Split off everything after `--` (sandbox command) before flag parsing.
    let (head, tail) = match raw.iter().position(|a| a == "--") {
        Some(i) => (raw[..i].to_vec(), raw[i + 1..].to_vec()),
        None => (raw, vec![]),
    };
    let mut a = Args(head);
    let cfg_path = match a.value("--config") {
        Ok(p) => p.map(PathBuf::from),
        Err(e) => return die(e),
    };
    let cfg = match config::load(cfg_path.as_deref()) {
        Ok(c) => c,
        Err(e) => return die(e),
    };
    if a.0.is_empty() || a.flag("--help") || a.flag("-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let cmd = a.0.remove(0);
    let sub = if a.0.first().is_some_and(|s| !s.starts_with('-')) {
        Some(a.0.remove(0))
    } else {
        None
    };
    let dir = Identity::dir_default();
    if cmd == "enroll" {
        return enroll_cmd(&cfg, sub.as_deref(), a, &dir);
    }
    let r: Result<(), String> = (|| match (cmd.as_str(), sub.as_deref()) {
        ("version", None) => {
            println!(
                "omnode {} ({}, {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH,
                sysinfo::platform()
            );
            Ok(())
        }
        ("identity", Some("init")) => {
            let no_bind = a.flag("--no-bind");
            a.done()?;
            let mut id = Identity::init(&dir)?;
            println!("device_id   {}", id.state.device_id);
            println!("fingerprint {}", id.fingerprint());
            let obs = wifi::observe(true);
            if no_bind {
                println!("network     not bound (--no-bind); run `omnode identity bind` on your own network");
                return Ok(());
            }
            if let Some(gw) = obs.gateway_ip.filter(|g| !sysinfo::is_private(*g)) {
                println!("network     not bound: gateway {gw} is public, not a home LAN (use `identity bind --force` if you really own it)");
                return Ok(());
            }
            match id.bind(&cfg, &obs) {
                Ok((h, kind, _)) => {
                    println!("network     bound (hash {}..., anchor {kind})", &h[..8])
                }
                Err(e) => println!("network     not bound: {e}"),
            }
            Ok(())
        }
        ("identity", Some("show")) => {
            a.done()?;
            let id = Identity::load(&dir)?;
            println!("device_id   {}", id.state.device_id);
            println!("fingerprint {}", id.fingerprint());
            Ok(())
        }
        ("identity", Some("bind")) => {
            let force = a.flag("--force");
            a.done()?;
            let mut id = Identity::load(&dir)?;
            let obs = wifi::observe(true);
            if let Some(gw) = obs
                .gateway_ip
                .filter(|g| !force && !sysinfo::is_private(*g))
            {
                return Err(format!("gateway {gw} is public, not a home LAN; refusing (use --force only for a network you own)"));
            }
            let (h, kind, new) = id.bind(&cfg, &obs)?;
            println!(
                "{} network hash {}... (anchor {kind})",
                if new { "bound" } else { "already trusted:" },
                &h[..8]
            );
            Ok(())
        }
        ("identity", Some("network")) => {
            a.done()?;
            let id = Identity::load(&dir)?;
            let obs = wifi::observe(true);
            let cur = id.current_network(&cfg, &obs);
            print_json(&serde_json::json!({
                "observed": obs.public(),
                "hash": cur.as_ref().map(|c| &c.0),
                "anchor": cur.as_ref().map(|c| c.1),
                "trusted": cur.as_ref().map(|c| c.2).unwrap_or(false),
                "trusted_count": id.state.trusted_networks.len(),
            }));
            Ok(())
        }
        ("discover", None) => {
            let o = discover::Opts {
                seconds: a.num("--seconds")?.unwrap_or(cfg.discover.seconds),
                dry_run: a.flag("--dry-run"),
                mdns_query: a.flag("--mdns-query") || cfg.discover.mdns_query,
                msearch: !a.flag("--no-msearch") && cfg.discover.ssdp_msearch,
                probe: a.flag("--probe"),
                probe_subnet: a.flag("--probe-subnet"),
            };
            let o = discover::Opts {
                probe: o.probe || o.probe_subnet,
                ..o
            };
            a.done()?;
            let id = Identity::load(&dir).ok();
            print_json(&discover::run(&cfg, &o, id.as_ref())?);
            Ok(())
        }
        ("announce", None) => {
            let secs = a.num("--seconds")?.unwrap_or(0);
            let dry = a.flag("--dry-run");
            a.done()?;
            let id = Identity::load(&dir)?;
            if dry {
                print_json(&announce::record(&cfg, &id));
                Ok(())
            } else {
                announce::run(&cfg, &id, secs)
            }
        }
        ("boot", None) => {
            let state_dir = a
                .value("--state")?
                .map(PathBuf::from)
                .unwrap_or(dir.clone());
            let o = boot::Opts {
                state_dir,
                listen: a
                    .value("--listen")?
                    .unwrap_or_else(|| boot::DEFAULT_LISTEN.into()),
                allow_lan: a.flag("--allow-lan"),
                code_ttl: a
                    .num("--code-ttl")?
                    .map(|v| v.clamp(1, 86_400) as i64)
                    .unwrap_or(boot::DEFAULT_CODE_TTL),
                code_file: a.value("--code-file")?.map(PathBuf::from),
                new_code: a.flag("--new-code"),
                require_seccomp: a.flag("--require-seccomp"),
            };
            a.done()?;
            boot::install_signal_handlers();
            let b = boot::start(&o)?;
            println!("{}", boot::boot_event(&b));
            use std::io::Write;
            let _ = std::io::stdout().flush();
            boot::serve(&b)
        }
        ("pair", Some("verify")) => {
            let url = a
                .value("--url")?
                .ok_or("pair verify: --url URL is required")?;
            let now = a
                .num("--now")?
                .map(|v| v as i64)
                .unwrap_or_else(zagreb::now_unix);
            let file = if a.0.first().is_some_and(|s| !s.starts_with('-')) {
                a.0.remove(0)
            } else {
                return Err(
                    "pair verify: give the JSON file `omnode pair` printed (- for stdin)".into(),
                );
            };
            a.done()?;
            let text = if file == "-" {
                let mut s = String::new();
                std::io::Read::read_to_string(&mut std::io::stdin().take(1 << 16), &mut s)
                    .map_err(|e| e.to_string())?;
                s
            } else {
                std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?
            };
            match pair::verify_document(&text, &url, now) {
                Ok(v) => {
                    print_json(&serde_json::json!({"ok": true, "verified": v}));
                    Ok(())
                }
                Err(e) => {
                    print_json(&serde_json::json!({"ok": false, "error": e}));
                    Err(format!("pair verify: refused: {e}"))
                }
            }
        }
        ("sandbox", Some("selftest")) => {
            a.done()?;
            let t = seccomp::self_test();
            print_json(&t);
            if t.enforced {
                Ok(())
            } else {
                Err("seccomp deny-list is not enforced on this device".into())
            }
        }
        ("pair", url) => {
            let no_qr = a.flag("--no-qr");
            let dry = a.flag("--dry-run");
            let offline = a.flag("--offline");
            let no_tunnel = a.flag("--no-tunnel");
            let verbose = a.flag("--verbose");
            let net = enroll::Net::new(a.flag("--insecure-http"), a.flag("--via-curl"));
            let wait = a.num("--wait")?;
            let poll_ms = a.num("--poll-ms")?.unwrap_or(3000).clamp(200, 60_000);
            let tunnel_secs = a.num("--tunnel-secs")?;
            let pair_dir = a
                .value("--state")?
                .map(PathBuf::from)
                .unwrap_or(dir.clone());
            let now = a
                .num("--now")?
                .map(|v| v as i64)
                .unwrap_or_else(zagreb::now_unix);
            let url = {
                let positional = url.map(String::from).or_else(|| {
                    if a.0.first().is_some_and(|s| !s.starts_with('-')) {
                        Some(a.0.remove(0))
                    } else {
                        None
                    }
                });
                positional
                    .or(cfg.pair.enroll_url.clone())
                    .ok_or("pair: give an enroll URL or set [pair] enroll_url")?
            };
            a.done()?;
            let id = if dry || offline || pair_dir.join("node.key").exists() {
                Identity::load(&pair_dir)?
            } else {
                let id = Identity::init(&pair_dir)?;
                println!(
                    "new device key created in {} (mode 600, never leaves this device)",
                    pair_dir.display()
                );
                id
            };
            let (p, sig, warns) = pair::build_at(&cfg, &id, &url, &wifi::observe(true), now)?;
            for w in warns {
                eprintln!("warning: {w}");
            }
            let body =
                serde_json::json!({"payload": p, "signature": sig, "signed_string": p.canonical()});
            if dry {
                print_json(&serde_json::json!({
                    "mode": "dry-run",
                    "would_send": {
                        "method": "POST",
                        "url": url,
                        "headers": {
                            "content-type": "application/json",
                            "accept": "application/json",
                            "user-agent": format!("omnode/{}", env!("CARGO_PKG_VERSION")),
                        },
                        "body": body,
                    },
                    "code": enroll::pairing_code(&p.public_key, &p.nonce),
                    "note": "Nothing was sent. Hermes enrolls only after the owner approves.",
                }));
                return Ok(());
            }
            if offline {
                print_json(&body);
                if !no_qr {
                    let q = pair::qr_payload(&id, &p);
                    println!(
                        "\nQR ({} chars, signed short form):\n{}",
                        q.len(),
                        pair::qr_text(&q)?
                    );
                }
                println!("Nothing was sent (--offline). `omnode pair {url}` sends it and waits for the owner.");
                return Ok(());
            }
            pair_flow(
                &pair_dir,
                &id,
                &url,
                &p,
                &body,
                net,
                PairOpts {
                    no_qr,
                    no_tunnel,
                    verbose,
                    wait,
                    poll: std::time::Duration::from_millis(poll_ms),
                    tunnel_secs,
                },
            )
        }
        ("tunnel", None) => {
            let net = enroll::Net::new(a.flag("--insecure-http"), a.flag("--via-curl"));
            let verbose = a.flag("--verbose");
            let secs = a.num("--secs")?;
            let tdir = a
                .value("--state")?
                .map(PathBuf::from)
                .unwrap_or(dir.clone());
            a.done()?;
            let id = Identity::load(&tdir)?;
            match tunnel::run(&tdir, &id, &tunnel::Opts { net, secs, verbose })? {
                tunnel::End::Deadline => Ok(()),
                tunnel::End::Refused(r) => Err(format!("tunnel refused: {r}; pair again")),
            }
        }
        ("sandbox", Some(s @ ("run" | "check"))) => {
            let mut o = sandbox::Opts::from_cfg(&cfg.sandbox);
            if let Some(v) = a.num("--wall")? {
                o.wall = std::time::Duration::from_secs(v.max(1));
            }
            if let Some(v) = a.num("--cpu")? {
                o.cpu_secs = v.max(1);
            }
            if let Some(v) = a.num("--mem")? {
                o.mem_bytes = v.max(16) << 20;
            }
            o.allow_net |= a.flag("--allow-net");
            o.require_isolation |= a.flag("--require-isolation");
            o.landlock &= !a.flag("--no-landlock");
            o.seccomp &= !a.flag("--no-seccomp");
            o.namespaces &= !a.flag("--no-namespaces");
            o.keep_dir = a.flag("--keep");
            a.done()?;
            if s == "check" {
                let r = sandbox::run(&["true".into()], &o)?;
                print_json(&r.isolation);
                return Ok(());
            }
            let r = sandbox::run(&tail, &o)?;
            print_json(&r);
            if r.exit_code == Some(0) {
                Ok(())
            } else {
                Err(format!(
                    "command failed (exit {:?}, signal {:?}, timed_out {})",
                    r.exit_code, r.signal, r.timed_out
                ))
            }
        }
        ("doctor", None) => {
            a.done()?;
            let id = Identity::load(&dir).ok();
            print_json(&doctor::run(&cfg, id.as_ref()));
            Ok(())
        }
        _ => Err(format!(
            "unknown command; see `omnode --help`

{USAGE}"
        )),
    })();
    match r {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => die(e),
    }
}

/// `omnode enroll request|claim|status`: exit 0 done, 3 waiting for the
/// owner, 2 refused or error. The credential is never printed.
fn enroll_cmd(
    cfg: &config::Config,
    sub: Option<&str>,
    mut a: Args,
    default_dir: &std::path::Path,
) -> ExitCode {
    let r: Result<enroll::Outcome, String> = (|| {
        let dir = a
            .value("--state")?
            .map(PathBuf::from)
            .unwrap_or_else(|| default_dir.to_path_buf());
        let allow_http = enroll::Net::new(a.flag("--insecure-http"), a.flag("--via-curl"));
        match sub {
            Some("request") => {
                let url = if a.0.first().is_some_and(|s| !s.starts_with('-')) {
                    a.0.remove(0)
                } else {
                    cfg.pair
                        .enroll_url
                        .clone()
                        .ok_or("enroll request: give the Hermes enroll URL")?
                };
                a.done()?;
                let id = Identity::load(&dir)?;
                let (p, sig, warns) = pair::build(cfg, &id, &url, &wifi::observe(true))?;
                for w in warns {
                    eprintln!("warning: {w}");
                }
                let doc = serde_json::json!({"payload": p, "signature": sig, "signed_string": p.canonical()});
                eprintln!(
                    "omnode: asking Hermes to enroll {} (fingerprint {}); the owner must approve this fingerprint",
                    id.state.device_id,
                    id.fingerprint()
                );
                enroll::request(&dir, &url, &doc, allow_http)
            }
            Some("claim") => {
                a.done()?;
                let id = Identity::load(&dir)?;
                enroll::claim(&dir, &id, allow_http)
            }
            Some("status") => {
                let scope = a.value("--scope")?;
                a.done()?;
                enroll::status(&dir, scope.as_deref(), allow_http)
            }
            _ => Err("enroll: use `enroll request URL`, `enroll claim` or `enroll status`".into()),
        }
    })();
    match r {
        Ok(enroll::Outcome::Done(v)) => {
            print_json(&serde_json::json!({"ok": true, "result": v}));
            ExitCode::SUCCESS
        }
        Ok(enroll::Outcome::Pending(v)) => {
            print_json(&serde_json::json!({"ok": true, "status": "pending", "result": v}));
            ExitCode::from(3)
        }
        Err(e) => {
            print_json(&serde_json::json!({"ok": false, "error": e}));
            die(e)
        }
    }
}

struct PairOpts {
    no_qr: bool,
    no_tunnel: bool,
    verbose: bool,
    /// Give up waiting for the owner after this many seconds (default: the
    /// expiry Hermes announced).
    wait: Option<u64>,
    poll: std::time::Duration,
    /// Stop the tunnel after this many seconds (tests); default: run until killed.
    tunnel_secs: Option<u64>,
}

fn out(msg: &str) {
    use std::io::Write;
    println!("{msg}");
    let _ = std::io::stdout().flush();
}

/// Exit codes of `omnode pair`: 0 done, 2 error, 4 refused by the owner,
/// 5 not approved in time, 6 credential refused by the tunnel.
fn pair_exit(code: i32, msg: &str) -> ! {
    eprintln!("omnode: {msg}");
    std::process::exit(code)
}

/// `omnode pair URL`: send the signed request, show QR + short code, wait
/// for the owner, store the credential, then keep the outbound tunnel up.
fn pair_flow(
    dir: &std::path::Path,
    id: &Identity,
    url: &str,
    p: &pair::Payload,
    doc: &serde_json::Value,
    net: enroll::Net,
    o: PairOpts,
) -> Result<(), String> {
    let code = enroll::pairing_code(&p.public_key, &p.nonce);
    out(&format!(
        "omnode pair: asking Hermes at {url} to enroll this device"
    ));
    let pending = match enroll::request(dir, url, doc, net)? {
        enroll::Outcome::Pending(v) | enroll::Outcome::Done(v) => v,
    };
    let h = &pending["hermes"];
    if h["code"].as_str() != Some(code.as_str()) {
        return Err(format!(
            "Hermes answered with code {} but this device computed {code}; not continuing (version mismatch or tampering)",
            h["code"]
        ));
    }
    let expires_unix = h["pending_expires_unix"].as_i64().unwrap_or(p.expires_unix);
    if !o.no_qr {
        let q = pair::qr_payload(id, p);
        out(&format!("\n{}", pair::qr_text(&q)?));
        out(&format!(
            "(QR: signed short form, {} chars: device id, public key, nonce)",
            q.len()
        ));
    }
    out("");
    out(&format!("  CODE         {code}"));
    out(&format!("  device       {}", id.state.device_id));
    out(&format!("  fingerprint  {}", id.fingerprint()));
    out(&format!(
        "  approve by   {}",
        h["pending_expires"].as_str().unwrap_or("?")
    ));
    out("");
    out(&format!(
        "The owner approves on the Hermes host with:  hermes-gateway enroll approve {code}"
    ));
    out(&format!(
        "(or refuses with: hermes-gateway enroll reject {code}). Waiting for the owner..."
    ));
    let local_deadline = match o.wait {
        Some(w) => zagreb::now_unix() + w as i64,
        None => expires_unix + 5,
    };
    let mut last = String::new();
    loop {
        if zagreb::now_unix() > local_deadline {
            pair_exit(
                5,
                "the owner did not approve in time; run `omnode pair` again for a new code",
            );
        }
        std::thread::sleep(o.poll);
        let (c, v) = match enroll::poll(url, id, net) {
            Ok(x) => x,
            Err(e) => {
                out(&format!("  (status poll failed: {e}; retrying)"));
                continue;
            }
        };
        let st = v["status"].as_str().unwrap_or("").to_string();
        if st != last {
            out(&format!(
                "  {} status: {st} (HTTP {c})",
                zagreb::now_rfc3339()
            ));
            last = st.clone();
        }
        match (c, st.as_str()) {
            (202, "pending") => continue,
            (200, "approved") => break,
            (403, "rejected") => pair_exit(4, "the owner refused this device"),
            (410, "expired") => pair_exit(
                5,
                "the owner did not approve in time; run `omnode pair` again for a new code",
            ),
            (403, "revoked") => pair_exit(4, "this device was revoked by the owner"),
            (429, _) => continue,
            _ => {
                return Err(format!(
                    "unexpected status from Hermes (HTTP {c}): {}",
                    v["error"].as_str().unwrap_or("")
                ))
            }
        }
    }
    match enroll::claim(dir, id, net)? {
        enroll::Outcome::Done(v) => out(&format!(
            "approved: scopes {} until {}; credential stored in {} (mode 600, never printed)",
            v["hermes"]["scopes"],
            v["hermes"]["expires"].as_str().unwrap_or("?"),
            v["credential_file"].as_str().unwrap_or("?")
        )),
        enroll::Outcome::Pending(_) => {
            return Err("Hermes still says pending after approval".into())
        }
    }
    if o.no_tunnel {
        out("--no-tunnel: not opening the tunnel; start it with `omnode tunnel`");
        return Ok(());
    }
    match tunnel::run(
        dir,
        id,
        &tunnel::Opts {
            net,
            secs: o.tunnel_secs,
            verbose: o.verbose,
        },
    )? {
        tunnel::End::Deadline => Ok(()),
        tunnel::End::Refused(r) => pair_exit(6, &format!("tunnel refused: {r}; pair again")),
    }
}
