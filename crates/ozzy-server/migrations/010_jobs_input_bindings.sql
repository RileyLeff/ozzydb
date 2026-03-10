ALTER TABLE jobs
    ADD COLUMN input_bindings JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN input_bindings_hash TEXT NOT NULL DEFAULT '';

ALTER TABLE jobs
    ADD CONSTRAINT chk_jobs_input_bindings_object
        CHECK (jsonb_typeof(input_bindings) = 'object');

DROP INDEX IF EXISTS idx_jobs_dedup;

CREATE INDEX idx_jobs_dedup
    ON jobs (project_id, endpoint_name, commit_id, params_hash, input_bindings_hash)
    WHERE status IN ('queued', 'running');
