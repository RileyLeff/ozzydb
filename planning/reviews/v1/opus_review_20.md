# Opus Review 20 (Round 5)

Date: 2026-02-07

## Findings

### HIGH

**H1: Unsafe string slicing in `parse_data_type` (schema.rs)**
File: `crates/ozzy-core/src/schema.rs:175-284`
Multiple `starts_with("prefix")` branches slice with hardcoded offsets (e.g. `&s[10..s.len()-1]`)
but never validate that the string actually contains the closing delimiter. Input like `"timestamp"`
(no brackets) would panic on `&s[10..8]` (underflow). Affects: timestamp, time32, time64, duration,
list, large_list, fixed_list, struct, dict branches.
Fix: Add length/closing-bracket guard before each slice.

**H2: `get_path_only` swallows database errors with `.ok()` (cache/local.rs)**
File: `crates/ozzy-core/src/cache/local.rs:305`
`stmt.query_row([hash], |row| row.get(0)).ok()` converts real SQLite errors (corruption, locked DB)
to `None`, making them indistinguishable from "not found". Used by `remove()`, so a DB error
silently skips file deletion.
Fix: Use `.optional()?` (from `rusqlite::OptionalExtension` already imported) to only convert
`QueryReturnedNoRows` to `None` while propagating real errors.

**H3: Missing lockfile sentinel check in `fetch_endpoint` (push_pull.rs)**
File: `crates/ozzy-server/src/api/v1/push_pull.rs:1072`
`pull()` at line 819 checks `!t.lockfile_hash.is_empty() && t.lockfile_hash != empty_lock_sentinel`
but `fetch_endpoint()` at line 1072 only checks `!t.lockfile_hash.is_empty()`, allowing it to
attempt fetching the empty-content sentinel hash as a real lockfile.
Fix: Add sentinel check to match pull().

**H4: Python `get_data_path` allows path traversal (project.py)**
File: `clients/python/src/ozzydb/project.py:344-349`
`get_data_path()` returns `self.root / source.path` without validating the resolved path stays
within the project root. A crafted `source.path = "../../etc/passwd"` escapes the project.
Fix: Resolve and call `.relative_to(self.root)` to verify containment.

**H5: Python JSON loading without error handling (project.py)**
File: `clients/python/src/ozzydb/project.py:94, 202`
`json.loads(commit_path.read_text())` and `json.loads(f.read_text())` (staged endpoints) have
no try/except. Corrupted JSON crashes with raw `json.JSONDecodeError`.
Fix: Wrap in try/except, raise descriptive error or skip with warning.

### MEDIUM

**M1: Negative paren depth in decorator parsing (commit.rs)**
File: `crates/ozzy-core/src/commit.rs:210-211`
`paren_depth -= close_count` can go negative if a line has unmatched `)`. This breaks the
termination condition at line 214 (`saw_open_paren && paren_depth == 0`), causing the loop
to consume extra lines.
Fix: Clamp: `paren_depth = (paren_depth - close_count).max(0);`

**M2: Python `None` iteration in endpoint parsing (project.py)**
File: `clients/python/src/ozzydb/project.py:317, 332`
`endpoint_data.get("nodes", [])` returns `None` (not `[]`) if key exists with value `null`.
This causes `TypeError: 'NoneType' object is not iterable`.
Fix: Use `endpoint_data.get("nodes") or []`.

**M3: Python bare `except Exception` in parquet metadata (project.py)**
File: `clients/python/src/ozzydb/project.py:117`
Catches all exceptions when reading parquet metadata, silently swallowing disk errors,
permission errors, and corruption.
Fix: Catch `(OSError, pq.lib.ArrowInvalid)` specifically.

**M4: Tags updated outside transaction boundary (push_pull.rs)**
File: `crates/ozzy-server/src/api/v1/push_pull.rs:480-509`
`tx.commit()` at line 475 completes the atomic transaction, then tag upserts happen afterward.
If server crashes between commit and tags, tags are lost but commit persists.
Fix: Move tag upserts into the transaction before `tx.commit()`.

**M5: `normalize_ref_name` allows nested ref prefixes (push_pull.rs)**
File: `crates/ozzy-server/src/api/v1/push_pull.rs:89-94`
Input `"refs/heads/refs/heads/main"` normalizes to `"refs/heads/main"` — still contains a
`refs/heads/` prefix after one strip. `validate_safe_name` then rejects `/` so this is
caught, but the error message is confusing.
Fix: After normalization, reject if result still contains `/`.

### LOW

**L1: Python `warnings` imported inside functions (client.py, project.py)**
File: `clients/python/src/ozzydb/client.py:162,388` and `project.py:148`
`import warnings` is done inside functions. Standard practice is module-level import.
Fix: Move to top-level imports.

**L2: Python endpoint node/edge dict access without `.get()` (project.py)**
File: `clients/python/src/ozzydb/project.py:313-314, 323-326`
`n["node_name"]`, `e["target_node"]`, etc. crash with `KeyError` on incomplete data.
Fix: Use `.get()` with defaults or validate before access.

**L3: Server SVG endpoint silently drops unparseable endpoints (projects.rs)**
File: `crates/ozzy-server/src/api/v1/projects.rs`
`serde_json::from_value` failure creates empty endpoint struct with no nodes/edges, silently
masking data corruption.
Fix: Log warning with `tracing::warn!`.
