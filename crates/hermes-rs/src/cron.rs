// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 2026 Slavik / Galion Studio

//! Cron scheduler for upstream `cron/jobs.json` (read-only: hermes-rs never
//! writes jobs.json; its own run state lives in `cron_state_file`, so a
//! shadow run cannot disturb the Python scheduler's bookkeeping).
//!
//! Done: `interval` and `once` schedules, `no_agent` bash scripts confined to
//! `HERMES_HOME/scripts` (cleared env, workdir, timeout + process-group
//! kill, redacted output), agent jobs through [`Agent`], upstream-format
//! output files. Stubbed: `cron` expressions (listed, never fired), delivery
//! to chat platforms (Galion has none), Python scripts (refused: hermes-rs
//! never runs Python).

use crate::agent::{Agent, Turn};
use crate::provider::{resolve_reasoning, resolve_target};
use serde_json::{json, Map, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct Scheduler {
    pub home: PathBuf,
    pub jobs_file: PathBuf,
    pub state_file: PathBuf,
    pub output_dir: PathBuf,
    pub script_timeout: Duration,
    pub agent: Option<Arc<Agent>>,
    pub cron_model: Option<String>,
    pub cron_provider: Option<String>,
    started: i64,
    state: Mutex<Map<String, Value>>,
    pub stop: Arc<AtomicBool>,
}

#[derive(Debug, PartialEq)]
pub struct RunOutcome {
    pub ok: bool,
    pub output: String,
    pub file: Option<PathBuf>,
}

impl Default for Scheduler {
    fn default() -> Self {
        Scheduler::new(Path::new("/nonexistent-hermes-home"), None)
    }
}

/// Seconds between runs, or None for unsupported/one-shot kinds.
fn interval_secs(job: &Value) -> Option<i64> {
    let s = &job["schedule"];
    (s["kind"] == "interval")
        .then(|| s["minutes"].as_f64())
        .flatten()
        .map(|m| (m * 60.0) as i64)
        .filter(|x| *x > 0)
}

impl Scheduler {
    pub fn new(home: &Path, agent: Option<Arc<Agent>>) -> Self {
        let (state_file, output_dir, timeout, model, provider) = match &agent {
            Some(a) => (
                a.cfg.cron_state_file.clone(),
                a.cfg.cron_output_dir.clone(),
                a.cfg.script_timeout_s,
                a.cfg.cron_model.clone(),
                a.cfg.cron_provider.clone(),
            ),
            None => (
                home.join("rs-cron-state.json"),
                home.join("rs-cron-output"),
                120,
                None,
                None,
            ),
        };
        let state = std::fs::read_to_string(&state_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        Scheduler {
            home: home.to_path_buf(),
            jobs_file: home.join("cron").join("jobs.json"),
            state_file,
            output_dir,
            script_timeout: Duration::from_secs(timeout.max(1)),
            agent,
            cron_model: model,
            cron_provider: provider,
            started: crate::time::now_secs(),
            state: Mutex::new(state),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// jobs.json, read fresh every call (Python may edit it).
    pub fn jobs(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.jobs_file)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["jobs"].as_array().cloned())
            .unwrap_or_default()
    }

    /// Next fire time (unix secs) for a job under hermes-rs's own state.
    /// Interval jobs with no own history anchor on scheduler start, so a
    /// shadow start never fires everything at once.
    pub fn next_run(&self, job: &Value) -> Option<i64> {
        if job["enabled"] == false || job["state"] == "completed" || job["state"] == "paused" {
            return None;
        }
        let id = job["id"].as_str()?;
        let st = self.state.lock().ok()?;
        let last = st.get(id).and_then(|s| s["last_run_at"].as_i64());
        match job["schedule"]["kind"].as_str() {
            Some("interval") => Some(last.unwrap_or(self.started) + interval_secs(job)?),
            Some("once") => {
                if last.is_some() {
                    return None;
                }
                crate::time::parse_rfc3339(job["schedule"]["run_at"].as_str()?)
            }
            _ => None, // "cron" expressions: stubbed
        }
    }

    pub fn list(&self) -> Vec<Value> {
        let st = self.state.lock().map(|s| s.clone()).unwrap_or_default();
        self.jobs()
            .into_iter()
            .map(|j| {
                let id = j["id"].as_str().unwrap_or("").to_string();
                let own = st.get(&id).cloned().unwrap_or(json!({}));
                json!({
                    "id": id,
                    "name": j["name"],
                    "schedule": j["schedule"]["display"],
                    "kind": j["schedule"]["kind"],
                    "no_agent": j["no_agent"],
                    "enabled": j["enabled"] != false,
                    "supported": matches!(j["schedule"]["kind"].as_str(), Some("interval" | "once")),
                    "next_run_at": self.next_run(&j).map(crate::time::zagreb_rfc3339),
                    "rs_last_run_at": own["last_run_at"].as_i64().map(crate::time::zagreb_rfc3339),
                    "rs_last_status": own["last_status"],
                })
            })
            .collect()
    }

    /// Run every job that is due at `now`; returns (id, outcome).
    pub fn tick(&self, now: i64) -> Vec<(String, RunOutcome)> {
        let mut done = Vec::new();
        for job in self.jobs() {
            if self.next_run(&job).is_some_and(|t| t <= now) {
                let id = job["id"].as_str().unwrap_or("").to_string();
                let out = self.run_job(&job, now);
                self.record(&id, now, &out);
                done.push((id, out));
            }
        }
        done
    }

    fn record(&self, id: &str, now: i64, out: &RunOutcome) {
        if let Ok(mut st) = self.state.lock() {
            st.insert(
                id.to_string(),
                json!({"last_run_at": now, "last_status": if out.ok { "ok" } else { "error" }}),
            );
            let tmp = self.state_file.with_extension("tmp");
            if std::fs::write(&tmp, Value::Object(st.clone()).to_string()).is_ok() {
                let _ = std::fs::rename(&tmp, &self.state_file);
            }
        }
    }

    pub fn run_job(&self, job: &Value, now: i64) -> RunOutcome {
        let (ok, output, mode) = if job["no_agent"] == true {
            let (ok, out) = self.run_script(job);
            (ok, out, "no_agent (script)")
        } else {
            let (ok, out) = self.run_agent(job);
            (ok, out, "agent")
        };
        let output = crate::redact(&output);
        let file = self.write_output(job, now, mode, &output);
        RunOutcome { ok, output, file }
    }

    fn write_output(&self, job: &Value, now: i64, mode: &str, output: &str) -> Option<PathBuf> {
        let id = job["id"]
            .as_str()
            .filter(|s| !s.is_empty() && !s.contains(['/', '\\', '.']))?;
        let dir = self.output_dir.join(id);
        std::fs::create_dir_all(&dir).ok()?;
        let stamp = crate::time::zagreb_rfc3339(now); // 2026-10-18T21:00:00+02:00
        let fname = format!("{}_{}.md", &stamp[..10], stamp[11..19].replace(':', "-"));
        let body = format!(
            "# Cron Job: {}\n\n**Job ID:** {}\n**Run Time:** {}\n**Mode:** {}\n\n---\n\n{}\n",
            job["name"].as_str().unwrap_or(id),
            id,
            stamp.replace('T', " "),
            mode,
            output
        );
        let p = dir.join(fname);
        std::fs::write(&p, body).ok()?;
        Some(p)
    }

    /// Resolve `script` inside HERMES_HOME/scripts (no escapes via `..` or symlinks).
    pub fn script_path(&self, script: &str) -> Result<PathBuf, String> {
        let base = self
            .home
            .join("scripts")
            .canonicalize()
            .map_err(|_| "scripts dir missing".to_string())?;
        let p = base
            .join(script)
            .canonicalize()
            .map_err(|_| format!("script not found: {script}"))?;
        if !p.starts_with(&base) || !p.is_file() {
            return Err(format!("script must live in {}", base.display()));
        }
        Ok(p)
    }

    fn run_script(&self, job: &Value) -> (bool, String) {
        let Some(script) = job["script"].as_str() else {
            return (false, "no_agent job has no script".into());
        };
        let path = match self.script_path(script) {
            Ok(p) => p,
            Err(e) => return (false, e),
        };
        let mut cmd = match path.extension().and_then(|e| e.to_str()) {
            Some("py") => {
                return (
                    false,
                    "Python scripts are not run by hermes-rs (left to Python Hermes)".into(),
                )
            }
            Some("sh") | Some("bash") | None => {
                let mut c = Command::new("bash");
                c.arg(&path);
                c
            }
            Some(other) => return (false, format!("unsupported script type .{other}")),
        };
        let workdir = job["workdir"]
            .as_str()
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .unwrap_or_else(|| self.home.clone());
        cmd.current_dir(workdir).env_clear();
        for k in ["PATH", "HOME", "LANG", "TZ", "USER"] {
            if let Ok(v) = std::env::var(k) {
                cmd.env(k, v);
            }
        }
        cmd.env("HERMES_HOME", &self.home).env("HERMES_CRON", "1");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => return (false, format!("spawn failed: {e}")),
        };
        let drain = |r: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = Vec::new();
                if let Some(mut r) = r {
                    let _ = r.by_ref().take(1 << 20).read_to_end(&mut s);
                    let _ = std::io::copy(&mut r, &mut std::io::sink());
                }
                String::from_utf8_lossy(&s).to_string()
            })
        };
        let out_t = drain(
            child
                .stdout
                .take()
                .map(|x| Box::new(x) as Box<dyn Read + Send>),
        );
        let err_t = drain(
            child
                .stderr
                .take()
                .map(|x| Box::new(x) as Box<dyn Read + Send>),
        );
        let t0 = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Some(s),
                Ok(None) if t0.elapsed() >= self.script_timeout => {
                    let _ = Command::new("kill")
                        .arg("-KILL")
                        .arg(format!("-{}", child.id()))
                        .status();
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(_) => break None,
            }
        };
        let out = out_t.join().unwrap_or_default();
        let err = err_t.join().unwrap_or_default();
        let mut text = out.trim_end().to_string();
        if !err.trim().is_empty() {
            text.push_str("\n\n[stderr]\n");
            text.push_str(err.trim_end());
        }
        match status {
            Some(s) if s.success() => (true, text),
            Some(s) => (
                false,
                format!("{text}\n\n[exit {}]", s.code().unwrap_or(-1)),
            ),
            None => (
                false,
                format!(
                    "{text}\n\n[timed out after {}s]",
                    self.script_timeout.as_secs()
                ),
            ),
        }
    }

    fn run_agent(&self, job: &Value) -> (bool, String) {
        let Some(agent) = &self.agent else {
            return (false, "no agent configured".into());
        };
        let prompt = job["prompt"].as_str().unwrap_or("").to_string();
        let model = job["model"]
            .as_str()
            .map(String::from)
            .or_else(|| self.cron_model.clone());
        let provider = job["provider"]
            .as_str()
            .map(String::from)
            .or_else(|| self.cron_provider.clone());
        let target = resolve_target(&agent.cfg, model.as_deref(), provider.as_deref());
        let reasoning = resolve_reasoning(&agent.cfg, &target.model, None, false);
        let sid = crate::new_id("cron_");
        let turn = Turn {
            session_id: sid.clone(),
            spend_session: sid,
            target,
            reasoning,
            system: "You are running as a scheduled cron job. There is no user present; reply with the result only.".into(),
            messages: vec![json!({"role": "user", "content": prompt})],
            max_tokens: None,
            depth: 1, // no delegation from cron
            max_iterations: agent.cfg.max_turns,
            cancel: Arc::new(AtomicBool::new(false)),
            steer: Arc::new(Mutex::new(Vec::new())),
        };
        let r = agent.run(turn, &|_| {});
        if r.failed {
            (false, r.error.unwrap_or_else(|| "agent failed".into()))
        } else {
            (true, r.final_text)
        }
    }

    /// Background loop: tick every 30 s until `stop` is set.
    pub fn spawn(self: &Arc<Self>) -> std::thread::JoinHandle<()> {
        let me = self.clone();
        std::thread::spawn(move || {
            while !me.stop.load(Ordering::Relaxed) {
                for (id, o) in me.tick(crate::time::now_secs()) {
                    eprintln!(
                        "{} cron {id} ok={}",
                        crate::time::zagreb_rfc3339(crate::time::now_secs()),
                        o.ok
                    );
                }
                for _ in 0..30 {
                    if me.stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        let h = std::env::temp_dir().join(crate::new_id("cronhome-"));
        std::fs::create_dir_all(h.join("scripts")).unwrap();
        std::fs::create_dir_all(h.join("cron")).unwrap();
        h
    }

    fn jobs(h: &Path, j: Value) {
        std::fs::write(
            h.join("cron/jobs.json"),
            json!({"jobs": j, "updated_at": "x"}).to_string(),
        )
        .unwrap();
    }

    #[test]
    fn interval_script_job_runs_once_due_and_writes_upstream_md() {
        let h = home();
        std::fs::write(
            h.join("scripts/digest.sh"),
            "echo digest for $HERMES_HOME\necho token sk-abcdefghijklmnopqrstuv\n",
        )
        .unwrap();
        jobs(
            &h,
            json!([{"id": "abc123", "name": "daily-digest", "no_agent": true, "script": "digest.sh",
            "schedule": {"kind": "interval", "minutes": 15, "display": "every 15m"}, "enabled": true}]),
        );
        let s = Scheduler::new(&h, None);
        assert_eq!(s.tick(s.started + 10).len(), 0, "not due right after start");
        let due = s.started + 15 * 60;
        let r = s.tick(due);
        assert_eq!(r.len(), 1);
        let o = &r[0].1;
        assert!(o.ok, "{o:?}");
        assert!(o.output.contains("digest for"));
        assert!(!o.output.contains("sk-abcdefghijklmnopqrstuv"), "redacted");
        let md = std::fs::read_to_string(o.file.as_ref().unwrap()).unwrap();
        assert!(md.starts_with("# Cron Job: daily-digest\n\n**Job ID:** abc123\n**Run Time:** "));
        assert!(md.contains("**Mode:** no_agent (script)\n\n---\n\n"));
        assert_eq!(s.tick(due + 60).len(), 0, "anchored on last run");
        assert_eq!(s.next_run(&s.jobs()[0]), Some(due + 900));
        // own state persisted, jobs.json untouched
        assert!(std::fs::read_to_string(&s.state_file)
            .unwrap()
            .contains("abc123"));
        assert!(!std::fs::read_to_string(h.join("cron/jobs.json"))
            .unwrap()
            .contains("last_run_at"));
        assert_eq!(s.list()[0]["supported"], true);
    }

    #[test]
    fn scripts_are_confined_python_refused_timeouts_kill() {
        let h = home();
        std::fs::write(h.join("outside.sh"), "echo escaped").unwrap();
        std::fs::write(h.join("scripts/x.py"), "print(1)").unwrap();
        std::fs::write(h.join("scripts/slow.sh"), "sleep 30 & sleep 30\necho never").unwrap();
        let mut s = Scheduler::new(&h, None);
        s.script_timeout = Duration::from_millis(300);
        let run = |script: &str| {
            s.run_job(
                &json!({"id": "j", "name": "n", "no_agent": true, "script": script}),
                0,
            )
        };
        assert!(run("../outside.sh").output.contains("must live in"));
        assert!(run("x.py").output.contains("Python scripts are not run"));
        let t0 = Instant::now();
        let o = run("slow.sh");
        assert!(!o.ok && o.output.contains("timed out"));
        assert!(t0.elapsed() < Duration::from_secs(5));
        assert_eq!(
            s.next_run(&json!({"id": "c", "schedule": {"kind": "cron", "expr": "0 9 * * *"}})),
            None
        );
        assert_eq!(s.next_run(&json!({"id": "o", "schedule": {"kind": "once", "run_at": "2026-10-18T21:00:00+02:00"}})), Some(1792350000));
    }
}
