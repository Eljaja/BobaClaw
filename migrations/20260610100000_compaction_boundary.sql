-- Compaction rows record the id of the last message they summarize, so effective
-- history = latest summary + every non-compaction message after that boundary.
-- NULL for regular messages and for legacy compaction rows.
ALTER TABLE messages ADD COLUMN covers_through_id INTEGER;
