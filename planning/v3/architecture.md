# v3 Architecture

See also: `soul.md` (project principles), `riley_setup.md` (infrastructure setup tasks).

---

## v2 Gap Analysis

v2 is ~87% complete. The gaps are concentrated in **remote compute** and **cloud storage**.

### What shipped

| Component | Status | Notes |
|-----------|--------|-------|
| ozzy.toml parser | Complete | 40+ validation rules, 25+ tests |
| Data plane (upload/download/yank/metadata) | Complete | Content-addressed, deduped |
| Collections (versioned sets, circular ref detection) | Complete | 20+ integration tests |
| Push endpoint (git integration, source caching) | Complete | GitHub App, JWT, webhooks |
| Fetch endpoint (DAG execution, caching) | Complete | Topological sort, per-node cache |
| Environment building (tiers 1/2/3) | Complete | Async builds from push |
| Runners (Python, R, command) | Complete | Input manifest, output collection |
| CLI (all commands) | Complete | 100+ tests |
| Frontend (all pages) | Complete | SvelteKit 5, 5 review rounds |
| Python client | Complete | 48 unit tests |
| Auth (GitHub device flow, tokens, collaborators) | Complete | Account vs project scoped |
| Secrets (encrypted, injected, version-tracked) | Complete | AES-256-GCM |
| Security (gVisor, network isolation, path validation) | Complete | 21 review rounds |
| Verification tiers | Complete | Server-verified, client-computed |

### What's missing

| Gap | Severity | What exists |
|-----|----------|-------------|
| **Fly Machines backend** | Critical | Only DockerBackend. ComputeBackend trait ready. Fly init script written but unused. |
| **R2 presigned URLs** | Critical | R2 store/get works. No presigned URL generation. |
| **R2 in production** | Moderate | R2 env vars plumbed but empty. Local-only storage. |
| **Streaming uploads** | Moderate | Memory-buffered, 100MB cap. |
| **Async job model** | Critical | Fetch is synchronous — blocks until DAG completes. |

---

## How Runners Work

A **runner** is a generated script that bridges OzzyDB's I/O contract with user code.

### Runner types

- **Python** (`runners/python.rs`): For `source = "path/to/file.py:function_name"`
- **R** (`runners/r.rs`): For `source = "path/to/file.R:function_name"`
- **Command** (`runners/command.rs`): For shell templates like `command = "ffmpeg -i ${input.video} ${output}"`

### Workspace layout inside the container

```
/workspace/
  init.sh           <- entrypoint (generated)
  runner.py          <- generated runner script (or runner.R / runner.sh)
  source/            <- user's code from git (extracted from source tarball)
    transforms/
      clean.py
  inputs/            <- input files (downloaded from presigned URLs or bind-mounted)
    readings         <- single file
    all_readings/    <- collection (directory of files)
  output/            <- results (collected after execution)
    result.parquet   <- primary output
```

### Execution flow (v2, synchronous)

```
HTTP GET /v1/fetch/owner/project/endpoint
  → Resolve project → commit → commit_state
  → Validate params, check yank status
  → Topological sort of DAG nodes
  → For each node:
      → Resolve inputs, compute materialized hash
      → Cache hit? Skip. Cache miss? Generate runner + init, execute via Docker
  → Return terminal node output inline
```

### Unified I/O path (v3 change)

v2 had two init script variants (`generate_docker_init` for bind mounts, `generate_fly_init` for presigned URLs). v3 **eliminates this split** — all compute uses presigned URL I/O, regardless of provider:

- **Production**: Presigned URLs point to R2
- **Local dev**: Presigned URLs point to MinIO (S3-compatible, same code path)

One init script. One I/O contract. No bind mounts. This eliminates the Docker-specific escape hatch and means the same compute code runs identically on Fly, Docker, or any future provider.

### Input/output contract

**Inputs**: Runner loads `OZZY_INPUT_MANIFEST` env var (JSON: name → path + content_type), auto-loads based on type (parquet → DataFrame, JSON → dict, images → bytes, etc.). User function signature: `def func(inputs, params)`.

**Outputs**: Runner writes to `/workspace/output/result.*`. Server finds primary output (prefers `result.*`), hashes it, stores content-addressed, records cache entry.

**Params**: Available as `OZZY_PARAMS` (JSON blob) and `OZZY_PARAM_*` (individual env vars).

---

## v3 Principles

### 1. No application compute on the API server

The Hetzner VPS runs the API server, Postgres, and Caddy. It does NOT run user transforms. The fetch endpoint dispatches compute to Fly Machines. The API server only orchestrates.

The `ComputeBackend` trait already exists:
```rust
pub trait ComputeBackend: Send + Sync {
    async fn run(&self, request: ComputeRequest) -> Result<ComputeResult>;
    fn available_machines(&self) -> Vec<MachineConfig>;
}
```

#### Multi-provider strategy

The trait maps cleanly to any Docker-container-based provider: Fly, Docker (local), AWS ECS, GCP Cloud Run, Kubernetes.

**Fly Machines** is the first remote backend:
- Fly init script already written (`generate_fly_init`)
- REST API: create machine → wait for stopped → collect exit code → destroy
- Pay-per-second Firecracker VMs, `auto_destroy: true`
- **No GPU** (deprecated August 2025)

**Decision**: Implement Fly first. Defer Modal (GPU) until demand exists. Keep DockerBackend for local dev stack.

| Provider | Model | GPU | Fits trait? | Priority |
|----------|-------|-----|-------------|----------|
| Fly Machines | Firecracker VM | No | Yes | **v3** |
| Docker (local) | Docker container | No | Yes (built) | **Local dev** |
| Modal | Python function / sandbox | Yes | Awkwardly | Defer |
| AWS ECS / GCP Cloud Run / K8s | Docker container | Varies | Yes | Future |

#### Environment image registry

Environment images (built from `[environments]` in ozzy.toml) are pushed to **GHCR** as the canonical registry, then **mirrored to `registry.fly.io`** for Fly machines (Fly can't pull from private external registries).

```
Build → push to ghcr.io/ozzydb/envs/{hash}:latest (canonical)
     → push to registry.fly.io/ozzydb-compute:{hash} (Fly mirror)
```

Future providers that can pull from GHCR directly skip the mirror step.

#### Provider-agnostic environment tracking

The server tracks which providers have which environment images via a DB table:

```sql
CREATE TABLE environment_provider_images (
    id SERIAL PRIMARY KEY,
    env_hash TEXT NOT NULL,           -- environment content hash
    provider TEXT NOT NULL,           -- 'fly', 'docker', 'ecs', etc.
    image_ref TEXT NOT NULL,          -- provider-specific image reference
    pushed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(env_hash, provider)
);
```

When dispatching compute, the orchestrator queries this table for the target provider. If the environment isn't available on the requested provider, the job fails with a clear error (or queues for a push, future enhancement).

#### Batch init for same-environment sequential nodes

When multiple sequential nodes in a DAG share the same environment, they're grouped into a **single machine dispatch** with a batch init script:

```
DAG: A → B → C  (all use env "sci-py")

Instead of:
  Machine 1: run A, destroy
  Machine 2: run B, destroy
  Machine 3: run C, destroy

v3 does:
  Machine 1: run A, then B, then C, destroy
```

The batch init script downloads inputs for A, runs A, uploads output, downloads inputs for B (which may include A's output), runs B, uploads, and so on. This eliminates cold starts for sequential same-environment chains.

Independent nodes with different environments still dispatch to separate machines in parallel.

### 2. No data storage on the API server

All blobs live on **Cloudflare R2**. Local disk is a read-through cache only.

- `ContentStorage` already supports R2 via `object_store` crate
- Add presigned URL generation via `aws-sdk-s3` (R2 is S3-compatible)
- Presigned GET for compute machine input downloads (4h TTL)
- Presigned PUT for compute machine output uploads (4h TTL)

#### Streaming downloads via presigned redirect

Data and job output downloads use **302 redirect to R2 presigned URL** — the server never proxies blob bytes:

```
GET /v1/jobs/{id}/output
  → Server generates presigned GET URL for output blob on R2
  → 302 Location: https://<account>.r2.cloudflarestorage.com/ozzydb/<hash>?X-Amz-...
  → Client follows redirect, downloads directly from R2 edge

GET /v1/data/{owner}/{project}/{name}/download
  → Same pattern: 302 redirect to R2
```

Benefits:
- **Zero server bandwidth** for downloads (R2 egress is free)
- **Faster** — Cloudflare edge is geographically distributed
- **Scales infinitely** — 1000 concurrent downloads don't touch the VPS
- Server returns `X-OzzyDB-Content-Hash` header so clients can verify after download

Presigned URL TTL: 1 hour. Content-addressed, so a leaked URL only exposes that specific immutable blob.

### 3. Transforms cannot reference local data

All data must be uploaded to OzzyDB before transforms can reference it. No `--local-data`, no bind mounts. The workflow is always: upload → reference → transform. The data contract is identical whether talking to cloud or local dev stack.

### 4. Local dev via switchable server endpoint

Users run a full local OzzyDB stack (server + Postgres + MinIO) and point their CLI at it:

```bash
ozzy remote set http://localhost:3000    # local
ozzy remote set https://api.ozzydb.com   # cloud (default)
```

All commands hit whichever server the CLI is pointed at. Same data contract everywhere.

**Local stack** (`docker-compose.dev.yml`):
- `ozzy-server` (same binary as production)
- `postgres:17`
- `minio` (S3-compatible, stands in for R2)
- DockerBackend for compute (no Fly locally)
- Simplified auth (auto-create dev user, skip GitHub OAuth)

### 5. Async job model with parallel DAG execution

Replace synchronous fetch with an async job model.

#### API contract

```
POST /v1/fetch/{owner}/{project}/{endpoint}?ref=main&threshold=12.5
  → 202 Accepted
  → { "job_id": "uuid", "status": "queued" }

GET /v1/jobs/{job_id}
  → { "status": "running", "nodes": {"qc": "done", "merge": "running", "final": "queued"} }

GET /v1/jobs/{job_id}
  → { "status": "done", "output_url": "/v1/jobs/{job_id}/output", "output_hash": "abc123..." }

GET /v1/jobs/{job_id}/output
  → 200, binary output

GET /v1/jobs/{job_id}/logs
  → streaming SSE of execution progress
```

CLI and Python client poll until completion, displaying progress.

#### Cache-hit fast path

Before creating any machines, the orchestrator checks the materialized cache for every DAG node. If all nodes are cached, the job completes immediately — no compute dispatched. The `/output` endpoint redirects to the cached blob on R2.

Partial cache hits skip only the cached nodes; uncached nodes dispatch normally.

#### Job deduplication

Before creating a new job, check for an existing active job with the same `(project_id, endpoint_name, commit_id, params_hash)`. If found, return the existing `job_id` instead of creating a duplicate. This prevents waste from duplicate fetch requests (e.g., user double-clicks, CI retries).

#### Parallel DAG execution

Independent nodes dispatch to separate machines concurrently. Sequential nodes sharing an environment are batched into a single machine (see batch init above).

```
DAG: A ──→ C
     B ──→ C

Execution:
  1. Check cache for A, B, C (fast path: all cached → done)
  2. Dispatch A and B in parallel (separate machines if different envs)
  3. Await both
  4. Dispatch C
  5. Await C, store output, mark job done
```

Server orchestrates with `tokio::join!` — dispatch ready nodes, await, check unblocked, repeat.

#### Job table schema

```sql
CREATE TABLE jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id INT NOT NULL REFERENCES projects(id),
    endpoint_name TEXT NOT NULL,
    commit_id INT NOT NULL REFERENCES commits(id),
    params JSONB NOT NULL DEFAULT '{}',
    params_hash TEXT NOT NULL,  -- blake3 of canonical params, for dedup lookups
    status TEXT NOT NULL DEFAULT 'queued',  -- queued, running, done, failed
    node_status JSONB NOT NULL DEFAULT '{}',  -- {"node_name": "queued|running|done|failed"}
    output_hash TEXT,
    output_content_type TEXT,
    error_message TEXT,
    created_by INT REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at TIMESTAMPTZ,
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ  -- for periodic cleanup
);

-- Deduplication index: find active jobs for same request
CREATE INDEX idx_jobs_dedup ON jobs (project_id, endpoint_name, commit_id, params_hash)
    WHERE status IN ('queued', 'running');
```

#### Job cleanup

Job rows are small (metadata only) — output blobs live in the materialized cache on R2, not duplicated. Add `expires_at TIMESTAMPTZ` column; periodic cleanup deletes expired rows. No separate output storage to manage. Keep it simple, worry about scale later.

#### Secrets delivery

Project secrets are injected into compute machines via presigned URL:

1. Server encrypts secrets blob (AES-256-GCM) with a one-time key derived from master key + job-specific salt
2. Encrypted blob stored on R2 at a unique path
3. Machine receives two env vars: `OZZY_SECRETS_URL` (presigned GET for encrypted blob) and `OZZY_SECRETS_KEY` (one-time decryption key)
4. Init script downloads blob, decrypts, injects secrets as env vars, deletes plaintext
5. Encrypted blob on R2 deleted after job completes (or expires)

The compute provider (Fly, Docker, etc.) sees the decryption key in env vars — this is the standard trust model for all cloud compute. The key is one-time (unique per job invocation), so compromising one key doesn't expose other jobs' secrets. The encrypted blob on R2 is worthless without the key.

#### Queue (deferred to v3.5)

In-memory orchestration with job table in Postgres is sufficient for initial v3. Proper queue (Postgres SKIP LOCKED or Redis) when concurrency demands it.

### 6. Streaming uploads

Stream uploads — BLAKE3 hash while streaming to R2, never buffer full file.

- `axum::body::Body` streaming with `StreamExt`
- Pipe chunks through BLAKE3 hasher and to R2 simultaneously
- R2 multipart upload for files >5MB
- `aws-sdk-s3` crate supports streaming multipart
- Raise body limit to 10GB+ (server memory stays flat)
- CLI upload progress bar

### 7. Compute rate limiting + admin dashboard

Guardrails before Fly goes live:

**Rate limits**:
- **Global concurrent machine cap**: Max Fly machines running at once (e.g., 20). Configurable.
- **Per-user concurrent job cap**: Max simultaneous jobs per user (e.g., 5). 429 when exceeded.
- **Per-user daily limit**: Optional, prevent monopolization.

**Implementation**:
- Track active jobs in `jobs` table (status = 'running')
- Check counts before Fly dispatch
- At capacity → job stays 'queued', background task dispatches when slot opens

**Admin dashboard**:
- `/admin` route (admin flag on user)
- View: active jobs, queued, recent history
- Configure: global cap, per-user cap, daily limits
- Actions: cancel job, kill machine, ban user
- View: cost estimate

### Platform hash (documentation, not code change)

Materialized hash includes `platform_hash` (os, arch, libc, cpu_features, blas, python_version). This is correct — core reproducibility guarantee.

Local macOS and remote Fly/Linux produce different hashes for identical inputs/code/params. Not a bug — different platforms can produce different floating-point results.

**Document prominently**:
- Cache is platform-specific
- Fly provides consistent hashes (same Linux image)
- Local and remote maintain separate caches
- Frame as feature: "cached results are bit-identical to fresh execution on same platform"

---

## Deferred to v4+

| Item | Reason |
|------|--------|
| **Managed Postgres** (Neon, Supabase, Fly Postgres) | No users yet. pg_dump to R2 as interim. |
| **Multi-server redundancy** | Single VPS fine for now. Scale when needed. |
| **Job queue** (Redis / Postgres SKIP LOCKED) | In-memory orchestration sufficient initially. |
| **Modal GPU integration** | No GPU demand yet. |

---

## Implementation Phases

### Phase 1: R2 Storage + Streaming Uploads
1. Create R2 bucket, set env vars in production (see `riley_setup.md`)
2. Verify existing upload/download/fetch flows work with R2
3. Add presigned URL generation (`aws-sdk-s3` crate)
4. Refactor upload to stream: BLAKE3 hash while streaming to R2 (multipart for large files)
5. Raise body limit (10GB+), add CLI upload progress bar
6. Migrate any existing local blobs to R2

### Phase 2: Async Job Model + Parallel DAG
1. Add `jobs` table to Postgres schema
2. Change `POST /v1/fetch/...` to return 202 + job_id
3. Add `GET /v1/jobs/{id}`, `/output`, `/logs` endpoints
4. Implement DAG orchestrator: dispatch independent nodes in parallel, await, dispatch dependents
5. Update CLI `ozzy fetch` to poll job status with progress display
6. Update Python client `fetch()` to poll
7. SSE endpoint for streaming progress (nice-to-have)

### Phase 3: Fly Backend + Rate Limiting
1. Implement `FlyBackend` against Machines API
   - Machine creation with presigned URL env vars
   - Poll via `/wait?state=stopped`
   - Collect exit code, destroy
2. Wire into DAG orchestrator (config-driven backend selection)
3. Rate limiting: global cap, per-user cap, queuing
4. Remove Docker socket mount from `docker-compose.prod.yml`
5. Orphan machine cleanup (periodic scan)
6. Build and push compute base image to `registry.fly.io`
7. E2E test with real Fly execution

### Phase 4: Admin Dashboard
1. Admin flag on users table
2. Admin API: list jobs, cancel, view/set rate limits
3. Admin page in frontend (or CLI `ozzy admin`)
   - Active/queued/recent jobs
   - Configure caps
   - Kill machine, ban user
   - Cost estimate

### Phase 5: Data-First Workflow + Local Dev Stack
1. Remove `ozzy run` command and `--local-data` (see cleanup inventory)
2. Polish `docker-compose.dev.yml` for users
3. Local dev auth bypass
4. Optional `ozzy dev up/down` CLI sugar
5. Document platform hash behavior
6. Update getting_started.md, README, Python client

### Phase 6: Polish
- Compute tier selection (cpu-small, cpu-large, gpu-small)
- Per-project compute config in ozzy.toml
- Automated pg_dump to R2
- `ozzy dev` improvements

---

## Cleanup Inventory

Once v3 lands, the following becomes dead or redundant.

### Delete entirely (~2000 lines)

| Item | Location | Lines | Reason |
|------|----------|-------|--------|
| `ozzy run` command | `crates/ozzy-cli/src/commands/run.rs` | ~1573 | Replaced by `ozzy fetch` against any server |
| `--local-data` flag | `run.rs` lines 432-445, 798-909 | ~120 | Goes with `ozzy run` |
| CLI pipeline helpers | `commands/shared.rs` — `execute_pipeline()`, `execute_node_cached/no_cache()` | ~300 | Only used by `ozzy run` |
| Python client `run()` | `clients/python/src/ozzydb/client.py` lines 259-315 | ~56 | Use `fetch()` instead |
| CLI run tests | ozzy-cli test files | ~200 | Command deleted |

### Remove from production config only

Stay in codebase for local dev stack, removed from `docker-compose.prod.yml`:

| Item | Reason |
|------|--------|
| Docker socket mount | Server no longer runs containers |
| Shared tmpdir mount | Data via presigned URLs |
| `COMPUTE_ENABLED` | Replace with Fly token presence check |
| `COMPUTE_MEMORY_LIMIT`, `CPU_LIMIT`, `TMPFS_SIZE` | Fly has its own resource config |

### Code that stays but shifts role

| Item | Production | Local dev |
|------|-----------|-----------|
| DockerBackend | Not used (Fly) | Active |
| `generate_fly_init()` | Used everywhere | Used everywhere (unified I/O) |
| Docker env builder | Not used | Active |
| R2 optional fallback | R2 mandatory | MinIO via S3 API |
| gVisor runtime config | Not used by Fly | Available for alternative providers |

### Additional deletions (v3 unified I/O)

| Item | Reason |
|------|--------|
| `generate_docker_init()` | Replaced by unified presigned URL init script everywhere |
| Bind mount logic in DockerBackend | Docker containers use presigned URLs to MinIO, no bind mounts |

### Tests to rewrite

| Item | Change |
|------|--------|
| Docker integration tests | Mock compute backend or target Fly |
| E2E tests | Mock Fly or run against local dev stack |

### Docs to update

| Item | Change |
|------|--------|
| `README.md` | Remove `ozzy run`, add local dev stack |
| `docs/getting_started.md` | Upload-first workflow |
| `CLAUDE.md` | Update CLI commands |

### Fly Machines API Reference

```
Base URL: https://api.machines.dev
Auth: Authorization: Bearer fly_...

POST /v1/apps/{app}/machines          Create machine
GET  /v1/apps/{app}/machines/{id}     Get machine (read exit code)
GET  /v1/apps/{app}/machines/{id}/wait?state=stopped&timeout=300
DELETE /v1/apps/{app}/machines/{id}?force=true
```

Machine create body:
```json
{
  "name": "ozzy-job-{uuid}",
  "region": "fra",
  "config": {
    "image": "registry.fly.io/ozzydb-compute:latest",
    "auto_destroy": true,
    "env": {
      "PYTHONHASHSEED": "0",
      "OMP_NUM_THREADS": "1",
      "OZZY_PARAMS": "{...}",
      "OZZY_INPUT_MANIFEST": "{...}",
      "OZZY_INPUT_URL_readings": "https://...r2.cloudflarestorage.com/...",
      "OZZY_OUTPUT_URL": "https://...r2.cloudflarestorage.com/...",
      "OZZY_SECRETS_URL": "https://...r2.cloudflarestorage.com/secrets/...",
      "OZZY_SECRETS_KEY": "<one-time AES key, hex>"
    },
    "guest": { "cpu_kind": "shared", "cpus": 1, "memory_mb": 512 },
    "restart": { "policy": "no" }
  }
}
```

Key caveats:
- Cold start: ~10-15s per new machine
- Machine limit: 50 per app (contact to increase)
- Rate limits: 1 req/s per action (burst 3)
- Failed + auto_destroy: ~2hr delay before cleanup
- Fly can't auth to external private registries — push to `registry.fly.io`
- Regions: `fra` (Frankfurt, closest to Hetzner), `ams`, `lhr`, `iad` as fallbacks
