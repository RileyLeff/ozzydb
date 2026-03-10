# Phase 2.3 Review

## Scope

Phase 2.3 makes `v4_project_revisions` the server-visible replacement for "what a pushed commit means".

Touched code:

- `crates/ozzy-server/migrations/005_v4_project_revision_payloads.sql`
- `crates/ozzy-server/src/db/v4/models.rs`
- `crates/ozzy-server/src/db/v4/queries.rs`
- `crates/ozzy-server/src/registry.rs`
- `crates/ozzy-server/src/api/v1/fetch.rs`
- `crates/ozzy-server/src/compute/orchestrator.rs`
- `crates/ozzy-server/src/api/v1/endpoints.rs`

## What Changed

1. `v4_project_revisions` now persists the authored runtime payloads needed to interpret a published commit:
   - `environments`
   - `transforms`
   - `endpoints`
   - `project_meta`
2. Added `PublishedProjectRevision` as the server-side object that combines:
   - the stored project revision row
   - the pinned `RegistrySnapshot`
   - bound runtime definitions
   - published endpoint definitions
3. `fetch`, `compute::orchestrator`, and endpoint inspection now resolve through `load_published_project_revision_by_commit(...)` instead of reading `commit_state` directly.
4. Added test coverage proving the new project-revision payloads round-trip through the DB layer and can be loaded into a published runtime object.

## Findings

### Blocking

None.

### Non-blocking

1. `push` still does not publish these payloads yet.
   - That means the runtime object model is now defined and consumed, but end-to-end publication of `PublishedProjectRevision` remains Phase 3 work.
   - This is expected and matches the implementation plan.

2. The existing `load_project_revision_snapshot_by_commit(...)` helper still exists beside `load_published_project_revision_by_commit(...)`.
   - This is acceptable.
   - It remains useful for lower-level snapshot tests and does not reintroduce `commit_state` into runtime paths.

## Verification

Executed:

- `cargo check -p ozzy-server`
- `cargo test -p ozzy-types`

Attempted:

- `cargo test -p ozzy-server --no-run`

The `--no-run` build progressed through compilation and only surfaced unrelated pre-existing warning noise in server tests before the link phase became a poor checkpoint signal in this harness. I did not use it as the primary gate.

## Exit Assessment

Phase 2.3 is complete enough to proceed.

`v4_project_revisions` is now the server-visible control object for runtime reads. The remaining gap is publication, not runtime semantics.
