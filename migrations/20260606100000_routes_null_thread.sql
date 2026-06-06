-- SQLite UNIQUE(channel, peer, thread_id) treats each NULL thread_id as distinct,
-- so DM routes duplicated on every message and get_session_id kept returning the
-- oldest (often ended) session. Collapse NULL duplicates first, then normalize.

DELETE FROM routes
WHERE thread_id IS NULL
  AND id NOT IN (
    SELECT MAX(id)
    FROM routes
    WHERE thread_id IS NULL
    GROUP BY channel, peer
  );

UPDATE routes SET thread_id = '' WHERE thread_id IS NULL;

DELETE FROM routes
WHERE id NOT IN (
    SELECT MAX(id)
    FROM routes
    GROUP BY channel, peer, thread_id
);
