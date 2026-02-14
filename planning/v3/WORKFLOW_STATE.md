# v3 Workflow State

## Current Phase: v3.1 COMPLETE
## Current Step: All steps implemented, ready for review

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
- Round 2: 4 fixes applied (M1 input population, M2 input TTL, m1 double resolution, m2 download paths)
- Round 3: 5 fixes applied (M1 source delivery, M2 input cleanup, m1 exit_code fallback, m2 doc, m3 safety comment)
- Round 4: 2 fixes applied (client timeout shadow, dead code)
- Rounds 5-6: CLEAN (convergence reached — 2 consecutive clean rounds)

### Phase 4: Admin Dashboard (COMPLETE)

#### Step 4.1: Admin flag + API
- Migration 003_v3_admin.sql: `is_admin BOOLEAN DEFAULT false` on users table
- `AdminUser` extractor in auth/middleware.rs (account-scoped + is_admin=true)
- Admin API endpoints: GET /v1/admin/jobs, POST /v1/admin/jobs/{id}/cancel, GET /v1/admin/rate-limits, GET /v1/admin/users
- DB queries: list_jobs_global, cancel_job, list_users, set_user_admin
- 4 unit tests
- Commit: `ab8ea5d`

#### Step 4.2: Admin frontend page
- Admin dashboard page at /admin with three tabs: Jobs, Users, Rate Limits
- Jobs tab: auto-refreshing table with cancel buttons, status filter
- Users tab: listing with admin badges
- Rate Limits tab: current config + active job count
- Nav dropdown shows Admin link for admin users
- is_admin field added to server UserInfo response + frontend types
- Commit: `49def7d`

#### Phase 4 Exhaustive Review: 3 rounds, converged (2 consecutive clean)
- Round 1: 5 fixes (cancel_job fetch_optional→execute, queued-only cancel, negative limit clamping, double-fetch, status validation)
- Rounds 2-3: CLEAN
- Review fix commit: `cba927c`
- Models: Claude Opus only (~389k tokens, exceeds Codex/Gemini limits)

### Phase 5: Cleanup + Local Dev Stack (COMPLETE)

#### Step 5.1: Delete dead code
- Deleted `crates/ozzy-cli/src/commands/run.rs` (~1572 lines)
- Removed Run variant from main.rs CLI enum + dispatch
- Removed Python client `run()` function + 3 tests + integration test ref
- Updated docs/getting_started.md (removed ozzy run references, renumbered steps)
- Updated docker.rs comment (removed ozzy run mention)
- Removed dead imports (shutil, subprocess from client.py)
- Skipped: generate_docker_init() and Docker bind mount logic (still used by server)
- Commit: `e243d61`

#### Step 5.2: Local dev Docker Compose
- Created `docker-compose.dev.yml` at repo root (PostgreSQL + MinIO + ozzy-server)
- Added DEV_AUTO_USER env var to config.rs (auto-creates admin user + token on startup)
- MinIO replaces R2 for local development (S3-compatible)
- Bind mount tmpdir for Docker compute containers
- Step 5.3 (ozzy dev CLI sugar) skipped as optional
- Commit: `22fe565`

#### Step 5.4: Documentation updates
- Updated README.md: line counts, async jobs, local dev section, self-hosting
- Updated CLAUDE.md: v2→v3 references, CLI command list, local dev section
- Commit: `12ffd17`

#### Phase 5 Exhaustive Review: 4 rounds, converged (2 consecutive clean)
- Round 1: 6 fixes (stale ozzy run refs, --lang flag, v3 error msg, Justfile, dev compose improvements)
- Round 2: 7 fixes (docs schema rewrite, DEV_AUTO_USER admin promotion, Fly env vars, services-up, init template, dead clone, test Config fields)
- Review fix commits: `edf04b4`, `5c63964`
- Rounds 3-4: CLEAN
- Models: Claude Opus only (~375k tokens, exceeds Codex/Gemini limits)

### Phase 6: Polish (SKIPPED — stretch goals, not blocking deployment)

### Phase 7: Deployment & Integration (COMPLETE)

#### Step 7.1: Docker Compose config updates
- Updated `.env.prod.example` with GitHub App, secrets encryption, and Fly env vars
- (docker-compose.prod.yml already had them from Phase 5 round 2 fixes)
- Commit: `12b329b` (bundled with 7.2)

#### Step 7.2: E2E tests rewrite for async job model
- Fixed critical bug: `compute: None` → `BackendSelector::Docker(...)` in TestServer
- Rewrote all fetch tests: GET → POST (v3 async API)
- Added `fetch_and_wait()` helper: POST → poll job status → download output
- 12 E2E tests: basic compute, cache hit, param override, param validation,
  unknown param, nonexistent endpoint, yank, private auth, wrong user,
  public no auth, endpoint inspection, commit API
- Commit: `12b329b`

#### Phase 7 Exhaustive Review: 4 rounds, converged (2 consecutive clean)
- Round 1: 3 fixes (COMPUTE_TMPDIR docs, RATE_LIMIT docs, dead MAX_TAR_SIZE_BYTES removal)
- Round 2: 1 fix (RATE_LIMIT passthrough in docker-compose.prod.yml)
- Rounds 3-4: CLEAN
- Review fix commits: `5cb0bbd`, `6a5a1d2`
- Models: Claude Opus only (~376k tokens, exceeds Codex/Gemini limits)

#### Step 7.3: Deploy to production
- Pushed 43 commits to GitHub
- Pulled on VPS (`ssh root@ozzydb`)
- Dropped v2 database schema, let v3 migrations recreate (22 tables including new `jobs` + `environment_provider_images`)
- Rebuilt server Docker image (12m 20s compile)
- Restarted services: postgres (kept running) + server (recreated) + caddy (restarted)
- Rebuilt frontend (`npm install && npm run build`)
- Smoke test passed: `https://api.ozzydb.com/health` → `{"status":"ok","version":"0.1.0"}`
- Frontend live: `https://ozzydb.com` → 200 OK

## v3 Implementation Complete

All phases deployed to production:
- Phase 1: R2 Storage + Presigned URLs
- Phase 2: Async Job Model + Parallel DAG
- Phase 3: Fly Backend + Rate Limiting
- Phase 4: Admin Dashboard
- Phase 5: Cleanup + Local Dev Stack
- Phase 6: Polish (skipped — stretch goals)
- Phase 7: Deployment & Integration

---

## v3.1: Multi-Provider Compute + Storage Cleanup (COMPLETE)

See `planning/v3/v3.1_compute_providers.md` for full plan.

### Step 1: Storage cleanup (COMPLETE)
- Made R2/S3 required (was optional), removed local-only storage fallback
- Removed ~500 lines: cache_dir, local_path(), ensure_parent(), has_remote(), byte-proxying
- Updated all test Configs, docker-compose files
- Commit: `2c5fc9a`

### Step 2: Test infrastructure (COMPLETE)
- Created `docker-compose.test.yml` (Postgres port 5433, MinIO port 9002, minio-init bucket creation)
- Added Justfile commands: `test-infra-up`, `test-infra-down`, `test-infra-clean`
- Rewrote TestServer in both `integration_tests.rs` and `e2e_tests.rs` to use Compose-provided services
- Removed `testcontainers` and `testcontainers-modules` dependencies
- Added `test_db_url()` and `test_r2_config()` helpers with `TEST_*` env var overrides
- Added table truncation on startup (idempotent test runs)
- Removed strict `content_type` assertions (MinIO returns binary/octet-stream for presigned redirects)
- All 20 integration + 12 E2E tests pass
- Commit: `e9cdda9`

### Step 3: Unified I/O (COMPLETE)
- Added R2_PRESIGN_ENDPOINT config for compute-facing presigned URLs
- Added compute_s3_client to ContentStorage with _for_compute presigned URL methods
- Collapsed generate_docker_init/generate_fly_init into single generate_init()
- Rewrote Docker backend: no bind mounts, downloads/uploads via R2 presigned URLs
- Removed all is_fly branching from orchestrator (~6 branch points eliminated)
- Secrets always delivered via R2 presigned URL (not raw env vars)
- Source code always uploaded to R2 for container download
- Commit: `b21dc40`

### Step 4: Slim ComputeRequest/ComputeResult (COMPLETE)
- Removed from ComputeRequest: runner_script, runner_ext, init_script, inputs, source_dir, network, runtime
- Removed from ComputeResult: output_dir, workspace_dir, cleanup()
- Removed local_path from InputSpec
- Moved build_input_manifest() and build_param_env_vars() to types.rs
- Orchestrator encodes all I/O (init/runner scripts, determinism vars, output URL) into env_vars
- Orchestrator downloads output tarball from R2 after compute
- Commit: `0a43ccb`

### Step 5: ComputeRegistry (COMPLETE)
- Replaced BackendSelector enum with ComputeRegistry (HashMap<String, Arc<dyn ComputeBackend>>)
- ComputeBackend trait uses boxed futures for object safety (dyn dispatch)
- ComputeRegistry::resolve(machine) looks up named providers or falls back to default
- AppState.compute is now ComputeRegistry (not Option<BackendSelector>)
- Added GET /v1/compute/providers endpoint
- Added COMPUTE_DEFAULT_PROVIDER config option
- Orchestrator passes node_def.machine to resolve() for per-node backend selection
- Commit: `fa6751d`

### Step 6: Config restructure (COMPLETE)
- Split ComputeConfig into global (timeout_secs, tmpdir, default_provider) + DockerProviderConfig (enabled, runtime, memory_limit, cpu_limit)
- Resource limits now live in DockerBackend config (not in ComputeRequest)
- Env vars renamed: COMPUTE_ENABLED → DOCKER_COMPUTE_ENABLED, etc. (backwards compat maintained)
- Updated all docker-compose files and env examples
- Commit: `c313192`

### Step 7: Final cleanup (COMPLETE)
- R2/S3 now required in prod compose (was optional)
- Updated .env.prod.example with new provider-specific env var names
- Removed stale COMPUTE_TMPFS_SIZE references
- Commit: `c849338`

### Exhaustive Review: CONVERGED (3 rounds, 2 consecutive clean)
- Round 1: 6 fixes (M1 secrets presigned URL, M2 wave drain handles, M5 secrets key collision, m7/m8 Fly timeouts, m9 output key, m10 redundant check)
- Round 2: CLEAN + 2 minor fixes (m3 source cleanup, m4 curl flags)
- Round 3: CLEAN
- Review fix commits: `3b8f906`, `0295059`
- Models: Claude Opus only (~377k tokens, exceeds Codex/Gemini limits)

### Deployment: v3.1 deployed to production (2026-02-14)
- Pushed all v3.1 commits to GitHub
- Pulled on VPS, updated .env.prod with new env var names
- Rebuilt server Docker image, restarted services
- R2 storage live: `https://983eaa1bf70a5df64e477fbe4a50aaf5.r2.cloudflarestorage.com/ozzydb`
- Compute providers: docker + fly (default: fly)
- Health check passed: `https://api.ozzydb.com/health` → `{"status":"ok","version":"0.1.0"}`
- Frontend serving: `https://ozzydb.com` → 200 OK
