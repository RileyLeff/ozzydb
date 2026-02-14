# v3 Workflow State

## Current Phase: Phase 2 — Async Job Model + Parallel DAG
## Current Step: Step 2.2 COMPLETE, starting Step 2.3

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

### Phase 2: Async Job Model + Parallel DAG (IN PROGRESS)

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
- All 110 library unit tests pass

## Deferred Steps

### Step 1.4: CLI upload progress bar
**Reason:** CLI `ozzy data add` is not yet implemented (stub only).

### Step 1.5: Deploy R2 to production
**Reason:** Requires SSH access to VPS. Will be done manually.

## What's Next
- Step 2.3: Job status + output + logs endpoints
- Step 2.4: DAG orchestrator (parallel node execution)
- Step 2.5: ComputeBackend trait
- Step 2.6: Update CLI ozzy fetch for async model
- Step 2.7: Update Python client fetch()
- Phase 2 exhaustive review
