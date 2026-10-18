// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
// tuner-publish: the trycloudflare quick-tunnel publisher for the tuner,
// replacing the hand-made transient unit `tuner-quick.service`
// (cloudflared tunnel --no-autoupdate --url http://127.0.0.1:8650).
// It runs cloudflared as a child, picks the public https://*.trycloudflare.com
// URL out of its log, writes it to a small JSON file, and restarts cloudflared
// with a 2..60 s backoff when it exits. No account, token or DNS is used; a
// quick tunnel gets a new random URL on every start.
//
// Config (env):
//   TUNER_PUBLISH_LOCAL  local URL to expose, default http://<TUNER_ADDR>
//                        (TUNER_ADDR default 127.0.0.1:8650)
//   TUNER_CLOUDFLARED    cloudflared binary, default `cloudflared` from PATH
//   TUNER_URL_FILE       where to write {"url","local","at"}; unset = log only.
//                        Removed again when cloudflared exits (the URL is dead).
//   TUNER_PUBLISH_ONCE=1 do not restart after cloudflared exits
// `--dry-run` prints the command and the config, starts nothing.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn main() {
    let local = tuner::env_or("TUNER_PUBLISH_LOCAL", &format!("http://{}", tuner::addr()));
    let bin = tuner::env_or("TUNER_CLOUDFLARED", "cloudflared");
    let url_file = std::env::var("TUNER_URL_FILE")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let once = std::env::var("TUNER_PUBLISH_ONCE").as_deref() == Ok("1");
    let args = ["tunnel", "--no-autoupdate", "--url", local.as_str()];
    if std::env::args().any(|a| a == "--dry-run") {
        println!("{bin} {}", args.join(" "));
        println!("url file: {}", url_file.as_deref().unwrap_or("(none)"));
        println!(
            "restart: {}",
            if once { "no" } else { "yes, 2..60 s backoff" }
        );
        return;
    }
    let mut delay = 2u64;
    loop {
        let started = Instant::now();
        match run(&bin, &args, &local, url_file.as_deref()) {
            Ok(code) => eprintln!("tuner-publish: cloudflared exited ({code:?})"),
            Err(e) => eprintln!("tuner-publish: cannot start {bin}: {e}"),
        }
        if let Some(f) = url_file.as_deref() {
            let _ = std::fs::remove_file(f);
        }
        if once {
            std::process::exit(1);
        }
        delay = tuner::next_delay(delay, started.elapsed().as_secs());
        eprintln!("tuner-publish: restarting in {delay} s");
        std::thread::sleep(Duration::from_secs(delay));
    }
}

fn run(
    bin: &str,
    args: &[&str],
    local: &str,
    url_file: Option<&str>,
) -> std::io::Result<Option<i32>> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(err) = child.stderr.take() {
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            eprintln!("{line}");
            if let Some(url) = tuner::quick_url(&line) {
                eprintln!("tuner-publish: public URL {url}");
                if let Some(f) = url_file {
                    write_url(f, &url, local);
                }
            }
        }
    }
    Ok(child.wait()?.code())
}

fn write_url(path: &str, url: &str, local: &str) {
    let body = format!(
        "{{\"url\":{},\"local\":{},\"at\":{}}}\n",
        tuner::quote(url),
        tuner::quote(local),
        tuner::quote(&zagreb_time::now_rfc3339())
    );
    let tmp = format!("{path}.tmp");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}
