-- v3: Async job model for DAG execution
-- Jobs track async fetch requests. Output blobs live in materialized_cache on R2.

-- ============================================================
-- Jobs
-- ============================================================
CREATE TABLE jobs (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id          UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    endpoint_name       TEXT NOT NULL,
    commit_id           UUID NOT NULL REFERENCES commits(id) ON DELETE CASCADE,
    params              JSONB NOT NULL DEFAULT '{}',
    params_hash         TEXT NOT NULL,          -- blake3 of canonical params JSON, for dedup
    status              TEXT NOT NULL DEFAULT 'queued'
                        CHECK (status IN ('queued', 'running', 'done', 'failed')),
    node_status         JSONB NOT NULL DEFAULT '{}',  -- {"node_name": "queued|running|done|failed"}
    output_hash         TEXT,                   -- materialized hash of terminal node output
    output_content_type TEXT,
    error_message       TEXT,
    created_by          UUID REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at          TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    expires_at          TIMESTAMPTZ             -- for periodic cleanup
);

-- Deduplication index: find active jobs for same request
CREATE INDEX idx_jobs_dedup ON jobs (project_id, endpoint_name, commit_id, params_hash)
    WHERE status IN ('queued', 'running');

-- Lookup by project (for admin/user job listing)
CREATE INDEX idx_jobs_project ON jobs (project_id, created_at DESC);

-- Cleanup of expired jobs
CREATE INDEX idx_jobs_expires ON jobs (expires_at) WHERE expires_at IS NOT NULL;

-- ============================================================
-- Environment provider images (multi-provider tracking)
-- ============================================================
CREATE TABLE environment_provider_images (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_hash    TEXT NOT NULL,               -- environment content hash
    provider    TEXT NOT NULL,               -- 'fly', 'docker', 'ecs', etc.
    image_ref   TEXT NOT NULL,               -- provider-specific image reference
    pushed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(env_hash, provider)
);
