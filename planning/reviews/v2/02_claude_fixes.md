# Fixes for Review 01 — Codex (Phase 1 Milestone)

**Date:** 2026-02-12

## MAJOR Fixes

### #1: Cross-project commit integrity
Added `UNIQUE (id, project_id)` to commits table, then composite FKs from `refs`, `endpoint_yanks`, and `materialized_cache` referencing `(commit_id, project_id) → commits(id, project_id)`. DB now rejects mismatched project/commit pairs at the constraint level.

### #2: Collection duplicate members
Added `UNIQUE (collection_version_id, member_hash)` constraint to `collection_members` table. Set semantics now enforced at DB level.

### #8: materialized_hash two-step hashing
Changed from two-step (hash inputs, then hash result with other components) to single-pass: `blake3(name1\0hash1\0name2\0hash2\0...\0transform\0params\0platform[\0secrets])`. Updated golden value tests.

### #9: collection_hash dedup
Added `sorted.dedup()` after sort in `collection_hash()`. Duplicate member hashes no longer change the result. Added `test_collection_hash_dedup` test.

### #10/#11: Collection atomicity
Replaced `create_collection_version()` + `add_collection_members()` with single `create_collection_version_with_members()` that runs everything in a transaction. Added `SELECT FOR UPDATE` on the collection row to prevent version number races under concurrency.

### #12: Commit/project consistency in query layer
Now enforced at DB level via composite FKs (#1 above). Application-layer callers can no longer create inconsistent state even if they pass wrong project_ids.

## MINOR Fixes

### #3: api_tokens scope/project CHECK
Added `CHECK ((scope = 'account' AND project_id IS NULL) OR (scope <> 'account' AND project_id IS NOT NULL))` constraint to `api_tokens` table.

### #6: Endpoint param name validation
Added name validation for `ep.params` keys in `validate_names()`.

### #7: Empty edge source refs
Added check for empty refs after `parse_edge_source()`. Empty prefixed refs like `data:`, `collection:`, `endpoint:` now produce a validation error.

### #13: upsert_ref ignores ref_type
Added `ref_type = EXCLUDED.ref_type` to the ON CONFLICT UPDATE clause.

### #14: create_collection_version race
Addressed by `SELECT FOR UPDATE` lock on the collection row within the transaction (#10/#11 above).

### #16: i64::MIN abs() edge case
Changed `rand::random::<i64>().abs()` to `(rand::random::<i64>() & i64::MAX)` in all test files.

## Intentionally Deferred

### #4: Rule 11 (content type compatibility) — NOTE
Content type compatibility between edge sources and transform inputs requires runtime data (DB lookups for data atom/collection types). Cannot be checked at TOML parse time. Will be validated at fetch/run time in Phase 4.

### #5: Endpoint param type validation — NOTE
Endpoint params use string `type_` field. Invalid types will be caught at runtime when validating consumer parameters in the fetch endpoint (Phase 4). Not a parse-time concern.

### #15: Tests skip silently if DATABASE_URL unset — NOTE
This is by design — the DB tests run against a real Postgres and are skipped in environments without one. CI configuration should ensure DATABASE_URL is set.

### #17: Missing negative tests — NOTE
Will add negative tests (cross-project rejection, atomicity failures) in Phase 2+ as the API endpoints that exercise these paths are built.
