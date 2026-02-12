# Codex Review 07 — Phase 2 Data Plane Exhaustive Review Round 1

**Model:** gpt-5.3-codex (xhigh reasoning)
**Scope:** Full Phase 2 codebase — data.rs, collections.rs, secrets.rs, queries.rs, content.rs
**Token count:** ~172k

## Findings

### MAJOR

1. **Secret names publicly enumerable on public projects** — `list_secrets` used `MaybeAuthUser` + `enforce_read_access`, allowing anonymous access on public projects.
2. **Non-atomic upload** — Storage write + content_refs upsert + data_atoms insert happened before collection validation; failures returned error but earlier writes persisted.
3. **Yanked collections bypass via upload --collection** — `upload_data` path that appends to a collection did not check `coll.yanked`.
4. **Collection version mutations vulnerable to lost updates** — Current membership was read outside transaction, then version insert done with precomputed members. Concurrent add/remove requests could overwrite each other.
5. **Endpoint collection members not content-addressed** — `member_hash` was set to endpoint ref string, not resolved materialized hash; existence not validated; flatten skipped them.
6. **Persisted r2_key doesn't match actual storage path** — DB stored `data/{hash}` but storage writes under `content/{h0h1}/{h2h3}/{hash}.bin`.

### MINOR

7. **Unvalidated content_type can trigger response build panic** — User-supplied content_type accepted verbatim; `Response::builder(...).unwrap()` in download handler.
8. **remove_members accepts invalid member type prefixes silently** — Any `type:ref` string accepted; unknown types not rejected.
9. **set_secret.created flag is race-prone** — `created` derived from pre-upsert existence check.

### NOTE

10. **Important regression paths untested** — No tests for upload atomicity, concurrent collection mutation, or public secret exposure.

## Reviewer Assumptions/Questions

1. Should secret metadata (names/version IDs) be private regardless of project visibility? → Yes, changed to write-access-only.
2. If endpoint-member support is intentionally deferred to Phase 3+, API should reject `member_type="endpoint"` in Phase 2 instead of accepting placeholder behavior. → Agreed, rejected.
