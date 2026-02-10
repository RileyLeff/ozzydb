# OzzyDB Code Review (Round 8 — Combined)

Date: February 7, 2026

Scope: Full codebase review by 3 models in parallel (gpt-5.3-codex xhigh, Gemini 3 Pro, Claude Opus 4.6). Findings deduplicated and merged below.

## Deduplicated Findings (Ordered by Severity)

### 1. High: `ApiError::from(anyhow::Error)` infers HTTP status from error message substrings

**Found by:** Gemini (#5), Opus (#1)

Server maps anyhow errors to HTTP status codes by substring matching ("not found" → 404, "invalid" → 400, etc.). Internal errors containing these words get misclassified.

Evidence: `crates/ozzy-server/src/api/v1/auth.rs:236-258`

Fix: Replace with typed error enum using `thiserror`.

### 2. High: JSON number canonicalization can produce hash collisions

**Found by:** Codex (#1)

`canonicalize_json` trims trailing `0` from all float renderings including exponent forms. Distinct numeric inputs can canonicalize to the same string.

Evidence: `crates/ozzy-core/src/canon.rs:124-127`

Fix: Only trim fractional trailing zeros when decimal point exists and no exponent present.

### 3. High: Duplicated access control logic between `push_pull.rs` and `projects.rs`

**Found by:** Opus (#2)

`collaborator_allows`, `user_has_project_permission`, and `enforce_read_access` are copy-pasted identically in both files. If one is updated, the other drifts.

Evidence: `push_pull.rs:66-120`, `projects.rs:66-122`

Fix: Extract to shared `api/v1/access.rs` module.

### 4. High: Push/pull are shallow — no recursive history transfer

**Found by:** Gemini (#1)

`ozzy push` only uploads HEAD commit, `ozzy pull` only downloads the requested ref's commit. Parent commits are not transferred, so `ozzy log` breaks on freshly pulled clones.

Evidence: `push.rs:45`, `pull.rs:260`, `push_pull.rs:205`

Fix: Implement recursive history discovery (push missing ancestors, pull until common ancestor). NOTE: This is a significant feature — defer to NEXT_STEPS if too large for a bugfix round.

### 5. Medium: Non-deterministic schema merge in multi-input endpoint validation

**Found by:** Gemini (#2), Codex (#2 — pull dirty-check variant)

When validating a multi-input pipeline, schemas are merged via `HashMap` iteration which is non-deterministic. Column type collisions are resolved arbitrarily.

Evidence: `crates/ozzy-cli/src/commands/endpoint.rs:312-324`

Fix: Use `BTreeMap` for schema merging; explicitly error on column type collisions.

### 6. Medium: Synchronous filesystem IO blocks Tokio executor in server

**Found by:** Gemini (#3)

`ContentStorage` uses `std::fs` (read, write, exists, remove_file) inside async handlers. Under load, this starves the Tokio thread pool.

Evidence: `crates/ozzy-server/src/storage/content.rs:165,141,218`

Fix: Switch to `tokio::fs` or wrap in `spawn_blocking`. NOTE: Defer to streaming/compute step.

### 7. Medium: Path sanitizer validates before backslash normalization

**Found by:** Codex (#4)

`sanitize_relative_path` validates components first, then replaces `\` with `/`. Strings like `transforms\\..\\x.py` pass validation then become traversal paths.

Evidence: `crates/ozzy-server/src/api/v1/push_pull.rs:151-167`

Fix: Normalize separators before validation.

### 8. Medium: `tag delete` and `tag show` skip `validate_safe_name`

**Found by:** Opus (#3)

`tag create` validates but `tag rm` and `tag show` don't. Path traversal via crafted tag names.

Evidence: `crates/ozzy-cli/src/commands/tag.rs:84,101`

Fix: Add `validate_safe_name(name)?` to both functions.

### 9. Medium: `github_poll` creates duplicate cli-session tokens (UNIQUE violation)

**Found by:** Opus (#6)

Second login attempt fails with 500 because `api_tokens` has `UNIQUE(user_id, name)` and the old "cli-session" token still exists.

Evidence: `auth.rs:87-98`, `001_initial.sql:155`

Fix: Delete old cli-session token before creating new one, or use `ON CONFLICT DO UPDATE`.

### 10. Medium: Commit hash computation duplicated in 3 places

**Found by:** Opus (#7)

CLI `commit.rs`, core `create_commit`, and server `expected_commit_hash` all independently compute commit hashes. Serialization differences could cause hash divergence.

Evidence: `commit.rs:52-62`, `commit.rs:42-51`, `push_pull.rs:52-63`

Fix: Centralize in `ozzy-core` as a single function.

### 11. Medium: `get_stream` skips BLAKE3 integrity check

**Found by:** Opus (#5)

`get()` verifies content hash but `get_stream()` does not. Corrupted content served silently.

Evidence: `content.rs:240-266`

Fix: Add hash verification (read-verify-stream, or streaming hash checker).

### 12. Medium: `LocalCache::remove` inflates access stats via `self.get()`

**Found by:** Opus (#4)

`remove()` calls `get()` which increments access_count and updates last_accessed, distorting LRU eviction ordering.

Evidence: `cache/local.rs:292-299`

Fix: Add `get_path_only` method that skips access stat updates.

### 13. Medium: Tag/branch name collision in pull resolution

**Found by:** Codex (#3)

CLI strips `refs/tags/` before request; server resolves bare ref names branch-first. `--ref refs/tags/v1.0` may pull the wrong commit if a branch `v1.0` exists.

Evidence: `pull.rs:247-248`, `queries.rs:555-570`

Fix: Preserve ref type end-to-end in API requests.

### 14. Medium: N+1 query in `GET /refs` API

**Found by:** Gemini (#6)

Ref listing fetches names, then executes a separate query per ref for commit hash.

Evidence: `projects.rs:410-414`

Fix: Use SQL JOIN between refs and commits tables.

### 15. Low: Zero-transform endpoints allowed at creation time

**Found by:** Codex (#5)

CLI accepts empty `--transforms`, `endpoint create` doesn't reject it. Fails at `run` time.

Evidence: `main.rs:273`, `endpoint.rs:59,171`

Fix: Require at least one transform in endpoint creation.

### 16. Low: Redundant `format_size` in data.rs

**Found by:** Opus (#9)

Duplicates `ozzy_core::cache::format_size` with slightly different precision.

Evidence: `data.rs:122-136` vs `local.rs:350-364`

Fix: Use shared function.

### 17. Low: SQLite cache opens new connection per operation

**Found by:** Opus (#10)

No connection pooling. Every get/put/contains opens and closes a connection.

Evidence: `cache/local.rs:97-99`

Fix: Store `Mutex<Connection>` in `LocalCache` struct.

### 18. Low: Test temp directory leaks and DB test pollution

**Found by:** Opus (#8, #11), Gemini (#9 — SVG i32 overflow)

Various test hygiene issues: `into_path()` prevents cleanup, DB tests don't rollback.

Fix: Use `TempDir` handle properly; wrap DB tests in transactions.

### 19. Low: Large DAG responses, redundant push transfers, HashMap in endpoint validation

**Found by:** Gemini (#7, #8), Opus (#12)

Minor efficiency/cosmetic issues: DAG embeds full schemas, push sends duplicate files, non-deterministic warning message ordering.

Fix: Return schema hashes in DAG; client-side content dedup; use BTreeMap.

### 20. Low: Tag deletions not synced to registry, nested dict type parsing

**Found by:** Gemini (#10, #11)

Stale tags accumulate on registry; schema parser fails on nested dict types.

Fix: Add tag deletion to push protocol; use recursive type parser.

---

## Summary

20 deduplicated findings from 3 independent reviewers (28 raw findings, 8 overlaps).

**Overlap analysis:**
- ApiError string matching: found by Gemini + Opus (high confidence — 2 independent reviewers)
- Schema non-determinism: found by Gemini + Codex (variants)
- No contradictions between reviewers

**Actionable now (quick fixes):** #2, #3, #5, #7, #8, #9, #10, #12, #13, #15, #16
**Defer to streaming/compute step:** #4, #6, #14
**Test hygiene (batch later):** #17, #18, #19, #20

**Severity trend:** No critical findings. Decreasing severity across rounds. Codebase is stabilizing.
