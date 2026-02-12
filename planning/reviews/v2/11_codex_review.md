# Codex Review Round 3 — Phase 2 Post-Fix Verification

Session ID: `019c53be-9fa5-7082-bbd3-a6686b6dd2d2`

## Findings

### MAJOR 1: Pool deadlock in cycle detection (FIXED)
`add_to_collection_atomically` holds advisory lock on tx, then calls cycle detection via `self.pool` (separate connections). Pool saturation under contention → deadlock.
**Fix**: Refactored `would_create_collection_cycle` to use `&mut *tx` for all reads.

### MAJOR 2: Stale child collection hashes (DESIGN DECISION)
Member hashes resolved before advisory lock. Child collection could update concurrently.
**Decision**: Intentional — content-addressed systems record point-in-time snapshots. Parent must be explicitly updated to pick up child's new hash.

### MAJOR 3: Upload + collection add not atomic (DESIGN DECISION)
Atom upload and collection-add are separate operations. If collection is yanked between them, atom persists but 410 is returned.
**Decision**: Acceptable — atom is valid data and exists independently. Full transactional atomicity would require wrapping storage writes + two DB operations in one tx, adding complexity for minimal benefit.

### MINOR 4: Streaming reads don't verify hash (DESIGN DECISION)
`get_stream()` remote branch doesn't verify hash. Hash verification requires consuming entire stream, defeating streaming purpose.
**Decision**: Documented for future hash-verifying stream wrapper if needed.

## Checklist Verification
1. CollectionMutResult caller handling: all correct, no missing match arms
2. Advisory lock pattern: sound after fix (cycle detection now uses tx connection)
3. ref_count ordering: correct (insert-before-upsert)
4. verify_content_hash: respected for full reads, not for streams (documented)
