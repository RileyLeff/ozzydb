-- Phase 4.1: first-class v4 artifacts.
--
-- Artifacts replace the v3 data-atom/collection split as the core persisted
-- data primitive. This migration is additive: it introduces artifact storage
-- and invocation bindings without yet rewriting the legacy data APIs.

CREATE TABLE v4_artifacts (
    id                  UUID PRIMARY KEY DEFAULT uuidv7(),
    project_id          UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    artifact_kind       TEXT NOT NULL CHECK (artifact_kind IN ('blob', 'manifest')),
    content_hash        TEXT REFERENCES content_refs(hash),
    manifest            JSONB,
    source_invocation_id UUID REFERENCES v4_invocations(id) ON DELETE SET NULL,
    created_by          UUID NOT NULL REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, project_id),
    CONSTRAINT chk_v4_artifacts_payload
        CHECK (
            (
                artifact_kind = 'blob'
                AND content_hash IS NOT NULL
                AND manifest IS NULL
            )
            OR
            (
                artifact_kind = 'manifest'
                AND content_hash IS NULL
                AND manifest IS NOT NULL
                AND jsonb_typeof(manifest) = 'object'
            )
        )
);

CREATE INDEX idx_v4_artifacts_project_created_at
    ON v4_artifacts (project_id, created_at DESC);

CREATE INDEX idx_v4_artifacts_content_hash
    ON v4_artifacts (content_hash)
    WHERE content_hash IS NOT NULL;

CREATE INDEX idx_v4_artifacts_source_invocation_id
    ON v4_artifacts (source_invocation_id)
    WHERE source_invocation_id IS NOT NULL;

CREATE TABLE v4_invocation_artifacts (
    invocation_id    UUID NOT NULL REFERENCES v4_invocations(id) ON DELETE CASCADE,
    binding_kind     TEXT NOT NULL CHECK (binding_kind IN ('input', 'output')),
    port_name        TEXT NOT NULL,
    artifact_id      UUID NOT NULL REFERENCES v4_artifacts(id) ON DELETE RESTRICT,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (invocation_id, binding_kind, port_name)
);

CREATE INDEX idx_v4_invocation_artifacts_artifact_id
    ON v4_invocation_artifacts (artifact_id);
