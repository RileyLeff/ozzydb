# Review 01 — Codex (Phase 1 Milestone)

**Date:** 2026-02-12
**Model:** gpt-5.3-codex (xhigh reasoning)
**Session:** 019c5374-6881-7853-afaf-446b084ba648
**Scope:** Full Phase 1 exhaustive review

## Findings

### `migrations/001_v2_initial.sql`

1. **[MAJOR] Cross-project commit integrity not enforced.** `refs`, `endpoint_yanks`, and `materialized_cache` each store both `project_id` and `commit_id`, but `commit_id` only FKs to `commits(id)`, not to same project. Allows rows pointing to commits from a different project.

2. **[MAJOR] Collections modeled as ordered lists, not sets.** Only `(collection_version_id, ordinal)` is unique, so duplicate members in the same version are allowed. Violates v2 "set of references" semantics.

3. **[MINOR] `api_tokens` allows inconsistent scope/project combinations.** No CHECK that `scope='account'` implies `project_id IS NULL`, or `scope LIKE 'project:%'` implies `project_id IS NOT NULL`.

### `crates/ozzy-core/src/toml_spec.rs`

4. **[MAJOR] Validation Rule 11 (content type compatibility) not implemented.** `validate_endpoints()` handles rules 4–10 but never checks edge source types against destination transform input types.

5. **[MINOR] Endpoint param type validation missing.** Transform param types validated, but endpoint params only validate `binds`; invalid endpoint types pass parse-time validation.

6. **[MINOR] Name validation skips endpoint parameter names.** Rule 1 says names must match `[a-zA-Z0-9_-]+`, but `ep.params` keys are not checked.

7. **[MINOR] Edge source parsing accepts empty refs.** `parse_edge_source()` returns variants for `data:`, `collection:`, `endpoint:` with empty suffixes.

### `crates/ozzy-core/src/hash.rs`

8. **[MAJOR] `materialized_hash()` two-step hashing differs from spec.** It first hashes sorted inputs to `input_hash`, then hashes components. Spec states one hash over concatenated sorted pairs + other components.

9. **[MAJOR] `collection_hash()` doesn't enforce set semantics.** Duplicate member hashes change the result.

### `crates/ozzy-server/src/db/queries.rs`

10. **[MAJOR] `add_collection_members()` non-transactional.** Loop inserts; partial commit on failure.

11. **[MAJOR] Collection version creation and member insertion not atomic.** Failures leave orphan/empty versions.

12. **[MAJOR] Query layer doesn't enforce commit/project consistency.** `upsert_ref`, `insert_endpoint_yank`, `insert_materialized_cache` accept arbitrary cross-project combinations.

13. **[MINOR] `upsert_ref()` ignores `ref_type` on conflict.** Existing row keeps old type.

14. **[MINOR] `create_collection_version()` MAX+1 pattern races under concurrency.**

### `crates/ozzy-server/tests/db_tests.rs`

15. **[MINOR] Tests silently skip if `DATABASE_URL` unset.** False-green in misconfigured CI.

16. **[MINOR] `rand::random::<i64>().abs()` edge case (`i64::MIN`).** Use `.unsigned_abs()` or mask.

17. **[MINOR] Missing negative tests.** No tests for cross-project commit rejection or partial collection atomicity.
