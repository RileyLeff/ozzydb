# Phase 4 Review Round 17 — Claude Opus via Codex

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 via Codex
**Commit before:** `f22c709`
**Commit after:** `4ca4919`
**Tests:** 90 core + 109 server + 4 CLI unit = 203 pass

## Findings (4 total, 2 real bugs fixed, 2 false positive/design note)

### Fixed

1. **HIGH — CLI `ozzy run` path traversal** (`run.rs`)
   - `source`, `lockfile`, `dockerfile` paths from ozzy.toml were joined with
     cwd without containment checks. Absolute paths or `..` segments could
     escape the project root.
   - Fix: Added `ensure_within_dir()` function that canonicalizes paths and
     verifies they start with the project root. Applied to `compute_source_hash`,
     `compute_env_hash` (both lockfile and Dockerfile tiers).

2. **MEDIUM — Failed env builds stuck in "building" state** (`push.rs`, `queries.rs`)
   - `build_environments_async` inserted a pending row (built_at IS NULL) then
     on build failure only logged the error, leaving the row stuck.
   - Fetch treats built_at IS NULL as "still building" → permanent 503.
   - Re-push of same commit returns early (idempotent), skipping rebuild.
   - Fix: On build failure, delete the pending row so next push retries.
     Added `delete_pending_environment_image()` query (only deletes if
     built_at IS NULL).

### False positive / Design notes

3. **MEDIUM claimed — Non-deterministic params_hash** — False positive (same as
   round 15). `resolve_node_params` builds a `serde_json::Map` (BTreeMap), not
   HashMap. Serialization is deterministic regardless of insertion order.

4. **LOW — Server source_hash uses name:sha instead of content** — Design note.
   Server doesn't have source content at fetch time; `transform_name:commit_sha`
   is an intentional surrogate. Less cache-efficient but not incorrect.
   Added as known limitation #40.

## Known Limitations Updated (40 total)

#40: Server fetch source_hash uses transform_name:commit_sha surrogate (reduces cache reuse across commits with unchanged source)
