# Opus Review 19 -- Round 4

## HIGH

### H1: `endpoint rm` of staged override resurrects committed version
**File:** `crates/ozzy-cli/src/commands/endpoint.rs:509-518`
When a staged `.json` overrides a committed endpoint, `rm` removes the `.json` and its `.deleted` marker (if any), but doesn't check whether the endpoint also exists in the latest commit. The committed version silently reappears on next commit.
**Fix:** After removing the staged `.json`, check if the endpoint exists in the latest commit and create a `.deleted` marker if so.

### H2: Unchecked hash slice indexing can panic on short/malformed strings
**Files:** `status.rs:19`, `log.rs:19`, `commit.rs:77`, `push.rs:163`, `shared.rs:505,509`, `runtime.rs:243`
Code uses `&hash[..8]` or `&hash[..12]` without bounds checking. A malformed server response or corrupted commit produces a panic instead of a clean error.
**Fix:** Use `hash.get(..N).unwrap_or(&hash)` consistently everywhere.

### H3: `hash_source_directory` follows symlinks
**File:** `crates/ozzy-core/src/canon.rs:39`
`WalkDir::new(dir)` follows symlinks by default. Symlinks to files outside the transforms dir would be included in the hash, causing non-deterministic hashing across environments.
**Fix:** Add `.follow_links(false)` to WalkDir.

### H4: `total_size()` swallows SQLite errors, returns 0
**File:** `crates/ozzy-core/src/cache/local.rs:329`
`.unwrap_or(0)` silently ignores SQLite errors. This can cause `evict_to_size()` to skip eviction entirely.
**Fix:** Propagate the error with `?`.

### H5: `status.rs` labels all staged endpoints as "new" even when updating
**File:** `crates/ozzy-cli/src/commands/status.rs:97`
All staged `.json` endpoints are labeled `new:` even if they update an existing committed endpoint.
**Fix:** Check if the endpoint exists in the latest commit and label as `modified:` accordingly.

## MEDIUM

### M1: `dict<>` type parsing breaks on nested types with commas
**File:** `crates/ozzy-core/src/schema.rs:272`
`inner.find(", ")` finds the first comma, which may be inside a nested type like `struct<a: int64, b: float64>`.
**Fix:** Use depth-aware comma finding (same pattern as struct parser).

### M2: `get_stream()` skips content hash verification
**File:** `crates/ozzy-server/src/storage/content.rs:252-259`
`get_stream()` reads local file and returns it without hash verification, unlike `get()` which verifies.
**Fix:** Add blake3 hash check before returning from get_stream().

### M3: Temp file collision in server `store()`
**File:** `crates/ozzy-server/src/storage/content.rs:181`
Two concurrent pushes of the same content race on the same `.tmp` file path.
**Fix:** Include process ID in temp filename (as `get()` already does for hydration).

### M4: Cross-platform path separators in `hash_source_directory`
**File:** `crates/ozzy-core/src/canon.rs:53`
`to_string_lossy()` uses OS-native separators. On Windows this produces backslashes, diverging from Unix hashes.
**Fix:** Normalize separators to forward slash after conversion.

### M5: `i64` to `u64` cast without negative check in `get_parquet_row_count`
**File:** `crates/ozzy-core/src/schema.rs:330`
`count as u64` silently wraps negative values from corrupted parquet metadata.
**Fix:** Use `count.try_into().unwrap_or(0)`.

### M6: `cache::get()` returns entry for file that no longer exists on disk
**File:** `crates/ozzy-core/src/cache/local.rs:228-237`
No check whether `file_path` exists before bumping access stats and returning the entry.
**Fix:** Check existence; if missing, auto-remove the DB entry and return None.

### M7: Python `_ozzy_type_to_arrow` silently falls back to `utf8` for unhandled types
**File:** `clients/python/src/ozzydb/client.py:322`
Types like `time32[ms]`, `duration[ns]`, `struct<...>`, `dict<...>` all silently become `utf8`.
**Fix:** Add parsers for missing types; warn on unknown fallback.

### M8: `list_by_prefix` does not validate `hash_prefix` input
**File:** `crates/ozzy-server/src/storage/content.rs:294-295`
Arbitrary string joined into filesystem path without hex validation. Potential directory traversal.
**Fix:** Validate that hash_prefix contains only hex chars.

### M9: Python `_load_latest_commit` doesn't cache `None` results
**File:** `clients/python/src/ozzydb/project.py:70-92`
Repeated filesystem reads when no commit exists; potential inconsistent view if commit is created between property accesses.
**Fix:** Use a sentinel to cache None results.

## LOW

### L1: `env_path` panics on short lockfile_hash
**File:** `crates/ozzy-core/src/runtime.rs:243`
`&lockfile_hash[..12]` panics if hash is shorter than 12 chars.
**Fix:** Use `.get(..12).unwrap_or(lockfile_hash)`.

### L2: Python `force=True` silently ignored for remote refs
**File:** `clients/python/src/ozzydb/client.py:158-161`
**Fix:** Warn or raise when force=True with remote ref.

### L3: Python `inspect()` assumes `nodes[-1]` is terminal DAG node
**File:** `clients/python/src/ozzydb/client.py:373-379`
Node list order is not guaranteed topological.
**Fix:** Compute actual terminal node from edges.

### L4: Python `_load_staged_transforms` bare `except Exception` swallows all errors
**File:** `clients/python/src/ozzydb/project.py:139-143`
**Fix:** Catch specific exceptions; log warning on failure.

### L5: Server tags pushed outside main transaction
**File:** `crates/ozzy-server/src/api/v1/push_pull.rs:481-509`
Already best-effort with tracing::warn. Note for future improvement.

### L6: Pull lockfile sentinel inconsistency
**File:** `crates/ozzy-server/src/api/v1/push_pull.rs:817`
Pull doesn't check for empty lockfile sentinel like push does.
**Fix:** Add sentinel check to skip empty lockfile hash in pull.
