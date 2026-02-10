# OzzyDB Code Review (Round 9 -- Combined)

Date: February 7, 2026

Scope: Full codebase review by 2 models (Gemini 3 Pro, Claude Opus 4.6). Codex hit OpenAI rate limit. Findings deduplicated and merged below.

## Deduplicated Findings (Ordered by Severity)

### 1. Critical: `module_name`/`function_name` injected unsanitized into Python import/call statements

**Found by:** Opus (#1), Gemini (#5 -- hyphen variant)

`module_name` from `file_stem()` and `function_name` from decorator parsing are interpolated directly into `import {module_name}` and `{module_name}.{function_name}(...)` in generated Python code. While `validate_safe_name` guards the CLI `transform add` path, transforms from `pull` or manually placed files bypass validation. A transform with a filename containing Python-hostile characters could cause unexpected behavior.

Evidence: `crates/ozzy-core/src/runtime.rs:63-74,332`

Fix: Add Python identifier validation in `extract_transform_info` -- both `module_name` and `function_name` must match `^[a-zA-Z_][a-zA-Z0-9_]*$`.

### 2. High: Race condition in content cleanup during push

**Found by:** Gemini (#1)

When a push fails, the server deletes all blobs it just stored. Because storage is content-addressed (deduped), another concurrent push may have stored and committed the same hash. The failing push's cleanup deletes a blob referenced by the successful push.

Evidence: `crates/ozzy-server/src/api/v1/push_pull.rs:255-260,310-315`

Fix: Don't auto-delete blobs on push failure. Use background garbage collection for unreferenced blobs.

### 3. High: Pull/fetch builds entire tar archive in memory (OOM risk)

**Found by:** Gemini (#3), Opus (#4)

The `pull` and `fetch_endpoint` handlers buffer entire tar archives in `Vec<u8>` before sending. With default `max_tar_size_bytes` of 1 GB, concurrent pulls can OOM the server. Data sources are also fetched twice (size estimate + tar build).

Evidence: `crates/ozzy-server/src/api/v1/push_pull.rs:612-777`

Fix: Stream tar archives instead of buffering. Reuse data from size estimation pass. NOTE: Defer to streaming/compute step.

### 4. High: Runtime failure when lockfile omits polars/pyarrow

**Found by:** Gemini (#4)

When `uv.lock` is present, the runtime creates an env from only lockfile packages. But the generated Python script explicitly imports `polars` and `pyarrow`. If the user's lockfile doesn't include these, the transform crashes.

Evidence: `crates/ozzy-core/src/runtime.rs:415-430`

Fix: Always include `polars` and `pyarrow` in the base requirements alongside lockfile packages.

### 5. Medium: `Content-Disposition` header injection via unsanitized path params

**Found by:** Opus (#8)

`ref_name` and `endpoint_name` from URL path params are interpolated into `Content-Disposition` headers without sanitization. Characters like `\r\n` or `"` could inject headers.

Evidence: `crates/ozzy-server/src/api/v1/push_pull.rs:773-776,1022-1031`

Fix: Sanitize by stripping `\r`, `\n`, `"`, non-ASCII from header values, or use RFC 6266 `filename*=` encoding.

### 6. Medium: `upsert_user_from_github` never updates `username` on GitHub login rename

**Found by:** Opus (#3)

The ON CONFLICT clause updates `github_login` but not `username`. If a user renames their GitHub account, `username` stays stale, breaking all `/{owner}/{project}` URL routing.

Evidence: `crates/ozzy-server/src/db/queries.rs:67-86`

Fix: Add `username = EXCLUDED.username` to the upsert, handling potential UNIQUE conflicts.

### 7. Medium: Fragile parenthesis parsing in Python decorators

**Found by:** Gemini (#6)

The `@ozzy.transform` parser uses a simple parenthesis counter without respecting string literals. Parentheses inside strings (e.g., `input_schema={"desc": "wait ) trap"}`) truncate the metadata.

Evidence: `crates/ozzy-core/src/commit.rs:150-165`

Fix: Implement a minimal string-aware state machine for paren counting.

### 8. Medium: Incomplete lockfile discovery (same-dir only)

**Found by:** Gemini (#7)

`collect_transforms` only looks for `uv.lock` in the same directory as the `.py` file. A root-level `uv.lock` is invisible to subdirectory transforms.

Evidence: `crates/ozzy-core/src/commit.rs:175-185`

Fix: Search upward from transform dir to project root for `uv.lock`.

### 9. Medium: `check_content` returns names instead of hashes for dedup

**Found by:** Opus (#2)

Two data sources with different names but the same content hash cause one to be marked "missing" even though the blob already exists. This wastes bandwidth.

Evidence: `crates/ozzy-server/src/api/v1/push_pull.rs:488-499`

Fix: Deduplicate by hash before checking, or return missing hashes instead of names.

### 10. Low: `canonicalize_json` produces invalid escapes for supplementary Unicode (U+10000+)

**Found by:** Opus (#10)

`format!("\\u{:04x}", c as u32)` produces 5+ hex digit escapes for code points above U+FFFF, which is invalid per RFC 8259 (JSON requires exactly 4 hex digits, using surrogate pairs for supplementary characters).

Evidence: `crates/ozzy-core/src/canon.rs:115`

Fix: Emit UTF-16 surrogate pairs for code points above U+FFFF, or pass non-control Unicode through as UTF-8.

### 11. Low: Non-atomic file writes in content storage

**Found by:** Opus (#9)

`store()` checks `!exists()` then writes directly. Concurrent stores of the same hash can race.

Evidence: `crates/ozzy-server/src/storage/content.rs:177-179`

Fix: Write to temp file, then atomic rename.

### 12. Low: Pagination `offset` unbounded (negative/huge values accepted)

**Found by:** Opus (#5)

`offset` is `i64` with no validation. Negative values cause SQL errors, huge values cause slow queries.

Evidence: `crates/ozzy-server/src/api/v1/projects.rs:27-36`

Fix: Validate `offset >= 0`, cap at reasonable max.

### 13. Low: `new_transform_count` counts all uploads, not just new ones

**Found by:** Opus (#12)

`new_transform_count` is set to `transform_files.len()` rather than incrementing only when `is_new` is true, overstating the count in `PushResponse`.

Evidence: `crates/ozzy-server/src/api/v1/push_pull.rs:318`

Fix: Track like `new_data_count` -- only increment when `is_new`.

### 14. Low: Code duplication between `run.rs` and `fetch.rs`

**Found by:** Opus (#6)

Near-identical copies of execution logic (param parsing, topological sort, node execution, cache interaction, nocache cleanup).

Evidence: `crates/ozzy-cli/src/commands/fetch.rs:77-265` vs `run.rs:10-435`

Fix: Extract shared execution module.

### 15. Low: Invalid Mermaid syntax for names with dots/hyphens

**Found by:** Gemini (#8)

`print_mermaid_dag` uses names directly as Mermaid node IDs. Dots/hyphens break rendering.

Evidence: `crates/ozzy-cli/src/commands/dag.rs:170-185`

Fix: Sanitize IDs, keep original names as labels.

### 16. Low: `list_projects` omits collaborated projects

**Found by:** Opus (#11)

Only shows owned projects. Collaborator projects are invisible.

Evidence: `crates/ozzy-server/src/api/v1/projects.rs:68-93`

Fix: Add `?include=collaborated` or UNION with collaborator query.

### 17. Low: `get_stream` skips hash verification (repeat from R8 #11, still deferred)

**Found by:** Opus (#7)

`get_stream()` serves content without verifying BLAKE3 hash. Corrupted local files served silently.

Evidence: `crates/ozzy-server/src/storage/content.rs:240-266`

Fix: Verify hash before streaming, or at minimum document limitation.

---

## Summary

17 deduplicated findings from 2 independent reviewers (21 raw, 4 overlaps). Codex was unavailable (rate limited).

**Overlap analysis:**
- Pull/fetch OOM: found by Gemini + Opus (high confidence)
- Module name injection: found by Opus (critical) + Gemini (medium -- hyphen variant)
- No contradictions between reviewers

**Actionable now (quick fixes):** #1, #5, #6, #9, #10, #12, #13, #15
**Defer to streaming/compute step:** #3, #4, #17
**Design decisions needed:** #8, #14, #16
**Acceptable risk:** #2 (GC pattern), #7 (decorator parser), #11 (atomic writes)

**Severity trend:** One critical finding (#1) -- module name injection. Medium findings are mostly edge cases and robustness. Code is stabilizing but still has a few meaningful gaps.
