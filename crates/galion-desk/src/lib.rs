// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio
//
//! galion-desk: the operator's desk endpoint.
//!
//! `GET /desk` returns one JSON snapshot of the pod (model line, services,
//! board, notes, the Hermes mindpack). `POST /desk` applies one small act
//! (note, drop, voice, stamp, hold, wake, rest, focus) and returns the
//! snapshot. `/health` is a fixed body that touches nothing.
//!
//! Routing is a prefix match and is kept byte-compatible with the Oct 10 live
//! binary (see [`route`] and `docs/cutover/desk.md`): `/deskx` is the desk,
//! `/healthz` is health, and health answers any method.
//!
//! All state lives in [`Desk`], not in globals, so tests can run several
//! servers in one process. Settings are documented in [`config`].

#![forbid(unsafe_code)]

pub mod config;
pub mod net;
pub mod scan;
pub mod view;

use config::Settings;
use scan::{escape, flag, json_amount, json_number, json_string};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PROBE_TIMEOUT: Duration = Duration::from_millis(200);

/// `/health` body. Fixed JSON. No process, no file and no socket.
pub fn health_json() -> &'static str {
    "{\"ok\":true}"
}

/// What a request line maps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// `POST /desk*`: apply an act, then snapshot.
    Act,
    /// `GET /desk*`: snapshot.
    Snapshot,
    /// any method, `/health*`.
    Health,
    NotFound,
}

/// Prefix routing, identical to the Oct 10 binary.
pub fn route(method: &str, path: &str) -> Route {
    if method == "POST" && path.starts_with("/desk") {
        Route::Act
    } else if method == "GET" && path.starts_with("/desk") {
        Route::Snapshot
    } else if path.starts_with("/health") {
        Route::Health
    } else {
        Route::NotFound
    }
}

/// One desk: settings plus the two in-flight flags.
pub struct Desk {
    pub settings: Settings,
    waking: AtomicBool,
    resting: AtomicBool,
}

impl Desk {
    pub fn new(settings: Settings) -> Arc<Desk> {
        Arc::new(Desk {
            settings,
            waking: AtomicBool::new(false),
            resting: AtomicBool::new(false),
        })
    }

    fn state(&self, name: &str) -> std::path::PathBuf {
        self.settings.state_dir.join(name)
    }

    fn one_file(&self, name: &str) -> std::path::PathBuf {
        self.settings.one_dir.join(name)
    }

    fn read(&self, path: &std::path::Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    fn ollama_open(&self) -> bool {
        net::tcp_open_addr(&self.settings.ollama, PROBE_TIMEOUT)
    }

    fn probe(addr: &str) -> bool {
        net::tcp_open_addr(addr, PROBE_TIMEOUT)
    }

    /// Answer one connection.
    pub fn handle(self: &Arc<Self>, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
        let t0 = std::time::Instant::now();
        let (code, body, what) = match net::read_request(&mut stream) {
            Err(net::ReadError::Empty) => return,
            Err(net::ReadError::TooLarge) => (
                413,
                "{\"error\":\"too large\"}".to_string(),
                "too_large".into(),
            ),
            Ok(req) => {
                let what = format!(
                    "{} {}",
                    req.method,
                    req.path.split('?').next().unwrap_or("")
                );
                let (code, body) = self.respond(&req.method, &req.path, &req.body);
                (code, body, what)
            }
        };
        nerve_client::observe_request("galion-desk", &what, code, t0.elapsed());
        let _ = stream.write_all(net::response(code, &body).as_bytes());
    }

    /// Status code and body for one request. Used by [`Desk::handle`] and
    /// directly by tests.
    pub fn respond(self: &Arc<Self>, method: &str, path: &str, body: &str) -> (u16, String) {
        match route(method, path) {
            Route::Act => {
                self.apply(body.trim());
                (200, self.snapshot())
            }
            Route::Snapshot => (200, self.snapshot()),
            Route::Health => (200, health_json().to_string()),
            Route::NotFound => (404, "{\"error\":\"not found\"}".to_string()),
        }
    }

    /// Accept loop. Returns only if the listener fails permanently.
    pub fn serve(self: Arc<Self>, listener: TcpListener) {
        for stream in listener.incoming().flatten() {
            let desk = Arc::clone(&self);
            thread::spawn(move || desk.handle(stream));
        }
    }

    /// Apply one act from a POST body. Unknown acts do nothing.
    pub fn apply(self: &Arc<Self>, body: &str) {
        match json_string(body, "act").as_str() {
            "note" => self.write_note(&json_string(body, "text")),
            "voice" => {
                let name = json_string(body, "name");
                if name == "samantha" || name == "jarvis" {
                    let _ = fs::create_dir_all(&self.settings.one_dir);
                    let _ = fs::write(self.one_file("voice"), format!("{name}\n"));
                }
            }
            "drop" => {
                let path = self.state("desk-notes.jsonl");
                if let Some(out) = view::drop_last(&self.read(&path)) {
                    let _ = fs::write(path, out);
                }
            }
            "stamp" => {
                let line = if self.ollama_open() { "up" } else { "down" };
                let hermes = if Self::probe(&self.settings.probes.hermes) {
                    "up"
                } else {
                    "down"
                };
                self.write_note(&format!("Stamp. Line {line}. Hermes {hermes}."));
            }
            "hold" => {
                let path = self.state("desk-hold");
                if fs::metadata(&path).is_ok() {
                    let _ = fs::remove_file(&path);
                } else {
                    let _ = fs::create_dir_all(&self.settings.state_dir);
                    let _ = fs::write(&path, "held\n");
                }
            }
            "wake" => self.wake(),
            "rest" => self.rest(),
            "focus" => {
                let id = view::focus_id(&json_string(body, "text"));
                let _ = fs::create_dir_all(&self.settings.state_dir);
                if id.is_empty() {
                    let _ = fs::remove_file(self.state("desk-focus"));
                } else {
                    let _ = fs::write(self.state("desk-focus"), format!("{id}\n"));
                }
            }
            _ => {}
        }
    }

    fn write_note(&self, text: &str) {
        let Some(text) = view::note_text(text) else {
            return;
        };
        let _ = fs::create_dir_all(&self.settings.state_dir);
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.state("desk-notes.jsonl"))
        {
            let line = format!("{{\"at\":{},\"text\":\"{}\"}}\n", now(), escape(&text));
            let _ = file.write_all(line.as_bytes());
        }
    }

    fn wake(self: &Arc<Self>) {
        if self.waking.swap(true, Ordering::SeqCst) {
            return;
        }
        let desk = Arc::clone(self);
        thread::spawn(move || {
            let body = "{\"model\":\"galion:latest\",\"prompt\":\"ok\",\"stream\":false,\"options\":{\"num_predict\":1}}";
            let _ = net::http_post(&desk.settings.ollama, "/api/generate", body);
            desk.waking.store(false, Ordering::SeqCst);
        });
    }

    fn rest(self: &Arc<Self>) {
        let name = self.ollama_ps().0;
        if name.is_empty() || self.resting.swap(true, Ordering::SeqCst) {
            return;
        }
        let desk = Arc::clone(self);
        thread::spawn(move || {
            let body = format!(
                "{{\"model\":\"{}\",\"keep_alive\":0,\"prompt\":\"\",\"stream\":false}}",
                escape(&name)
            );
            let _ = net::http_post(&desk.settings.ollama, "/api/generate", &body);
            desk.resting.store(false, Ordering::SeqCst);
        });
    }

    fn ollama_ps(&self) -> (String, u64, u64) {
        let raw = net::http_get(&self.settings.ollama, "/api/ps");
        if raw.is_empty() {
            return (String::new(), 0, 0);
        }
        (
            json_string(&raw, "name"),
            json_number(&raw, "size_vram"),
            json_number(&raw, "context_length"),
        )
    }

    /// Run the mindpack binary (desk-hermes) with stdin closed. Its stdout
    /// must be one JSON object, otherwise `{}`.
    fn mindpack(&self) -> String {
        let out = Command::new(&self.settings.hermes_bin)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        match out {
            Ok(out) if out.status.success() => {
                view::mindpack_json(&String::from_utf8_lossy(&out.stdout))
            }
            _ => "{}".to_string(),
        }
    }

    fn services(&self) -> String {
        let p = &self.settings.probes;
        let rows: [(&str, Option<&str>); 8] = [
            ("Console", Some(&p.console)),
            ("Origin", Some(&p.origin)),
            ("Hermes", Some(&p.hermes)),
            ("Laptop", None),
            ("One", Some(&p.one)),
            ("Room", Some(&p.room)),
            ("Tulu", Some(&p.tulu)),
            ("Tunnel", Some(&p.tunnel)),
        ];
        rows.iter()
            .map(|(name, addr)| {
                let up = match addr {
                    Some(addr) => Self::probe(addr),
                    None => self.ollama_open(),
                };
                format!("{{\"name\":\"{name}\",\"up\":{up}}}")
            })
            .collect::<Vec<_>>()
            .join(",")
    }

    fn board(&self, link: &str) -> String {
        let survival = self.read(&self.state("survival.json"));
        let gw = self.read(&self.settings.hermes_dir.join("gateway_state.json"));
        let machine = self.read(&self.state("machine.json"));
        let studio = match json_string(link, "name") {
            name if name.is_empty() => "Galion Studio".to_string(),
            name => name,
        };
        let laptop_ram = format!(
            "{} / {} GB",
            json_amount(link, "free_gb"),
            json_amount(link, "ram_gb")
        );
        format!(
            "{{\"studio\":\"{}\",\"gpu\":\"{}\",\"cpu\":\"{}\",\"laptop_ram\":\"{}\",\"pod_ram\":\"{}\",\"hermes_ms\":{},\"model_ms\":{},\"agents\":{},\"version\":\"{}\",\"disk\":\"{}%\",\"load\":\"{}\",\"pod\":\"{}\",\"france\":{}}}",
            escape(&studio),
            escape(&view::short_gpu(&json_string(link, "gpu"))),
            escape(&view::short_cpu(&json_string(link, "cpu"))),
            escape(&laptop_ram),
            escape(&view::pod_ram(&self.read("/proc/meminfo".as_ref()))),
            json_number(link, "hermes_ms"),
            json_number(link, "model_ms"),
            json_number(&gw, "active_agents"),
            escape(&json_string(&gw, "code_version")),
            json_amount(&survival, "volume_used_pct"),
            escape(&view::load_line(
                &self.read("/proc/loadavg".as_ref()),
                &self.read("/proc/cpuinfo".as_ref()),
            )),
            escape(&json_string(&machine, "publicIp")),
            flag(link, "france"),
        )
    }

    /// The `/desk` JSON. Key order is the live order.
    pub fn snapshot(&self) -> String {
        let link = self.read(&self.state("link.json"));
        let ps = self.ollama_ps();
        let (model, vram, ctx) = if ps.0.is_empty() {
            (
                view::clean_model(&json_string(&link, "model")),
                0,
                view::last_ctx(&link),
            )
        } else {
            (view::clean_model(&ps.0), ps.1, ps.2)
        };
        let voice_raw = fs::read_to_string(self.one_file("voice")).ok();
        let voice = view::voice(voice_raw.as_deref());
        let (heard, said, turns) = view::one_last(&self.read(&self.one_file("session.jsonl")));
        let p = &self.settings.probes;
        format!(
            "{{\"foundation\":\"rust\",\"at\":{},\"line\":{},\"model\":\"{}\",\"vram\":{},\"ctx\":{},\"hermes\":{},\"one\":{},\"tunnel\":{},\"voice\":\"{}\",\"heard\":\"{}\",\"said\":\"{}\",\"turns\":{},\"waking\":{},\"resting\":{},\"hold\":{},\"services\":[{}],\"board\":{},\"events\":[{}],\"mindpack\":{},\"focus\":\"{}\",\"notes\":[{}]}}",
            now(),
            self.ollama_open(),
            escape(&model),
            vram,
            ctx,
            Self::probe(&p.hermes),
            Self::probe(&p.one),
            Self::probe(&p.tunnel),
            voice,
            escape(&heard),
            escape(&said),
            turns,
            self.waking.load(Ordering::SeqCst),
            self.resting.load(Ordering::SeqCst),
            fs::metadata(self.state("desk-hold")).is_ok(),
            self.services(),
            self.board(&link),
            view::events(&link),
            self.mindpack(),
            escape(self.read(&self.state("desk-focus")).trim()),
            view::notes(&self.read(&self.state("desk-notes.jsonl"))),
        )
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_matches_the_live_desk_probe_matrix() {
        // Observed on the VPS live desk (Oct 10 build) with GET, 2026-10-20.
        for (path, want) in [
            ("/", Route::NotFound),
            ("/health", Route::Health),
            ("/healthz", Route::Health),
            ("/health/x", Route::Health),
            ("/desk", Route::Snapshot),
            ("/desk/", Route::Snapshot),
            ("/deskx", Route::Snapshot),
            ("/desk?x=1", Route::Snapshot),
            ("/nope", Route::NotFound),
            ("/DESK", Route::NotFound),
        ] {
            assert_eq!(route("GET", path), want, "{path}");
        }
        assert_eq!(route("POST", "/desk"), Route::Act);
        assert_eq!(route("POST", "/health"), Route::Health);
        assert_eq!(route("HEAD", "/desk"), Route::NotFound);
        assert_eq!(route("PUT", "/desk"), Route::NotFound);
    }

    #[test]
    fn health_body_is_fixed() {
        assert_eq!(health_json(), "{\"ok\":true}");
    }
}
