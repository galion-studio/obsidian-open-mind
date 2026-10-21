// SPDX-License-Identifier: MIT
// Copyright (c) 3000 Galion / galion-studio

//! The schemas and statements the spike runs. `MEMORY_SCHEMA` and
//! `HERMES_SCHEMA` are verbatim copies of `crates/openmind-memory/src/store.rs`
//! `SCHEMA_SQL` and `crates/hermes-rs/src/session_sqlite.rs` `SCHEMA` at main
//! 13a296c. `LEGACY_FTS` is a representative upstream-Hermes FTS5 index with
//! sync triggers (the Python state.db keeps "FTS5 search indexes",
//! stack/hermes/config.yaml.redacted; its exact DDL is not in this repo).

pub const MEMORY_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS embedding_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS l1_records (
  record_id TEXT PRIMARY KEY, content TEXT NOT NULL, type TEXT DEFAULT '', priority INTEGER DEFAULT 50,
  scene_name TEXT DEFAULT '', session_key TEXT DEFAULT '', session_id TEXT DEFAULT '',
  timestamp_str TEXT DEFAULT '', timestamp_start TEXT DEFAULT '', timestamp_end TEXT DEFAULT '',
  created_time TEXT DEFAULT '', updated_time TEXT DEFAULT '', metadata_json TEXT DEFAULT '{}'
);
CREATE INDEX IF NOT EXISTS idx_l1_type ON l1_records(type);
CREATE INDEX IF NOT EXISTS idx_l1_session_key ON l1_records(session_key);
CREATE INDEX IF NOT EXISTS idx_l1_session_id ON l1_records(session_id);
CREATE INDEX IF NOT EXISTS idx_l1_scene ON l1_records(scene_name);
CREATE INDEX IF NOT EXISTS idx_l1_ts_start ON l1_records(timestamp_start);
CREATE INDEX IF NOT EXISTS idx_l1_ts_end ON l1_records(timestamp_end);
CREATE INDEX IF NOT EXISTS idx_l1_session_updated ON l1_records(session_id, updated_time);
CREATE INDEX IF NOT EXISTS idx_l1_sessionkey_updated ON l1_records(session_key, updated_time);
CREATE TABLE IF NOT EXISTS l0_conversations (
  record_id TEXT PRIMARY KEY, session_key TEXT NOT NULL, session_id TEXT DEFAULT '',
  role TEXT NOT NULL DEFAULT '', message_text TEXT NOT NULL, recorded_at TEXT DEFAULT '', timestamp INTEGER DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_l0_session ON l0_conversations(session_key);
CREATE INDEX IF NOT EXISTS idx_l0_session_id ON l0_conversations(session_id);
CREATE INDEX IF NOT EXISTS idx_l0_recorded ON l0_conversations(recorded_at);
CREATE INDEX IF NOT EXISTS idx_l0_timestamp ON l0_conversations(timestamp);
CREATE VIRTUAL TABLE IF NOT EXISTS l1_fts USING fts5(
  content, content_original UNINDEXED, record_id UNINDEXED, type UNINDEXED, priority UNINDEXED,
  scene_name UNINDEXED, session_key UNINDEXED, session_id UNINDEXED, timestamp_str UNINDEXED,
  timestamp_start UNINDEXED, timestamp_end UNINDEXED, metadata_json UNINDEXED
);
CREATE VIRTUAL TABLE IF NOT EXISTS l0_fts USING fts5(
  message_text, message_text_original UNINDEXED, record_id UNINDEXED, session_key UNINDEXED,
  session_id UNINDEXED, role UNINDEXED, recorded_at UNINDEXED, timestamp UNINDEXED
);
"#;
pub const HERMES_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS system_prompts (hash TEXT PRIMARY KEY, prompt TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY, source TEXT NOT NULL, user_id TEXT, session_key TEXT, chat_id TEXT, chat_type TEXT,
    thread_id TEXT, display_name TEXT, origin_json TEXT, expiry_finalized INTEGER DEFAULT 0, model TEXT,
    model_config TEXT, system_prompt TEXT, system_prompt_hash TEXT, parent_session_id TEXT, started_at REAL NOT NULL,
    ended_at REAL, end_reason TEXT, message_count INTEGER DEFAULT 0, tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0, output_tokens INTEGER DEFAULT 0, cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0, reasoning_tokens INTEGER DEFAULT 0, cwd TEXT, git_branch TEXT,
    git_repo_root TEXT, git_metadata_generation INTEGER NOT NULL DEFAULT 0, billing_provider TEXT,
    billing_base_url TEXT, billing_mode TEXT, estimated_cost_usd REAL, actual_cost_usd REAL, cost_status TEXT,
    cost_source TEXT, pricing_version TEXT, title TEXT, title_source TEXT, last_activity_at REAL,
    last_activity_description TEXT, last_activity_provenance TEXT, api_call_count INTEGER DEFAULT 0,
    handoff_state TEXT, handoff_platform TEXT, handoff_error TEXT, compression_failure_cooldown_until REAL,
    compression_failure_error TEXT, compression_fallback_streak INTEGER NOT NULL DEFAULT 0,
    compression_ineffective_count INTEGER NOT NULL DEFAULT 0, compression_recovery_deadline REAL,
    profile_name TEXT, transport_profile TEXT, rewind_count INTEGER NOT NULL DEFAULT 0,
    archived INTEGER NOT NULL DEFAULT 0, pinned INTEGER NOT NULL DEFAULT 0, hidden INTEGER NOT NULL DEFAULT 0,
    last_read_at REAL, tool_names TEXT,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id),
    FOREIGN KEY (system_prompt_hash) REFERENCES system_prompts(hash)
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT, session_id TEXT NOT NULL REFERENCES sessions(id), role TEXT NOT NULL,
    content TEXT, tool_call_id TEXT, tool_calls TEXT, tool_name TEXT, effect_disposition TEXT,
    timestamp REAL NOT NULL, token_count INTEGER, finish_reason TEXT, reasoning TEXT, reasoning_content TEXT,
    reasoning_details TEXT, codex_reasoning_items TEXT, codex_message_items TEXT, platform_message_id TEXT,
    observed INTEGER DEFAULT 0, _compressed_summary INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1,
    compacted INTEGER NOT NULL DEFAULT 0, api_content TEXT, display_kind TEXT, display_metadata TEXT,
    display_identity BLOB, display_order INTEGER
);
CREATE INDEX IF NOT EXISTS idx_sessions_source ON sessions(source);
CREATE INDEX IF NOT EXISTS idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_session_id ON messages(session_id, id);
"#;
pub const LEGACY_FTS: &str = r#"
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(content, content=messages, content_rowid=id);
CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
END;
CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE ON messages BEGIN
    INSERT INTO messages_fts(messages_fts, rowid, content) VALUES ('delete', old.id, old.content);
    INSERT INTO messages_fts(rowid, content) VALUES (new.id, new.content);
END;
"#;

/// hermes-rs `ROW_COLS`.
pub const ROW_COLS: &str = "id, source, session_key, model, billing_provider, title, parent_session_id, started_at, ended_at, message_count, input_tokens, output_tokens, estimated_cost_usd";
