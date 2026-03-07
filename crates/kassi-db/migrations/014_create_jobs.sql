CREATE TABLE jobs (
    id              BIGSERIAL PRIMARY KEY,
    queue           TEXT NOT NULL,
    payload         JSONB NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    attempts        INTEGER NOT NULL DEFAULT 0,
    max_attempts    INTEGER NOT NULL DEFAULT 5,
    scheduled_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    failed_at       TIMESTAMPTZ,
    last_error      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_job_status CHECK (status IN ('pending', 'running', 'completed', 'failed', 'dead'))
);

CREATE INDEX idx_jobs_poll ON jobs(queue, scheduled_at) WHERE status = 'pending';
