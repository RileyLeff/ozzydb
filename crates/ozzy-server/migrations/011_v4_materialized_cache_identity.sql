DROP TABLE IF EXISTS materialized_cache;

CREATE TABLE materialized_cache (
    materialized_hash       TEXT PRIMARY KEY,
    project_id              UUID NOT NULL REFERENCES projects(id),
    project_revision_id     UUID NOT NULL REFERENCES v4_project_revisions(id),
    endpoint_name           TEXT NOT NULL,
    node_name               TEXT NOT NULL,
    transform_version_id    UUID NOT NULL REFERENCES v4_transform_versions(id),
    environment_version_id  UUID NOT NULL REFERENCES v4_environment_versions(id),
    params_hash             TEXT NOT NULL,
    input_artifact_bindings JSONB NOT NULL,
    source_hash             TEXT NOT NULL,
    secrets_hash            TEXT,
    output_artifact_id      UUID NOT NULL REFERENCES v4_artifacts(id),
    output_hash             TEXT NOT NULL,
    output_r2_key           TEXT NOT NULL,
    output_content_type     TEXT NOT NULL,
    output_byte_size        BIGINT NOT NULL,
    computed_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_accessed           TIMESTAMPTZ NOT NULL DEFAULT now(),
    access_count            INT NOT NULL DEFAULT 1,
    CHECK (jsonb_typeof(input_artifact_bindings) = 'object')
);

CREATE INDEX idx_cache_project ON materialized_cache (project_id, endpoint_name);
CREATE INDEX idx_cache_accessed ON materialized_cache (last_accessed);
CREATE INDEX idx_cache_transform_version ON materialized_cache (transform_version_id);
