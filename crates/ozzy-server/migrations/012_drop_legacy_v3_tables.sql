-- Phase 8 cleanup: remove superseded v3 control-plane and data/collection tables.
-- v4 uses project revisions, first-class artifacts, and conformance records instead.

DROP TABLE IF EXISTS endpoint_yanks;
DROP TABLE IF EXISTS collection_members;
DROP TABLE IF EXISTS collection_versions;
DROP TABLE IF EXISTS collections;
DROP TABLE IF EXISTS data_metadata_log;
DROP TABLE IF EXISTS data_atoms;
DROP TABLE IF EXISTS commit_state;
