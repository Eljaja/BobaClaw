CREATE TABLE IF NOT EXISTS scheduled_tasks (
    id TEXT PRIMARY KEY,
    agent_group TEXT NOT NULL,
    prompt TEXT NOT NULL,
    deliver_text TEXT,
    run_at REAL NOT NULL,
    deliver_channel TEXT,
    deliver_peer TEXT,
    source_session_id TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at REAL NOT NULL,
    completed_at REAL,
    last_error TEXT
);

CREATE INDEX IF NOT EXISTS idx_scheduled_tasks_due
    ON scheduled_tasks(status, run_at);
