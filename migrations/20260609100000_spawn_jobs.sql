CREATE TABLE IF NOT EXISTS spawn_jobs (
    id TEXT PRIMARY KEY,
    subagent_id TEXT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    agent_group TEXT NOT NULL,
    ingress TEXT NOT NULL,
    deliver_channel TEXT,
    deliver_peer TEXT,
    deliver_thread_id TEXT,
    label TEXT,
    task_preview TEXT,
    backend TEXT,
    status TEXT NOT NULL DEFAULT 'running',
    exit_code INTEGER,
    result_preview TEXT,
    result_body TEXT,
    parent_request_id TEXT,
    wake_parent INTEGER NOT NULL DEFAULT 1,
    notified_at REAL,
    created_at REAL NOT NULL,
    started_at REAL,
    finished_at REAL
);

CREATE INDEX IF NOT EXISTS idx_spawn_jobs_session ON spawn_jobs(session_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_spawn_jobs_status ON spawn_jobs(status);
