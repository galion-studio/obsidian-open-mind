// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
// Copyright (c) 3000 Galion / galion-studio

//! `gaia-cortex hermes-dispatch`: deliver cortex handoffs to the Hermes boss.
//!
//! Started by a systemd `.path` unit when `*.json` appears in the cortex's
//! Hermes inbox (inotify: no timer, no poll loop). One run drains the inbox
//! and exits. Per handoff:
//!
//! 1. claim: rename the file into `<inbox>/processing/` (same mount; it ends
//!    in `done/`, `refused/` or `failed/`, never deleted) so the `.path` unit cannot
//!    re-fire on it, and a crash never repeats a paid call;
//! 2. budget: the combined logged Hermes spend (sum of every session in
//!    hermes-rs's spend file, i.e. all callers: research helper, chat,
//!    handoffs) plus a per-handoff reserve must stay under the ceiling
//!    (Mate t218: $20 total, hard max here). Otherwise refuse, and drop the
//!    `BUDGET-STOP` file into the inbox so the cortex stops handing off;
//! 3. ask the Hermes boss model once (no retries) with the
//!    `cortex-escalation` skill as the system prompt: diagnose, propose a
//!    fix, change nothing;
//! 4. write the proposal to `<state>/proposals/<id>.md` +
//!    `<state>/proposals.jsonl` (status `awaiting-mate-approval`) and notify
//!    Mate (Telegram when `TELEGRAM_BOT_TOKEN` + `TELEGRAM_CHAT_ID` are set;
//!    otherwise the proposal file is the inbox).
//!
//! Secrets: the gateway key is read from the env var NAMED by
//! `HANDOFF_KEY_ENV` (default `API_SERVER_KEY`) and goes to curl on stdin
//! (`-K -`), never argv, never logged.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The dispatcher never allows more than this, whatever the env says.
pub const HARD_BUDGET_USD: f64 = 20.0;
pub const SESSION_KEY: &str = "cortex-escalation";

#[derive(Debug, Clone)]
pub struct DispatchCfg {
    pub inbox: PathBuf,
    pub state_dir: PathBuf,
    pub spend_file: PathBuf,
    pub budget_usd: f64,
    pub reserve_usd: f64,
    pub hermes_url: String,
    pub model: String,
    pub skill: PathBuf,
    pub max_tokens: u32,
    pub timeout_s: u64,
    pub key_env: String,
    pub max_per_run: usize,
}

impl DispatchCfg {
    pub fn from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, String> {
        let s = |k: &str, d: &str| {
            get(k)
                .filter(|v| !v.trim().is_empty())
                .unwrap_or_else(|| d.into())
        };
        let f = |k: &str, d: f64| {
            get(k)
                .and_then(|v| v.trim().parse::<f64>().ok())
                .unwrap_or(d)
        };
        let n = |k: &str, d: u64| {
            get(k)
                .and_then(|v| v.trim().parse::<u64>().ok())
                .unwrap_or(d)
        };
        let c = DispatchCfg {
            inbox: s("HANDOFF_INBOX", "/var/lib/gaia-cortex/hermes-inbox").into(),
            state_dir: s("HANDOFF_STATE_DIR", "/var/lib/gaia-hermes-handoff").into(),
            spend_file: s("HERMES_SPEND_FILE", "/var/lib/hermes-rs/rs-spend.json").into(),
            budget_usd: f("HANDOFF_BUDGET_USD", HARD_BUDGET_USD).clamp(0.0, HARD_BUDGET_USD),
            reserve_usd: f("HANDOFF_RESERVE_USD", 1.0).max(0.0),
            hermes_url: s("HERMES_URL", "http://127.0.0.1:8642"),
            model: s("HANDOFF_MODEL", "boss"),
            skill: s(
                "HANDOFF_SKILL",
                "/opt/galion/hermes/skills/galion/cortex-escalation/SKILL.md",
            )
            .into(),
            max_tokens: n("HANDOFF_MAX_TOKENS", 3000).min(8000) as u32,
            timeout_s: n("HANDOFF_TIMEOUT_S", 900).min(1800),
            key_env: s("HANDOFF_KEY_ENV", "API_SERVER_KEY"),
            max_per_run: n("HANDOFF_MAX_PER_RUN", 6).min(6) as usize,
        };
        if !loopback_http(&c.hermes_url) {
            return Err(format!(
                "HERMES_URL must be http://<loopback>:port, got {}",
                c.hermes_url
            ));
        }
        if c.key_env.is_empty()
            || !c
                .key_env
                .bytes()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("HANDOFF_KEY_ENV must be an env var NAME".into());
        }
        Ok(c)
    }
}

fn loopback_http(u: &str) -> bool {
    ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"]
        .iter()
        .any(|p| u.starts_with(p))
}

/// One HTTP request: (status, body). Injectable for tests.
pub trait Http {
    fn send(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: Option<&str>,
        timeout_s: u64,
    ) -> Result<(u16, String), String>;
}

/// Combined logged Hermes spend: every session hermes-rs has recorded
/// (it keeps up to 2000 sessions, dropping only the cheapest). Fails closed.
pub fn hermes_spent(spend_file: &Path) -> Result<f64, String> {
    let t = std::fs::read_to_string(spend_file)
        .map_err(|e| format!("spend file {}: {e}", spend_file.display()))?;
    let v: Value = serde_json::from_str(&t).map_err(|e| format!("spend file: {e}"))?;
    let sessions = v["sessions"]
        .as_object()
        .ok_or("spend file: no sessions map")?;
    let sum: f64 = sessions.values().filter_map(Value::as_f64).sum();
    if !sum.is_finite() || sum < 0.0 {
        return Err("spend file: bad total".into());
    }
    Ok(sum)
}

fn append(p: &Path, v: &Value) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(p)
    {
        let _ = writeln!(f, "{v}");
    }
}

fn mv(from: &Path, dir: &Path) -> PathBuf {
    let to = dir.join(from.file_name().unwrap_or_default());
    let _ = std::fs::rename(from, &to);
    to
}

fn stop_file(cfg: &DispatchCfg, why: &str, spent: f64, ts: &str) {
    let _ = crate::handoff::write_atomic(
        &cfg.inbox,
        crate::handoff::STOP_FILE,
        &json!({"ts": ts, "why": why, "spent_usd": spent, "budget_usd": cfg.budget_usd}),
    );
}

fn notify(http: &dyn Http, get: &dyn Fn(&str) -> Option<String>, text: &str) -> &'static str {
    let (Some(tok), Some(chat)) = (get("TELEGRAM_BOT_TOKEN"), get("TELEGRAM_CHAT_ID")) else {
        return "no-channel-configured";
    };
    if !tok
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b':' || b == b'_' || b == b'-')
    {
        return "bad-token";
    }
    let text: String = text.chars().take(3800).collect();
    let body =
        json!({"chat_id": chat.trim(), "text": text, "disable_web_page_preview": true}).to_string();
    let url = format!("https://api.telegram.org/bot{}/sendMessage", tok.trim());
    let h = [("Content-Type".to_string(), "application/json".to_string())];
    match http.send(&url, &h, Some(&body), 30) {
        Ok((200, _)) => "telegram-sent",
        Ok(_) | Err(_) => "telegram-failed",
    }
}

/// Drain the inbox once. Returns a JSON report (no secrets).
pub fn run(
    cfg: &DispatchCfg,
    http: &dyn Http,
    get: &dyn Fn(&str) -> Option<String>,
    now: &dyn Fn() -> String,
) -> Value {
    // Claimed files stay on the inbox mount (systemd gives the inbox and the
    // state dir separate bind mounts: rename across them fails with EXDEV).
    let arch = |d: &str| cfg.inbox.join(d);
    for d in ["processing", "done", "refused", "failed"] {
        let _ = std::fs::create_dir_all(arch(d));
    }
    let _ = std::fs::create_dir_all(cfg.state_dir.join("proposals"));
    let ledger = cfg.state_dir.join("ledger.jsonl");
    let props = cfg.state_dir.join("proposals.jsonl");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&cfg.inbox)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension().is_some_and(|x| x == "json")
                        && !p
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let mut out: Vec<Value> = vec![];
    let skill = std::fs::read_to_string(&cfg.skill).ok();
    for (i, f) in files.iter().enumerate() {
        // 1. claim first: whatever happens next, this file is never re-sent.
        let claimed = mv(f, &arch("processing"));
        if !claimed.exists() {
            out.push(json!({"file": f, "status": "claim-failed"}));
            continue;
        }
        let ts = now();
        let ticket: Value = match std::fs::read_to_string(&claimed)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
        {
            Some(v) => v,
            None => {
                mv(&claimed, &arch("failed"));
                out.push(json!({"file": f, "status": "bad-json"}));
                continue;
            }
        };
        let id = ticket["handoff_id"]
            .as_str()
            .unwrap_or("hoff-unknown")
            .to_string();
        let head = format!(
            "incident {} | subject {} | rule {} | severity {}",
            ticket["incident"].as_str().unwrap_or("?"),
            ticket["subject"].as_str().unwrap_or("?"),
            ticket["rule"].as_str().unwrap_or("?"),
            ticket["severity"].as_str().unwrap_or("?")
        );
        let refuse = |why: String, spent: f64, status: &str| -> Value {
            let dest = if status == "refused-budget" {
                "refused"
            } else {
                "failed"
            };
            mv(&claimed, &arch(dest));
            let r = json!({"ts": ts, "handoff_id": id, "incident": ticket["incident"], "status": status,
                           "why": why, "spent_usd": spent, "cost_usd": 0.0});
            append(&ledger, &r);
            append(&props, &r);
            r
        };
        // 2. budget, re-read before every call.
        let spent = match hermes_spent(&cfg.spend_file) {
            Ok(s) => s,
            Err(e) => {
                out.push(refuse(
                    format!("cannot read Hermes spend ({e}); fail closed"),
                    -1.0,
                    "refused-budget",
                ));
                continue;
            }
        };
        if spent + cfg.reserve_usd > cfg.budget_usd {
            let why = format!(
                "Hermes spend ${spent:.4} + reserve ${:.2} > ceiling ${:.2}",
                cfg.reserve_usd, cfg.budget_usd
            );
            stop_file(cfg, &why, spent, &ts);
            let mut r = refuse(why.clone(), spent, "refused-budget");
            r["notify"] = json!(notify(
                http,
                get,
                &format!(
                    "Galion cortex -> Hermes: handoff {id} REFUSED, budget reached. {why}. {head}"
                )
            ));
            out.push(r);
            continue;
        }
        if i >= cfg.max_per_run {
            out.push(refuse(
                "max handoffs per run reached".into(),
                spent,
                "refused-cap",
            ));
            continue;
        }
        let Some(skill) = &skill else {
            out.push(refuse(
                format!("skill {} not readable", cfg.skill.display()),
                spent,
                "failed",
            ));
            continue;
        };
        let Some(key) = get(&cfg.key_env).filter(|k| !k.trim().is_empty()) else {
            out.push(refuse(format!("{} not set", cfg.key_env), spent, "failed"));
            continue;
        };
        // 3. one call, no retry.
        let sid = format!("cortex-{id}");
        let user = format!(
            "Cortex escalation handoff {id} ({head}). Follow the cortex-escalation skill: diagnose, propose a fix, change nothing.\n\n```json\n{}\n```",
            serde_json::to_string_pretty(&ticket).unwrap_or_default().chars().take(24_000).collect::<String>()
        );
        let body = json!({"model": cfg.model, "stream": false, "max_tokens": cfg.max_tokens,
                          "messages": [{"role": "system", "content": skill}, {"role": "user", "content": user}]})
        .to_string();
        let hdrs = vec![
            (
                "Authorization".to_string(),
                format!("Bearer {}", key.trim()),
            ),
            ("Content-Type".to_string(), "application/json".to_string()),
            ("X-Hermes-Session-Id".to_string(), sid.clone()),
            ("X-Hermes-Session-Key".to_string(), SESSION_KEY.to_string()),
        ];
        let base = cfg.hermes_url.trim_end_matches('/');
        let res = http.send(
            &format!("{base}/v1/chat/completions"),
            &hdrs,
            Some(&body),
            cfg.timeout_s,
        );
        let sess = http
            .send(&format!("{base}/api/sessions/{sid}"), &hdrs[..1], None, 30)
            .ok()
            .and_then(|(c, b)| (c == 200).then_some(b))
            .and_then(|b| serde_json::from_str::<Value>(&b).ok())
            .unwrap_or(Value::Null);
        let cost = sess["estimated_cost_usd"].as_f64().unwrap_or(0.0);
        let model = sess["model"].as_str().unwrap_or(&cfg.model).to_string();
        let (code, text) = match res {
            Ok((c, b)) => {
                let v: Value = serde_json::from_str(&b).unwrap_or(Value::Null);
                let t = v["choices"][0]["message"]["content"]
                    .as_str()
                    .map(str::to_string);
                (
                    c,
                    t.unwrap_or_else(|| {
                        v["error"]["message"]
                            .as_str()
                            .unwrap_or("")
                            .chars()
                            .take(300)
                            .collect()
                    }),
                )
            }
            Err(e) => (0, e),
        };
        let total = hermes_spent(&cfg.spend_file).unwrap_or(spent + cost);
        if code != 200 {
            let mut r = refuse(format!("Hermes HTTP {code}: {text}"), total, "failed");
            r["cost_usd"] = json!(cost);
            r["notify"] = json!(notify(http, get, &format!("Galion cortex -> Hermes: handoff {id} FAILED (Hermes HTTP {code}); no retry. {head}")));
            out.push(r);
            continue;
        }
        // 4. proposal + notification; nothing is changed.
        let ppath = cfg.state_dir.join("proposals").join(format!("{id}.md"));
        // The written proposal still starts with a real SPDX header. The tag is
        // split in this source file so REUSE comply does not parse the template
        // as a second license expression.
        let md = format!(
            concat!(
                "<!-- SPDX-",
                "License-Identifier: MIT -->\n<!-- Copyright (c) 3000 Galion / galion-studio -->\n# Hermes proposal {id}\n\n- {head}\n- received {ts}\n- model {model}, cost ${cost:.4}, Hermes total ${total:.4} of ${:.2}\n- status: AWAITING MATE APPROVAL. Nothing was changed. Cortex restarts (allowlist) need no approval; every code, config or service change does.\n\n{text}\n"
            ),
            cfg.budget_usd
        );
        let _ = std::fs::write(&ppath, md);
        mv(&claimed, &arch("done"));
        let msg = format!(
            "Galion cortex -> Hermes boss ({model}) proposal {id}\n{head}\nNothing changed; reply APPROVE {id} or REJECT {id}.\n\n{}",
            text.chars().take(3000).collect::<String>()
        );
        let nt = notify(http, get, &msg);
        let r = json!({"ts": ts, "handoff_id": id, "incident": ticket["incident"], "subject": ticket["subject"],
                       "rule": ticket["rule"], "status": "awaiting-mate-approval", "model": model, "cost_usd": cost,
                       "hermes_total_usd": total, "proposal": ppath, "notify": nt});
        append(&ledger, &r);
        append(&props, &r);
        out.push(r);
        if total + cfg.reserve_usd > cfg.budget_usd {
            stop_file(cfg, "ceiling reached after this handoff", total, &now());
        }
    }
    json!({"handled": out.len(), "results": out, "budget_usd": cfg.budget_usd,
           "hermes_spent_usd": hermes_spent(&cfg.spend_file).ok()})
}

/// `curl -K -`: url, headers and body path go in on stdin (never argv).
/// Only in builds with the `llm-advisor` feature (paid calls need it).
pub struct CurlHttp {
    pub tmp_dir: PathBuf,
}

impl Http for CurlHttp {
    #[cfg(not(feature = "llm-advisor"))]
    fn send(
        &self,
        _: &str,
        _: &[(String, String)],
        _: Option<&str>,
        _: u64,
    ) -> Result<(u16, String), String> {
        Err("built without the llm-advisor cargo feature".into())
    }

    #[cfg(feature = "llm-advisor")]
    fn send(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: Option<&str>,
        timeout_s: u64,
    ) -> Result<(u16, String), String> {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let bad =
            |s: &str| s.contains('"') || s.contains('\\') || s.contains('\n') || s.contains('\r');
        if bad(url) || headers.iter().any(|(k, v)| bad(k) || bad(v)) {
            return Err("unusable characters in url or header".into());
        }
        let body_path = self
            .tmp_dir
            .join(format!("hermes-dispatch-{}.json", std::process::id()));
        if let Some(b) = body {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create(true).truncate(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
            let mut f = o.open(&body_path).map_err(|e| format!("body file: {e}"))?;
            f.write_all(b.as_bytes())
                .map_err(|e| format!("body file: {e}"))?;
        }
        let child = Command::new("curl")
            .args([
                "-sS",
                "--max-time",
                &timeout_s.to_string(),
                "--proto",
                "=https,http",
                "-w",
                "\n%{http_code}",
                "-K",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_file(&body_path);
                return Err(format!("curl: {e}"));
            }
        };
        if let Some(mut si) = child.stdin.take() {
            let _ = writeln!(si, "url = \"{url}\"");
            for (k, v) in headers {
                let _ = writeln!(si, "header = \"{k}: {v}\"");
            }
            if body.is_some() {
                let _ = writeln!(si, "data-binary = \"@{}\"", body_path.display());
            }
        }
        let out = child.wait_with_output();
        if body.is_some() {
            let _ = std::fs::remove_file(&body_path);
        }
        let out = out.map_err(|e| format!("curl: {e}"))?;
        let s = String::from_utf8_lossy(&out.stdout).to_string();
        let (b, c) = s.rsplit_once('\n').unwrap_or(("", s.as_str()));
        let code: u16 = c.trim().parse().unwrap_or(0);
        if code == 0 {
            return Err(format!("curl exit {:?}", out.status.code()));
        }
        Ok((code, b.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Mock {
        calls: RefCell<Vec<String>>,
        chat: (u16, String),
        cost: f64,
        spend_file: PathBuf,
    }

    impl Http for Mock {
        fn send(
            &self,
            url: &str,
            h: &[(String, String)],
            body: Option<&str>,
            _: u64,
        ) -> Result<(u16, String), String> {
            self.calls.borrow_mut().push(url.to_string());
            assert!(h.iter().all(|(_, v)| !v.contains('\n')));
            if url.ends_with("/v1/chat/completions") {
                let b: Value = serde_json::from_str(body.unwrap()).unwrap();
                assert_eq!(b["model"], "boss");
                assert!(b["messages"][0]["content"]
                    .as_str()
                    .unwrap()
                    .contains("SKILL"));
                // the call is billed: hermes-rs records the session
                let mut v: Value =
                    serde_json::from_str(&std::fs::read_to_string(&self.spend_file).unwrap())
                        .unwrap();
                v["sessions"]["cortex-x"] = json!(self.cost);
                std::fs::write(&self.spend_file, v.to_string()).unwrap();
                return Ok(self.chat.clone());
            }
            if url.contains("/api/sessions/") {
                return Ok((
                    200,
                    json!({"estimated_cost_usd": self.cost, "model": "anthropic/claude-opus-5.5"})
                        .to_string(),
                ));
            }
            if url.contains("api.telegram.org") {
                return Ok((200, "{}".into()));
            }
            Err("unexpected".into())
        }
    }

    fn setup(tag: &str, spent: f64, n: usize) -> (PathBuf, DispatchCfg) {
        let d = std::env::temp_dir().join(format!("cortex-dispatch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("inbox")).unwrap();
        std::fs::write(
            d.join("spend.json"),
            json!({"sessions": {"a": spent / 2.0, "b": spent / 2.0}}).to_string(),
        )
        .unwrap();
        std::fs::write(d.join("SKILL.md"), "SKILL cortex-escalation").unwrap();
        for i in 0..n {
            crate::handoff::write_atomic(&d.join("inbox"), &format!("hoff-{i}.json"),
                &json!({"handoff_id": format!("hoff-{i}"), "incident": format!("inc-{i}"), "subject": "unit:x", "rule": "R3-cap", "severity": "error"})).unwrap();
        }
        let get = |k: &str| match k {
            "HANDOFF_INBOX" => Some(d.join("inbox").display().to_string()),
            "HANDOFF_STATE_DIR" => Some(d.join("state").display().to_string()),
            "HERMES_SPEND_FILE" => Some(d.join("spend.json").display().to_string()),
            "HANDOFF_SKILL" => Some(d.join("SKILL.md").display().to_string()),
            "HANDOFF_BUDGET_USD" => Some("50".into()),
            _ => None,
        };
        let c = DispatchCfg::from_env(&get).unwrap();
        assert_eq!(c.budget_usd, 20.0, "clamped to the $20 ceiling");
        (d, c)
    }

    fn env_with_key(k: &str) -> Option<String> {
        (k == "API_SERVER_KEY").then(|| "test-key".to_string())
    }

    #[test]
    fn proposal_written_once_no_retry_and_nothing_left() {
        let (d, c) = setup("ok", 1.14, 1);
        let m = Mock {
            calls: RefCell::new(vec![]),
            chat: (
                200,
                json!({"choices": [{"message": {"content": "diagnosis + proposed patch"}}]})
                    .to_string(),
            ),
            cost: 0.08,
            spend_file: c.spend_file.clone(),
        };
        let r = run(&c, &m, &env_with_key, &|| {
            "2026-10-21T23:00:00+02:00".into()
        });
        assert_eq!(r["results"][0]["status"], "awaiting-mate-approval");
        assert_eq!(r["results"][0]["notify"], "no-channel-configured");
        let md = std::fs::read_to_string(c.state_dir.join("proposals/hoff-0.md")).unwrap();
        assert!(md.contains("AWAITING MATE APPROVAL") && md.contains("proposed patch"));
        let top_json = std::fs::read_dir(&c.inbox)
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
            .count();
        assert_eq!(top_json, 0, "inbox drained");
        assert!(c.inbox.join("done/hoff-0.json").exists());
        // second run: nothing to do, no calls
        let n = m.calls.borrow().len();
        run(&c, &m, &env_with_key, &|| "t".into());
        assert_eq!(m.calls.borrow().len(), n);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn budget_refusal_writes_stop_file_and_calls_nothing_paid() {
        let (d, c) = setup("budget", 19.5, 2);
        let m = Mock {
            calls: RefCell::new(vec![]),
            chat: (200, "{}".into()),
            cost: 0.0,
            spend_file: c.spend_file.clone(),
        };
        let r = run(&c, &m, &env_with_key, &|| "t".into());
        assert_eq!(r["results"][0]["status"], "refused-budget");
        assert_eq!(r["results"][1]["status"], "refused-budget");
        assert!(m.calls.borrow().is_empty(), "no paid call");
        assert!(c.inbox.join(crate::handoff::STOP_FILE).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn failure_is_not_retried_and_missing_spend_fails_closed() {
        let (d, c) = setup("fail", 0.0, 1);
        let m = Mock {
            calls: RefCell::new(vec![]),
            chat: (502, json!({"error": {"message": "upstream"}}).to_string()),
            cost: 0.0,
            spend_file: c.spend_file.clone(),
        };
        let r = run(&c, &m, &env_with_key, &|| "t".into());
        assert_eq!(r["results"][0]["status"], "failed");
        assert_eq!(
            m.calls
                .borrow()
                .iter()
                .filter(|u| u.ends_with("/completions"))
                .count(),
            1
        );
        assert!(c.inbox.join("failed/hoff-0.json").exists());
        std::fs::remove_file(&c.spend_file).unwrap();
        crate::handoff::write_atomic(&c.inbox, "hoff-9.json", &json!({"handoff_id": "hoff-9"}))
            .unwrap();
        let r = run(&c, &m, &env_with_key, &|| "t".into());
        assert_eq!(r["results"][0]["status"], "refused-budget");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn config_guards() {
        let bad = |k: &str| (k == "HERMES_URL").then(|| "https://openrouter.ai/api".to_string());
        assert!(DispatchCfg::from_env(&bad).is_err());
        let c = DispatchCfg::from_env(&|_| None).unwrap();
        assert_eq!(
            (c.model.as_str(), c.budget_usd, c.max_per_run),
            ("boss", 20.0, 6)
        );
    }
}
