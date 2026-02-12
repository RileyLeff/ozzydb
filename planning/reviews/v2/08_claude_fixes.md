# Claude Fixes 08 — Phase 2 Review Round 1 Response

**Commit:** `fa40759`

## MAJOR fixes

### 1. Secret names no longer publicly enumerable
- **File:** `secrets.rs:163-174`
- Changed `list_secrets` from `MaybeAuthUser` + `enforce_read_access` to `AuthUser` + `enforce_write_access`
- Removed unused `MaybeAuthUser` and `enforce_read_access` imports

### 2. Upload atomicity: validate collection before writes
- **File:** `data.rs:249-262`
- Moved collection existence check and yanked check BEFORE storage writes
- If collection doesn't exist or is yanked, error returned before any side effects

### 3. Yanked collection bypass blocked
- **File:** `data.rs:256-261`
- Added explicit `coll.yanked` check in early validation block

### 4. Collection mutations now atomic (prevents lost updates)
- **File:** `queries.rs:952-1094` — two new methods:
  - `add_to_collection_atomically()`: locks row → reads current members → merges (dedup by type+ref) → computes hash → creates version, all in one transaction
  - `remove_from_collection_atomically()`: same pattern for removals
- **File:** `collections.rs:531-534` — `add_members` now calls `add_to_collection_atomically`
- **File:** `collections.rs:620-628` — `remove_members` now calls `remove_from_collection_atomically`
- **File:** `data.rs:319-326` — upload collection path now calls `add_to_collection_atomically`
- Hash computation (`collection_hash`) happens inside the transaction

### 5. Endpoint member type rejected in Phase 2
- **File:** `collections.rs:155-158`
- `resolve_member_hash` now returns error for `member_type="endpoint"`
- Error message: "Endpoint members are not supported yet. Use 'data' or 'collection'."

### 6. r2_key matches actual storage path
- **File:** `content.rs:131-139` — added `storage_key()` public method
- **File:** `data.rs:271-272` — upload uses `state.storage.storage_key(&hash, "bin")` for r2_key

## MINOR fixes

### 7. Download response builder no longer panics
- **File:** `data.rs:486-494`
- Replaced `.unwrap()` with `.map_err(|e| ApiError::Internal(...))`
- Added content_type validation on upload: reject control characters (`data.rs:248-253`)

### 8. Unknown member type prefixes rejected
- **File:** `collections.rs:620-634`
- Validates each removal ref has `type:name` format
- Validates type is one of `["data", "collection"]`, else returns 400

### 9. set_secret.created race (noted)
- Cosmetic issue: concurrent calls may report incorrect `created` flag
- The upsert semantics are correct regardless; only the informational `created` field may be wrong
- Added to review_notes_README.md as intentional tradeoff
