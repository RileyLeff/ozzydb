-- Phase 4.2: typed manifest-backed bundle and collection artifacts.
--
-- Keep bundle/collection structure inside v4_artifacts manifests instead of
-- dedicated collection tables. This constraint deliberately validates the
-- outer manifest shape in the database; deeper semantic checks stay in Rust.

ALTER TABLE v4_artifacts
ADD CONSTRAINT chk_v4_artifacts_manifest_shape
CHECK (
    artifact_kind <> 'manifest'
    OR (
        manifest ? 'kind'
        AND jsonb_typeof(manifest->'kind') = 'string'
        AND (
            (
                manifest->>'kind' = 'bundle'
                AND manifest ? 'entries'
                AND jsonb_typeof(manifest->'entries') = 'object'
            )
            OR
            (
                manifest->>'kind' = 'collection'
                AND manifest ? 'items'
                AND jsonb_typeof(manifest->'items') = 'array'
            )
        )
    )
);
