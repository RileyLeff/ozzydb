**HIGH**
1. **Project-scoped tokens can manage account-wide tokens (auth scope bypass)**  
Severity: HIGH  
File: `crates/ozzy-server/src/auth/middleware.rs:181`, `crates/ozzy-server/src/api/v1/auth.rs:177`, `crates/ozzy-server/src/api/v1/auth.rs:199`  
Problematic code:  
`if !has_any_scope(&scopes, ScopeAction::Read) { ... }`  
`async fn list_tokens(AuthUser { user, .. }: AuthUser, ...)`  
`async fn delete_token(AuthUser { user, .. }: AuthUser, ...)`  
Issue: `AuthUser` only requires any `read` scope, including project-scoped scopes like `read:alice/project1`. That token can then list/delete the user’s API tokens globally.  
Suggested fix: Use a stricter extractor for account endpoints (`/auth/token`, `/auth/me`) that requires unscoped `read`/`owner`/`admin` (or explicit `account:*` scope), not project-scoped scopes.

2. **`pull` writes untrusted extra archive files into project root**  
Severity: HIGH  
File: `crates/ozzy-cli/src/commands/pull.rs:265`, `crates/ozzy-cli/src/commands/pull.rs:294`, `crates/ozzy-cli/src/commands/shared.rs:60`  
Problematic code:  
`for (path, content) in extracted_files { ... let dest_path = checked_destination(..., &path)?; ... File::create(&dest_path)?; }`  
Issue: Path traversal is blocked, but there is no allowlist of expected files. Any extra tar entry (for example `.ozzy/...` or `ozzy.toml`) is written if it stays under project root.  
Suggested fix: Build an explicit allowed-path set (`commit.json`, all expected `data/*`, expected `transforms/*`, expected lockfile paths) and reject archive entries not in that set.

3. **`pull` accepts incomplete archives without failing (missing expected files)**  
Severity: HIGH  
File: `crates/ozzy-cli/src/commands/pull.rs:248`, `crates/ozzy-cli/src/commands/pull.rs:268`, `crates/ozzy-cli/src/commands/pull.rs:279`  
Problematic code:  
`if let Some(expected) = expected_data.get(&rel) { ... }`  
`if let Some(expected) = expected_transforms.get(&rel) { ... }`  
Issue: Hash verification runs only for files that are present in the tar. There is no final check that all expected files were seen. A partial archive can be accepted and then pruning can delete valid local files.  
Suggested fix: Track `seen_data`/`seen_transforms` during extraction, then fail if `seen != expected` before writing refs/pruning.

**MEDIUM**
1. **`fetch` transform hash verification can be skipped due name/path mismatch**  
Severity: MEDIUM  
File: `crates/ozzy-cli/src/commands/fetch.rs:165`, `crates/ozzy-cli/src/commands/fetch.rs:193`, `crates/ozzy-server/src/api/v1/push_pull.rs:1052`, `crates/ozzy-server/src/api/v1/push_pull.rs:909`  
Problematic code:  
`(format!("transforms/{}.py", name), hash.clone())`  
Server emits transform file path from `t.source_storage_key`, while manifest only has `name -> hash`.  
Issue: If transform source path is not exactly `transforms/{name}.py` (nested paths, renamed files), verification is skipped silently.  
Suggested fix: Include `source_path` in endpoint manifest and verify by exact path; alternatively verify by a required hash multiset/count independent of path naming.

2. **Archive max-size check in `/pull` misses lockfiles**  
Severity: MEDIUM  
File: `crates/ozzy-server/src/api/v1/push_pull.rs:835`, compare `crates/ozzy-server/src/api/v1/push_pull.rs:1089`  
Problematic code:  
`total_size += lockfile_content.len() as u64; ... builder.append_data(...)` (no limit check in `pull`)  
Issue: Lockfile bytes are added to `total_size`, but not checked against `max_tar_size_bytes` in `pull` (unlike `fetch_endpoint`).  
Suggested fix: Add the same `if total_size > max_size { return Err(...) }` check after adding lockfile bytes in `pull`.

3. **Temp-file naming is not unique per write, causing race collisions**  
Severity: MEDIUM  
File: `crates/ozzy-server/src/storage/content.rs:181`, `crates/ozzy-server/src/storage/content.rs:242`  
Problematic code:  
`with_extension(format!("{}.{}.tmp", extension, std::process::id()))`  
`with_extension(format!("{}.tmp", std::process::id()))`  
Issue: Concurrent writes in one process to same target can use the same temp path and fail/race.  
Suggested fix: Use unique temp names (`NamedTempFile`/UUID/random suffix), then atomic rename; if rename fails because destination already exists, treat as success after validating hash.

4. **Server trusts client-provided `schema_hash` without verifying against actual parquet schema**  
Severity: MEDIUM  
File: `crates/ozzy-server/src/api/v1/push_pull.rs:285`, `crates/ozzy-server/src/db/queries.rs:309`  
Problematic code:  
`let schema = extract_parquet_schema(content)...` then later insert uses `.bind(&ds.schema_hash)`  
Issue: Extracted schema is stored, but `ds.schema_hash` from commit metadata is not validated to equal hash(schema_json). This can persist inconsistent metadata.  
Suggested fix: Compute server-side schema hash from extracted schema JSON and reject commit when it differs from `ds.schema_hash` (or overwrite with server-computed value).

5. **Invalid client input is converted to 500 instead of 400 in push/pull paths**  
Severity: MEDIUM  
File: `crates/ozzy-server/src/api/v1/push_pull.rs:223`, `crates/ozzy-server/src/api/v1/push_pull.rs:565`, `crates/ozzy-server/src/auth.rs:266`  
Problematic code:  
`let safe_name = sanitize_relative_path(filename)?;`  
`if !state.storage.exists(hash, "parquet").await?`  
`impl From<anyhow::Error> for ApiError { ... Self::Internal(err) }`  
Issue: User-caused validation failures (`sanitize_relative_path`, invalid content hash format) bubble as `anyhow` and become 500.  
Suggested fix: Map these specific validation failures to `ApiError::bad_request(...)` at call sites (or add typed validation errors with 400 mapping).

6. **Python client allows ref-path traversal via `refs.head`**  
Severity: MEDIUM  
File: `clients/python/src/ozzydb/project.py:79`  
Problematic code:  
`if head.startswith("refs/"): ref_path = self.ozzy_dir / head`  
Issue: A malicious `ozzy.toml` can set `refs.head` like `refs/../../../../tmp/x`, causing out-of-tree reads.  
Suggested fix: Normalize and validate ref paths (`.`/`..`/absolute disallowed) and enforce resolved path starts with `.ozzy/refs` before reading.

**LOW**
1. **Error response schema is inconsistent across endpoints**  
Severity: LOW  
File: `crates/ozzy-core/src/registry/protocol.rs:221`, `crates/ozzy-server/src/api/v1/auth.rs:324`, `crates/ozzy-server/src/auth/middleware.rs:346`  
Problematic code:  
Protocol expects `{ error, message, details }`, but auth API returns only `{ "error": message }`.  
Issue: Inconsistent API contract/error shape depending on endpoint path.  
Suggested fix: Standardize all API errors to the shared protocol struct (`error`, `message`, optional `details`) and return consistent JSON across middleware and handlers.