# v3 Workflow State

## Current Phase: Phase 3 — Fly Backend + Rate Limiting
## Current Step: Starting Phase 3

## Completed Steps

### Phase 1: R2 Storage + Presigned URLs + Streaming Uploads (COMPLETE)

#### Step 1.1: Presigned URL generation
- Commit: `641f5ba`

#### Step 1.2: Streaming downloads via presigned redirect
- Commit: `4029222`

#### Step 1.3: Streaming uploads
- Commit: `6ff2edd`

#### Phase 1 Review: 3 rounds, converged (2 consecutive clean)
- Commit: `e798544`

### Phase 2: Async Job Model + Parallel DAG (COMPLETE)

#### Step 2.1: Jobs table migration + DB operations + tests
- Created `migrations/002_v3_jobs.sql` (jobs + environment_provider_images tables)
- Added `Job` and `EnvironmentProviderImage` models
- Added 9 query functions: create_job, get_job, find_active_job, update_job_status, update_node_status, set_job_output, set_job_error, list_jobs, cleanup_expired_jobs
- Added 2 env provider image queries: get_provider_image, upsert_provider_image
- 8 new DB tests
- Commit: `2d52909`

#### Step 2.2: Convert fetch endpoint to async POST
- Changed route from GET to POST
- Added FetchResponse struct (job_id, status, output_url, output_hash)
- Handler flow: validate → dedup check → cache-hit fast path → create job + spawn background → return 202
- Added check_all_node_caches() for inline cache checking
- Added compute_materialized_hash() helper for individual node hash computation
- Background execution via execute_job() with tokio::spawn
- Refactored helpers with _inner pattern for dual error types (ApiError / anyhow)
- Commit: `477c5b4`

#### Step 2.3: Job status + output endpoints
- Created `api/v1/jobs.rs` with GET /v1/jobs/{id} (status) and GET /v1/jobs/{id}/output (redirect/proxy)
- JobStatusResponse with per-node breakdown
- Access control: enforces read access on owning project
- 6 integration tests
- Commit: `039e08f`

#### Step 2.4: DAG orchestrator (parallel wave execution)
- Created `compute/orchestrator.rs` with run_job, execute_node, compute_waves
- Wavefront scheduling: nodes grouped into waves, independent nodes run concurrently via tokio::spawn
- Self-contained helpers: resolve_edge_source, compute_source_hash, resolve_secrets_hash
- Removed ~460 lines of duplicated execute_job from fetch.rs
- Made 10+ fetch.rs helpers pub(crate) for orchestrator access
- 4 unit tests (linear, parallel, single, diamond DAG)
- Commit: `a7a792f`

#### Step 2.5: ComputeBackend trait
- Added `ComputeBackend` trait to `compute/types.rs` (RPITIT-style, no async_trait)
- Created `DockerBackend` struct in `docker.rs` implementing the trait
- Added `BackendSelector` enum to `compute/mod.rs` with `from_config()` factory
- Added `compute: Option<BackendSelector>` to `AppState`
- Updated orchestrator to use backend from state instead of direct docker::run()
- Updated main.rs + all test files (api_tests, e2e_tests, integration_tests)
- 2 unit tests (disabled/enabled config)
- Commit: `8865b2a`

#### Step 2.6: Update CLI ozzy fetch for async model
- Rewrote `ozzy fetch` to POST + poll + download pattern
- Per-node status display during polling
- Handles presigned URL redirects for output download
- 3 new unit tests (format_node_status)
- Commit: `21dd3fe`

#### Step 2.7: Update Python client fetch()
- Rewrote `fetch()` and `fetch_lazy()` for async POST + poll
- Added `_download_job_output()` helper for redirect/proxy handling
- Added `poll_interval`, `timeout`, `verbose` parameters
- 2 new test cases (poll_until_done, job_error)
- Commit: `fae0b13`

## Deferred Steps

### Step 1.4: CLI upload progress bar
**Reason:** CLI `ozzy data add` is not yet implemented (stub only).

### Step 1.5: Deploy R2 to production
**Reason:** Requires SSH access to VPS. Will be done manually.

#### Phase 2 Exhaustive Review: 4 rounds, converged (2 consecutive clean)
- Round 1: 8 fixes (job output storage/lookup, status mismatch, secrets hash, param sanitization, wave ordering, poll timeout)
- Round 2: 1 fix (orchestrator missing secret handling)
- Rounds 3-4: CLEAN
- Review fix commits: `e0b4379`, `19e0ca3`
- Models: Claude Opus only (Gemini: E2BIG at 368k tokens, Codex: skipped at 368k > 258k limit)

### Phase 3: Fly Backend + Rate Limiting (COMPLETE)

#### Step 3.1: FlyBackend + BackendSelector
- FlyBackend implementing ComputeBackend trait (fly.rs)
- FlyConfig + RateLimitConfig added to config.rs
- BackendSelector priority: Fly (if R2) > Docker > None
- Updated orchestrator init script selection
- Commit: `f2e0c70`

#### Step 3.2: Environment image management
- environments.rs: provider tracking (docker/fly), image ref formatting
- DB queries: get_provider_image, upsert_provider_image
- Commit: `844f19c`

#### Step 3.3: Rate limiting integration
- Wired check_limits() into fetch endpoint before async job creation
- Added TooManyRequests (429) variant to ApiError
- Rate limits checked after cache-hit fast path (cache hits don't count against limits)
- Anonymous users: global limit only; authenticated: per-user + global
- Commit: `6e0fbcf`

#### Step 3.4: Orphan machine cleanup
- cleanup_orphans() method on FlyBackend (age-based, ozzy-job-* naming convention)
- Periodic tokio background task (every 5 min, max_age derived from config timeout)
- Commit: `c056eb0`

#### Step 3.5: Secrets delivery for compute
- compute/secrets.rs: prepare_secrets() uploads JSON blob to R2, returns presigned GET URL
- Orchestrator uses presigned URL for Fly (OZZY_SECRETS_URL), raw env vars for Docker
- Fly init script downloads + exports secrets via Python urllib
- Cleanup of R2 blob on both success and failure paths (guaranteed via scope guard)
- Added store_by_key() and presigned_get_url_by_key() to ContentStorage
- Commit: `c3b5adb`

#### Phase 3 Exhaustive Review (IN PROGRESS)
- Round 1: 11 fixes applied (M1-M4, m1-m4, m8, n7, secrets TTL)
  - M1: OZZY_INPUT_DOWNLOADS presigned URLs for Fly inputs
  - M2: Tarball extraction safety settings
  - M3: Rate limit TOCTOU documented as advisory
  - M4: Presigned PUT URL TTL = timeout+5min
  - m1: Raw response logging for exit_code debugging
  - m2: Orphan max_age derived from config timeout
  - m3: Conditional input downloads in init script
  - m4: Secrets cleanup on all error paths (scope guard)
  - m8: Unset sensitive env vars before user code
  - n7: Default exit_code=-1 on state fetch failure
  - Secrets presigned GET URL TTL = timeout+5min
- Review fix commit: `c0a934c`
- Round 2: 4 fixes applied (M1 input population, M2 input TTL, m1 double resolution, m2 download paths)
  - M1: Orchestrator now builds proper InputSpec from resolved edges
  - M2: Input download presigned URL TTL = timeout+300 (was 3600)
  - m1: Reuse already-resolved input hashes for Fly downloads
  - m2: Download dest paths match manifest format (/workspace/inputs/{name})
  - Added resolve_input_content_type helper
- Review fix commit: `715d893`
- Models: Claude Opus only (Gemini: E2BIG, Codex: context too large)
- Round 3: 5 fixes applied (M1 source delivery, M2 input cleanup, m1 exit_code fallback, m2 doc, m3 safety comment)
  - M1: Fly source code delivery — upload tarball to R2, presigned download, init script extracts to /workspace/source/
  - M2: Docker input temp files cleaned up after compute (prevents disk leak)
  - m1: Fallback exit_code parsing from Fly events array (nested exit_event.exit_code)
  - m2: Documented python3/curl requirement for Fly environment images
  - m3: Safety comment on source_dir TempDir lifetime
- Review fix commit: `117c4cc`
- Round 4: 2 fixes applied (client timeout shadow, dead code)
  - minor-1: Removed hardcoded 600s reqwest client timeout (was shadowing per-request timeouts)
  - minor-2: Removed dead input_hashes_snapshot code in orchestrator wave loop
- Review fix commit: `e031bb0`
- Rounds 5-6: CLEAN (convergence reached — 2 consecutive clean rounds)

## What's Next
- Phase 4 (next in implementation plan)
