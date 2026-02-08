-- Materialized cache: tracks server-side compute results stored in R2.
CREATE TABLE materialized_cache (
    materialized_hash VARCHAR(64) PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    endpoint_name VARCHAR(255) NOT NULL,
    commit_hash VARCHAR(64) NOT NULL,
    platform_hash VARCHAR(64) NOT NULL,
    byte_size BIGINT NOT NULL,
    row_count BIGINT,
    access_count INTEGER NOT NULL DEFAULT 1,
    pinned BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_accessed TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_materialized_project ON materialized_cache(project_id);
CREATE INDEX idx_materialized_endpoint ON materialized_cache(endpoint_name);
