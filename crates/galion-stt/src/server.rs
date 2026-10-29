// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! axum server: `GET /health` and `WS /stream` (protocol in `protocol.rs`).
//! Loopback only. Logs counts and timings per session, never audio or text.

use crate::audio::{pcm16le_to_f32, samples_to_ms, SAMPLE_RATE};
use crate::engine::EnginePool;
use crate::protocol::{parse_client, ClientMsg, ServerMsg};
use crate::session::{run_engine, AudioQueue, SessionStats};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Server settings.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub bind: SocketAddr,
    pub max_session: Duration,
    pub partial_interval: Duration,
    /// Queue capacity in chunks (drop-oldest beyond this).
    pub queue_chunks: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind: SocketAddr::from(([127, 0, 0, 1], 8651)),
            max_session: Duration::from_secs(600),
            partial_interval: Duration::from_millis(300),
            queue_chunks: 256,
        }
    }
}

/// Shared state.
pub struct AppState {
    pub pool: Arc<EnginePool>,
    pub cfg: ServerConfig,
    next_id: AtomicU64,
    sessions_total: AtomicU64,
    sessions_refused: AtomicU64,
}

impl AppState {
    pub fn new(pool: Arc<EnginePool>, cfg: ServerConfig) -> Arc<Self> {
        Arc::new(AppState {
            pool,
            cfg,
            next_id: AtomicU64::new(1),
            sessions_total: AtomicU64::new(0),
            sessions_refused: AtomicU64::new(0),
        })
    }
}

/// Refuse anything but a loopback bind address.
pub fn check_bind(addr: &SocketAddr) -> Result<(), String> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(format!(
            "refusing to bind {addr}: galion-stt listens on loopback only (127.0.0.1 / ::1)"
        ))
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/stream", get(stream))
        .with_state(state)
}

async fn health(State(s): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let max = s.pool.size();
    Json(serde_json::json!({
        "status": "ok",
        "service": "galion-stt",
        "engine": "moonshine-tiny-streaming-en",
        "sample_rate": SAMPLE_RATE,
        "format": "pcm16le-mono",
        "sessions_active": max - s.pool.available(),
        "sessions_max": max,
        "sessions_total": s.sessions_total.load(Ordering::Relaxed),
        "sessions_refused": s.sessions_refused.load(Ordering::Relaxed),
        "partial_ms": s.cfg.partial_interval.as_millis() as u64,
        "max_session_secs": s.cfg.max_session.as_secs(),
    }))
}

async fn stream(ws: WebSocketUpgrade, State(s): State<Arc<AppState>>) -> Response {
    // 250 ms of PCM16 is 8000 bytes; control frames are tiny.
    ws.max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |sock| session(sock, s))
        .into_response()
}

async fn send_msg(sock: &mut WebSocket, m: &ServerMsg) -> bool {
    sock.send(Message::Text(m.to_json().into())).await.is_ok()
}

async fn close(sock: &mut WebSocket, code: u16, reason: &'static str) {
    let _ = sock
        .send(Message::Close(Some(CloseFrame {
            code,
            reason: reason.into(),
        })))
        .await;
}

async fn session(mut sock: WebSocket, s: Arc<AppState>) {
    let id = s.next_id.fetch_add(1, Ordering::Relaxed);
    let Some(mut guard) = s.pool.try_take() else {
        s.sessions_refused.fetch_add(1, Ordering::Relaxed);
        eprintln!(
            "galion-stt: session {id} refused: all {} engines busy",
            s.pool.size()
        );
        let _ = send_msg(
            &mut sock,
            &ServerMsg::Error {
                message: format!("busy: all {} dictation sessions are in use", s.pool.size()),
            },
        )
        .await;
        close(&mut sock, 1013, "busy").await;
        return;
    };
    s.sessions_total.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    eprintln!("galion-stt: session {id} start");

    let queue = Arc::new(AudioQueue::new(s.cfg.queue_chunks));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ServerMsg>();
    let (stats_tx, stats_rx) = tokio::sync::oneshot::channel::<SessionStats>();
    let q2 = Arc::clone(&queue);
    let interval = s.cfg.partial_interval;
    let spawned = std::thread::Builder::new()
        .name(format!("stt-engine-{id}"))
        .spawn(move || {
            let stats = match guard.transcriber().open_stream() {
                Err(e) => {
                    let _ = tx.send(ServerMsg::Error { message: e });
                    SessionStats {
                        error: true,
                        ..SessionStats::default()
                    }
                }
                Ok(mut stream) => run_engine(&mut stream, &q2, interval, |m| tx.send(m).is_ok()),
            };
            let _ = stats_tx.send(stats);
            // `guard` drops here: the transcriber goes back to the pool.
        });
    if spawned.is_err() {
        let _ = send_msg(
            &mut sock,
            &ServerMsg::Error {
                message: "could not start engine thread".into(),
            },
        )
        .await;
        close(&mut sock, 1011, "internal error").await;
        return;
    }

    let max_samples = s.cfg.max_session.as_secs() * u64::from(SAMPLE_RATE);
    let deadline = tokio::time::sleep(s.cfg.max_session);
    tokio::pin!(deadline);
    let mut stopping = false;
    let mut client_gone = false;
    let mut samples_in: u64 = 0;
    let mut dropped_samples: u64 = 0;
    let mut bad_frames: u64 = 0;
    let mut last_warn: Option<Instant> = None;

    loop {
        tokio::select! {
            msg = sock.recv(), if !stopping => match msg {
                Some(Ok(Message::Binary(b))) => match pcm16le_to_f32(&b) {
                    Ok(f) => {
                        samples_in += f.len() as u64;
                        let d = queue.push(f);
                        if d > 0 {
                            dropped_samples += d;
                            if last_warn.is_none_or(|t| t.elapsed() >= Duration::from_secs(5)) {
                                last_warn = Some(Instant::now());
                                eprintln!(
                                    "galion-stt: session {id} WARN overload: dropped {} ms of oldest audio so far",
                                    samples_to_ms(dropped_samples)
                                );
                                let m = ServerMsg::Error {
                                    message: format!(
                                        "overload: server is behind, dropped {} ms of the oldest audio",
                                        samples_to_ms(dropped_samples)
                                    ),
                                };
                                if !send_msg(&mut sock, &m).await {
                                    queue.abort();
                                    client_gone = true;
                                    break;
                                }
                            }
                        }
                        if samples_in > max_samples {
                            let m = ServerMsg::Error {
                                message: format!(
                                    "session audio limit of {} s reached; finishing",
                                    s.cfg.max_session.as_secs()
                                ),
                            };
                            let _ = send_msg(&mut sock, &m).await;
                            queue.stop();
                            stopping = true;
                        }
                    }
                    Err(e) => {
                        bad_frames += 1;
                        if bad_frames <= 3 {
                            let m = ServerMsg::Error { message: e.to_string() };
                            let _ = send_msg(&mut sock, &m).await;
                        }
                    }
                },
                Some(Ok(Message::Text(t))) => match parse_client(t.as_str()) {
                    Ok(ClientMsg::Stop) => {
                        queue.stop();
                        stopping = true;
                    }
                    Err(e) => {
                        let _ = send_msg(&mut sock, &ServerMsg::Error { message: e }).await;
                    }
                },
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    queue.abort();
                    client_gone = true;
                    break;
                }
            },
            ev = rx.recv() => match ev {
                Some(m) => {
                    if !send_msg(&mut sock, &m).await {
                        queue.abort();
                        client_gone = true;
                        break;
                    }
                }
                // Engine thread finished (after stop, or on error).
                None => break,
            },
            _ = &mut deadline, if !stopping => {
                let m = ServerMsg::Error {
                    message: format!(
                        "session time limit of {} s reached; finishing",
                        s.cfg.max_session.as_secs()
                    ),
                };
                let _ = send_msg(&mut sock, &m).await;
                queue.stop();
                stopping = true;
            }
        }
    }
    if !client_gone {
        close(&mut sock, 1000, "done").await;
    }
    let st = tokio::time::timeout(Duration::from_secs(30), stats_rx)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    eprintln!(
        "galion-stt: session {id} end: wall_ms={} audio_ms={} chunks={} dropped_ms={} bad_frames={} \
         infer_calls={} infer_ms_total={} infer_ms_max={} partials={} finals={} first_text_ms={} \
         stop_to_final_ms={} client_gone={} error={}",
        started.elapsed().as_millis(),
        samples_to_ms(samples_in),
        st.chunks,
        samples_to_ms(dropped_samples),
        bad_frames,
        st.transcribe_calls,
        st.transcribe_ms_total,
        st.transcribe_ms_max,
        st.partials,
        st.finals,
        st.first_text_ms.map_or("-".into(), |v| v.to_string()),
        st.stop_to_final_ms.map_or("-".into(), |v| v.to_string()),
        client_gone,
        st.error,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_only() {
        assert!(check_bind(&"127.0.0.1:8651".parse().unwrap()).is_ok());
        assert!(check_bind(&"[::1]:8651".parse().unwrap()).is_ok());
        assert!(check_bind(&"0.0.0.0:8651".parse().unwrap()).is_err());
        assert!(check_bind(&"192.168.1.5:8651".parse().unwrap()).is_err());
    }

    #[test]
    fn defaults() {
        let c = ServerConfig::default();
        assert_eq!(c.bind.port(), 8651);
        assert!(c.bind.ip().is_loopback());
        assert_eq!(c.partial_interval, Duration::from_millis(300));
        assert_eq!(c.max_session, Duration::from_secs(600));
    }
}
