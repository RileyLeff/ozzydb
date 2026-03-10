-- Phase 2.3: make v4_project_revisions the authored runtime control object.
--
-- These payloads mirror the authored declarations for the published commit so
-- runtime paths no longer need to consult commit_state once a v4 project
-- revision exists.

ALTER TABLE v4_project_revisions
    ADD COLUMN environments JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN transforms JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN endpoints JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN project_meta JSONB NOT NULL DEFAULT '{}'::jsonb;
