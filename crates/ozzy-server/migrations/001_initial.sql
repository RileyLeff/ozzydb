-- OzzyDB Registry Server - Initial Schema
-- Migration: 001_initial.sql

-- Enable UUID generation
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- ============================================================================
-- Users
-- ============================================================================

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    username VARCHAR(255) UNIQUE NOT NULL,
    email VARCHAR(255),
    github_id BIGINT UNIQUE,
    github_login VARCHAR(255),
    avatar_url TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_users_github_id ON users(github_id);
CREATE INDEX idx_users_username ON users(username);

-- ============================================================================
-- Projects
-- ============================================================================

CREATE TABLE projects (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    owner_user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    slug VARCHAR(255) NOT NULL,
    description TEXT,
    visibility VARCHAR(20) NOT NULL DEFAULT 'private'
        CHECK (visibility IN ('public', 'private', 'org')),
    default_branch VARCHAR(255) NOT NULL DEFAULT 'main',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(owner_user_id, slug)
);

CREATE INDEX idx_projects_owner ON projects(owner_user_id);
CREATE INDEX idx_projects_visibility ON projects(visibility);

-- ============================================================================
-- Commits
-- ============================================================================

CREATE TABLE commits (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    hash VARCHAR(64) NOT NULL,
    parent_hashes TEXT[] NOT NULL DEFAULT '{}',
    author_id UUID REFERENCES users(id) ON DELETE SET NULL,
    author_name VARCHAR(255) NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, hash)
);

CREATE INDEX idx_commits_project ON commits(project_id);
CREATE INDEX idx_commits_hash ON commits(hash);
CREATE INDEX idx_commits_author ON commits(author_id);

-- ============================================================================
-- Data Sources
-- ============================================================================

CREATE TABLE data_sources (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    commit_id UUID NOT NULL REFERENCES commits(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    content_hash VARCHAR(64) NOT NULL,
    schema_hash VARCHAR(64) NOT NULL,
    storage_key VARCHAR(512) NOT NULL,
    schema_json JSONB NOT NULL DEFAULT '{}',
    row_count BIGINT,
    byte_size BIGINT,
    UNIQUE(commit_id, name)
);

CREATE INDEX idx_data_sources_commit ON data_sources(commit_id);
CREATE INDEX idx_data_sources_content_hash ON data_sources(content_hash);

-- ============================================================================
-- Transforms
-- ============================================================================

CREATE TABLE transforms (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    commit_id UUID NOT NULL REFERENCES commits(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    content_hash VARCHAR(64) NOT NULL,
    runtime VARCHAR(50) NOT NULL DEFAULT 'python',
    source_storage_key VARCHAR(512) NOT NULL,
    function_name VARCHAR(255) NOT NULL,
    lockfile_hash VARCHAR(64) NOT NULL,
    params_schema JSONB NOT NULL DEFAULT '{}',
    input_schema JSONB,
    output_schema JSONB,
    reproducible BOOLEAN NOT NULL DEFAULT true,
    UNIQUE(commit_id, name)
);

CREATE INDEX idx_transforms_commit ON transforms(commit_id);
CREATE INDEX idx_transforms_content_hash ON transforms(content_hash);

-- ============================================================================
-- Endpoints
-- ============================================================================

CREATE TABLE endpoints (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    commit_id UUID NOT NULL REFERENCES commits(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    definition JSONB NOT NULL,
    UNIQUE(commit_id, name)
);

CREATE INDEX idx_endpoints_commit ON endpoints(commit_id);

-- ============================================================================
-- Refs (branches and tags)
-- ============================================================================

CREATE TABLE refs (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    ref_type VARCHAR(20) NOT NULL CHECK (ref_type IN ('branch', 'tag')),
    commit_id UUID NOT NULL REFERENCES commits(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(project_id, name, ref_type)
);

CREATE INDEX idx_refs_project ON refs(project_id);
CREATE INDEX idx_refs_commit ON refs(commit_id);

-- ============================================================================
-- API Tokens
-- ============================================================================

CREATE TABLE api_tokens (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    token_hash VARCHAR(64) UNIQUE NOT NULL,
    token_prefix VARCHAR(20) NOT NULL, -- First few chars for identification
    scopes TEXT[] NOT NULL DEFAULT '{}',
    expires_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, name)
);

CREATE INDEX idx_api_tokens_user ON api_tokens(user_id);
CREATE INDEX idx_api_tokens_hash ON api_tokens(token_hash);

-- ============================================================================
-- Content References (for deduplication tracking)
-- ============================================================================

CREATE TABLE content_refs (
    content_hash VARCHAR(64) PRIMARY KEY,
    storage_key VARCHAR(512) NOT NULL,
    content_type VARCHAR(50) NOT NULL, -- 'data' or 'transform'
    byte_size BIGINT NOT NULL,
    ref_count INTEGER NOT NULL DEFAULT 1,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_content_refs_type ON content_refs(content_type);

-- ============================================================================
-- Triggers for updated_at
-- ============================================================================

CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = NOW();
    RETURN NEW;
END;
$$ language 'plpgsql';

CREATE TRIGGER update_users_updated_at
    BEFORE UPDATE ON users
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_projects_updated_at
    BEFORE UPDATE ON projects
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TRIGGER update_refs_updated_at
    BEFORE UPDATE ON refs
    FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
