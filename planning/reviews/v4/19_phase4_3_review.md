# Phase 4.3 Review

## Scope

Attach conformance explicitly to first-class artifacts and make persisted
verification semantics match the v4 conformance model.

Files reviewed:

- `crates/ozzy-server/migrations/009_v4_conformance_artifact_fk.sql`
- `crates/ozzy-server/src/db/v4/queries.rs`

## Findings

- Fixed: `v4_conformance_records.artifact_id` now has a real foreign-key
  relationship to `v4_artifacts`.
- Fixed: conformance writes now reject mismatched artifact/type pairs across
  projects instead of silently accepting impossible provenance.
- Fixed: persisted completed verification attempts now update semantic
  conformance status to `verified` or `rejected` instead of leaving rows stuck
  in `declared`.
- Fixed: failed verification attempts now preserve semantic status while still
  updating attempt history and `updated_at`.
- Added direct query surfaces for artifact -> conformance and conformance ->
  attempt inspection.
- No silent fallback behavior introduced.

## Verification

- `cargo test -p ozzy-core`
- `cargo test -p ozzy-types`
- `cargo check -p ozzy-server --tests`

## Result

Conformance is now explicitly and correctly attached to first-class artifacts in
the persisted v4 model. Phase 5 can use artifact-bound conformance as real
execution input/output state instead of treating it as detached bookkeeping.
