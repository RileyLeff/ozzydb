# Claude Fixes — Round 4

Commit: `a2b87bf`

## MAJOR 4: Yanked bypass in resolve_member_hash (FIXED)
Added `yanked` checks in `resolve_member_hash` for both data atoms and collections. Returns `ApiError::gone` (410) if the member is yanked, preventing yanked content from being added to collections.

## MAJOR 5: Yanked content in flatten (FIXED)
- `flatten_collection` now returns empty Vec for yanked collections (skips entire subtree)
- Individual data atoms checked via `get_data_atom` — yanked atoms are skipped with `continue`

## MINOR 1: Ordinal gaps (FIXED)
Replaced `enumerate()` index with a separate `next_ordinal` counter that only increments when a member is actually added. No more gaps from skipped duplicates.

## MAJORs 1-3: Memory buffering / streaming (KNOWN LIMITATION)
Documented as Phase 2 known limitation. 100MB DefaultBodyLimit prevents OOM. Streaming upload/download deferred to future work.

## MINORs 2-4: Noted for future
- N+1 query optimization for collection listing
- Cross-platform path handling in filename_stem
- create_collection_version_with_members kept for db_tests
