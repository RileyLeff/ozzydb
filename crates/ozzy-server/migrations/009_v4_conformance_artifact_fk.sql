-- Phase 4.3: conformance is explicitly attached to first-class artifacts.
--
-- The initial v4 registry migration introduced artifact_id on
-- v4_conformance_records but deliberately deferred the foreign key until the
-- artifact model existed as a real persisted primitive. Phase 4.3 closes that
-- gap.

ALTER TABLE v4_conformance_records
ADD CONSTRAINT fk_v4_conformance_records_artifact_id
FOREIGN KEY (artifact_id) REFERENCES v4_artifacts(id) ON DELETE CASCADE;
