-- Fixture for crates/one-defence tests. Not the live guardian.db.
-- Schema matches the health_checks table created by galion-light/scripts/guardian-tick.py.
-- Older rows are present so the defence.py query must keep only MAX(id) per service.
CREATE TABLE health_checks (
    id INTEGER PRIMARY KEY,
    service TEXT,
    status TEXT,
    response_time_ms REAL,
    error TEXT,
    checked_at TEXT
);
INSERT INTO health_checks (service, status, response_time_ms, error, checked_at) VALUES
    ('console', 'critical', 999.9, 'old', '2026-01-01T00:00:00Z'),
    ('hermes', 'healthy', 10.0, NULL, '2026-01-01T00:00:00Z'),
    ('vision', 'healthy', 80.0, NULL, '2026-01-01T00:00:01Z'),
    ('console', 'healthy', 45.7, NULL, '2026-01-02T00:00:00Z'),
    ('whisper', 'degraded', NULL, 'timeout', '2026-01-02T00:00:01Z'),
    ('hermes', 'healthy', 12.9, NULL, '2026-01-02T00:00:02Z'),
    ('vision', 'healthy', 0.4, NULL, '2026-01-02T00:00:03Z');
