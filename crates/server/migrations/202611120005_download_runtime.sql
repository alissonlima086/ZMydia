-- Sprint 6: observable, restart-safe download runtime state.
ALTER TABLE download_jobs ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE download_jobs ADD COLUMN process_id INTEGER;
ALTER TABLE download_jobs ADD COLUMN started_at TEXT;
ALTER TABLE download_jobs ADD COLUMN last_progress_at TEXT;
ALTER TABLE download_jobs ADD COLUMN finished_at TEXT;
ALTER TABLE download_jobs ADD COLUMN owner_key TEXT;

CREATE INDEX download_jobs_runtime_idx
    ON download_jobs(status, created_at, attempt_count);
CREATE INDEX download_jobs_owner_runtime_idx
    ON download_jobs(owner_key, status);
