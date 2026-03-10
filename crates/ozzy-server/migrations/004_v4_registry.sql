-- OzzyDB v4 registry foundation.
-- This migration is additive. It introduces the first-class registry object
-- model alongside the existing v2/v3 control plane so the server can move off
-- commit_state JSON execution incrementally.

-- ============================================================
-- v4 Canonical types
-- ============================================================

CREATE TABLE v4_canonical_types (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    canonical_key   TEXT NOT NULL UNIQUE,
    expr            JSONB NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- v4 Versioned registry objects
-- ============================================================

CREATE TABLE v4_type_versions (
    id                  UUID PRIMARY KEY DEFAULT uuidv7(),
    project_id          UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name                TEXT NOT NULL,
    version             TEXT NOT NULL,
    canonical_type_id   UUID NOT NULL REFERENCES v4_canonical_types(id),
    expr                JSONB NOT NULL,
    published_by        UUID NOT NULL REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, project_id),
    UNIQUE (project_id, name, version)
);

CREATE INDEX idx_v4_type_versions_project_name_version
    ON v4_type_versions (project_id, name, version);

CREATE INDEX idx_v4_type_versions_canonical_type_id
    ON v4_type_versions (canonical_type_id);

CREATE TABLE v4_environment_versions (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    version         TEXT NOT NULL,
    definition      JSONB NOT NULL,
    published_by    UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, project_id),
    UNIQUE (project_id, name, version)
);

CREATE INDEX idx_v4_environment_versions_project_name_version
    ON v4_environment_versions (project_id, name, version);

CREATE TABLE v4_transform_versions (
    id                      UUID PRIMARY KEY DEFAULT uuidv7(),
    project_id              UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name                    TEXT NOT NULL,
    version                 TEXT NOT NULL,
    environment_version_id  UUID NOT NULL REFERENCES v4_environment_versions(id),
    source_ref              TEXT,
    command                 TEXT,
    description             TEXT,
    params_schema           JSONB NOT NULL DEFAULT '{}'::jsonb,
    network_access          BOOLEAN NOT NULL DEFAULT false,
    secrets                 TEXT[] NOT NULL DEFAULT '{}'::text[],
    published_by            UUID NOT NULL REFERENCES users(id),
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_v4_transform_versions_implementation
        CHECK (num_nonnulls(source_ref, command) = 1),
    UNIQUE (id, project_id),
    UNIQUE (project_id, name, version)
);

CREATE INDEX idx_v4_transform_versions_project_name_version
    ON v4_transform_versions (project_id, name, version);

CREATE INDEX idx_v4_transform_versions_environment_version_id
    ON v4_transform_versions (environment_version_id);

CREATE TABLE v4_transform_ports (
    id                      UUID PRIMARY KEY DEFAULT uuidv7(),
    transform_version_id    UUID NOT NULL REFERENCES v4_transform_versions(id) ON DELETE CASCADE,
    port_kind               TEXT NOT NULL CHECK (port_kind IN ('input', 'output')),
    port_name               TEXT NOT NULL,
    type_version_id         UUID NOT NULL REFERENCES v4_type_versions(id),
    description             TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (transform_version_id, port_kind, port_name)
);

CREATE INDEX idx_v4_transform_ports_transform_kind_name
    ON v4_transform_ports (transform_version_id, port_kind, port_name);

CREATE INDEX idx_v4_transform_ports_type_version_id
    ON v4_transform_ports (type_version_id);

-- ============================================================
-- v4 Registry revisions and memberships
-- ============================================================

CREATE TABLE v4_registry_revisions (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    created_by      UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, project_id)
);

CREATE INDEX idx_v4_registry_revisions_project_created_at
    ON v4_registry_revisions (project_id, created_at DESC);

CREATE TABLE v4_registry_revision_type_versions (
    registry_revision_id UUID NOT NULL REFERENCES v4_registry_revisions(id) ON DELETE CASCADE,
    type_version_id      UUID NOT NULL REFERENCES v4_type_versions(id),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (registry_revision_id, type_version_id)
);

CREATE INDEX idx_v4_registry_revision_type_versions_type_version_id
    ON v4_registry_revision_type_versions (type_version_id);

CREATE TABLE v4_registry_revision_environment_versions (
    registry_revision_id UUID NOT NULL REFERENCES v4_registry_revisions(id) ON DELETE CASCADE,
    environment_version_id UUID NOT NULL REFERENCES v4_environment_versions(id),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (registry_revision_id, environment_version_id)
);

CREATE INDEX idx_v4_registry_revision_environment_versions_environment_version_id
    ON v4_registry_revision_environment_versions (environment_version_id);

CREATE TABLE v4_registry_revision_transform_versions (
    registry_revision_id UUID NOT NULL REFERENCES v4_registry_revisions(id) ON DELETE CASCADE,
    transform_version_id UUID NOT NULL REFERENCES v4_transform_versions(id),
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (registry_revision_id, transform_version_id)
);

CREATE INDEX idx_v4_registry_revision_transform_versions_transform_version_id
    ON v4_registry_revision_transform_versions (transform_version_id);

-- ============================================================
-- v4 Project revisions
-- ============================================================

CREATE TABLE v4_project_revisions (
    id                  UUID PRIMARY KEY DEFAULT uuidv7(),
    project_id          UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_commit_id    UUID NOT NULL UNIQUE,
    registry_revision_id UUID NOT NULL REFERENCES v4_registry_revisions(id),
    ozzy_toml_hash      TEXT NOT NULL,
    ozzy_toml_raw       TEXT NOT NULL,
    created_by          UUID NOT NULL REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY (source_commit_id, project_id) REFERENCES commits(id, project_id),
    FOREIGN KEY (registry_revision_id, project_id) REFERENCES v4_registry_revisions(id, project_id)
);

CREATE INDEX idx_v4_project_revisions_project_created_at
    ON v4_project_revisions (project_id, created_at DESC);

CREATE INDEX idx_v4_project_revisions_registry_revision_id
    ON v4_project_revisions (registry_revision_id);

-- ============================================================
-- v4 Invocations
-- ============================================================

CREATE TABLE v4_invocations (
    id                  UUID PRIMARY KEY DEFAULT uuidv7(),
    project_revision_id UUID NOT NULL REFERENCES v4_project_revisions(id) ON DELETE CASCADE,
    transform_version_id UUID NOT NULL REFERENCES v4_transform_versions(id),
    endpoint_name       TEXT,
    node_name           TEXT,
    params              JSONB NOT NULL DEFAULT '{}'::jsonb,
    params_hash         TEXT NOT NULL,
    input_bindings      JSONB NOT NULL DEFAULT '{}'::jsonb,
    output_bindings     JSONB NOT NULL DEFAULT '{}'::jsonb,
    status              TEXT NOT NULL CHECK (
        status IN ('queued', 'running', 'succeeded', 'failed', 'canceled')
    ),
    error_message       TEXT,
    created_by          UUID REFERENCES users(id),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at          TIMESTAMPTZ,
    completed_at        TIMESTAMPTZ,
    CONSTRAINT chk_v4_invocations_started_after_created
        CHECK (started_at IS NULL OR started_at >= created_at),
    CONSTRAINT chk_v4_invocations_completed_after_started
        CHECK (
            completed_at IS NULL
            OR (started_at IS NOT NULL AND completed_at >= started_at)
        )
);

CREATE INDEX idx_v4_invocations_project_revision_status_created_at
    ON v4_invocations (project_revision_id, status, created_at DESC);

CREATE INDEX idx_v4_invocations_transform_version_id
    ON v4_invocations (transform_version_id);

-- ============================================================
-- v4 Conformance and verification attempts
-- ============================================================

CREATE TABLE v4_conformance_records (
    id              UUID PRIMARY KEY DEFAULT uuidv7(),
    artifact_id     UUID NOT NULL,
    type_version_id UUID NOT NULL REFERENCES v4_type_versions(id),
    status          TEXT NOT NULL CHECK (status IN ('declared', 'verified', 'rejected')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (artifact_id, type_version_id)
);

CREATE INDEX idx_v4_conformance_records_artifact_id
    ON v4_conformance_records (artifact_id);

CREATE INDEX idx_v4_conformance_records_type_version_id
    ON v4_conformance_records (type_version_id);

CREATE TABLE v4_verification_attempts (
    id                      UUID PRIMARY KEY DEFAULT uuidv7(),
    conformance_record_id   UUID NOT NULL REFERENCES v4_conformance_records(id) ON DELETE CASCADE,
    verifier                TEXT NOT NULL,
    attempt_kind            TEXT NOT NULL CHECK (attempt_kind IN ('completed', 'failed')),
    verdict                 TEXT CHECK (verdict IN ('verified', 'rejected')),
    diagnostics             JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence                JSONB,
    failure_error           TEXT,
    created_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT chk_v4_verification_attempt_shape
        CHECK (
            (attempt_kind = 'completed' AND verdict IS NOT NULL AND failure_error IS NULL)
            OR
            (attempt_kind = 'failed' AND verdict IS NULL AND failure_error IS NOT NULL)
        )
);

CREATE INDEX idx_v4_verification_attempts_conformance_created_at
    ON v4_verification_attempts (conformance_record_id, created_at DESC);
