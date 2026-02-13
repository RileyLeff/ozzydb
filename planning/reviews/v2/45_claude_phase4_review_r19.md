# Phase 4 Review Round 19 — Claude Opus via Subagent

**Date:** 2026-02-13
**Model:** Claude Opus 4.6 (subagent — Codex rate-limited)
**Commit before:** `9ad73d4`
**Commit after:** `7b88797`
**Tests:** 90 core + 144 server + 44 CLI unit = 278 pass

## Findings (2 total, 2 real bugs fixed)

### Fixed

1. **HIGH — Docker container name mismatch on timeout kill** (`compute/docker.rs`)
   - Container created with `format!("ozzydb-{}", &workspace_id[..8])` (8-char prefix)
   - Timeout branch recreated name with `format!("ozzydb-{}", workspace_id)` (full UUID)
   - `docker kill` targeted wrong name, kill silently failed, container kept running
   - Fix: Reuse the `container_name` variable from line 51 in the timeout branch
   - Bonus: Changed `&workspace_id[..8]` to `.get(..8).unwrap_or()` for safe slicing

2. **MEDIUM — Server lockfile_hash divergence** (`api/v1/fetch.rs`)
   - Server used `""` (empty string) for lockfile_hash in `transform_hash()` computation
   - CLI computed actual `blake3(lockfile_content)` — different hashes for same transform
   - This meant CLI-computed materialized hashes would never match server's
   - Fix: `resolve_environment_image()` now returns `(env_image, env_hash, lockfile_hash)`
   - BaseLockfile: `blake3(lockfile_bytes)`, Prebuilt/Dockerfile: `blake3(b"")`
   - Matches CLI's `compute_lockfile_hash()` behavior exactly
