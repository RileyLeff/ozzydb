# Phase 4.2 Review

## Scope

Replace the dedicated collection ontology in the v4 model with typed
bundle/collection artifacts backed by `v4_artifacts` manifests.

Files reviewed:

- `crates/ozzy-core/src/artifacts.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/migrations/008_v4_artifact_manifest_checks.sql`

## Findings

- Fixed: manifest-backed bundle/collection structure is no longer raw JSON in
  the v4 write path. New code publishes typed manifests and decodes them
  explicitly.
- Fixed: manifest artifacts now fail fast if they reference missing artifacts or
  artifacts outside the current project.
- Fixed: the database now enforces the outer manifest shape for bundle and
  collection artifacts instead of accepting any JSON object.
- No silent fallback behavior introduced. Invalid manifest payloads now error
  explicitly in Rust or at the database boundary.

## Verification

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

## Result

The v4 artifact model now has a typed representation for bundle and collection
structure. The old collection subsystem still exists in the legacy API/runtime
surface, but it is no longer the direction of the new core model. Phase 4.3 can
now attach conformance to first-class artifacts instead of special-casing data
atoms versus collections.
