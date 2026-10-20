// SPDX-License-Identifier: MIT
// SPDX-FileCopyrightText: 3000 Galion / galion-studio

//! In-memory or temp-file FTS index. One connection, no network.

use rusqlite::{params, Connection, Transaction};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

const MAX_CONTENT: usize = 8192;
const MAX_QUERY: usize = 2048;
const MAX_SESSION: usize = 512;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);
INSERT OR IGNORE INTO meta (key, value) VALUES ('next_id', 1);
INSERT OR IGNORE INTO meta (key, value) VALUES ('offload_seq', 1);
CREATE VIRTUAL TABLE IF NOT EXISTS docs USING fts5(
    id UNINDEXED,
    kind UNINDEXED,
    session_key UNINDEXED,
    role UNINDEXED,
    mem_type UNINDEXED,
    scene UNINDEXED,
    recorded_at UNINDEXED,
    content
);
CREATE TABLE IF NOT EXISTS offload_ref (
    result_ref TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    content TEXT NOT NULL,
    recorded_at TEXT NOT NULL,
    seq INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS offload_canvas (
    session_id TEXT PRIMARY KEY,
    content TEXT NOT NULL,
    recorded_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS session_end (
    session_key TEXT PRIMARY KEY,
    flushed_at TEXT NOT NULL
);
";

/// Why a store or search call failed. HTTP maps [`Error::BadRequest`] to 400.
#[derive(Debug)]
pub enum Error {
    BadRequest(String),
    Store(String),
    Io(String),
    Bind(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::BadRequest(msg) | Error::Store(msg) | Error::Io(msg) | Error::Bind(msg) => {
                f.write_str(msg)
            }
        }
    }
}

impl std::error::Error for Error {}

fn store_err(err: rusqlite::Error) -> Error {
    Error::Store(err.to_string())
}

/// `memory` is an L1 note. `conversation` is an L0 message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Memory,
    Conversation,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Memory => "memory",
            Kind::Conversation => "conversation",
        }
    }

    fn parse(raw: &str) -> Result<Self, Error> {
        match raw {
            "memory" => Ok(Kind::Memory),
            "conversation" => Ok(Kind::Conversation),
            _ => Err(Error::BadRequest(format!("unknown kind: {raw}"))),
        }
    }
}

/// One row to store.
#[derive(Debug, Clone)]
pub struct PutRequest {
    pub kind: Kind,
    pub content: String,
    pub role: String,
    pub session_key: String,
    pub mem_type: String,
    pub scene: String,
}

impl PutRequest {
    /// An L1 note. Type defaults to `episodic` at insert time when empty.
    pub fn memory(content: impl Into<String>) -> Self {
        Self {
            kind: Kind::Memory,
            content: content.into(),
            role: String::new(),
            session_key: String::new(),
            mem_type: "episodic".to_string(),
            scene: String::new(),
        }
    }

    /// One L0 message. `role` is `user`, `assistant`, or `system`.
    pub fn conversation(
        session_key: impl Into<String>,
        role: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            kind: Kind::Conversation,
            content: content.into(),
            role: role.into(),
            session_key: session_key.into(),
            mem_type: String::new(),
            scene: String::new(),
        }
    }
}

/// What [`Index::put`] stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub id: String,
    pub recorded_at: String,
}

/// v1 `POST /capture` counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOut {
    pub l0_recorded: u32,
    pub memory_id: String,
    pub recorded_at: String,
}

/// Search filters. Blank `query` is a bad request. `limit` 0 means 5.
#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
    pub kind: Option<Kind>,
    pub session_key: Option<String>,
    pub mem_type: Option<String>,
    pub scene: Option<String>,
}

impl SearchRequest {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 5,
            kind: None,
            session_key: None,
            mem_type: None,
            scene: None,
        }
    }
}

/// One search hit. `recorded_at` is Europe/Zagreb RFC 3339.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub kind: Kind,
    pub session_key: String,
    pub role: String,
    pub mem_type: String,
    pub scene: String,
    pub content: String,
    pub recorded_at: String,
    pub score: f64,
}

/// FTS5 index. `:memory:` or a caller-supplied file. Never a live gateway db.
pub struct Index {
    conn: Connection,
}

impl Index {
    /// Process-local index. Dropped with the connection. No file is left.
    pub fn open_memory() -> Result<Self, Error> {
        let conn = Connection::open_in_memory().map_err(store_err)?;
        Self::prepare(conn)
    }

    /// File index. The parent directory must already exist. Typical use is a
    /// temp directory the test deletes.
    pub fn open_path(path: &Path) -> Result<Self, Error> {
        let conn = Connection::open(path).map_err(store_err)?;
        Self::prepare(conn)
    }

    fn prepare(conn: Connection) -> Result<Self, Error> {
        conn.busy_timeout(std::time::Duration::from_secs(1))
            .map_err(store_err)?;
        conn.execute_batch(SCHEMA).map_err(store_err)?;
        Ok(Self { conn })
    }

    /// Load a JSON array of notes. Each object needs `kind` and `content`.
    pub fn load_fixture_str(&mut self, text: &str) -> Result<usize, Error> {
        let items: Vec<FixtureItem> =
            serde_json::from_str(text).map_err(|err| Error::BadRequest(err.to_string()))?;
        let mut n = 0usize;
        for item in items {
            self.put(&item.into_put()?)?;
            n += 1;
        }
        Ok(n)
    }

    pub fn load_fixture_path(&mut self, path: &Path) -> Result<usize, Error> {
        let text = std::fs::read_to_string(path)
            .map_err(|err| Error::Io(format!("read {}: {err}", path.display())))?;
        self.load_fixture_str(&text)
    }

    /// Store one memory note or one conversation message.
    pub fn put(&mut self, req: &PutRequest) -> Result<Stored, Error> {
        self.put_all(std::slice::from_ref(req))?
            .into_iter()
            .next()
            .ok_or_else(|| Error::Store("put stored nothing".into()))
    }

    /// Store every row in one transaction, or none of them.
    pub fn put_all(&mut self, reqs: &[PutRequest]) -> Result<Vec<Stored>, Error> {
        for req in reqs {
            validate(req)?;
        }
        let tx = self.conn.transaction().map_err(store_err)?;
        let mut stored = Vec::with_capacity(reqs.len());
        for req in reqs {
            stored.push(insert(&tx, req)?);
        }
        tx.commit().map_err(store_err)?;
        Ok(stored)
    }

    /// v1 capture: two L0 rows plus one episodic note of the joined text.
    /// No model is called. `scheduler_notified` is therefore false at the HTTP edge.
    pub fn capture(
        &mut self,
        session_key: &str,
        user: &str,
        assistant: &str,
    ) -> Result<CaptureOut, Error> {
        let session_key = session_key.trim();
        if session_key.is_empty() || user.trim().is_empty() || assistant.trim().is_empty() {
            return Err(Error::BadRequest(
                "Missing required fields: user_content, assistant_content, session_key".into(),
            ));
        }
        let user_row = PutRequest::conversation(session_key, "user", user.trim());
        let assistant_row = PutRequest::conversation(session_key, "assistant", assistant.trim());
        let mut note = PutRequest::memory(format!("{}\n{}", user.trim(), assistant.trim()));
        note.session_key = session_key.to_string();
        validate(&user_row)?;
        validate(&assistant_row)?;
        validate(&note)?;
        let tx = self.conn.transaction().map_err(store_err)?;
        insert(&tx, &user_row)?;
        insert(&tx, &assistant_row)?;
        let stored = insert(&tx, &note)?;
        tx.commit().map_err(store_err)?;
        Ok(CaptureOut {
            l0_recorded: 2,
            memory_id: stored.id,
            recorded_at: stored.recorded_at,
        })
    }

    /// Token search. Strategy at the HTTP edge is `fts` (no embeddings).
    pub fn search(&self, req: &SearchRequest) -> Result<Vec<Hit>, Error> {
        let query = req.query.trim();
        if query.is_empty() {
            return Err(Error::BadRequest("Missing required field: query".into()));
        }
        if query.len() > MAX_QUERY {
            return Err(Error::BadRequest("query is too long".into()));
        }
        let query_tokens = tokens(query);
        if query_tokens.is_empty() {
            return Ok(Vec::new());
        }
        let match_q = query_tokens
            .iter()
            .map(|token| format!("\"{token}\""))
            .collect::<Vec<_>>()
            .join(" OR ");
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, session_key, role, mem_type, scene, recorded_at, content
                 FROM docs WHERE docs MATCH ?1",
            )
            .map_err(store_err)?;
        let rows = stmt
            .query_map(params![match_q], |row| {
                Ok(RawHit {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    session_key: row.get(2)?,
                    role: row.get(3)?,
                    mem_type: row.get(4)?,
                    scene: row.get(5)?,
                    recorded_at: row.get(6)?,
                    content: row.get(7)?,
                })
            })
            .map_err(store_err)?;
        let mut hits = Vec::new();
        for row in rows {
            let raw = row.map_err(store_err)?;
            let kind = Kind::parse(&raw.kind)?;
            if req.kind.is_some_and(|want| want != kind) {
                continue;
            }
            if let Some(session) = req.session_key.as_deref() {
                if !session.is_empty() && raw.session_key != session {
                    continue;
                }
            }
            if let Some(mem_type) = req.mem_type.as_deref() {
                if !mem_type.is_empty() && raw.mem_type != mem_type {
                    continue;
                }
            }
            if let Some(scene) = req.scene.as_deref() {
                if !scene.is_empty()
                    && !raw
                        .scene
                        .to_ascii_lowercase()
                        .contains(&scene.to_ascii_lowercase())
                {
                    continue;
                }
            }
            let score = overlap(&query_tokens, &raw.content);
            if score <= 0.0 {
                continue;
            }
            hits.push(Hit {
                id: raw.id,
                kind,
                session_key: raw.session_key,
                role: raw.role,
                mem_type: raw.mem_type,
                scene: raw.scene,
                content: raw.content,
                recorded_at: raw.recorded_at,
                score,
            });
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        let limit = clamp_limit(req.limit);
        hits.truncate(limit);
        Ok(hits)
    }

    /// v1 `POST /recall`. FTS only: embeddings stay off, so context is the
    /// matching L0 lines for this session, then the matching L1 notes.
    pub(crate) fn recall(&self, query: &str, session_key: &str) -> Result<RecallOut, Error> {
        let session_key = session_key.trim();
        if query.trim().is_empty() || session_key.is_empty() {
            return Err(Error::BadRequest(
                "Missing required fields: query, session_key".into(),
            ));
        }
        if session_key.len() > MAX_SESSION {
            return Err(Error::BadRequest("session_key is too long".into()));
        }
        let mut memories = SearchRequest::new(query);
        memories.kind = Some(Kind::Memory);
        memories.session_key = Some(session_key.to_string());
        let mem_hits = self.search(&memories)?;

        let mut talks = SearchRequest::new(query);
        talks.kind = Some(Kind::Conversation);
        talks.session_key = Some(session_key.to_string());
        let talk_hits = self.search(&talks)?;

        let mut parts = Vec::new();
        for hit in &talk_hits {
            parts.push(hit.content.as_str());
        }
        for hit in &mem_hits {
            parts.push(hit.content.as_str());
        }
        Ok(RecallOut {
            context: clip_chars(&parts.join("\n"), 2000),
            memory_count: mem_hits.len(),
            strategy: "fts",
        })
    }

    /// v1 `POST /session/end`. Records a Zagreb flush time and reports success.
    pub(crate) fn end_session(&mut self, session_key: &str) -> Result<(), Error> {
        let session_key = session_key.trim();
        if session_key.is_empty() {
            return Err(Error::BadRequest(
                "Missing required field: session_key".into(),
            ));
        }
        if session_key.len() > MAX_SESSION {
            return Err(Error::BadRequest("session_key is too long".into()));
        }
        let flushed_at = zagreb_stamp()?;
        self.conn
            .execute(
                "INSERT INTO session_end (session_key, flushed_at) VALUES (?1, ?2)
                 ON CONFLICT(session_key) DO UPDATE SET flushed_at = excluded.flushed_at",
                params![session_key, flushed_at],
            )
            .map_err(store_err)?;
        Ok(())
    }

    /// Fixture `POST /v2/offload/ingest`. Writes one markdown ref per pair and
    /// rebuilds the session canvas. No model runs.
    pub(crate) fn ingest_offload(
        &mut self,
        session_id: &str,
        pairs: &[ToolPairRow],
    ) -> Result<Vec<String>, Error> {
        if !valid_offload_session(session_id) {
            return Err(Error::BadRequest("missing or invalid session_id".into()));
        }
        if pairs.is_empty() || pairs.len() > 100 {
            return Err(Error::BadRequest(
                "tool_pairs must hold 1 to 100 items".into(),
            ));
        }
        let session_id = session_id.trim();
        let refs_dir = format!("offload/{}/refs", sanitize_session(session_id));
        let tx = self.conn.transaction().map_err(store_err)?;
        let mut node_ids = Vec::with_capacity(pairs.len());
        for pair in pairs {
            let tool_name = pair.tool_name.trim();
            if tool_name.is_empty() || tool_name.len() > 256 {
                return Err(Error::BadRequest("tool pair tool_name is empty".into()));
            }
            if pair.tool_call_id.len() > 256 {
                return Err(Error::BadRequest(
                    "tool pair tool_call_id is too long".into(),
                ));
            }
            if pair.params_json.len() > MAX_CONTENT || pair.result_text.len() > MAX_CONTENT {
                return Err(Error::BadRequest("tool pair is too long".into()));
            }
            let seq = next_offload_seq(&tx)?;
            let node_id = tool_node_id(&pair.tool_call_id, seq);
            let result_ref = format!("{refs_dir}/{node_id}.md");
            let recorded_at = zagreb_stamp()?;
            let content = render_tool_ref(&node_id, tool_name, pair);
            tx.execute(
                "INSERT INTO offload_ref (
                    result_ref, session_id, node_id, tool_name, content, recorded_at, seq
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(result_ref) DO UPDATE SET
                    tool_name = excluded.tool_name,
                    content = excluded.content",
                params![
                    result_ref,
                    session_id,
                    node_id,
                    tool_name,
                    content,
                    recorded_at,
                    seq
                ],
            )
            .map_err(store_err)?;
            node_ids.push(node_id);
        }
        rewrite_canvas(&tx, session_id)?;
        tx.commit().map_err(store_err)?;
        Ok(node_ids)
    }

    /// Fixture `POST /v2/offload/query-mmd`. One canvas file, or nothing yet.
    pub(crate) fn offload_canvas(&self, session_id: &str) -> Result<Option<OffloadCanvas>, Error> {
        if !valid_offload_session(session_id) {
            return Err(Error::BadRequest(
                "missing or invalid session_id in body".into(),
            ));
        }
        let session_id = session_id.trim();
        let mut stmt = self
            .conn
            .prepare("SELECT content, recorded_at FROM offload_canvas WHERE session_id = ?1")
            .map_err(store_err)?;
        let mut rows = stmt.query(params![session_id]).map_err(store_err)?;
        if let Some(row) = rows.next().map_err(store_err)? {
            Ok(Some(OffloadCanvas {
                content: row.get(0).map_err(store_err)?,
                recorded_at: row.get(1).map_err(store_err)?,
            }))
        } else {
            Ok(None)
        }
    }

    /// Fixture `POST /v2/offload/read-ref`. The caller already checked ownership.
    pub(crate) fn read_offload_ref(&self, result_ref: &str) -> Result<Option<String>, Error> {
        let mut stmt = self
            .conn
            .prepare("SELECT content FROM offload_ref WHERE result_ref = ?1")
            .map_err(store_err)?;
        let mut rows = stmt.query(params![result_ref]).map_err(store_err)?;
        if let Some(row) = rows.next().map_err(store_err)? {
            Ok(Some(row.get(0).map_err(store_err)?))
        } else {
            Ok(None)
        }
    }
}

/// v1 `POST /recall` body the provider reads (`context` is the injected text).
pub(crate) struct RecallOut {
    pub context: String,
    pub memory_count: usize,
    pub strategy: &'static str,
}

/// One tool pair accepted by the offload ingest fixture.
pub(crate) struct ToolPairRow {
    pub tool_name: String,
    pub tool_call_id: String,
    pub params_json: String,
    pub result_text: String,
    pub error: Option<String>,
}

/// Session canvas returned by the offload MMD fixture.
pub(crate) struct OffloadCanvas {
    pub content: String,
    pub recorded_at: String,
}

/// Offload session ids: alphanumeric plus `_ . - :`, at most 500 chars.
pub(crate) fn valid_offload_session(session_id: &str) -> bool {
    let session_id = session_id.trim();
    !session_id.is_empty()
        && session_id.len() <= 500
        && session_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | ':'))
}

/// A result ref is owned only when it is a direct markdown child of this
/// session's refs directory. The session id is sanitized the same way the
/// provider builds `offload/<id>/refs/<node>.md`.
pub(crate) fn owned_result_ref(session_id: &str, result_ref: &str) -> Option<String> {
    if !valid_offload_session(session_id) || result_ref.len() > 1000 {
        return None;
    }
    let prefix = format!("offload/{}/refs/", sanitize_session(session_id.trim()));
    let filename = result_ref.strip_prefix(&prefix)?;
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains('\0')
        || filename == "."
        || filename == ".."
        || !filename.ends_with(".md")
        || filename.len() <= ".md".len()
    {
        return None;
    }
    Some(result_ref.to_string())
}

fn sanitize_session(session_id: &str) -> String {
    session_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

struct RawHit {
    id: String,
    kind: String,
    session_key: String,
    role: String,
    mem_type: String,
    scene: String,
    recorded_at: String,
    content: String,
}

fn insert(tx: &Transaction<'_>, req: &PutRequest) -> Result<Stored, Error> {
    let prefix = match req.kind {
        Kind::Memory => 'm',
        Kind::Conversation => 'c',
    };
    let id = alloc_id(tx, prefix)?;
    let recorded_at = zagreb_stamp()?;
    let mem_type = if req.kind == Kind::Memory && req.mem_type.trim().is_empty() {
        "episodic".to_string()
    } else {
        req.mem_type.trim().to_string()
    };
    tx.execute(
        "INSERT INTO docs (id, kind, session_key, role, mem_type, scene, recorded_at, content)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            req.kind.as_str(),
            req.session_key.trim(),
            req.role.trim(),
            mem_type,
            req.scene.trim(),
            recorded_at,
            req.content.trim(),
        ],
    )
    .map_err(store_err)?;
    Ok(Stored { id, recorded_at })
}

fn zagreb_stamp() -> Result<String, Error> {
    let recorded_at = zagreb_time::now_rfc3339();
    if recorded_at.contains('Z') || recorded_at.contains("+00:00") {
        return Err(Error::Store("refusing a non-Zagreb timestamp".to_string()));
    }
    Ok(recorded_at)
}

fn clip_chars(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn next_offload_seq(tx: &Transaction<'_>) -> Result<i64, Error> {
    let n: i64 = tx
        .query_row(
            "SELECT value FROM meta WHERE key = 'offload_seq'",
            [],
            |row| row.get(0),
        )
        .map_err(store_err)?;
    tx.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'offload_seq'",
        params![n + 1],
    )
    .map_err(store_err)?;
    Ok(n)
}

/// Node ids match `^n[A-Za-z0-9]+$` and stay within 24 characters.
/// An empty tool-call id falls back to `n` plus a hex sequence. No hash crate.
fn tool_node_id(tool_call_id: &str, seq: i64) -> String {
    let stripped: String = tool_call_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect();
    if stripped.is_empty() {
        return format!("n{seq:012x}");
    }
    let mut id = format!("n{stripped}");
    if id.len() > 24 {
        id.truncate(24);
    }
    id
}

fn render_tool_ref(node_id: &str, tool_name: &str, pair: &ToolPairRow) -> String {
    let mut lines = vec![
        format!("# node_id: {node_id}"),
        format!("tool: {tool_name}"),
        format!("tool_call_id: {}", pair.tool_call_id),
        "```json".to_string(),
        pair.params_json.clone(),
        "```".to_string(),
        "## result".to_string(),
        pair.result_text.clone(),
    ];
    if let Some(error) = pair.error.as_deref() {
        lines.push("## error".to_string());
        lines.push(error.to_string());
    }
    lines.push(String::new());
    lines.join("\n")
}

fn mermaid_label(tool_name: &str) -> String {
    let mut out = String::new();
    for ch in tool_name.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push('\''),
            '\n' | '\r' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

fn render_canvas(nodes: &[(String, String)]) -> String {
    let mut lines = vec![
        "%% each node drills to refs/<nodeId>.md".to_string(),
        "flowchart TD".to_string(),
    ];
    for (node_id, tool_name) in nodes {
        let label = mermaid_label(tool_name);
        lines.push(format!("  {node_id}[\"{label}\"]"));
    }
    for pair in nodes.windows(2) {
        lines.push(format!("  {} --> {}", pair[0].0, pair[1].0));
    }
    lines.push(String::new());
    lines.join("\n")
}

fn rewrite_canvas(tx: &Transaction<'_>, session_id: &str) -> Result<(), Error> {
    let mut stmt = tx
        .prepare(
            "SELECT node_id, tool_name FROM offload_ref
             WHERE session_id = ?1 ORDER BY seq, node_id",
        )
        .map_err(store_err)?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(store_err)?;
    let mut nodes = Vec::new();
    for row in rows {
        nodes.push(row.map_err(store_err)?);
    }
    drop(stmt);
    let content = render_canvas(&nodes);
    let recorded_at = zagreb_stamp()?;
    tx.execute(
        "INSERT INTO offload_canvas (session_id, content, recorded_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(session_id) DO UPDATE SET
            content = excluded.content,
            recorded_at = excluded.recorded_at",
        params![session_id, content, recorded_at],
    )
    .map_err(store_err)?;
    Ok(())
}

fn alloc_id(tx: &Transaction<'_>, prefix: char) -> Result<String, Error> {
    let n: i64 = tx
        .query_row("SELECT value FROM meta WHERE key = 'next_id'", [], |row| {
            row.get(0)
        })
        .map_err(store_err)?;
    tx.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'next_id'",
        params![n + 1],
    )
    .map_err(store_err)?;
    Ok(format!("{prefix}{n:08x}"))
}

fn validate(req: &PutRequest) -> Result<(), Error> {
    let content = req.content.trim();
    if content.is_empty() {
        return Err(Error::BadRequest("content is empty".into()));
    }
    if content.len() > MAX_CONTENT {
        return Err(Error::BadRequest("content is too long".into()));
    }
    if req.session_key.trim().len() > MAX_SESSION {
        return Err(Error::BadRequest("session_key is too long".into()));
    }
    if req.kind == Kind::Conversation {
        if req.session_key.trim().is_empty() {
            return Err(Error::BadRequest("session_key is empty".into()));
        }
        match req.role.trim() {
            "user" | "assistant" | "system" => {}
            _ => {
                return Err(Error::BadRequest(
                    "role must be user, assistant, or system".into(),
                ))
            }
        }
    }
    Ok(())
}

fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch.to_ascii_lowercase());
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn overlap(query: &[String], content: &str) -> f64 {
    if query.is_empty() {
        return 0.0;
    }
    let doc: std::collections::BTreeSet<String> = tokens(content).into_iter().collect();
    let hit = query.iter().filter(|token| doc.contains(*token)).count();
    hit as f64 / query.len() as f64
}

pub(crate) fn clamp_limit(limit: usize) -> usize {
    if limit == 0 {
        5
    } else {
        limit.min(100)
    }
}

#[derive(Debug, Deserialize)]
struct FixtureItem {
    kind: String,
    content: String,
    #[serde(default)]
    role: String,
    #[serde(default)]
    session_key: String,
    #[serde(default)]
    mem_type: String,
    #[serde(default)]
    scene: String,
}

impl FixtureItem {
    fn into_put(self) -> Result<PutRequest, Error> {
        let kind = Kind::parse(&self.kind)?;
        Ok(PutRequest {
            kind,
            content: self.content,
            role: self.role,
            session_key: self.session_key,
            mem_type: self.mem_type,
            scene: self.scene,
        })
    }
}

/// Read a string field. Missing, null, or blank is `None`.
pub(crate) fn field_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
}

pub(crate) fn field_limit(value: &Value) -> usize {
    match value.get("limit").and_then(Value::as_u64) {
        Some(n) => usize::try_from(n).unwrap_or(100),
        None => 0,
    }
}
