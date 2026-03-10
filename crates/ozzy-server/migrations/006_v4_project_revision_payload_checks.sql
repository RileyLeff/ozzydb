-- Harden v4 project revision payloads.
-- These JSONB columns are runtime control-plane inputs and must always be
-- object-shaped documents.

ALTER TABLE v4_project_revisions
    ADD CONSTRAINT chk_v4_project_revisions_environments_object
        CHECK (jsonb_typeof(environments) = 'object'),
    ADD CONSTRAINT chk_v4_project_revisions_transforms_object
        CHECK (jsonb_typeof(transforms) = 'object'),
    ADD CONSTRAINT chk_v4_project_revisions_endpoints_object
        CHECK (jsonb_typeof(endpoints) = 'object'),
    ADD CONSTRAINT chk_v4_project_revisions_project_meta_object
        CHECK (jsonb_typeof(project_meta) = 'object');
