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

### Phase 3: Fly Backend + Rate Limiting (IN PROGRESS)

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
- Commit: (pending)

#### Step 3.4: Orphan machine cleanup
- cleanup_orphans() method on FlyBackend (age-based, ozzy-job-* naming convention)
- Periodic tokio background task (every 5 min, 30 min age threshold)
- Commit: `c056eb0`

#### Step 3.5: Secrets delivery for compute
- compute/secrets.rs: prepare_secrets() uploads JSON blob to R2, returns presigned GET URL
- Orchestrator uses presigned URL for Fly (OZZY_SECRETS_URL), raw env vars for Docker
- Fly init script downloads + exports secrets via Python urllib
- Cleanup of R2 blob on both success and failure paths
- Added store_by_key() and presigned_get_url_by_key() to ContentStorage
- Commit: (pending)

## What's Next
- Phase 3 exhaustive review
