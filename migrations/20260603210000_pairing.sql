CREATE TABLE IF NOT EXISTS pairing_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    channel TEXT NOT NULL,
    peer TEXT NOT NULL,
    code TEXT NOT NULL,
    display_name TEXT,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at REAL NOT NULL,
    resolved_at REAL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_pairing_pending_peer
    ON pairing_requests(channel, peer) WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_pairing_code ON pairing_requests(code) WHERE status = 'pending';
