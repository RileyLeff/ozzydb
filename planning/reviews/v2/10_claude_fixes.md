# Claude Fixes — Round 2

Commit: `f270258`

## MAJOR 1: ref_count drift — reorder upload
Reordered `data.rs` upload to insert data atom record BEFORE upserting content_refs. If atom insert fails (e.g., duplicate name), ref_count won't be incremented.

## MAJOR 2: cycle TOCTOU — advisory lock + DB-layer cycle detection
- Added `pg_advisory_xact_lock(hashtext(project_id::text))` to serialize all collection mutations per-project
- Moved `would_create_collection_cycle` from `collections.rs` into `queries.rs` as a private DB method
- Cycle detection runs inside the advisory-locked transaction

## MAJOR 3: yanked TOCTOU — re-check inside transaction
Both `add_to_collection_atomically` and `remove_from_collection_atomically` now re-check `yanked` status under the advisory lock + `FOR UPDATE` row lock.

## CollectionMutResult enum
Introduced `CollectionMutResult<T>` enum with `Ok(T)`, `Yanked(String)`, `CycleDetected(String)` variants. This separates business-logic rejections from internal errors so API handlers can return the correct HTTP status (400/410 vs 500). Updated all callers in `collections.rs` and `data.rs`.

## MINOR 4: verify_content_hash for remote reads
Wrapped remote read hash verification in `if self.verify_content_hash` check in `content.rs`.

## MINOR 5: flatten path includes current collection
Fixed `flatten_collection` to push the current collection name onto the path before emitting leaf atoms.

## NOTE 6: Testing gap
Acknowledged. Additional integration tests for these edge cases will be addressed if flagged as major in subsequent rounds.
