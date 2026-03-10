# Phase 2.3 Cleanup Review

## Scope

Cleanup pass after the initial Phase 2.3 cutover.

Touched code:

- `crates/ozzy-core/src/toml_spec.rs`
- `crates/ozzy-server/src/api/v1/commits.rs`
- `crates/ozzy-server/src/api/v1/endpoints.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/mod.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/src/registry.rs`
- `crates/ozzy-server/migrations/006_v4_project_revision_payload_checks.sql`

## What Changed

1. Removed `NodeDef.machine` from the authored endpoint schema and made legacy `machine` fields parse-time errors.
2. Removed `machine` from endpoint inspection responses and from orchestrator backend selection. Runtime execution now uses the server-selected default provider.
3. Moved commit detail reads from legacy `commit_state` onto `PublishedProjectRevision`, with no empty-object fallback.
4. Added DB-level `CHECK` constraints requiring `v4_project_revisions.environments`, `transforms`, `endpoints`, and `project_meta` to be JSON objects.
5. Renamed snapshot-binding errors to refer to published project revisions instead of commit-state terminology.
6. Added a DB-backed regression test proving non-object project-revision payloads are rejected.

## Findings

### Blocking

None.

### Non-blocking

1. `commit_state` still exists in the codebase for push/publication and legacy commit queries outside the runtime cutover.
   - This is expected Phase 3 work.
   - The runtime control path no longer depends on it.

## Verification

Executed:

- `cargo test -p ozzy-core`
- `cargo check -p ozzy-server --tests`
- `cargo test -p ozzy-types`

Notes:

- `cargo check -p ozzy-server --tests` completed successfully with pre-existing warning noise in server test files unrelated to this cleanup.

## Exit Assessment

This cleanup resolves the remaining Phase 2.3 issues identified before Phase 3.1:

- no user-facing `machine` field in the live v4 control plane
- no commit-detail fallback to `commit_state`
- DB-level shape checks for published project revision payloads
- no lingering commit-state terminology in the new snapshot binding errors

Phase 3.1 can proceed.
