# Gemini Review Round 4 — Phase 2 Post-Fix Verification

(Codex hit rate limit; switched to Gemini for this round)

## Findings

### MAJOR 1-3: Memory buffering / streaming (KNOWN LIMITATION)
Upload reads entire file into memory, download returns full content, local get_stream reads whole file.
**Decision**: Body size limit (100MB default via DefaultBodyLimit) prevents OOM. Streaming upload/download is a future architectural enhancement beyond Phase 2 scope.

### MAJOR 4: Yanked bypass in resolve_member_hash (FIXED)
`resolve_member_hash` didn't check yanked status on data atoms or collections. Could add yanked content to collections.
**Fix**: Added yanked checks returning 410 Gone.

### MAJOR 5: Yanked content in flatten (FIXED)
`flatten_collection` recursively resolved without checking yanked status. Consumers would get hashes for unavailable data.
**Fix**: `flatten_collection` now skips yanked collections entirely and checks yanked status on individual data atoms before including them.

### MINOR 1: Ordinal gaps (FIXED)
Using enumerate index caused gaps when duplicate members were skipped.
**Fix**: Use separate counter that only increments on actual push.

### MINOR 2: N+1 queries in collection listing (NOTED)
`list_collections` does 2 queries per collection. Performance optimization for later.

### MINOR 3: Cross-platform path in filename_stem (NOTED)
Uses `rsplit('/')` — could break with Windows paths. Edge case, low priority.

### MINOR 4: Redundant create_collection_version_with_members (NOTED)
Used by db_tests.rs for direct testing. Not redundant, just has a narrower use case.

## Positive Notes
- Advisory locks pattern is sound
- Content-type control character validation is good
- Cycle detection correctly uses transaction connection
- Hash verification for non-streaming reads is correct
