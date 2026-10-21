// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! The subset of systemd unit syntax that the cutover units use. Keys the sim does
//! not enforce (sandboxing, Nice, CPUQuota, User=...) are kept in `ignored` so the
//! rehearsal docs can list exactly what the sim does not model.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restart {
    No,
    Always,
    OnFailure,
}

#[derive(Debug, Clone)]
pub struct Unit {
    pub name: String,
    pub exec: Vec<String>,
    pub env: Vec<(String, String)>,
    pub restart: Restart,
    pub restart_sec: Duration,
    pub timeout_stop: Duration,
    pub start_limit_interval: Duration,
    pub start_limit_burst: u32,
    pub workdir: Option<String>,
    pub text: String,
    pub ignored: Vec<String>,
}

/// systemd time span: `2`, `2s`, `500ms`, `10min`, `1h`, `infinity`, `1min 30s`.
pub fn parse_span(v: &str) -> Result<Duration, String> {
    let v = v.trim();
    if v == "infinity" {
        return Ok(Duration::from_secs(u64::from(u32::MAX)));
    }
    let mut total = Duration::ZERO;
    for part in v.split_whitespace() {
        let split = part
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(part.len());
        let (num, unit) = part.split_at(split);
        let n: f64 = num.parse().map_err(|_| format!("bad time span {v:?}"))?;
        let mult = match unit {
            "" | "s" | "sec" | "second" | "seconds" => 1.0,
            "ms" | "msec" => 0.001,
            "us" | "usec" => 0.000_001,
            "m" | "min" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hour" | "hours" => 3600.0,
            _ => return Err(format!("bad time unit {unit:?} in {v:?}")),
        };
        total += Duration::from_secs_f64(n * mult);
    }
    Ok(total)
}

/// Split on whitespace, honouring double quotes (`"A=b c" D=e`).
pub fn split_words(v: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut any = false;
    for c in v.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            c if c.is_whitespace() && !quoted => {
                if any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => {
                cur.push(c);
                any = true;
            }
        }
    }
    if any {
        out.push(cur);
    }
    out
}

pub fn parse(name: &str, text: &str) -> Result<Unit, String> {
    let mut u = Unit {
        name: name.to_string(),
        exec: Vec::new(),
        env: Vec::new(),
        restart: Restart::No,
        restart_sec: Duration::from_millis(100),
        timeout_stop: Duration::from_secs(90),
        start_limit_interval: Duration::from_secs(10),
        start_limit_burst: 5,
        workdir: None,
        text: text.to_string(),
        ignored: Vec::new(),
    };
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_string();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("{name}: not key=value: {line:?}"));
        };
        let (k, v) = (k.trim(), v.trim());
        match (section.as_str(), k) {
            ("Service", "ExecStart") => {
                let v = v.trim_start_matches(['-', '@', ':', '+', '!']);
                u.exec = split_words(v);
            }
            ("Service", "Environment") => {
                for w in split_words(v) {
                    if let Some((ek, ev)) = w.split_once('=') {
                        u.env.retain(|(k2, _)| k2 != ek);
                        u.env.push((ek.to_string(), ev.to_string()));
                    }
                }
            }
            ("Service", "Restart") => {
                u.restart = match v {
                    "no" => Restart::No,
                    "always" => Restart::Always,
                    "on-failure" => Restart::OnFailure,
                    other => return Err(format!("{name}: Restart={other} not modelled")),
                }
            }
            ("Service", "RestartSec") => u.restart_sec = parse_span(v)?,
            ("Service", "TimeoutStopSec") => u.timeout_stop = parse_span(v)?,
            ("Service", "WorkingDirectory") => u.workdir = Some(v.to_string()),
            ("Unit" | "Service", "StartLimitIntervalSec") => {
                u.start_limit_interval = parse_span(v)?
            }
            ("Unit" | "Service", "StartLimitBurst") => {
                u.start_limit_burst = v.parse().map_err(|_| format!("{name}: bad burst"))?
            }
            ("Service", "Type") if v != "simple" => {
                return Err(format!("{name}: Type={v} not modelled (simple only)"))
            }
            ("Service", "Type") => {}
            _ => u.ignored.push(format!("[{section}] {k}")),
        }
    }
    if u.exec.is_empty() {
        return Err(format!("{name}: no ExecStart"));
    }
    if !u.exec[0].starts_with('/') {
        return Err(format!("{name}: ExecStart must be an absolute path"));
    }
    Ok(u)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TUNER: &str = "[Unit]\nDescription=t\n[Service]\nExecStart=/opt/galion/tuner/bin/tuner\n\
Environment=TUNER_ADDR=127.0.0.1:8650\nEnvironment=TUNER_ROOT=/opt/galion/tuner/www\n\
Restart=always\nRestartSec=2\nNice=10\nNoNewPrivileges=true\n[Install]\nWantedBy=multi-user.target\n";

    #[test]
    fn parses_live_tuner_shape() {
        let u = parse("tuner", TUNER).unwrap();
        assert_eq!(u.exec, vec!["/opt/galion/tuner/bin/tuner"]);
        assert_eq!(u.env.len(), 2);
        assert_eq!(u.restart, Restart::Always);
        assert_eq!(u.restart_sec, Duration::from_secs(2));
        assert_eq!(u.start_limit_burst, 5);
        assert_eq!(u.start_limit_interval, Duration::from_secs(10));
        assert!(u.ignored.contains(&"[Service] Nice".to_string()));
    }

    #[test]
    fn spans_and_words() {
        assert_eq!(parse_span("500ms").unwrap(), Duration::from_millis(500));
        assert_eq!(parse_span("10min").unwrap(), Duration::from_secs(600));
        assert_eq!(parse_span("1min 30s").unwrap(), Duration::from_secs(90));
        assert!(parse_span("ten").is_err());
        assert_eq!(
            split_words(r#""A=b c" D=e"#),
            vec!["A=b c".to_string(), "D=e".to_string()]
        );
    }

    #[test]
    fn nerve_shape_and_rejects() {
        let t = "[Unit]\nStartLimitIntervalSec=600\nStartLimitBurst=5\n[Service]\nType=simple\n\
ExecStart=/opt/galion/rust/bin/galion-nerve serve\nRestart=always\nRestartSec=10\n";
        let u = parse("galion-nerve", t).unwrap();
        assert_eq!(u.exec, vec!["/opt/galion/rust/bin/galion-nerve", "serve"]);
        assert_eq!(u.start_limit_interval, Duration::from_secs(600));
        assert!(parse("x", "[Service]\nType=forking\nExecStart=/bin/x\n").is_err());
        assert!(parse("x", "[Service]\nExecStart=x\n").is_err());
        assert!(parse("x", "[Service]\nRestart=always\n").is_err());
    }
}
