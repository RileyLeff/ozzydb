# Phase 8.1 Review

Date: 2026-03-11
Phase: 8.1 — Delete superseded v3 code

## Scope reviewed
- `crates/ozzy-server/src/db/models.rs`
- `crates/ozzy-server/src/db/queries.rs`
- `crates/ozzy-server/src/api/v1/push.rs`
- `crates/ozzy-server/src/api/v1/commits.rs`
- `crates/ozzy-cli/src/commands/shared.rs`
- `crates/ozzy-cli/src/commands/push.rs`
- `crates/ozzy-cli/src/commands/init.rs`
- `crates/ozzy-server/migrations/012_drop_legacy_v3_tables.sql`
- deleted dead files:
  - `crates/ozzy-server/src/api/v1/data.rs`
  - `crates/ozzy-server/src/api/v1/collections.rs`
  - `crates/ozzy-server/tests/integration_tests.rs`
  - `crates/ozzy-server/tests/e2e_tests.rs`

## Summary
- Removed dead server files for the old data/collection API surface.
- Removed the unused `commit_state`, data atom, data metadata, collection, and endpoint-yank DB query/model surface.
- Added a schema cleanup migration dropping the superseded v3 tables.
- Collapsed the public push surface to GitHub-only instead of carrying a fake `git_provider` abstraction.
- Trimmed stale DB test sections that only exercised removed v3 codepaths.

## Findings
- No blocking findings from the self-review.
- The legacy frontend still references old data/collection routes and `git_provider`, but frontend remains explicitly deferred in v4 and was not touched in this phase.
- `db_tests` still has pre-existing `unused_parens` warning noise; that is cleanup-only and does not affect semantic correctness.

## Verification
- `cargo fmt`
- `cargo check -p ozzy-server --tests`

## Notes
- This pass intentionally deletes old surfaces instead of preserving compatibility shims.
- Internal DB rows still retain `git_provider = 'github'` as stored metadata, but the public/client/server control path no longer accepts arbitrary providers.
