-- Agent-created recurring jobs need delivery targets (like scheduled_tasks).
ALTER TABLE cron_jobs ADD COLUMN deliver_channel TEXT;
ALTER TABLE cron_jobs ADD COLUMN deliver_peer TEXT;
ALTER TABLE cron_jobs ADD COLUMN deliver_text TEXT;
ALTER TABLE cron_jobs ADD COLUMN source_session_id TEXT;
