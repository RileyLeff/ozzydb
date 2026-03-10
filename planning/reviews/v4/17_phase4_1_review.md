# Phase 4.1 Review

## Scope

Introduce the first-class `Artifact` persistence foundation:

- `v4_artifacts`
- `v4_invocation_artifacts`
- Rust models and query helpers for both

## Findings

- Fixed during review: do **not** add the `v4_conformance_records.artifact_id`
  foreign key yet. That belongs in Phase 4.3 once artifact-backed conformance
  is actually migrated onto the new model. Adding it in Phase 4.1 would make
  the migration fail for the wrong reason on any dev DB that already contains
  pre-artifact conformance rows.

## Result

- `Artifact` now exists as a real v4 primitive in the DB/query layer.
- Invocation-to-artifact bindings exist as first-class rows instead of only JSON
  payloads.
- The old `DataAtom` / `Collection` runtime paths are still untouched, which is
  correct for this phase.

## Verification

- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`
