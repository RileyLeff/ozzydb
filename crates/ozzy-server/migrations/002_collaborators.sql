-- Add collaborator table for shared project access.
-- Migration: 002_collaborators.sql

CREATE TABLE project_collaborators (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    permission VARCHAR(20) NOT NULL CHECK (permission IN ('read', 'write', 'admin')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (project_id, user_id)
);

CREATE INDEX idx_project_collaborators_project ON project_collaborators(project_id);
CREATE INDEX idx_project_collaborators_user ON project_collaborators(user_id);
