# Opus Review: Fixes & Missing Implementations

## Round 1 (Complete)

### 1. `ozzy fetch` — Execute transforms after download ✅
### 2. Python client — Remote registry support ✅
### 3. Schema extraction on push ✅
### 4. API endpoint tests ✅
### 5. Compilation warnings ✅
### 6. `reproducible=False` has no effect ✅

---

## Round 2 (Complete)

### 7. `get_project()` endpoint missing access control ✅
Added `MaybeAuthUser` extractor and visibility-based access control.
Private projects require auth + ownership; public projects are open.

### 8. Race condition on first push ✅
Added `get_or_create_project()` using PostgreSQL `ON CONFLICT` upsert.
Push handler now uses atomic upsert instead of get-then-create.

### 9. `ozzy transform test` is a stub ✅
Implemented real test execution: runs transform on available data source,
reports timing, output row count, file size, and output schema.

### 10. Dead code: `pull::clone()` and `auth::status()` ✅
Removed both unused functions.

### 11. Replace unwrap() calls with proper error handling ✅
Replaced unwrap() in `run.rs`, `fetch.rs`, `remote.rs`, `data.rs`, `status.rs`
with `ok_or_else()`, `.context()`, and `unwrap_or_default()`.

### 12. `PythonRuntime::default()` panics if uv not installed ✅
Removed the `Default` impl entirely. Callers should use `PythonRuntime::new()`
which returns `Result`.

### 13. Registry client error type mismatch ✅
Added `Registry(String)` variant to `ozzy_core::error::Error`.
Client keeps `anyhow::Result` for HTTP ergonomics (consumed via CLI's anyhow boundary).

### 14. `cache::clear()` project-specific filtering is a stub ✅
Removed misleading `--project` flag. Cache is content-addressed with no
project association — flag was not implementable without schema changes.

### 15. `total_size_bytes: 0` in pull/fetch manifests ✅
Added `get_content_total_size()` DB method querying `content_refs` table.
Both `pull_manifest` and `fetch_endpoint_manifest` now return actual sizes.

### 16. Remote name validation missing ✅
Added `validate_safe_name()` calls to `remote add` and `remote rm`.

### 17. Transform decorator parser silently fails ✅
Added `eprintln!` warnings when `inputs=`, `params=`, `input_schema=`, or
`output_schema=` are present but fail to parse.

---

## Round 3: Architecture Spec Audit (Phase 1 & Phase 2) — Complete

### Phase 1 Issues

### 18. Params JSON not canonicalized ✅
`run.rs` and `fetch.rs` now use `canon::hash_json()` for params hashing
instead of `effective_params.to_string()`.

### 19. Commit JSON not canonicalized ✅
`commit.rs` now uses `canon::hash_json()` for commit hash computation.

### 20. Schema propagation through pipeline ✅
`validate_pipeline_schema()` in `endpoint.rs` now tracks `current_columns`
as mutable `Vec<String>`, adds columns from `output_schema.adds`, and
removes columns from `output_schema.drops`. Multi-step pipelines now
correctly propagate schema changes.

### 21. Multi-input schema validation ✅
`endpoint.rs` now validates that all named inputs declared in the first
transform's `input_schema.inputs` are provided by the endpoint definition.

### 22. Schema type checking ✅
Added `types_compatible()` helper supporting type normalization (Python/JSON
type names → Arrow types) and numeric widening. `requires` can now be a
dict `{"col": "float64"}` for type-checked validation.

### Phase 2 Issues

### 23. `@latest` ref handling ✅
`get_ref_by_name()` now resolves `@latest` and `latest` to the most
recently updated ref via `ORDER BY updated_at DESC LIMIT 1`.

### 24. Ref listing endpoint ✅
Added `GET /{owner}/{project}/refs` endpoint returning `ListRefsResponse`
with separate `branches` and `tags` arrays including commit hashes.

### 25. Lockfile content in push/pull ✅
Push handler now accepts `lockfiles/` prefix in multipart uploads.
Pull tar archive includes `transforms/uv.lock` from stored lockfile hashes.

### 26. Token scope validation ✅
Added `WriteAuthUser` extractor requiring "write" scope. Push endpoint
now uses `WriteAuthUser` instead of `AuthUser`. Added `InsufficientScope`
error variant with 403 response.

### 27. Multipart upload size enforcement ✅
Push handler now tracks `total_upload_size` across all multipart fields
and rejects uploads exceeding `max_upload_size_bytes`.

### 28. Cleanup on push failure ✅
Push handler now tracks `stored_hashes` for newly stored files. If storage
or commit fails, orphaned blobs are cleaned up via `storage.delete()`.

### 29. Python `inspect()` schema conversion ✅
Implemented `_json_schema_to_arrow()` and `_ozzy_type_to_arrow()` in
`client.py`. Converts `output_schema.fields` and `output_schema.adds`
to PyArrow Schema objects.

### 30. Credentials file permissions ✅
`save_credentials()` in `auth.rs` now sets 0600 permissions on Unix
systems using `std::os::unix::fs::PermissionsExt`.

---

## Round 4: Deep Architecture Audit — Complete

### Critical Issues

### 31. Transform hash incomplete ✅
`materialized_hash` now uses the full `hash::transform_hash()` which
includes source_hash, lockfile_hash, runtime, and params_schema_hash.
Applied to both `run.rs` and `fetch.rs`.

### 32. No output schema validation at runtime ✅
Added `validate_output_schema()` in `run.rs` that checks the output
parquet file against the transform's declared `output_schema` (both
`adds` and `fields` declarations). Runs after every transform execution.

### 33. `reproducible=False` not enforced ✅
Non-reproducible transforms now skip cache lookup AND cache storage.
Added `execute_node_no_cache()` in both `run.rs` and `fetch.rs` that
executes without touching the cache at all.

### High Priority Issues

### 34. Platform fingerprint hash not canonical ✅
`platform.rs` now uses `canon::hash_json()` via `serde_json::to_value()`
instead of `serde_json::to_string()` for deterministic hashing.

### 35. DataSource schema hash not canonical ✅
`commit.rs` now uses `canon::hash_json()` for schema hashing instead
of `serde_json::to_string()`.

### 36. Python client `_is_local_ref()` bug ✅
Fixed operator precedence bug where `os.sep in ref` caused remote refs
like `owner/project/endpoint` to be misidentified as local paths.
Now uses explicit `if` blocks with proper logic.

### 37. Python test imports non-existent function ✅
Changed `_parse_ref` to `_parse_local_ref` in `test_client.py` (both
import and usage sites).

### 38. `fail_on_remote_error` config never used ✅
`TieredCache` now stores and respects the `fail_on_remote_error` policy.
Remote errors in `get_path()` and `put()` are caught: if the flag is
false, a warning is printed and execution continues; if true, the error
propagates.

### Medium Priority Issues

### 39. DAG execution sequential — Deferred
Optimization, not a correctness issue. Can be parallelized later with
`FuturesUnordered` when needed.

### 40. `fetch_lazy()` not actually lazy — Deferred
Would require significant redesign of the execution model. Documented
as a known limitation.

### 41. Server CORS too permissive ✅
CORS is now configurable via `CORS_ORIGINS` env var. Defaults to `*`
for development. Accepts comma-separated origins for production.
Methods restricted to GET, POST, DELETE, OPTIONS.

### 42. No rate limiting — Deferred
Requires adding `tower-governor` or similar middleware. Noted for
production deployment.

### 43. Missing input validation on server ✅
Added `validate_slug()` to `create_project()` endpoint. Enforces:
lowercase alphanumeric + hyphens/underscores, 1-100 chars, no
leading/trailing hyphens.

### 44. No pagination on list endpoints ✅
Added `PaginationParams` (limit/offset) to `list_projects` endpoint.
Added `list_user_projects_paginated()` DB method with limit capped
at 100. Default limit is 50.
