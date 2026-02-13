# Phase 4 Review Round 15 — Claude Opus

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 via Codex
**Commit before:** `c6e2ddd`
**Commit after:** `95628c8`
**Tests:** 90 core + 109 server = 199 pass

## Findings (4 total, 3 real bugs fixed, 1 false positive)

### Fixed

1. **MEDIUM — Collection add fails on members with identical hashes** (`queries.rs`)
   - DB enforces `UNIQUE(collection_version_id, member_hash)` but merge dedup only checked `(type, ref)`
   - Two different refs resolving to same content hash would cause constraint violation
   - Fix: Also deduplicate by `member_hash` before insert

2. **MEDIUM — Python runner generates invalid imports for hyphenated paths** (`runners/python.rs`)
   - `from my-transforms.qc import func` is invalid Python syntax
   - `validate_source_ref` allows hyphens in file paths (valid filenames)
   - Fix: Replaced dotted imports with `importlib.util.spec_from_file_location` for path-based loading

3. **MEDIUM — Project-scoped token creation leaks private project existence** (`auth.rs`)
   - `create_token` resolved project without checking caller's access rights
   - Any authenticated user could mint tokens for arbitrary projects + infer existence via 404
   - Fix: Added owner/collaborator check before token creation, returns 403 if unauthorized

### False positive

4. **HIGH claimed — Non-deterministic params_hash due to HashMap** — False positive.
   `serde_json::Map` is backed by `BTreeMap` (no `preserve_order` feature enabled), which produces
   deterministic sorted-key serialization regardless of HashMap insertion order.

## Known Limitations Updated (still 37)

No new additions this round.
