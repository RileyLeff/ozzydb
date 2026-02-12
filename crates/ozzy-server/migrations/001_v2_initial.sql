-- OzzyDB v2 Schema — Clean Slate
-- All v1 tables dropped. This is the single source of truth.

-- ============================================================
-- Users
-- ============================================================
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    github_id       BIGINT UNIQUE,
    username        TEXT NOT NULL UNIQUE,
    display_name    TEXT,
    email           TEXT,
    avatar_url      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- API tokens
-- ============================================================
CREATE TABLE api_tokens (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    scope           TEXT NOT NULL,              -- "account" | "project:{owner}/{slug}"
    project_id      UUID,                       -- NULL for account tokens, set after projects table exists
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ,
    UNIQUE (user_id, name)
);

CREATE INDEX idx_api_tokens_user ON api_tokens (user_id);
CREATE INDEX idx_api_tokens_hash ON api_tokens (token_hash);

-- ============================================================
-- Projects
-- ============================================================
CREATE TABLE projects (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id        UUID NOT NULL REFERENCES users(id),
    slug            TEXT NOT NULL,
    description     TEXT,
    visibility      TEXT NOT NULL DEFAULT 'private' CHECK (visibility IN ('public', 'private')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_id, slug)
);

-- Now add the FK from api_tokens to projects
ALTER TABLE api_tokens
    ADD CONSTRAINT fk_api_tokens_project
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE;

CREATE TABLE project_collaborators (
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL DEFAULT 'read' CHECK (role IN ('read', 'write', 'admin')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id)
);

-- ============================================================
-- Commits (git-referenced)
-- ============================================================
CREATE TABLE commits (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    git_provider    TEXT NOT NULL,              -- "github" | "gitlab"
    git_repo        TEXT NOT NULL,              -- "owner/repo"
    git_commit_sha  TEXT NOT NULL,              -- full 40-char SHA
    ozzy_toml_hash  TEXT NOT NULL,              -- blake3 of ozzy.toml content
    pushed_by       UUID NOT NULL REFERENCES users(id),
    message         TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, git_commit_sha)
);

-- Parsed + cached ozzy.toml content
CREATE TABLE commit_state (
    commit_id       UUID PRIMARY KEY REFERENCES commits(id) ON DELETE CASCADE,
    ozzy_toml_raw   TEXT NOT NULL,
    environments    JSONB NOT NULL,
    transforms      JSONB NOT NULL,
    endpoints       JSONB NOT NULL,
    project_meta    JSONB NOT NULL,
    parsed_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Refs (branches and tags)
CREATE TABLE refs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ref_name        TEXT NOT NULL,
    ref_type        TEXT NOT NULL CHECK (ref_type IN ('branch', 'tag')),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, ref_name)
);

-- ============================================================
-- Data atoms
-- ============================================================
CREATE TABLE data_atoms (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    hash            TEXT NOT NULL,              -- blake3 of raw bytes
    content_type    TEXT NOT NULL,              -- MIME type
    byte_size       BIGINT NOT NULL,
    r2_key          TEXT NOT NULL,              -- key in R2 bucket (data/{hash})
    uploaded_by     UUID NOT NULL REFERENCES users(id),
    yanked          BOOLEAN NOT NULL DEFAULT false,
    yank_reason     TEXT,
    yanked_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

-- Content deduplication across projects
CREATE TABLE content_refs (
    hash            TEXT PRIMARY KEY,
    r2_key          TEXT NOT NULL,
    content_type    TEXT NOT NULL,
    byte_size       BIGINT NOT NULL,
    ref_count       INT NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Data metadata (append-only log)
-- ============================================================
CREATE TABLE data_metadata_log (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    data_atom_id    UUID NOT NULL REFERENCES data_atoms(id) ON DELETE CASCADE,
    field           TEXT NOT NULL,              -- "description", "tags", "license", "schema"
    value           JSONB NOT NULL,
    set_by          UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_metadata_latest
    ON data_metadata_log (data_atom_id, field, created_at DESC);

-- ============================================================
-- Collections
-- ============================================================
CREATE TABLE collections (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    created_by      UUID NOT NULL REFERENCES users(id),
    yanked          BOOLEAN NOT NULL DEFAULT false,
    yank_reason     TEXT,
    yanked_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE TABLE collection_versions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id   UUID NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    version_number  INT NOT NULL,
    hash            TEXT NOT NULL,
    created_by      UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (collection_id, version_number)
);

CREATE TABLE collection_members (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_version_id   UUID NOT NULL REFERENCES collection_versions(id) ON DELETE CASCADE,
    member_type             TEXT NOT NULL CHECK (member_type IN ('data', 'endpoint', 'collection')),
    member_ref              TEXT NOT NULL,
    member_hash             TEXT NOT NULL,
    ordinal                 INT NOT NULL,
    UNIQUE (collection_version_id, ordinal)
);

-- ============================================================
-- Endpoint yanking
-- ============================================================
CREATE TABLE endpoint_yanks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    endpoint_name   TEXT NOT NULL,
    commit_id       UUID NOT NULL REFERENCES commits(id),
    yank_reason     TEXT NOT NULL,
    yanked_by       UUID NOT NULL REFERENCES users(id),
    yanked_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, endpoint_name, commit_id)
);

-- ============================================================
-- Secrets (encrypted, per-project)
-- ============================================================
CREATE TABLE secrets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    encrypted_value BYTEA NOT NULL,
    version_id      UUID NOT NULL DEFAULT gen_random_uuid(),
    set_by          UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

-- ============================================================
-- Environment images
-- ============================================================
CREATE TABLE environment_images (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_hash        TEXT NOT NULL UNIQUE,
    image_ref       TEXT NOT NULL,
    build_type      TEXT NOT NULL CHECK (build_type IN ('base_lockfile', 'dockerfile', 'prebuilt')),
    base_image      TEXT,
    build_log_r2_key TEXT,
    built_at        TIMESTAMPTZ,
    build_duration_ms INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Source cache (cached git tarballs)
-- ============================================================
CREATE TABLE source_cache (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    git_provider    TEXT NOT NULL,
    git_repo        TEXT NOT NULL,
    git_commit_sha  TEXT NOT NULL,
    r2_key          TEXT NOT NULL,
    byte_size       BIGINT NOT NULL,
    cached_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_accessed   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (git_provider, git_repo, git_commit_sha)
);

-- ============================================================
-- Materialized cache (transform outputs)
-- ============================================================
CREATE TABLE materialized_cache (
    materialized_hash   TEXT PRIMARY KEY,
    project_id          UUID NOT NULL REFERENCES projects(id),
    commit_id           UUID NOT NULL REFERENCES commits(id),
    endpoint_name       TEXT NOT NULL,
    node_name           TEXT NOT NULL,
    transform_name      TEXT NOT NULL,
    output_hash         TEXT NOT NULL,
    output_r2_key       TEXT NOT NULL,
    output_content_type TEXT NOT NULL,
    output_byte_size    BIGINT NOT NULL,
    platform            TEXT NOT NULL,
    verification_tier   INT NOT NULL DEFAULT 1,
    computed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_accessed       TIMESTAMPTZ NOT NULL DEFAULT now(),
    access_count        INT NOT NULL DEFAULT 1
);

-- ============================================================
-- GitHub App installations
-- ============================================================
CREATE TABLE github_installations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    installation_id BIGINT NOT NULL UNIQUE,
    account_type    TEXT NOT NULL,              -- "User" or "Organization"
    account_login   TEXT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Indexes
-- ============================================================
CREATE INDEX idx_commits_project     ON commits (project_id, created_at DESC);
CREATE INDEX idx_data_atoms_project  ON data_atoms (project_id);
CREATE INDEX idx_data_atoms_hash     ON data_atoms (hash);
CREATE INDEX idx_collections_project ON collections (project_id);
CREATE INDEX idx_coll_versions       ON collection_versions (collection_id, version_number DESC);
CREATE INDEX idx_coll_members        ON collection_members (collection_version_id);
CREATE INDEX idx_cache_project       ON materialized_cache (project_id, endpoint_name);
CREATE INDEX idx_cache_accessed      ON materialized_cache (last_accessed);
CREATE INDEX idx_refs_project        ON refs (project_id);
CREATE INDEX idx_endpoint_yanks      ON endpoint_yanks (project_id, endpoint_name);
CREATE INDEX idx_gh_installs_login   ON github_installations (account_login);

-- ============================================================
-- Triggers for updated_at
-- ============================================================
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = now();
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER update_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_projects_updated_at
    BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_refs_updated_at
    BEFORE UPDATE ON refs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_secrets_updated_at
    BEFORE UPDATE ON secrets
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_gh_installs_updated_at
    BEFORE UPDATE ON github_installations
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
