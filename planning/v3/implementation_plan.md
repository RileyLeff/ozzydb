# v3 Implementation Plan

See `architecture.md` for the full design and `soul.md` for project principles.

---

## Phase 1: R2 Storage + Presigned URLs + Streaming Uploads

**Goal:** All blob storage goes through R2. Presigned URL infrastructure ready for compute. Uploads stream instead of buffering.

### Step 1.1: Presigned URL generation

Add `aws-sdk-s3` crate for presigned URL generation (the existing `object_store` crate doesn't support presigning). Extend `ContentStorage` with:

- `presigned_get(hash, ext, ttl) -> Result<Url>` — generates S3-compatible presigned GET URL
- `presigned_put(key, ttl) -> Result<Url>` — generates S3-compatible presigned PUT URL

R2 uses AWS Sig V4, so `aws-sdk-s3` works directly. The `R2Config` already has endpoint/bucket/credentials — reuse those to build the S3 client.

**Files:**
- `crates/ozzy-server/Cargo.toml` — add `aws-sdk-s3`, `aws-config`
- `crates/ozzy-server/src/storage/content.rs` — add presigned methods, hold an `aws_sdk_s3::Client` alongside existing `object_store`
- `crates/ozzy-server/src/storage/mod.rs` — re-export if needed

**Tests:**
- Unit test: presigned URL format contains expected bucket/key/signature params
- Integration test (MinIO): generate presigned GET, upload via object_store, download via presigned URL with reqwest

### Step 1.2: Streaming downloads via presigned redirect

Change data download and (future) job output endpoints to return 302 redirects to R2 presigned URLs instead of proxying bytes through the server.

**Files:**
- `crates/ozzy-server/src/api/v1/data.rs` — modify download handler: generate presigned GET → return 302 with `Location` header + `X-OzzyDB-Content-Hash` header
- Keep fallback: if R2 not configured (local dev without MinIO), proxy bytes as before

**Tests:**
- Integration test: upload data, request download, verify 302 redirect to presigned URL
- Integration test: follow redirect, verify content matches original

### Step 1.3: Streaming uploads

Replace memory-buffered upload with streaming: BLAKE3 hash while streaming to R2, multipart for large files.

**Files:**
- `crates/ozzy-server/src/api/v1/data.rs` — rewrite upload handler to stream multipart field body
- `crates/ozzy-server/src/storage/content.rs` — add `store_stream(stream, ext) -> Result<(String, u64)>` that hashes + uploads concurrently
- `crates/ozzy-server/src/config.rs` — raise `max_upload_size_bytes` default to 10GB

**Implementation:**
- Use `aws-sdk-s3` `CreateMultipartUpload` / `UploadPart` / `CompleteMultipartUpload` for files >5MB
- Single `PutObject` for files ≤5MB
- BLAKE3 hasher receives each chunk as it's forwarded to R2
- After upload completes, rename the R2 object to its content-addressed key (or upload to temp key, then copy+delete)

**Tests:**
- Unit test: streaming hash produces same result as buffered hash
- Integration test (MinIO): stream upload of 10MB file, verify hash, download and compare
- Integration test: verify small files (<5MB) use single PUT, large files use multipart

### Step 1.4: CLI upload progress bar

Add progress bar to `ozzy data add` for uploads.

**Files:**
- `crates/ozzy-cli/src/commands/data.rs` — wrap upload with indicatif progress bar
- `crates/ozzy-cli/Cargo.toml` — add `indicatif` if not already present

**Tests:**
- Manual test (progress bar is visual)

### Step 1.5: Deploy R2 to production

Copy `.env.prod` to VPS, redeploy server, verify R2 connectivity.

**Steps:**
- Copy R2 env vars to VPS `.env.prod`
- Rebuild + restart server
- Smoke test: upload a file via CLI, verify it lands in R2 bucket
- Migrate any existing local blobs to R2 (scan local storage dir, upload each to R2)

---

## Phase 2: Async Job Model + Parallel DAG

**Goal:** Fetch returns a job ID. Server orchestrates DAG execution asynchronously with parallel independent nodes.

### Step 2.1: Jobs table + migration

Add the `jobs` table and `environment_provider_images` table to Postgres.

**Files:**
- `crates/ozzy-server/migrations/002_v3_jobs.sql` — new migration with jobs table, environment_provider_images table, dedup index
- `crates/ozzy-server/src/db/` — add `jobs.rs` module with CRUD operations:
  - `create_job()` — insert new job, return job_id
  - `get_job()` — fetch job by ID
  - `find_active_job()` — dedup lookup by (project_id, endpoint_name, commit_id, params_hash)
  - `update_job_status()` — transition status
  - `update_node_status()` — update individual node status in JSONB
  - `set_job_output()` — set output_hash + content_type on completion
  - `cleanup_expired_jobs()` — delete where expires_at < now()

**Tests:**
- DB tests: create, read, update status, dedup lookup, cleanup

### Step 2.2: Fetch endpoint → async (POST + 202)

Convert the fetch endpoint from synchronous GET to async POST returning 202 + job_id.

**Files:**
- `crates/ozzy-server/src/api/v1/fetch.rs` — major rewrite:
  - `POST /v1/fetch/{owner}/{slug}/{endpoint}` → validate request, check dedup, create job row, spawn orchestrator task, return 202 `{ job_id, status: "queued" }`
  - Cache-hit fast path: if all nodes cached, complete job immediately, return 200 with output URL
  - Extract DAG resolution logic into reusable functions (already partially factored)

**Tests:**
- API test: POST fetch → 202 with job_id
- API test: POST fetch with all nodes cached → immediate completion
- API test: duplicate POST → returns existing job_id

### Step 2.3: Job status + output + logs endpoints

Add endpoints for polling job status and retrieving results.

**Files:**
- `crates/ozzy-server/src/api/v1/jobs.rs` — new module:
  - `GET /v1/jobs/{id}` → job status with per-node breakdown
  - `GET /v1/jobs/{id}/output` → 302 redirect to presigned URL for output blob (or 404 if not done)
  - `GET /v1/jobs/{id}/logs` → job logs (simple JSON for now, SSE later)
- `crates/ozzy-server/src/api/v1/mod.rs` — register new routes

**Tests:**
- API test: create job, poll status, verify transitions (queued → running → done)
- API test: completed job → /output returns 302 to presigned URL
- API test: incomplete job → /output returns 404 or 409

### Step 2.4: DAG orchestrator

Implement the async DAG executor that runs as a spawned Tokio task.

**Files:**
- `crates/ozzy-server/src/compute/orchestrator.rs` — new module:
  - `run_job(job_id, ...)` — main orchestration loop:
    1. Topological sort (reuse existing Kahn's algorithm from fetch.rs)
    2. Identify ready nodes (all inputs satisfied)
    3. Check materialized cache for each ready node
    4. Dispatch uncached ready nodes in parallel via `ComputeBackend` trait
    5. Await results, update node_status in DB
    6. Repeat until terminal node completes or failure
    7. Set job output_hash, mark done/failed
  - Batch init: group sequential same-environment nodes into single dispatch
- `crates/ozzy-server/src/compute/mod.rs` — register orchestrator module

**Tests:**
- Unit test: batch grouping algorithm (same-env sequential → single group)
- Unit test: parallel dispatch identification (independent nodes)
- Integration test: mock compute backend, verify correct dispatch order and parallelism

### Step 2.5: ComputeBackend trait

Extract the compute abstraction. Currently `docker.rs` has a bare `run()` function — wrap it in a trait.

**Files:**
- `crates/ozzy-server/src/compute/types.rs` — add trait definition:
  ```rust
  #[async_trait]
  pub trait ComputeBackend: Send + Sync {
      async fn run(&self, request: ComputeRequest) -> Result<ComputeResult>;
  }
  ```
  - Update `ComputeRequest`: remove `local_path` from `InputSpec`, add presigned URL fields
  - Update `ComputeResult`: remove `output_dir`/`workspace_dir` (local paths), add `output_storage_key`
- `crates/ozzy-server/src/compute/docker.rs` — implement trait for `DockerBackend` struct
  - Adapt to unified I/O: generate presigned URLs from MinIO/R2, use `generate_fly_init()` (unified init script)
  - Remove bind mount input logic
- `crates/ozzy-server/src/compute/mod.rs` — export trait, add `BackendSelector` that picks Docker or Fly based on config

**Tests:**
- Unit test: BackendSelector returns correct backend based on config
- Existing Docker integration tests should still pass (adapted for new interface)

### Step 2.6: Update CLI `ozzy fetch`

CLI fetch becomes async: POST, poll, display progress, download output.

**Files:**
- `crates/ozzy-cli/src/commands/fetch.rs` — rewrite:
  - POST to fetch endpoint, get job_id
  - Poll `GET /v1/jobs/{id}` in a loop with progress display
  - On completion, follow `/output` redirect to download result
  - Display per-node status (queued/running/done) during poll

**Tests:**
- CLI integration test: mock server, verify poll loop behavior

### Step 2.7: Update Python client `fetch()`

Python client fetch becomes async-aware: POST, poll, return result.

**Files:**
- `clients/python/src/ozzydb/client.py` — update `fetch()`:
  - POST instead of GET
  - Poll loop with configurable interval
  - Follow presigned URL redirect for output download
  - Display progress if verbose

**Tests:**
- Python unit tests for new poll logic

---

## Phase 3: Fly Backend + Rate Limiting

**Goal:** Production compute runs on Fly Machines. Rate limiting prevents runaway costs.

### Step 3.1: FlyBackend implementation

Implement the `ComputeBackend` trait for Fly Machines.

**Files:**
- `crates/ozzy-server/src/compute/fly.rs` — new module:
  - `FlyBackend` struct with Fly API token, app name, region
  - `run()` implementation:
    1. Generate presigned URLs for inputs (GET) and output (PUT)
    2. Build machine config JSON (image, env vars, guest config)
    3. `POST /v1/apps/{app}/machines` — create machine
    4. `GET /v1/apps/{app}/machines/{id}/wait?state=stopped` — await completion
    5. Read machine status for exit code
    6. `DELETE /v1/apps/{app}/machines/{id}?force=true` — cleanup
    7. Verify output was uploaded to R2 (check presigned PUT target)
    8. Return `ComputeResult` with output storage key
  - Batch init: for sequential same-env nodes, generate combined init script
  - Error handling: Fly API errors, timeouts, non-zero exit codes
- `crates/ozzy-server/Cargo.toml` — add `reqwest` features if not present (for Fly API calls)
- `crates/ozzy-server/src/config.rs` — add `FlyConfig` (token, app_name, region, api_url)

**Tests:**
- Unit test: machine config JSON generation
- Unit test: init script generation with presigned URLs
- Integration test: mock Fly API server, verify create → wait → delete sequence
- E2E test (optional, requires real Fly): create machine with simple Python script, verify output

### Step 3.2: Environment image management

Build and push environment images to GHCR + mirror to Fly registry. Track in DB.

**Files:**
- `crates/ozzy-server/src/compute/environments.rs` — new module:
  - `push_to_provider(env_hash, provider)` — push image to provider's registry
  - `get_image_ref(env_hash, provider) -> Option<String>` — lookup from `environment_provider_images` table
  - `ensure_available(env_hash, provider)` — push if not tracked, return image ref
- `crates/ozzy-server/src/db/environments.rs` — DB operations for `environment_provider_images`

**Tests:**
- DB test: insert, lookup, upsert
- Unit test: image ref formatting for different providers

### Step 3.3: Rate limiting

Implement global and per-user concurrent job caps.

**Files:**
- `crates/ozzy-server/src/compute/rate_limit.rs` — new module:
  - `check_limits(user_id) -> Result<(), RateLimitError>` — query active job counts, enforce caps
  - `RateLimitConfig` — global_cap, per_user_cap (from server config or DB)
- `crates/ozzy-server/src/api/v1/fetch.rs` — check rate limits before creating job
- `crates/ozzy-server/src/config.rs` — add rate limit config fields
- `crates/ozzy-server/migrations/003_v3_rate_limits.sql` — rate limit config table (if admin-configurable)

**Tests:**
- Unit test: rate limit logic (at cap → error, under cap → ok)
- API test: exceed per-user cap → 429

### Step 3.4: Orphan machine cleanup

Periodic task to find and destroy orphaned Fly machines.

**Files:**
- `crates/ozzy-server/src/compute/fly.rs` — add `cleanup_orphans()`:
  - List all machines in the app
  - Compare against active jobs in DB
  - Destroy machines with no matching job (or older than threshold)
- `crates/ozzy-server/src/main.rs` — spawn periodic cleanup task (every 5 minutes)

**Tests:**
- Unit test: orphan detection logic
- Integration test with mock Fly API

### Step 3.5: Secrets delivery for compute

Implement the presigned-URL-based secrets injection for compute machines.

**Files:**
- `crates/ozzy-server/src/compute/secrets.rs` — new module:
  - `prepare_secrets(project_id, job_id) -> Result<(Url, String)>` — encrypt secrets blob, upload to R2, return (presigned_url, decryption_key)
  - `cleanup_secrets(r2_key)` — delete encrypted blob after job completes
- `crates/ozzy-server/src/compute/orchestrator.rs` — call `prepare_secrets()` before dispatch, pass URL + key as env vars, cleanup after job

**Tests:**
- Unit test: encrypt/decrypt roundtrip
- Integration test: secrets available inside compute container

### Step 3.6: Production deployment

Wire Fly into production, remove Docker socket mount.

**Steps:**
- Copy Fly env vars to VPS `.env.prod`
- Build and push base compute image to `registry.fly.io`
- Update `docker-compose.prod.yml`: remove Docker socket mount, shared tmpdir
- Rebuild + restart server
- E2E smoke test: push a project, fetch an endpoint, verify Fly execution

---

## Phase 4: Admin Dashboard

**Goal:** Admin can monitor jobs, configure rate limits, and manage users.

### Step 4.1: Admin flag + API

Add admin flag to users table, admin-only API endpoints.

**Files:**
- `crates/ozzy-server/migrations/004_v3_admin.sql` — `ALTER TABLE users ADD COLUMN is_admin BOOLEAN DEFAULT false`
- `crates/ozzy-server/src/auth/` — add `AdminUser` extractor (rejects non-admin)
- `crates/ozzy-server/src/api/v1/admin.rs` — new module:
  - `GET /v1/admin/jobs` — list active/queued/recent jobs with filtering
  - `POST /v1/admin/jobs/{id}/cancel` — cancel job, kill machine
  - `GET /v1/admin/rate-limits` — current config
  - `PUT /v1/admin/rate-limits` — update config
  - `GET /v1/admin/users` — list users
  - `POST /v1/admin/users/{id}/ban` — ban user (prevent job creation)

**Tests:**
- API test: non-admin → 403
- API test: admin can list/cancel jobs
- API test: admin can update rate limits

### Step 4.2: Admin frontend page

Add admin page to SvelteKit frontend.

**Files:**
- `frontend/src/routes/admin/+page.svelte` — admin dashboard:
  - Active/queued/recent jobs table with auto-refresh
  - Rate limit configuration form
  - User management (ban/unban)
  - Cost estimate (jobs × tier × duration)
- `frontend/src/lib/api.ts` — admin API client functions

**Tests:**
- Manual testing (visual)
- TypeScript type checking (`npm run check`)

---

## Phase 5: Cleanup + Local Dev Stack

**Goal:** Remove dead code, build local dev Docker Compose, update docs.

### Step 5.1: Delete dead code

Remove `ozzy run` and related code per cleanup inventory.

**Files:**
- Delete `crates/ozzy-cli/src/commands/run.rs` (~1573 lines)
- `crates/ozzy-cli/src/commands/mod.rs` — remove run command registration
- `crates/ozzy-cli/src/commands/shared.rs` — remove `execute_pipeline()`, `execute_node_cached()`, `execute_node_no_cache()`
- `clients/python/src/ozzydb/client.py` — remove `run()` method
- Delete `generate_docker_init()` from `crates/ozzy-server/src/runners/init.rs`
- Remove bind mount logic from `crates/ozzy-server/src/compute/docker.rs`
- Delete associated tests

**Tests:**
- `just test` — verify nothing breaks
- `cargo build` — verify no dead code warnings

### Step 5.2: Local dev Docker Compose

Create `docker-compose.dev.yml` for local development.

**Files:**
- `docker-compose.dev.yml` (repo root):
  - `ozzy-server` (same binary, dev config)
  - `postgres:17`
  - `minio` (S3-compatible, replaces R2)
  - Auto-create dev user (skip GitHub OAuth)
- `crates/ozzy-server/docker/.env.dev` — dev defaults (MinIO creds, local URLs)

**Tests:**
- `docker compose -f docker-compose.dev.yml up` — verify stack starts
- Upload + fetch through local stack

### Step 5.3: `ozzy dev` CLI sugar (optional)

Add convenience commands for local dev stack.

**Files:**
- `crates/ozzy-cli/src/commands/dev.rs` — `ozzy dev up`, `ozzy dev down`, `ozzy dev status`
- Shells out to `docker compose` with the dev compose file

### Step 5.4: Documentation updates

Update all docs for v3 reality.

**Files:**
- `README.md` — remove `ozzy run`, add local dev stack, update architecture section
- `docs/getting_started.md` — upload-first workflow, async fetch
- `CLAUDE.md` — update CLI commands, remove `ozzy run`
- New `docs/platform_hash.md` — explain platform hash behavior, remote vs local caching

---

## Phase 6: Polish (stretch goals)

Not detailed here — tracked in architecture.md:
- Compute tier selection (cpu-small, cpu-large)
- Per-project compute config in ozzy.toml
- Automated pg_dump to R2
- `ozzy dev` improvements
