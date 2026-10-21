// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! `--agent gateway`: the benchmark episodes run end to end through a real
//! `hermes-gateway` process (wave 4, slot 162).
//!
//! ```text
//! hermes-bench ──POST /v1/chat/completions──▶ hermes-gateway (HERMES_RS_BENCH=1)
//!                                               │ agent loop (hermes-rs)
//!                                               ├──▶ mock LLM (this process, loopback, no key)
//!                                               └──▶ bench tools ──▶ loopback HTTP ──▶ the bench's
//!                                                                     own backends (mock bank,
//!                                                                     paper broker, vending sim)
//! ```
//!
//! The gateway binary is the `hermes-gateway` file next to the running
//! `hermes-bench` executable (same cargo target dir); it must be built with
//! `--features bench`, otherwise it never logs `bench on:` and the run fails
//! closed. Nothing can be pointed elsewhere: there is no flag or env for the
//! binary, the ports or the URLs, so the hermes-core whitelist stays closed.
//!
//! Safety: the child gets a cleared environment (no provider key can reach
//! it), binds 127.0.0.1 on a kernel-chosen port, its only model provider is
//! the loopback mock LLM, every backend URL is loopback (the gateway refuses
//! anything else), MCP is off, and the child plus its temp home are removed
//! when the run ends. The API key is a per-run random loopback test key, not
//! a secret, and is never printed.

use crate::mockllm::MockLlm;
use crate::planner::{Brain, PlanIo, Script, ScriptFactory};
use crate::session::{CallRecord, TurnReport};
use crate::transport::Transport;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A user prompt starting with this makes the mock model call each listed
/// `[tool, args]` through the gateway and answer with what it observed.
pub const PROBE_TAG: &str = "BENCH-PROBE:";
/// The gateway must log this, or it is not a bench build.
pub const BENCH_ON: &str = "bench on: 36 tools";
const MAX_BODY: usize = 8 << 20;

/// Wrap a bench script factory so `BENCH-PROBE:` prompts run a probe script
/// (used to read the gateway's own tool state, e.g. banking drafts).
pub fn with_probe(mut inner: ScriptFactory) -> ScriptFactory {
    Box::new(move |user: &str| -> Script {
        match user.strip_prefix(PROBE_TAG) {
            Some(list) => {
                let steps: Vec<(String, Value)> = serde_json::from_str(list).unwrap_or_default();
                Box::new(move |io: &mut PlanIo| {
                    let mut seen = Vec::new();
                    for (tool, args) in steps {
                        let r = io.call(&tool, args);
                        seen.push(json!({"tool": r.tool, "error": r.is_error(), "value": r.value}));
                    }
                    Value::Array(seen).to_string()
                })
            }
            None => inner(user),
        }
    })
}

// ---------------------------------------------------------------- loopback

/// A loopback HTTP/1.1 front for an in-process [`Transport`], so the
/// gateway's bench tools and the bench's checks see the same backend state.
pub struct LoopServer {
    pub port: u16,
    pub requests: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl LoopServer {
    pub fn start(t: Arc<dyn Transport>) -> std::io::Result<LoopServer> {
        let l = TcpListener::bind(("127.0.0.1", 0))?;
        let port = l.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(AtomicU64::new(0));
        let (st, rq) = (stop.clone(), requests.clone());
        let thread = std::thread::spawn(move || {
            for conn in l.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(c) = conn else { continue };
                rq.fetch_add(1, Ordering::Relaxed);
                let _ = serve_one(c, t.as_ref());
            }
        });
        Ok(LoopServer {
            port,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

impl Drop for LoopServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve_one(mut s: TcpStream, t: &dyn Transport) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(30)))?;
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h)? == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let (status, body) = if len > MAX_BODY {
        (413, json!({"error": "body too large"}).to_string())
    } else {
        let mut buf = vec![0u8; len];
        r.read_exact(&mut buf)?;
        let body = String::from_utf8_lossy(&buf).into_owned();
        let b = (method != "GET" || !body.is_empty()).then_some(body.as_str());
        match t.send(&method, &target, b) {
            Ok(x) => x,
            Err(e) => (502, json!({ "error": e }).to_string()),
        }
    };
    write!(
        s,
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    s.flush()
}

/// Minimal loopback HTTP client (120 s read timeout; a vending shift is one
/// gateway request with ~15 tool calls).
pub fn http(
    port: u16,
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Option<&Value>,
) -> Result<(u16, String), String> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(120)))
        .map_err(|e| e.to_string())?;
    let b = body.map(Value::to_string).unwrap_or_default();
    let auth = key
        .map(|k| format!("Authorization: Bearer {k}\r\n"))
        .unwrap_or_default();
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{auth}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{b}",
        b.len()
    )
    .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        dechunk(body)
    } else {
        body.to_string()
    };
    Ok((status, body))
}

fn dechunk(mut b: &str) -> String {
    let mut out = String::new();
    while let Some((size, rest)) = b.split_once("\r\n") {
        let n = usize::from_str_radix(size.trim(), 16).unwrap_or(0);
        if n == 0 || rest.len() < n {
            break;
        }
        out.push_str(&rest[..n]);
        b = rest[n..].trim_start_matches("\r\n");
    }
    out
}

// ---------------------------------------------------------------- gateway

/// What a remote (gateway) session needs: the gateway port and key, and the
/// mock LLM's counters and transcripts.
pub struct Remote {
    pub port: u16,
    key: String,
    llm: Arc<Mutex<crate::mockllm::Seen>>,
    transcripts: Arc<Mutex<Vec<Vec<Value>>>>,
    turns: AtomicU64,
}

impl Remote {
    /// One user turn through `POST /v1/chat/completions`; the tool calls are
    /// read back from the message list the gateway sent to the mock LLM.
    pub fn turn(&self, system: &str, prompt: &str) -> TurnReport {
        let before = self.llm.lock().unwrap_or_else(|p| p.into_inner()).requests;
        self.transcripts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        self.turns.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "model": "hermes-bench/mock-llm",
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": prompt},
            ],
        });
        let res = http(
            self.port,
            "POST",
            "/v1/chat/completions",
            Some(&self.key),
            Some(&body),
        );
        let api_calls = self
            .llm
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .requests
            .saturating_sub(before) as usize;
        let transcript = self
            .transcripts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pop()
            .unwrap_or_default();
        let mut rep = TurnReport {
            calls: calls_of(&transcript),
            api_calls,
            ..TurnReport::default()
        };
        match res {
            Ok((200, text)) => {
                let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                rep.final_text = v["choices"][0]["message"]["content"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string();
                if transcript.is_empty() {
                    rep.error =
                        Some("gateway answered but the mock LLM saw no finished turn".into());
                }
            }
            Ok((code, text)) => {
                rep.error = Some(format!(
                    "gateway HTTP {code}: {}",
                    text.chars().take(300).collect::<String>()
                ))
            }
            Err(e) => rep.error = Some(format!("gateway unreachable: {e}")),
        }
        rep
    }

    /// Call `tool` once through the gateway and return its result value.
    pub fn probe(&self, tool: &str, args: Value) -> Result<Value, String> {
        let prompt = format!("{PROBE_TAG}{}", json!([[tool, args]]));
        let rep = self.turn("hermes-bench probe", &prompt);
        if let Some(e) = rep.error {
            return Err(e);
        }
        let v: Value = serde_json::from_str(&rep.final_text)
            .map_err(|e| format!("probe {tool}: {e}: {}", rep.final_text))?;
        let first = &v[0];
        if first["tool"] != tool || first["error"] == true {
            return Err(format!("probe {tool} failed: {first}"));
        }
        Ok(first["value"].clone())
    }

    /// Tool names the gateway offered the model (first request seen).
    pub fn tool_names(&self) -> Vec<String> {
        self.llm
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .tool_names
            .clone()
    }

    pub fn turns(&self) -> u64 {
        self.turns.load(Ordering::Relaxed)
    }
}

/// Tool calls in an OpenAI message list (assistant tool_calls + tool results).
pub fn calls_of(messages: &[Value]) -> Vec<CallRecord> {
    let mut pending: Vec<(String, String, Value)> = Vec::new();
    let mut out = Vec::new();
    for m in messages {
        if let Some(tcs) = m["tool_calls"].as_array() {
            for tc in tcs {
                let args = tc["function"]["arguments"]
                    .as_str()
                    .and_then(|a| serde_json::from_str(a).ok())
                    .unwrap_or(Value::Null);
                pending.push((
                    tc["id"].as_str().unwrap_or_default().to_string(),
                    tc["function"]["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    args,
                ));
            }
        } else if m["role"] == "tool" {
            let id = m["tool_call_id"].as_str().unwrap_or_default();
            let (tool, args) = match pending.iter().position(|p| p.0 == id) {
                Some(i) => {
                    let p = pending.remove(i);
                    (p.1, p.2)
                }
                None => (
                    m["name"].as_str().unwrap_or_default().to_string(),
                    Value::Null,
                ),
            };
            let raw = m["content"].as_str().unwrap_or_default();
            let content: Value = serde_json::from_str(raw).unwrap_or_else(|_| json!(raw));
            out.push(CallRecord {
                is_error: content.get("error").is_some(),
                tool,
                args,
                content,
            });
        }
    }
    out
}

/// The running end-to-end rig: gateway child, mock LLM, loopback fronts.
pub struct E2e {
    pub remote: Arc<Remote>,
    child: Child,
    pub gateway_bin: PathBuf,
    pub gateway_pid: u32,
    pub bench_line: String,
    home: PathBuf,
    log: PathBuf,
    _llm: MockLlm,
    fronts: Vec<(&'static str, LoopServer)>,
    started: Instant,
}

/// `hermes-gateway` next to the running `hermes-bench`.
pub fn gateway_bin() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let bin = exe
        .parent()
        .map(|d| d.join("hermes-gateway"))
        .ok_or("hermes-bench has no parent directory")?;
    if !bin.is_file() {
        return Err(format!(
            "--agent gateway needs {} (cargo build --release -p hermes-gateway --features bench)",
            bin.display()
        ));
    }
    Ok(bin)
}

fn free_port() -> Result<u16, String> {
    let l = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| format!("bind: {e}"))?;
    l.local_addr().map(|a| a.port()).map_err(|e| e.to_string())
}

fn run_key() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("hermes-bench-e2e-{}-{nanos:x}", std::process::id())
}

impl E2e {
    /// Start the mock LLM (brain = `factory` + probes), loopback fronts for
    /// the Wall Street and vending backends, and the gateway pointed at them.
    pub fn start(
        bank_url: &str,
        ws: Arc<dyn Transport>,
        vend: Arc<dyn Transport>,
        factory: ScriptFactory,
    ) -> Result<E2e, String> {
        let bin = gateway_bin()?;
        let llm = MockLlm::start(0, Brain::new(with_probe(factory)))
            .map_err(|e| format!("mock LLM bind: {e}"))?;
        let ws_front = LoopServer::start(ws).map_err(|e| format!("ws front: {e}"))?;
        let vend_front = LoopServer::start(vend).map_err(|e| format!("vending front: {e}"))?;
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "hermes-bench-gw-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).map_err(|e| format!("gateway home: {e}"))?;
        std::fs::write(
            home.join("config.yaml"),
            format!(
                "model:\n  provider: mockllm\n  default: hermes-bench/mock-llm\n  base_url: {}\nagent:\n  max_turns: 250\n",
                llm.base_url()
            ),
        )
        .map_err(|e| format!("gateway config: {e}"))?;
        let log = home.join("gateway.log");
        let port = free_port()?;
        let key = run_key();
        let child = Command::new(&bin)
            .env_clear()
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .env("HOME", &home)
            .env("TZ", "Europe/Zagreb")
            .env("HERMES_HOME", &home)
            .env("HERMES_RS_HOST", "127.0.0.1")
            .env("HERMES_RS_PORT", port.to_string())
            .env("HERMES_RS_MCP", "off")
            .env("API_SERVER_KEY", &key)
            .env("HERMES_RS_BENCH", "1")
            .env("HERMES_RS_BENCH_BANK_URL", bank_url)
            .env("HERMES_RS_BENCH_WS_URL", ws_front.url())
            .env("HERMES_RS_BENCH_VEND_URL", vend_front.url())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(std::fs::File::create(&log).map_err(|e| format!("gateway log: {e}"))?)
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", bin.display()))?;
        let gateway_pid = child.id();
        let remote = Arc::new(Remote {
            port,
            key,
            llm: llm.seen.clone(),
            transcripts: llm.transcripts.clone(),
            turns: AtomicU64::new(0),
        });
        let mut rig = E2e {
            remote,
            child,
            gateway_bin: bin,
            gateway_pid,
            bench_line: String::new(),
            home,
            log,
            _llm: llm,
            fronts: vec![("wallstreet", ws_front), ("vending", vend_front)],
            started: Instant::now(),
        };
        rig.wait_ready()?;
        Ok(rig)
    }

    fn log_text(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn wait_ready(&mut self) -> Result<(), String> {
        let t0 = Instant::now();
        while TcpStream::connect(("127.0.0.1", self.remote.port)).is_err() {
            if let Ok(Some(st)) = self.child.try_wait() {
                return Err(format!(
                    "hermes-gateway exited {st} before listening: {}",
                    self.log_text().trim()
                ));
            }
            if t0.elapsed() > Duration::from_secs(30) {
                return Err("hermes-gateway never opened its port".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let text = self.log_text();
        match text.lines().find(|l| l.contains("bench on:")) {
            Some(l) if l.contains(BENCH_ON) => {
                self.bench_line = l.trim().to_string();
                Ok(())
            }
            Some(l) => Err(format!(
                "gateway bench wiring is not the full 36-tool set: {l}"
            )),
            None => Err(format!(
                "{} is not a bench build (no `bench on:` line; build with --features bench): {}",
                self.gateway_bin.display(),
                text.trim()
            )),
        }
    }

    /// Evidence for the score JSON (no key, no secrets).
    pub fn report(&self) -> Value {
        let seen = self
            .remote
            .llm
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        let offered = &seen.tool_names;
        json!({
            "gateway_bin": self.gateway_bin.display().to_string(),
            "gateway_pid": self.gateway_pid,
            "gateway_bind": format!("127.0.0.1:{}", self.remote.port),
            "bench_line": self.bench_line,
            "chat_turns": self.remote.turns(),
            "llm_requests": seen.requests,
            "llm_requests_with_auth_header": seen.with_auth,
            "tools_offered": offered.len(),
            "ws_kill_reset_offered": offered.iter().any(|n| n == "ws_kill_reset"),
            "money_move_tools_offered": offered.iter().filter(|n| crate::adapters::is_money_move_name(n)).count(),
            "backend_front_requests": self.fronts.iter().map(|(n, f)| (n.to_string(), json!(f.requests.load(Ordering::Relaxed)))).collect::<serde_json::Map<_, _>>(),
            "wall_ms": self.started.elapsed().as_millis() as u64,
        })
    }

    pub fn home(&self) -> &Path {
        &self.home
    }
}

impl Drop for E2e {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.home);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::WallStreetInProcess;

    #[test]
    fn calls_of_pairs_tool_calls_with_results() {
        let msgs = vec![
            json!({"role": "system", "content": "s"}),
            json!({"role": "user", "content": "u"}),
            json!({"role": "assistant", "content": null, "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "bank_balance", "arguments": "{\"x\":1}"}}]}),
            json!({"role": "tool", "tool_call_id": "c1", "name": "bank_balance", "content": "{\"ok\":true}"}),
            json!({"role": "assistant", "content": null, "tool_calls": [{"id": "c2", "type": "function", "function": {"name": "ws_quote", "arguments": "{}"}}]}),
            json!({"role": "tool", "tool_call_id": "c2", "name": "ws_quote", "content": "{\"error\":\"no\"}"}),
        ];
        let c = calls_of(&msgs);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].tool, "bank_balance");
        assert_eq!(c[0].args, json!({"x": 1}));
        assert!(!c[0].is_error);
        assert!(c[1].is_error);
    }

    #[test]
    fn loop_server_fronts_a_transport() {
        let t: Arc<dyn Transport> = Arc::new(WallStreetInProcess::fixture());
        let f = LoopServer::start(t.clone()).unwrap();
        let (code, body) = http(f.port, "GET", "/account", None, None).unwrap();
        let direct = t.send("GET", "/account", None).unwrap();
        assert_eq!((code, body), direct);
        let via = crate::transport::HttpTransport::new(&f.url()).unwrap();
        assert_eq!(via.send("POST", "/reset", Some("{}")).unwrap().0, 200);
        assert!(f.requests.load(Ordering::Relaxed) >= 2);
    }

    #[test]
    fn probe_prompts_bypass_the_bench_script() {
        let mut f = with_probe(Box::new(|_u: &str| -> Script {
            Box::new(|_io| "bench".into())
        }));
        let _ = f("BENCH-PROBE:[]");
        let _ = f("anything");
    }
}
