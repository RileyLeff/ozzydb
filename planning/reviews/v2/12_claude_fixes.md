# Claude Fixes — Round 3

Commit: `5d2c98f`

## MAJOR 1: Pool deadlock in cycle detection (FIXED)
Refactored `would_create_collection_cycle` to accept `&mut sqlx::PgConnection` (the transaction) instead of using `self.pool`. All DFS queries now run on the same connection holding the advisory lock, eliminating pool exhaustion deadlock risk.

## MAJOR 2, MAJOR 3, MINOR 4: Design decisions documented
These findings were analyzed and determined to be intentional design decisions. Added to `review_notes_README.md`:
- Collection member hashes are point-in-time snapshots
- Upload + collection add are separate operations
- Streaming reads don't verify content hash
