# OzzyDB Architecture Specification

**Version**: 0.3.0-draft
**Author**: Riley Leff
**Date**: February 2026
**Revised**: Incorporates feedback from Gemini review (local-first strategy, adjacency-based DAG model, platform-aware hashing, strict network isolation) and Codex review (determinism contracts, platform fingerprinting, local commit/ref model, canonicalization rules)

---

## 1. Vision

Git versions text. OzzyDB versions *data transformations*.

Scientific data is almost never consumed raw. It passes through calibration, quality control, unit conversion, filtering, aggregation — a long pipeline of transformations that vary by consumer, by paper, by lab. Today, researchers manage this by saving dozens of derived files (`data_final_v3_corrected_REAL.csv`) with no record of how they were produced, no reproducibility guarantee, and no way for a collaborator to retrieve exactly the right version.

OzzyDB solves this by applying the Kolmogorov complexity insight: the minimal description of a derived dataset is often `f(raw)`, not the materialized output. Instead of versioning data, we version the *instructions* for producing data — pure functions over immutable raw inputs — and serve the results as lazy, cached API endpoints.

### Core Principles

1. **Transforms are the versioned artifact, not data.** Raw data is immutable. Derived data is always reproducible from `(raw, transforms, params, dependencies)`.
2. **Content-addressed everything.** The identity of any artifact is its hash. Same inputs → same hash → cache hit.
3. **Lazy materialization with aggressive caching.** Nothing is computed until requested. Once computed, results are cached at every node in the DAG.
4. **Bring your own tools.** Transforms are written in normal Python, R, Julia, Rust, Go, etc. No DSL, no lock-in. The only contract is the function signature and the data schema.
5. **Endpoints are API-servable.** Every named pipeline is an HTTP endpoint. Data is a service, not a file.

---

## 2. Conceptual Model

### 2.1 The Transform DAG

OzzyDB's core abstraction is a directed acyclic graph (DAG) of transformations:

```
                        ┌─► apply_calibration(leff_2024) ─► [corrected]
raw ─► quality_control ─┤
                        └─► apply_calibration(granier_1987) ─► [corrected_granier]
```

**Nodes** are either:
- **Data sources**: Raw, immutable blobs (parquet files in object storage)
- **Transforms**: Pure functions `(ArrowRecordBatch, Params) → ArrowRecordBatch`

**Edges** are data dependencies.

**Endpoints** are named pointers to specific nodes in the DAG, analogous to git refs or branches.

### 2.2 Content Addressing

Every artifact in the system is identified by its content hash:

```
raw_data_hash        = blake3(raw_parquet_bytes)
transform_hash       = blake3(canonical_source + lockfile_hash + runtime_version + params_schema_hash)
materialized_hash    = blake3(input_hash + transform_hash + canonical_params_hash + platform_fingerprint)
```

**Why `platform` is in the hash**: Python floating-point operations can produce bitwise-different results across CPU architectures (ARM vs x86, different SIMD implementations). Rather than sacrificing precision by canonicalizing floats, we include the platform in the hash. This means ARM and x86 caches are separate — which is correct behavior. The identity `(input, transform, params, platform)` is honest about what actually determines the output.

### 2.2.1 Platform Fingerprint

The platform fingerprint is a structured hash of the execution environment:

```json
{
  "os": "linux",
  "arch": "x86_64",
  "libc": "glibc-2.31",
  "cpu_features": ["avx2", "fma"],
  "blas": "openblas-0.3.21",
  "python_version": "3.11.8"
}
```

**Components:**
- `os`: Operating system (linux, darwin, windows)
- `arch`: CPU architecture (x86_64, aarch64)
- `libc`: C library and version (glibc-2.31, musl-1.2.3)
- `cpu_features`: Relevant SIMD extensions (avx2, avx512, neon) — affects numpy/scipy behavior
- `blas`: BLAS implementation (openblas, mkl, accelerate) — affects linear algebra results
- `python_version`: Full Python version for Python transforms

The fingerprint is hashed: `platform_hash = blake3(canonical_json(fingerprint))`.

**Detection**: The CLI and runtime detect these at execution time. For server-side compute, the platform is fixed per compute worker and recorded in the cache entry.

### 2.2.2 Canonicalization Rules

To ensure consistent hashing, all inputs are canonicalized:

**Source code:**
- UTF-8 encoding
- LF line endings (CRLF → LF)
- No trailing whitespace
- Files sorted alphabetically by path
- Hash: `blake3(sorted_files.map(|f| f.path + "\0" + f.content).join("\0"))`

**Parameters (JSON):**
- Keys sorted alphabetically (recursive)
- No whitespace between tokens
- Numbers in shortest decimal representation
- No trailing zeros after decimal point
- Unicode escaped as `\uXXXX`

**Parquet files (for raw data ingest):**
- Row group size: 1M rows (configurable)
- Compression: zstd level 3
- Column order: alphabetical
- Null encoding: consistent across writes

### 2.2.3 Determinism Contract

A transform is **deterministic** if: given the same inputs, params, dependencies, and platform, it always produces bitwise-identical output.

**Default runtime policy** (enforced by OzzyDB):
```
PYTHONHASHSEED=0
OMP_NUM_THREADS=1
MKL_NUM_THREADS=1
OPENBLAS_NUM_THREADS=1
NUMEXPR_NUM_THREADS=1
```

Transforms that violate determinism (use `time.time()`, unseeded RNG, network access, nondeterministic parallel reductions) must be marked `reproducible=false`:

```python
@ozzy.transform(
    reproducible=False,  # Explicitly opt out of determinism guarantee
    params={...}
)
def train_model(df: pl.LazyFrame, params: ozzy.Params) -> pl.LazyFrame:
    # Uses random initialization, etc.
    ...
```

**Consequences of `reproducible=False`:**
- Cannot be included in DOI-minted releases
- Cache entries are still valid but tagged as non-reproducible
- `ozzy transform test` will warn about nondeterminism
- Downstream transforms inherit the flag

This gives us:
- **Deduplication**: Identical transforms or data are stored once (per platform).
- **Cache validity**: If any input changes, the hash changes, cache misses, recomputation occurs.
- **Reproducibility**: A hash is a complete, verifiable description of how data was produced.
- **Cross-platform honesty**: Two machines with different architectures won't incorrectly share cached results that differ at the bit level.

### 2.3 Endpoints and Refs

Endpoints are human-readable names that resolve to a specific DAG node at a specific commit:

```
rileyleff/sapflux/corrected@latest       → HEAD of the corrected endpoint
rileyleff/sapflux/corrected@v1.0.0       → Tagged release
rileyleff/sapflux/corrected@a1b2c3d4     → Specific commit hash
doi:10.5281/ozzy.rileyleff.sapflux.v1.0.0/corrected → DOI resolution
```

### 2.4 Local Commit and Ref Model

In local-first mode, the project directory contains a full commit graph — not just a single state.

**On-disk structure:**

```
my-project/
├── ozzy.toml                    # Project metadata + current HEAD
├── .ozzy/
│   ├── commits/
│   │   ├── a1b2c3d4.json        # Commit objects (content-addressed)
│   │   ├── e5f6g7h8.json
│   │   └── ...
│   ├── refs/
│   │   ├── heads/
│   │   │   └── main             # Branch refs (contains commit hash)
│   │   └── tags/
│   │       └── v1.0.0           # Tag refs (contains commit hash)
│   └── objects/
│       ├── data/                # Content-addressed data blobs
│       │   └── {hash}.parquet
│       └── transforms/          # Content-addressed transform sources
│           └── {hash}/
│               ├── source.py
│               └── uv.lock
├── data/                        # Working directory (staged data)
│   └── raw.parquet
└── transforms/                  # Working directory (staged transforms)
    └── quality_control.py
```

**Commit object format:**

```json
{
  "hash": "a1b2c3d4...",
  "parent_hashes": ["e5f6g7h8..."],  // Array for merge commits
  "author": "rileyleff",
  "message": "Add calibration step",
  "timestamp": "2026-02-04T12:00:00Z",
  "data_sources": {
    "raw": {"hash": "...", "schema_hash": "..."}
  },
  "transforms": {
    "quality_control": {"hash": "...", "runtime": "python-3.11"}
  },
  "endpoints": {
    "corrected": {"dag_hash": "..."}
  }
}
```

**ozzy.toml:**

```toml
[project]
name = "sapflux"
owner = "rileyleff"

[refs]
head = "refs/heads/main"          # Current branch
remote = "https://ozzy.dev"       # Optional remote registry

[workspace]
# Tracks uncommitted changes
staged_data = ["data/raw.parquet"]
staged_transforms = ["transforms/quality_control.py"]
```

**Ref resolution:**
1. `@latest` → read `refs/heads/main` → get commit hash
2. `@v1.0.0` → read `refs/tags/v1.0.0` → get commit hash
3. `@a1b2c3d4` → direct commit hash lookup

---

## 3. System Architecture

### 3.1 High-Level Overview

OzzyDB is designed **local-first**: the full system works on a single machine with no server. Server components are added incrementally for collaboration and optional server-side compute.

**Local Mode (no server required):**

```
┌─────────────────────────────────────────────────────────────────────┐
│                         User's Machine                               │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │                    CLI / Python Client                        │   │
│  └──────────────────────────────┬───────────────────────────────┘   │
│                                 │                                    │
│         ┌───────────────────────┼───────────────────────┐           │
│         ▼                       ▼                       ▼           │
│  ┌─────────────┐    ┌───────────────────┐    ┌─────────────────┐   │
│  │ Project Dir │    │ Local Compute     │    │ ~/.ozzy/cache/  │   │
│  │ ozzy.toml   │───►│ Python (uv)       │───►│ SQLite index    │   │
│  │ data/       │    │ R (renv)          │    │ parquet files   │   │
│  │ transforms/ │    │ Julia (Pkg)       │    │ runtime envs    │   │
│  └─────────────┘    └───────────────────┘    └─────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
```

**Remote Mode (server for sharing + optional compute):**

```
┌─────────────────────────────────────────────────────────────────────┐
│                        Client Libraries                             │
│            Python  ·  R  ·  Julia  ·  CLI  ·  REST                  │
└──────────────────────────────┬──────────────────────────────────────┘
                               │ HTTPS (Arrow IPC / JSON)
                               ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         API Gateway (Axum)                          │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌───────────────────┐  │
│   │   Auth    │  │ Endpoint │  │  Push /   │  │   DOI / Release   │  │
│   │  (OAuth)  │  │ Resolver │  │   Pull    │  │    Management     │  │
│   └──────────┘  └──────────┘  └──────────┘  └───────────────────┘  │
└──────────────────────────────┬──────────────────────────────────────┘
                               │
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                     ▼
┌──────────────────┐ ┌──────────────────┐ ┌──────────────────────────┐
│    PostgreSQL     │ │   Object Store   │ │    Compute Engine        │
│                   │ │  (Local NVMe     │ │    (optional)            │
│ · Project meta   │ │   or R2)         │ │                          │
│ · Transform DAGs │ │                   │ │  ┌────────────────────┐  │
│ · Endpoint refs  │ │ · Raw data        │ │  │  gVisor Containers │  │
│ · Cache index    │ │   (parquet)       │ │  │  Python (uv)       │  │
│ · Auth / ACLs    │ │ · Transforms      │ │  │  R (renv)          │  │
│ · Audit log      │ │   (source + lock) │ │  │  Julia (Manifest)  │  │
│ · DOI registry   │ │ · Cached results  │ │  └────────────────────┘  │
│                   │ │   (parquet)       │ │  ┌────────────────────┐  │
│                   │ │ · WASM blobs      │ │  │  WASM Runtime      │  │
│                   │ │                   │ │  │  (wasmtime)        │  │
│                   │ │                   │ │  └────────────────────┘  │
└──────────────────┘ └──────────────────┘ └──────────────────────────┘
                                                       │
                                                       ▼
                                              ┌──────────────────┐
                                              │    Job Queue      │
                                              │  (Postgres-backed)│
                                              └──────────────────┘
```

**Key insight**: The server is primarily a registry (metadata + blob storage). Compute is optional — clients can always execute locally by downloading raw data + transforms.

### 3.2 Technology Choices

| Component | Technology | Rationale |
|-----------|-----------|-----------|
| API Server | Rust + Axum + Tokio | Performance, safety, async-native |
| Database | PostgreSQL | Proven, JSONB for flexible metadata, LISTEN/NOTIFY for job queue |
| Object Store | Local NVMe (primary), R2 (backup) | Fast local storage, optional cloud redundancy |
| Data Format (storage) | Apache Parquet | Columnar, compressed, self-describing schema |
| Data Format (wire) | Apache Arrow IPC | Zero-copy, streaming, language-agnostic |
| Container Isolation | gVisor (runsc) | Good security/performance tradeoff, drop-in Docker runtime |
| WASM Runtime | wasmtime | Production-grade, Bytecode Alliance, resource limits |
| Native Python | uv | Fast, lockfile-based, reproducible |
| Native R | renv | Standard R dependency management |
| Native Julia | Pkg (Manifest.toml) | Built-in, reproducible |
| Hashing | BLAKE3 | Fast, parallelizable, cryptographically secure |
| Auth | OAuth 2.0 + API keys | GitHub-style, scoped tokens |
| DOI | DataCite REST API | Standard for dataset DOIs |
| Hosting | Hetzner dedicated servers | Cost-effective, excellent NVMe, EU-based |

**Why gVisor over Firecracker**: Firecracker provides stronger isolation (full microVM) but is harder to operate. For a scientific community where users are known researchers with real identities, gVisor's container-level isolation is sufficient. It's a drop-in Docker runtime swap — trivial to adopt. Move to Firecracker only if handling genuinely adversarial users.

---

## 4. Data Model (PostgreSQL)

### 4.1 Schema

```sql
-- Organizations and users
CREATE TABLE organizations (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    slug            TEXT UNIQUE NOT NULL,        -- e.g., "ameriflux"
    display_name    TEXT NOT NULL,
    created_at      TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    username        TEXT UNIQUE NOT NULL,        -- e.g., "rileyleff"
    email           TEXT UNIQUE NOT NULL,
    github_id       TEXT,                        -- OAuth link
    created_at      TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE org_memberships (
    org_id          UUID REFERENCES organizations(id),
    user_id         UUID REFERENCES users(id),
    role            TEXT NOT NULL CHECK (role IN ('owner', 'admin', 'member', 'reader')),
    PRIMARY KEY (org_id, user_id)
);

-- Projects (repositories)
CREATE TABLE projects (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_user_id   UUID REFERENCES users(id),
    owner_org_id    UUID REFERENCES organizations(id),
    slug            TEXT NOT NULL,               -- e.g., "sapflux"
    description     TEXT,
    visibility      TEXT NOT NULL DEFAULT 'private'
                    CHECK (visibility IN ('public', 'private', 'org')),
    default_branch  TEXT NOT NULL DEFAULT 'main', -- Branch that @latest resolves to
    created_at      TIMESTAMPTZ DEFAULT now(),
    UNIQUE (owner_user_id, slug),
    UNIQUE (owner_org_id, slug),
    CHECK (
        (owner_user_id IS NOT NULL AND owner_org_id IS NULL) OR
        (owner_user_id IS NULL AND owner_org_id IS NOT NULL)
    )
);

-- Commits (immutable snapshots of the project state)
CREATE TABLE commits (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id),
    hash            TEXT UNIQUE NOT NULL,        -- BLAKE3 of full state
    author_id       UUID NOT NULL REFERENCES users(id),
    message         TEXT,
    created_at      TIMESTAMPTZ DEFAULT now()
);

-- Commit parents (supports merge commits with multiple parents)
CREATE TABLE commit_parents (
    commit_id       UUID NOT NULL REFERENCES commits(id),
    parent_id       UUID NOT NULL REFERENCES commits(id),
    parent_order    INT NOT NULL,                -- 0 = first parent, 1 = second, etc.
    PRIMARY KEY (commit_id, parent_order)
);

CREATE INDEX idx_commits_project ON commits(project_id, created_at DESC);
CREATE INDEX idx_commit_parents ON commit_parents(parent_id);

-- Data sources (raw data blobs)
CREATE TABLE data_sources (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    name            TEXT NOT NULL,               -- e.g., "raw"
    content_hash    TEXT NOT NULL,               -- BLAKE3 of parquet bytes
    r2_key          TEXT NOT NULL,               -- Object store path
    schema_json     JSONB NOT NULL,              -- Arrow schema + semantic types
    row_count       BIGINT,
    byte_size       BIGINT,
    UNIQUE (commit_id, name)
);

-- Transforms (versioned function definitions)
CREATE TABLE transforms (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    name            TEXT NOT NULL,               -- e.g., "quality_control"
    content_hash    TEXT NOT NULL,               -- BLAKE3(source + lockfile + runtime)
    runtime_type    TEXT NOT NULL                -- "python", "r", "julia", "wasm"
                    CHECK (runtime_type IN ('python', 'r', 'julia', 'wasm')),
    source_r2_key   TEXT NOT NULL,               -- Source code in R2
    lockfile_hash   TEXT NOT NULL,               -- Hash of dependency lockfile
    lockfile_r2_key TEXT NOT NULL,               -- Lockfile in R2
    runtime_version TEXT NOT NULL,               -- e.g., "python-3.11.8"
    params_schema   JSONB NOT NULL,              -- JSON Schema for parameters
    input_schema    JSONB,                       -- Expected input Arrow schema
    output_schema   JSONB,                       -- Produced output Arrow schema
    description     TEXT,
    reproducible    BOOLEAN NOT NULL DEFAULT TRUE,
    UNIQUE (commit_id, name)
);

-- WASM-specific metadata (for compiled transforms)
CREATE TABLE wasm_blobs (
    transform_id    UUID PRIMARY KEY REFERENCES transforms(id),
    wasm_hash       TEXT NOT NULL,               -- BLAKE3 of .wasm binary
    wasm_r2_key     TEXT NOT NULL,
    target_triple   TEXT,                        -- Source language info
    byte_size       BIGINT
);

-- Endpoints (named pipelines)
CREATE TABLE endpoints (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    name            TEXT NOT NULL,               -- e.g., "corrected"
    description     TEXT,
    UNIQUE (commit_id, name)
);

-- Refs (named pointers to commits, like git refs)
-- This table resolves @latest, @v1.0.0, branch names, etc.
CREATE TABLE refs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id),
    name            TEXT NOT NULL,               -- e.g., "heads/main", "tags/v1.0.0"
    ref_type        TEXT NOT NULL                -- "branch", "tag"
                    CHECK (ref_type IN ('branch', 'tag')),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    updated_at      TIMESTAMPTZ DEFAULT now(),
    UNIQUE (project_id, name)
);

CREATE INDEX idx_refs_project ON refs(project_id);

-- Special handling: @latest resolves to the default branch (usually "heads/main")
-- This is stored in projects.default_branch (to be added)

-- Pipeline nodes (transforms within an endpoint's DAG)
-- This replaces the old pipeline_steps table with an adjacency model
-- that properly represents multi-input transforms (joins, enrichments)
CREATE TABLE pipeline_nodes (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    endpoint_id     UUID NOT NULL REFERENCES endpoints(id),
    node_name       TEXT NOT NULL,               -- Unique within endpoint, e.g., "qc", "calibrate"
    transform_name  TEXT NOT NULL,               -- References transforms.name in same commit
    params          JSONB NOT NULL DEFAULT '{}', -- Concrete parameter values
    UNIQUE (endpoint_id, node_name)
);

-- Pipeline edges (data flow between nodes)
-- Each edge represents one named input to a transform
CREATE TABLE pipeline_edges (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_node_id  UUID NOT NULL REFERENCES pipeline_nodes(id),
    input_name      TEXT NOT NULL,               -- "main", "left", "right", "calibration", etc.
    source_type     TEXT NOT NULL                -- What the input connects to
                    CHECK (source_type IN ('data_source', 'node', 'external')),
    source_ref      TEXT NOT NULL,               -- Name of data_source or node_name (for local refs)
    -- For external (cross-project) dependencies, all fields must be populated for reproducibility
    external_owner  TEXT,                        -- e.g., "nist"
    external_project TEXT,                       -- e.g., "calibration-tables"
    external_endpoint TEXT,                      -- e.g., "thermocouples"
    external_commit_hash TEXT,                   -- REQUIRED for reproducibility: pinned commit hash
    PRIMARY KEY (target_node_id, input_name),
    -- Ensure external deps are fully specified
    CHECK (
        (source_type != 'external') OR
        (external_owner IS NOT NULL AND external_project IS NOT NULL AND
         external_endpoint IS NOT NULL AND external_commit_hash IS NOT NULL)
    )
);

-- Materialization cache index
CREATE TABLE cache_entries (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    materialized_hash TEXT UNIQUE NOT NULL,      -- BLAKE3(input_hash + transform_hash + params_hash + platform)
    platform        TEXT NOT NULL,               -- e.g., "x86_64-linux", "aarch64-darwin"
    storage_key     TEXT NOT NULL,               -- Path in local storage or R2
    row_count       BIGINT,
    byte_size       BIGINT,
    created_at      TIMESTAMPTZ DEFAULT now(),
    last_accessed   TIMESTAMPTZ DEFAULT now(),
    access_count    BIGINT DEFAULT 1,
    ttl_days        INT                          -- NULL = permanent
);

CREATE INDEX idx_cache_lru ON cache_entries(last_accessed ASC);
CREATE INDEX idx_cache_platform ON cache_entries(platform);

-- Releases and DOIs
CREATE TABLE releases (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    version         TEXT NOT NULL,               -- Semver: "v1.0.0"
    doi             TEXT UNIQUE,                 -- DataCite DOI
    doi_metadata    JSONB,                       -- DataCite metadata blob
    created_at      TIMESTAMPTZ DEFAULT now(),
    UNIQUE (project_id, version)
);

CREATE TABLE release_endpoints (
    release_id      UUID NOT NULL REFERENCES releases(id),
    endpoint_name   TEXT NOT NULL,
    PRIMARY KEY (release_id, endpoint_name)
);

-- Streaming data buffer (for real-time/sensor data)
CREATE TABLE data_buffers (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id),
    source_name     TEXT NOT NULL,
    r2_key          TEXT NOT NULL,               -- Append-only buffer file in R2
    row_count       BIGINT DEFAULT 0,
    last_appended   TIMESTAMPTZ DEFAULT now(),
    auto_commit     BOOLEAN DEFAULT TRUE,
    commit_interval INTERVAL DEFAULT '1 hour',
    UNIQUE (project_id, source_name)
);

-- Access control (beyond org membership)
CREATE TABLE project_collaborators (
    project_id      UUID NOT NULL REFERENCES projects(id),
    user_id         UUID REFERENCES users(id),
    org_id          UUID REFERENCES organizations(id),
    permission      TEXT NOT NULL CHECK (permission IN ('read', 'write', 'admin')),
    CHECK (
        (user_id IS NOT NULL AND org_id IS NULL) OR
        (user_id IS NULL AND org_id IS NOT NULL)
    )
);

-- API tokens (scoped)
CREATE TABLE api_tokens (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id),
    name            TEXT NOT NULL,
    token_hash      TEXT NOT NULL,               -- BLAKE3 of token value
    scopes          TEXT[] NOT NULL,             -- e.g., ["read:rileyleff/sapflux", "write:rileyleff/*"]
    expires_at      TIMESTAMPTZ,
    created_at      TIMESTAMPTZ DEFAULT now()
);

-- Audit log
CREATE TABLE audit_log (
    id              BIGSERIAL PRIMARY KEY,
    timestamp       TIMESTAMPTZ DEFAULT now(),
    user_id         UUID REFERENCES users(id),
    project_id      UUID REFERENCES projects(id),
    action          TEXT NOT NULL,               -- "push", "fetch", "release", "doi_mint", etc.
    metadata        JSONB DEFAULT '{}'
);

CREATE INDEX idx_audit_project ON audit_log(project_id, timestamp DESC);

-- Job queue for async materialization
CREATE TABLE jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_type        TEXT NOT NULL,               -- "materialize", "auto_commit_buffer"
    status          TEXT NOT NULL DEFAULT 'pending'
                    CHECK (status IN ('pending', 'running', 'completed', 'failed')),
    payload         JSONB NOT NULL,
    result          JSONB,
    error           TEXT,
    created_at      TIMESTAMPTZ DEFAULT now(),
    started_at      TIMESTAMPTZ,
    completed_at    TIMESTAMPTZ,
    retry_count     INT DEFAULT 0,
    max_retries     INT DEFAULT 3
);

CREATE INDEX idx_jobs_pending ON jobs(created_at) WHERE status = 'pending';
```

### 4.2 Key Queries

**Resolve an endpoint to its full DAG:**

```sql
-- Get all nodes and edges for an endpoint at a specific commit
-- This traverses the adjacency graph to build the full execution plan
WITH RECURSIVE dag AS (
    -- Start from all nodes in the endpoint
    SELECT
        pn.id AS node_id,
        pn.node_name,
        pn.transform_name,
        pn.params,
        pe.input_name,
        pe.source_type,
        pe.source_ref,
        pe.external_project,
        0 AS depth
    FROM endpoints e
    JOIN pipeline_nodes pn ON pn.endpoint_id = e.id
    LEFT JOIN pipeline_edges pe ON pe.target_node_id = pn.id
    WHERE e.commit_id = $commit_id AND e.name = $endpoint_name

    UNION ALL

    -- Recursively resolve upstream nodes (within same endpoint)
    SELECT
        pn2.id,
        pn2.node_name,
        pn2.transform_name,
        pn2.params,
        pe2.input_name,
        pe2.source_type,
        pe2.source_ref,
        pe2.external_project,
        dag.depth + 1
    FROM dag
    JOIN pipeline_nodes pn2 ON pn2.node_name = dag.source_ref
        AND pn2.endpoint_id = (
            SELECT id FROM endpoints
            WHERE commit_id = $commit_id AND name = $endpoint_name
        )
    LEFT JOIN pipeline_edges pe2 ON pe2.target_node_id = pn2.id
    WHERE dag.source_type = 'node'
)
SELECT DISTINCT * FROM dag ORDER BY depth DESC, node_name ASC;
```

This adjacency model properly represents multi-input transforms like joins:

```
-- Example: A transform that joins two upstream nodes
INSERT INTO pipeline_nodes (endpoint_id, node_name, transform_name, params)
VALUES ($endpoint_id, 'enriched', 'join_calibration', '{}');

INSERT INTO pipeline_edges (target_node_id, input_name, source_type, source_ref)
VALUES
    ($enriched_node_id, 'left', 'node', 'qc_output'),
    ($enriched_node_id, 'right', 'data_source', 'calibration_table');
```

**Check cache for a materialized result:**

```sql
SELECT r2_key, byte_size
FROM cache_entries
WHERE materialized_hash = $hash;

-- On hit, update access stats:
UPDATE cache_entries
SET last_accessed = now(), access_count = access_count + 1
WHERE materialized_hash = $hash;
```

---

## 5. Object Storage Layout (Cloudflare R2)

```
ozzy-store/
├── raw/
│   └── {content_hash}.parquet              # Raw data blobs (deduplicated)
│
├── transforms/
│   ├── source/{content_hash}/              # Transform source bundles
│   │   ├── source.py (or .r, .jl, .rs)
│   │   ├── lockfile (uv.lock, renv.lock, etc.)
│   │   └── metadata.json
│   └── wasm/{wasm_hash}.wasm               # Compiled WASM blobs
│
├── cache/
│   └── {materialized_hash}.parquet         # Cached transform outputs
│
├── buffers/
│   └── {project_id}/{source_name}/
│       └── buffer_{timestamp}.parquet      # Append-only buffer segments
│
└── schemas/
    └── {schema_hash}.json                  # Deduplicated schema definitions
```

All keys are content-addressed (by hash), so:
- Identical raw data uploaded by different users is stored once.
- Identical transform outputs are cached once regardless of who requested them.
- R2 lifecycle rules can evict old cache entries based on `last_accessed`.

---

## 6. Compute Engine

### 6.0 Transform Interface

All transforms, regardless of runtime, implement the same logical interface:

```
transform(
    inputs: Dict[str, ArrowRecordBatch],  # Named inputs (e.g., {"main": ..., "calibration": ...})
    params: Dict[str, Any]                 # Runtime parameters (validated against schema)
) -> ArrowRecordBatch
```

**Contract:**
1. Inputs are ordered deterministically (sorted by input name)
2. Input batches are in deterministic row order
3. Output must be a valid Arrow RecordBatch
4. Output schema must match declared output schema (if declared)
5. Transform must not access network, filesystem (outside working dir), or system time
6. If `reproducible=True` (default), same inputs must produce bitwise-identical output

**Python implementation:**

```python
@ozzy.transform(
    params={"threshold": float, "method": str},
    input_schema={"main": ["timestamp", "value", "sensor_id"]},
    output_schema={"adds": ["is_valid"], "passthrough": "all"},
    reproducible=True  # default
)
def quality_control(
    inputs: dict[str, pl.LazyFrame],
    params: ozzy.Params
) -> pl.LazyFrame:
    df = inputs["main"]
    return df.with_columns(
        (pl.col("value") > params.threshold).alias("is_valid")
    )
```

**Multi-input transforms (joins):**

```python
@ozzy.transform(
    params={"join_key": str},
    input_schema={
        "left": ["sensor_id", "raw_value"],
        "right": ["sensor_id", "calibration_factor"]
    }
)
def apply_calibration(
    inputs: dict[str, pl.LazyFrame],
    params: ozzy.Params
) -> pl.LazyFrame:
    return inputs["left"].join(
        inputs["right"],
        on=params.join_key,
        how="left"
    ).with_columns(
        (pl.col("raw_value") * pl.col("calibration_factor")).alias("calibrated")
    )
```

### 6.1 Execution Model

When a client requests data from an endpoint, the server:

1. **Resolves** the endpoint to its full DAG (recursive query)
2. **Computes the materialized hash** for each node bottom-up
3. **Checks cache** at each node, starting from the final output
4. **Finds the frontier** — the deepest cached ancestor
5. **Executes** transforms from the frontier to the requested node
6. **Caches** each intermediate result
7. **Returns** the final result as Arrow IPC stream

```
Request: GET /rileyleff/sapflux/corrected@v1.0.0

DAG:
  raw (hash: aaa) ──► qc (hash: bbb) ──► calibrate (hash: ccc)

Cache check:
  ccc → MISS
  bbb → HIT (cached at cache/bbb.parquet)

Execution plan:
  1. Read cache/bbb.parquet
  2. Execute calibrate(bbb, params) → ccc
  3. Write cache/ccc.parquet
  4. Stream ccc to client
```

### 6.2 Native Runtime Execution

For Python, R, and Julia transforms, the server:

1. **Checks for a cached environment** matching `(runtime_type, runtime_version, lockfile_hash)`
2. **Creates one if missing:**
   - Python: `uv venv --python {version} && uv pip sync {lockfile}`
   - R: `R -e "renv::restore(lockfile='{path}')"`
   - Julia: `julia --project={dir} -e "Pkg.instantiate()"`
3. **Runs the transform** in the environment with resource limits (CPU time, memory, no network)
4. **Captures output** as Arrow IPC

Environments are cached on the compute nodes. A given `(runtime, lockfile_hash)` pair is built once.

**Isolation model**: Each transform execution runs in a sandboxed environment. This is a **hard architectural invariant**:

> **TRANSFORMS MUST NOT HAVE NETWORK ACCESS. PERIOD.**
>
> Any external data a transform needs (calibration tables, ML models, lookup data) must be declared as a dependency and fetched by the OzzyDB runtime *before* execution. The transform receives pre-fetched data as an injected parameter. The transform code itself never touches the network.

This is non-negotiable because:
1. Network access breaks reproducibility (external resources change)
2. Network access breaks content addressing (the hash can't capture what was fetched)
3. Network access is a security hole (data exfiltration, arbitrary requests)

Transforms cannot:
- Access the network (no exceptions — see §6.4 for how external data is handled)
- Read the filesystem outside their working directory
- Execute longer than the configured timeout
- Allocate more than the configured memory limit

In local-first mode, isolation is the user's responsibility. In server-side execution, transforms run in sandboxed containers (gVisor for simplicity, Firecracker for maximum isolation).

### 6.3 WASM Runtime Execution

For Rust, Go, C++, and other WASM-compiled transforms:

1. **Load WASM blob** from R2 (content-addressed, cached locally)
2. **Instantiate** in wasmtime with:
   - Memory limit (configurable per transform, default 4 GB)
   - Fuel metering (CPU time limit)
   - No WASI filesystem/network access
3. **Pass input** as Arrow IPC bytes through WASM linear memory
4. **Receive output** as Arrow IPC bytes
5. **Validate** output schema matches declaration

The WASM function signature (host-side):

```rust
// The WASM module exports:
// transform(input_ptr: i32, input_len: i32, params_ptr: i32, params_len: i32) -> i32
// Returns a pointer to the output buffer (length-prefixed)

fn execute_wasm_transform(
    wasm_bytes: &[u8],
    input: &RecordBatch,
    params: &serde_json::Value,
) -> Result<RecordBatch> {
    let engine = Engine::new(&Config::new().consume_fuel(true))?;
    let module = Module::new(&engine, wasm_bytes)?;
    let mut store = Store::new(&engine, ());
    store.set_fuel(MAX_FUEL)?;

    let instance = Instance::new(&mut store, &module, &[])?;
    let memory = instance.get_memory(&mut store, "memory").unwrap();

    // Serialize input to Arrow IPC
    let input_bytes = serialize_to_ipc(input)?;
    let params_bytes = serde_json::to_vec(params)?;

    // Write to WASM memory
    let input_ptr = wasm_alloc(&instance, &mut store, input_bytes.len())?;
    memory.write(&mut store, input_ptr, &input_bytes)?;

    let params_ptr = wasm_alloc(&instance, &mut store, params_bytes.len())?;
    memory.write(&mut store, params_ptr, &params_bytes)?;

    // Call transform
    let transform = instance.get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "transform")?;
    let result_ptr = transform.call(&mut store, (
        input_ptr as i32, input_bytes.len() as i32,
        params_ptr as i32, params_bytes.len() as i32,
    ))?;

    // Read output
    let output_bytes = wasm_read_output(&memory, &store, result_ptr as usize)?;
    deserialize_from_ipc(&output_bytes)
}
```

### 6.4 External Dependencies in Transforms

Some transforms need external data (calibration tables, lookup data, ML models). **These must be declared explicitly and are fetched by the OzzyDB runtime, not by the transform itself.**

This is how external data gets into a transform without breaking the network isolation invariant:

```python
@ozzy.transform(
    dependencies={
        # All external data must be an OzzyDB ref (pinned version required)
        "calibration": "nist/calibration-tables/thermocouples@v2.1",
        "model": "rileyleff/sapflux-ml/model@a1b2c3d4",
    },
    params={...}
)
def apply_ml_correction(df: pl.LazyFrame, params: ozzy.Params, deps: ozzy.Deps) -> pl.LazyFrame:
    # deps are pre-fetched by the runtime and injected
    # The transform code NEVER makes network requests
    model = deps["model"]
    cal = deps["calibration"]
    ...
```

**Key points:**
1. Dependencies must be OzzyDB refs (not arbitrary URLs)
2. Dependencies must be pinned to a specific version (no `@latest` for reproducibility)
3. The runtime fetches dependencies *before* invoking the transform
4. Dependency hashes are included in the materialized hash
5. If a dependency changes, the output hash changes → cache miss → recomputation

This design means external data is version-controlled and content-addressed just like everything else in OzzyDB.

### 6.5 Large Data / Chunked Execution

For datasets that don't fit in memory, transforms can declare their execution mode:

```python
@ozzy.transform(
    execution_mode="streaming",  # Default is "batch" (full dataset in memory)
    params={...}
)
def filter_bad_readings(df: pl.LazyFrame, params: ozzy.Params) -> pl.LazyFrame:
    # This runs on chunks — LazyFrame handles it
    return df.filter(pl.col("battery_v") > params.threshold)
```

**Modes:**
- `batch`: Full dataset loaded, transform receives complete LazyFrame. Required for global operations (sorts, aggregations, joins).
- `streaming`: Data processed in chunks. Transform must be expressible as row-wise or partition-wise operations. Enables processing of arbitrarily large data.

The server manages chunking:

```
For streaming transforms:
  1. Read input parquet in row-group-sized chunks (e.g., 1M rows)
  2. Execute transform on each chunk
  3. Concatenate output chunks into output parquet
  4. Cache the final result
```

### 6.6 Job Queue

Expensive materializations run asynchronously:

1. Client requests an endpoint
2. Server checks cache → MISS
3. Server enqueues a materialization job, returns `202 Accepted` with job ID
4. Client polls `GET /jobs/{id}` or supplies a webhook URL
5. On completion, result is cached and client is notified

For fast materializations (small data, cached environment), the server can execute synchronously and return `200 OK` directly. The threshold is configurable (default: 30 seconds).

```rust
// Simplified job processing loop
async fn process_jobs(pool: PgPool, r2: R2Client) {
    loop {
        let job = sqlx::query_as!(Job,
            "UPDATE jobs SET status = 'running', started_at = now()
             WHERE id = (
                 SELECT id FROM jobs WHERE status = 'pending'
                 ORDER BY created_at LIMIT 1 FOR UPDATE SKIP LOCKED
             ) RETURNING *"
        ).fetch_optional(&pool).await;

        if let Some(job) = job {
            match execute_materialization(&job, &pool, &r2).await {
                Ok(result) => mark_completed(&pool, job.id, result).await,
                Err(e) if job.retry_count < job.max_retries => {
                    mark_pending_retry(&pool, job.id).await;
                }
                Err(e) => mark_failed(&pool, job.id, e).await,
            }
        } else {
            // No jobs, wait for NOTIFY
            pool.listen("new_job").await;
        }
    }
}
```

---

## 7. API Design

### 7.1 REST Endpoints

#### Data Retrieval

```
GET /api/v1/{owner}/{project}/{endpoint}
GET /api/v1/{owner}/{project}/{endpoint}@{ref}

Query params:
  format=arrow|parquet|csv         (default: arrow)
  compute=server|local             (default: server)
  columns=col1,col2,col3           (projection pushdown)
  filter=col1>100                  (filter pushdown, simple predicates)
  limit=1000                       (row limit)
  offset=0                         (pagination)
  override_params={"transform_name": {"key": "value"}}

Response headers:
  X-Ozzy-Hash: {materialized_hash}
  X-Ozzy-Row-Count: 847231456
  X-Ozzy-Cache-Status: HIT|MISS|PENDING
  Content-Type: application/vnd.apache.arrow.stream

If materialization is pending:
  202 Accepted
  Location: /api/v1/jobs/{job_id}
  Retry-After: 30
```

#### Project Management

```
POST   /api/v1/{owner}/{project}                    # Create project
GET    /api/v1/{owner}/{project}                     # Project metadata
DELETE /api/v1/{owner}/{project}                     # Delete project

POST   /api/v1/{owner}/{project}/push                # Push commit
GET    /api/v1/{owner}/{project}/commits              # List commits
GET    /api/v1/{owner}/{project}/dag                  # Get transform DAG
GET    /api/v1/{owner}/{project}/dag.svg              # Render DAG as SVG

GET    /api/v1/{owner}/{project}/endpoints            # List endpoints
GET    /api/v1/{owner}/{project}/transforms           # List transforms
GET    /api/v1/{owner}/{project}/schemas/{name}       # Get schema definition
```

#### Releases and DOIs

```
POST   /api/v1/{owner}/{project}/releases             # Create release
GET    /api/v1/{owner}/{project}/releases              # List releases
POST   /api/v1/{owner}/{project}/releases/{ver}/doi    # Mint DOI
GET    /api/v1/doi/{doi}                               # Resolve DOI to endpoint
```

#### Streaming Data

```
POST   /api/v1/{owner}/{project}/buffer/{source}      # Append to buffer
GET    /api/v1/{owner}/{project}/buffer/{source}/status # Buffer status
POST   /api/v1/{owner}/{project}/buffer/{source}/commit # Force commit buffer
```

#### Access Control

```
GET    /api/v1/{owner}/{project}/collaborators
POST   /api/v1/{owner}/{project}/collaborators
DELETE /api/v1/{owner}/{project}/collaborators/{user}
POST   /api/v1/{owner}/{project}/tokens               # Create scoped API token
```

#### Jobs

```
GET    /api/v1/jobs/{id}                               # Job status
GET    /api/v1/jobs/{id}/result                        # Job result (redirects to data)
```

#### Client-Side Compute

```
GET    /api/v1/{owner}/{project}/{endpoint}/plan       # Get execution plan
GET    /api/v1/{owner}/{project}/transforms/{name}/source  # Download transform source
GET    /api/v1/{owner}/{project}/transforms/{name}/wasm    # Download WASM blob
GET    /api/v1/{owner}/{project}/transforms/{name}/lockfile # Download lockfile
```

### 7.2 Wire Format

All data transfer uses **Arrow IPC streaming format** by default. This provides:
- Zero-copy deserialization in client libraries (via `pyarrow`, `arrow-rs`, `Arrow.jl`)
- Schema included in the stream header
- Chunked transfer for large datasets
- Compression (LZ4 or ZSTD) at the record batch level

Clients can request `format=parquet` for persistence or `format=csv` for compatibility, but these are slower and larger.

---

## 8. Client Libraries

### 8.1 Python Client (`ozzydb`)

```python
import ozzydb as ozzy

# ═══════════════════════════════════════════════════════════════════
# LOCAL-FIRST USAGE (no server required)
# ═══════════════════════════════════════════════════════════════════

# Fetch from a local project directory (executes DAG locally)
df = ozzy.fetch("./my-project/corrected")
df = ozzy.fetch("~/data/sapflux/corrected")

# Inspect local project
meta = ozzy.inspect("./my-project/corrected")
print(meta.schema)       # Arrow schema
print(meta.dag)          # Transform DAG
print(meta.hash)         # Content hash

# ═══════════════════════════════════════════════════════════════════
# REMOTE USAGE (requires server)
# ═══════════════════════════════════════════════════════════════════

# Configuration
ozzy.configure(
    token="ozzy_...",           # Or read from OZZY_TOKEN env var
    cache_dir="~/.ozzy/cache",  # Local cache location
    default_compute="local",    # "local" (default) or "server"
)

# Fetch from remote (downloads data + transforms, executes locally by default)
df = ozzy.fetch("rileyleff/sapflux/corrected")
df = ozzy.fetch("rileyleff/sapflux/corrected@v1.0.0")
df = ozzy.fetch("rileyleff/sapflux/corrected", as_pandas=True)

# Server-side compute (if available and configured)
df = ozzy.fetch("rileyleff/sapflux/corrected", compute="server")

# Lazy fetch (returns polars LazyFrame, defers download)
lf = ozzy.fetch_lazy("rileyleff/sapflux/corrected")
result = lf.filter(pl.col("year") == 2024).collect()

# Fetch as file path (downloads parquet to cache)
path = ozzy.fetch("rileyleff/sapflux/corrected", format="parquet")

# Override params
df = ozzy.fetch(
    "rileyleff/sapflux/corrected",
    override_params={"apply_calibration": {"seed": 99}}
)

# Apply a remote transform to local data
result = ozzy.apply(
    local_df,
    transform="si-units-org/conversions/celsius_to_kelvin",
    params={"column": "temp_c"}
)

# Inspect remote project
meta = ozzy.inspect("rileyleff/sapflux/corrected")
print(meta.lineage)      # Full provenance chain

# Cache management
ozzy.cache.size()        # Total cache size
ozzy.cache.clear()       # Clear all
ozzy.cache.evict("rileyleff/sapflux/*")  # Clear specific project
```

**Implementation notes:**
- Built on `httpx` (async) + `pyarrow` (Arrow IPC) + `polars` (DataFrames)
- Local-first: paths starting with `./`, `~/`, or `/` are treated as local projects
- Remote refs (like `owner/project/endpoint`) require a configured server
- Local cache is a content-addressed directory of parquet files
- Cache index stored in a local SQLite DB at `~/.ozzy/cache/index.db`
- Supports async: `await ozzy.fetch_async(...)`

### 8.2 R Client (`ozzy`)

```r
library(ozzy)

ozzy_configure(token = Sys.getenv("OZZY_TOKEN"))

# Fetch as tibble
df <- ozzy_fetch("rileyleff/sapflux/corrected")

# Fetch as Arrow Table (zero-copy)
tbl <- ozzy_fetch("rileyleff/sapflux/corrected", format = "arrow")

# Fetch with version
df <- ozzy_fetch("rileyleff/sapflux/corrected@v1.0.0")
```

Built on `httr2` + `arrow` (R Arrow bindings).

### 8.3 Julia Client (`Ozzy.jl`)

```julia
using Ozzy

Ozzy.configure(token=ENV["OZZY_TOKEN"])

df = Ozzy.fetch("rileyleff/sapflux/corrected")          # DataFrame
tbl = Ozzy.fetch("rileyleff/sapflux/corrected", format=:arrow)  # Arrow.Table
```

Built on `HTTP.jl` + `Arrow.jl`.

### 8.4 CLI

The CLI is the primary interface for data producers. It wraps the API and manages local project state.

```bash
# Project lifecycle
ozzy init                                    # Initialize project
ozzy data add <file> --name <name>           # Stage raw data
ozzy data ls                                 # List data sources
ozzy transform add <file:function>           # Register transform
ozzy transform ls                            # List transforms
ozzy transform test <name> [--sample 1000]   # Test on sample data
ozzy endpoint create <name>                  # Create endpoint
ozzy endpoint ls                             # List endpoints
ozzy dag show [--format ascii|svg|json]      # Visualize DAG

# Version control
ozzy push [-m "commit message"]              # Push to remote
ozzy pull                                    # Pull latest
ozzy log                                     # Commit history
ozzy diff <ref1> <ref2>                      # Compare outputs between refs
ozzy status                                  # Show local changes

# Releases
ozzy release create <version> [--endpoints ...]
ozzy release ls
ozzy doi mint <version>

# Access control
ozzy auth login                              # GitHub OAuth flow
ozzy auth token create <name> [--scopes ...] # Create API token
ozzy collaborator add <user> [--permission ...]
ozzy visibility set <public|private|org>

# Data operations
ozzy fetch <ref>                             # Download to local cache
ozzy run <endpoint> [--output file.parquet]  # Execute locally
ozzy cache ls
ozzy cache clear [--project ...]

# Streaming
ozzy buffer append <source> <file>           # Append to buffer
ozzy buffer commit <source>                  # Force commit
ozzy buffer status <source>                  # Show buffer state
```

---

## 9. Schema System

### 9.1 Physical Schema

The physical schema is an Arrow schema, stored as JSON:

```json
{
  "fields": [
    {"name": "timestamp", "type": "timestamp[μs, UTC]", "nullable": false},
    {"name": "sensor_id", "type": "utf8", "nullable": false},
    {"name": "raw_mv", "type": "float64", "nullable": false},
    {"name": "temp_c", "type": "float64", "nullable": true},
    {"name": "battery_v", "type": "float64", "nullable": true}
  ]
}
```

### 9.2 Semantic Schema

Layered on top of the physical schema, semantic types provide meaning:

```json
{
  "fields": {
    "timestamp": {
      "dtype": "timestamp[μs, UTC]",
      "nullable": false,
      "semantic": {
        "domain": "time",
        "kind": "instant",
        "timezone": "UTC"
      }
    },
    "raw_mv": {
      "dtype": "float64",
      "nullable": false,
      "semantic": {
        "domain": "electrical",
        "kind": "voltage",
        "unit": "millivolts",
        "range": [0, 5000],
        "instrument": "granier_probe"
      }
    },
    "flux_kg_m2_s": {
      "dtype": "float64",
      "nullable": true,
      "semantic": {
        "domain": "hydrology",
        "kind": "sap_flux_density",
        "unit": "kg/m²/s",
        "method": "thermal_dissipation"
      }
    }
  }
}
```

### 9.3 Transform Contracts

Each transform declares its input/output schema:

```json
{
  "transform": "apply_calibration",
  "input": {
    "requires": ["raw_mv", "delta_t", "delta_t_max"],
    "schema_match": "superset"
  },
  "output": {
    "adds": ["flux_kg_m2_s"],
    "removes": [],
    "modifies": [],
    "passthrough": "all"
  },
  "params": {
    "calibration_curve": {"type": "string", "enum": ["granier_1987", "leff_2024"]},
    "seed": {"type": "integer", "required": true}
  }
}
```

**Schema matching modes:**
- `exact`: Input must match exactly
- `superset`: Input must contain at least the required columns (extra are fine)
- `pattern`: Input must match a regex/glob pattern on column names

### 9.4 Composability Validation

When chaining transforms, OzzyDB validates the schema at each edge:

```
ozzy endpoint create corrected --input raw --transform qc --transform calibrate

Validating pipeline:
  raw → qc
    ✓ qc requires [battery_v, raw_mv] — present in raw
  qc → calibrate
    ✗ calibrate requires [delta_t, delta_t_max] — NOT present in qc output

Error: Schema mismatch at step 2. 'apply_calibration' requires columns
[delta_t, delta_t_max] which are not produced by 'quality_control'.
```

This catches pipeline errors at definition time, not execution time.

---

## 10. Streaming / Real-Time Data

### 10.1 Buffer Model

For continuously-updated datasets (sensors, web scrapers, etc.):

```
Incoming data → Buffer (append-only) → Auto-commit → Committed history
                  │                         │
                  ▼                         ▼
            @bleeding                    @stable
```

**Buffer behavior:**
- Append-only log of record batches in R2
- Each append is a separate small parquet file (fast writes)
- Auto-commit compacts buffer segments into a single parquet, creates a new commit
- Configurable commit interval (default: 1 hour) and size threshold (default: 100 MB)

**Endpoints:**
- `@stable` or `@latest`: Only committed data (consistent, reproducible)
- `@bleeding`: Committed + buffer (eventually consistent, not reproducible)

### 10.2 Ingest API

```
POST /api/v1/{owner}/{project}/buffer/{source}
Content-Type: application/vnd.apache.arrow.stream

[Arrow IPC record batches]

Response:
  200 OK
  X-Ozzy-Buffer-Rows: 1847293
  X-Ozzy-Buffer-Size: 23MB
  X-Ozzy-Next-Commit: 2026-02-04T15:00:00Z
```

Client library:

```python
# Sensor data ingestion
sensor = ozzy.buffer("rileyleff/sapflux/raw")

while True:
    reading = read_sensor()
    sensor.append(reading)  # Batches locally, flushes periodically
    time.sleep(300)  # Every 5 minutes
```

---

## 11. Authentication and Access Control

### 11.1 Auth Flow

**Primary**: GitHub OAuth 2.0 (scientists already have GitHub accounts)

```
User → ozzy auth login → Browser OAuth → GitHub → Callback → JWT issued
```

**API access**: Scoped tokens

```bash
$ ozzy auth token create my-lab-scripts --scopes read:rileyleff/sapflux
Token: ozzy_sk_a1b2c3d4e5f6...

# Use via header:
Authorization: Bearer ozzy_sk_a1b2c3d4e5f6...

# Or env var:
export OZZY_TOKEN=ozzy_sk_a1b2c3d4e5f6...
```

### 11.2 Permission Model

GitHub-style, with project-level granularity:

| Scope | Permissions |
|-------|------------|
| `read` | Fetch endpoints, view metadata, view schemas |
| `write` | Push commits, create endpoints, append to buffers |
| `admin` | Manage collaborators, change visibility, delete project |
| `owner` | Transfer ownership, delete project permanently |

**Visibility levels:**
- `public`: Anyone can read. No auth required for `read` operations.
- `org`: Members of the owning organization can read. Others need explicit grant.
- `private`: Only explicit collaborators can access.

**Organization roles:**
- `owner`: Full control over all org projects
- `admin`: Can manage members, create projects
- `member`: Can read all org projects, write to assigned projects
- `reader`: Read-only access to all org projects

---

## 12. DOI Integration

### 12.1 Minting Process

1. User creates a release: `ozzy release create v1.0.0 --endpoints corrected`
2. User requests DOI: `ozzy doi mint v1.0.0`
3. Server validates:
   - All endpoints in the release are fully reproducible (no `reproducible=False` transforms)
   - All transform hashes are pinned (no `@latest` references in dependencies)
4. Server calls DataCite REST API with metadata:

```json
{
  "data": {
    "type": "dois",
    "attributes": {
      "doi": "10.5281/ozzy.rileyleff.sapflux.v1.0.0",
      "creators": [{"name": "Leff, Riley"}],
      "titles": [{"title": "Sap flux measurements from the Ameriflux network"}],
      "publisher": "OzzyDB",
      "publicationYear": 2026,
      "types": {"resourceTypeGeneral": "Dataset"},
      "relatedIdentifiers": [
        {
          "relatedIdentifier": "https://ozzy.dev/rileyleff/sapflux/corrected@v1.0.0",
          "relationType": "IsIdenticalTo"
        }
      ],
      "descriptions": [{
        "description": "Transform lineage: raw → quality_control(battery_threshold=11.5) → apply_calibration(calibration_curve=leff_2024, seed=42)",
        "descriptionType": "TechnicalInfo"
      }]
    }
  }
}
```

5. DOI resolves to an OzzyDB landing page showing:
   - Full transform DAG visualization
   - Schema at each stage
   - All parameter values
   - One-click "reproduce this" code snippet
   - Download links in multiple formats

### 12.2 Citation in Papers

```
Data available at doi:10.5281/ozzy.rileyleff.sapflux.v1.0.0.
Exact figure data: ozzy fetch rileyleff/sapflux/corrected/leff_et_al_2026/figure_1@v1.0.0
```

The reader can then:

```python
import ozzydb as ozzy
df = ozzy.fetch("doi:10.5281/ozzy.rileyleff.sapflux.v1.0.0/corrected")
# Gets *exactly* the data used in the paper
```

---

## 13. Schema Migration

When raw data schema changes (new sensor added, column renamed, type changed):

### 13.1 Migration Definitions

```python
# migrations/001_add_humidity.py
import ozzy

@ozzy.migration(
    from_schema="schemas/raw_v1.json",
    to_schema="schemas/raw_v2.json",
    description="Add relative_humidity column from new sensor"
)
def add_humidity(df: pl.LazyFrame) -> pl.LazyFrame:
    return df.with_columns(
        pl.lit(None).cast(pl.Float64).alias("relative_humidity")
    )
```

### 13.2 Behavior

- Migrations are registered in order and stored alongside commits
- When a transform expects `raw_v2` but the underlying data is `raw_v1`, OzzyDB automatically applies the migration chain
- Migrations are part of the content hash (changing a migration changes all downstream hashes)
- `ozzy migrate --dry-run` shows what would change without applying

**Implementation note**: This feature should be deferred until real users hit the problem. The implicit migration chain is valuable UX, but designing it with concrete cases will produce a better system. For now, schema changes can be handled by versioning data sources explicitly (e.g., `raw_v1`, `raw_v2`) and letting users choose which to use.

---

## 14. Caching Strategy

### 14.1 Global Cache (Cross-Project Deduplication)

**The cache is global, not per-project.** If two unrelated projects both run `quality_control(battery_threshold=11.5)` on the same raw data with the same lockfile, they get the same materialized hash and hit the same cache entry.

This falls out naturally from content addressing:
```
same raw_data_hash + same transform_hash + same params_hash + same platform
    → same materialized_hash
    → cache hit (regardless of who requested it)
```

This is a significant efficiency win for common operations (unit conversions, standard QC routines) applied to shared datasets.

**Cache ACL Gating**: Even though cache blobs are content-addressed and deduplicated, access is gated by project permissions:

1. When a user requests an endpoint, the server first checks project ACLs
2. Only after ACL validation does the server check the cache
3. Direct cache access by hash is not exposed in the API
4. Cache entries do not store or reveal project names or endpoint names

This prevents existence leakage — a user cannot probe whether a specific hash exists in the cache unless they have access to a project that would produce that hash.

For public projects, cache entries are globally accessible (the data is public anyway).

### 14.2 Server Cache

**Location**: Local NVMe or R2 (`cache/{materialized_hash}.parquet`)
**Index**: PostgreSQL `cache_entries` table
**Eviction**: LRU with configurable TTL

```
Eviction policy:
  1. Never evict results referenced by a DOI release
  2. Never evict results accessed in the last 24 hours
  3. Evict by LRU when cache exceeds size budget
  4. Optional TTL per cache entry
```

### 14.3 Client Cache

**Location**: `~/.ozzy/cache/` (configurable)
**Index**: Local SQLite database
**Structure**:

```
~/.ozzy/
├── cache/
│   ├── index.db              # SQLite: hash → file path, metadata
│   └── data/
│       ├── {hash1}.parquet
│       ├── {hash2}.parquet
│       └── ...
├── envs/                     # Cached runtime environments (for local compute)
│   ├── python-3.11-{lockfile_hash}/
│   └── ...
└── config.toml               # User config
```

**Invalidation**: Content-addressed, so entries never become stale. They're either valid or missing. Eviction is purely a space management concern.

### 14.4 Environment Cache

Runtime environments are expensive to build. Cache them aggressively:

```
Server: environments/{runtime_type}-{runtime_version}-{lockfile_hash}/
Client: ~/.ozzy/envs/{runtime_type}-{runtime_version}-{lockfile_hash}/
```

An environment is valid forever (deterministic from its inputs). Only evict when disk space is needed.

---

## 15. Security Considerations

### 15.1 Untrusted Code Execution

Transforms are user-submitted code. Isolation is critical:

| Runtime | Isolation Method |
|---------|-----------------|
| WASM | wasmtime sandbox (memory limits, fuel metering, no WASI by default) |
| Python | Container (gVisor or Firecracker) with no network, restricted fs |
| R | Container with no network, restricted fs |
| Julia | Container with no network, restricted fs |

### 15.2 Supply Chain

- All artifacts are content-addressed: tampering is detectable
- Lockfiles pin exact dependency versions: no supply chain drift
- WASM blobs are deterministically compiled (with verification): same source → same binary
- Audit log records all pushes, fetches, and access changes

### 15.3 Data Privacy

- Private projects: data never leaves R2, access requires valid auth token
- Scoped tokens: limit access to specific projects/endpoints
- No server-side logging of data contents (only metadata)
- Client-side compute option: raw data can stay on the client for sensitive datasets

---

## 16. Deployment Architecture

OzzyDB follows a **local-first** development strategy. The system is designed to be useful immediately as a local tool, with server-side features added incrementally.

### 16.0 Local-First (No Server)

The first deployment is no deployment at all:

```
┌─────────────────────────────────────────────────────────────────────┐
│                         User's Machine                               │
│                                                                      │
│  ┌──────────┐     ┌──────────────┐     ┌──────────────────────────┐ │
│  │ ozzy CLI │────►│ ~/.ozzy/     │     │ Local Transform Execution │ │
│  │          │     │ ├── cache/   │◄────│ (Python/R/Julia via uv,   │ │
│  │          │     │ ├── envs/    │     │  renv, Pkg)               │ │
│  │          │     │ └── config   │     └──────────────────────────┘ │
│  └──────────┘     └──────────────┘                                   │
│                          │                                           │
│                          ▼                                           │
│                   ┌──────────────┐                                   │
│                   │ Project Dir  │                                   │
│                   │ ├── ozzy.toml│                                   │
│                   │ ├── data/    │                                   │
│                   │ └── transforms/                                  │
│                   └──────────────┘                                   │
└─────────────────────────────────────────────────────────────────────┘
```

**What works locally:**
- `ozzy init`, `ozzy data add`, `ozzy transform add`, `ozzy endpoint create`
- `ozzy run <endpoint>` — full DAG execution on your laptop
- `ozzy dag show` — visualize the transform graph
- Content-addressed caching in `~/.ozzy/cache/`

**What requires a server:**
- `ozzy push` / `ozzy pull` (sharing with collaborators)
- `ozzy fetch` from remote projects
- DOI minting and releases

This validates the entire data model and UX before building any infrastructure.

### 16.1 Registry-Only Server (Hetzner Single Box)

The first server deployment is a **dumb registry** — no server-side compute:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Hetzner Dedicated Server                          │
│                    (e.g., AX41-NVMe, ~€50/mo)                        │
│                                                                      │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────────────┐ │
│  │ Axum Server  │────►│  PostgreSQL   │     │ Caddy (HTTPS)        │ │
│  │ (registry +  │     │  (metadata,   │     │ (reverse proxy,      │ │
│  │  auth only)  │     │   DAGs, refs) │     │  auto TLS)           │ │
│  └──────────────┘     └──────────────┘     └──────────────────────┘ │
│         │                                                            │
│         ▼                                                            │
│  ┌──────────────────────────────────────────────────────────────────┤
│  │                     Local NVMe Storage                            │
│  │  (raw data, transforms, lockfiles — mirrors R2 for cost savings) │
│  └──────────────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │  Cloudflare R2   │ (optional, for CDN/redundancy)
                    │  (S3-compatible) │
                    └──────────────────┘
```

**What the server does:**
- Stores project metadata, DAGs, and refs in PostgreSQL
- Stores raw data, transforms, and lockfiles on local NVMe (optionally synced to R2)
- Handles auth (GitHub OAuth, API tokens)
- Serves data for `ozzy fetch` and `ozzy pull`

**What the server does NOT do:**
- Execute transforms (clients do this locally)
- Manage compute environments
- Run job queues

This is the sweet spot for early users: collaboration and sharing work, but no complex infrastructure. Clients download raw data + transforms and execute locally.

### 16.2 Server-Side Compute (Hetzner + Containers)

Once local-first is proven and users want server-side materialization:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    Hetzner Dedicated Server(s)                       │
│                                                                      │
│  ┌──────────────┐     ┌──────────────┐     ┌──────────────────────┐ │
│  │ Axum Server  │────►│  PostgreSQL   │     │ Compute Workers      │ │
│  │ (API +       │     │  (+ job queue │◄────│ (gVisor containers,  │ │
│  │  job dispatch)     │   via LISTEN/ │     │  isolated execution) │ │
│  │              │     │   NOTIFY)     │     │                      │ │
│  └──────────────┘     └──────────────┘     └──────────────────────┘ │
│         │                                           │                │
│         ▼                                           ▼                │
│  ┌──────────────────────────────────────────────────────────────────┤
│  │                     Local NVMe Storage                            │
│  │     (raw data, transforms, cached results, runtime envs)         │
│  └──────────────────────────────────────────────────────────────────┘
└─────────────────────────────────────────────────────────────────────┘
```

**New capabilities:**
- `compute=server` option in `ozzy.fetch()` — server runs the DAG
- Async job queue for expensive materializations
- Environment caching on the server (build once, reuse)
- gVisor isolation for untrusted transform code

**Scaling on Hetzner:**
- Add more dedicated servers for compute workers
- PostgreSQL can run on a separate box with read replicas
- Local NVMe is fast and cheap; R2 for overflow/backup

### 16.3 Production Scale (Multi-Region)

If OzzyDB grows beyond what a few Hetzner boxes can handle:

```
┌──────────┐     ┌──────────────┐     ┌──────────────┐
│ Cloudflare│────►│ API Servers  │────►│ PostgreSQL   │
│ (CDN/WAF) │     │ (Axum, N×)   │     │ (primary +   │
└──────────┘     └──────┬───────┘     │  read replicas)│
                        │             └──────────────┘
                        ▼
                 ┌──────────────┐     ┌──────────────┐
                 │ Compute Pool │────►│ Cloudflare   │
                 │ (Firecracker │     │ R2           │
                 │  microVMs)   │     │              │
                 └──────────────┘     └──────────────┘
```

**When to consider this:**
- Hundreds of concurrent users
- Multi-tenant with untrusted code (Firecracker > gVisor)
- Global latency requirements (edge caching, regional compute)
- Need for formal SLAs

**For now, this is not the priority.** Start local-first, add a single Hetzner box when sharing is needed, and scale from there based on real usage patterns.

---

## 17. Implementation Roadmap

The roadmap follows a **local-first** strategy: build a useful local tool first, then add server features incrementally. This validates the core data model and UX before investing in infrastructure.

### Phase 1: Local-First CLI (Weeks 1-6)

**Goal**: A working local tool with Riley's sap flux data. No server required.

**Core CLI (Rust):**
- [ ] `ozzy init` — create `ozzy.toml` and `.ozzy/` directory
- [ ] `ozzy data add <file> --name <name>` — register a raw data source (parquet)
- [ ] `ozzy data ls` — list data sources
- [ ] `ozzy transform add <file:function>` — register a Python transform
- [ ] `ozzy transform ls` — list transforms
- [ ] `ozzy endpoint create <name> --input <source> --transforms <t1,t2,...>` — define a pipeline
- [ ] `ozzy endpoint ls` — list endpoints
- [ ] `ozzy dag show` — visualize the DAG (ASCII or SVG)
- [ ] `ozzy run <endpoint> [--output file.parquet]` — execute the DAG locally
- [ ] `ozzy commit [-m "message"]` — create a local commit
- [ ] `ozzy log` — show commit history

**Local execution engine:**
- [ ] DAG resolution from `.ozzy/commits/`
- [ ] Content-addressed hashing (BLAKE3) with platform fingerprint
- [ ] Platform fingerprint detection (os, arch, libc, blas)
- [ ] Canonicalization (source code, params JSON)
- [ ] Local cache in `~/.ozzy/cache/` (SQLite index + parquet files)
- [ ] Python transform execution via `uv` (create env from lockfile, run transform)
- [ ] Deterministic execution defaults (`PYTHONHASHSEED=0`, `OMP_NUM_THREADS=1`, etc.)
- [ ] Cache hit/miss logic based on materialized hash

**Schema validation (moved from Phase 3):**
- [ ] Extract Arrow schema from parquet files on `data add`
- [ ] Store schema in commit object
- [ ] Validate pipeline composition at `endpoint create` time
- [ ] Reject pipelines with schema mismatches

**Python client:**
- [ ] `ozzy.fetch("./path/to/project/endpoint")` — run local project, return DataFrame
- [ ] `ozzy.inspect("./path/to/project/endpoint")` — get metadata, schema, DAG

**Validation:**
- [ ] End-to-end test with sap flux data: raw → qc → calibrate → corrected
- [ ] Verify cache invalidation when transform code changes
- [ ] Verify deterministic hashing (same inputs → same hash on same platform)
- [ ] Test schema validation catches mismatched transforms

### Phase 2: Registry Server (Weeks 6-10)

**Goal**: Share projects with collaborators. Server is a dumb registry — no compute.

**Server (Rust/Axum on Hetzner):**
- [ ] PostgreSQL schema (projects, commits, data sources, transforms, endpoints, refs)
- [ ] Refs table for `@latest`, tags, and branches
- [ ] `POST /push` — upload commit (data + transforms + lockfiles + DAG)
- [ ] `GET /pull` — download project state
- [ ] `GET /resolve/{owner}/{project}/{endpoint}@{ref}` — resolve ref to commit hash + DAG metadata
- [ ] `GET /{owner}/{project}/{endpoint}@{ref}` — download raw data + transforms (client executes)
- [ ] GitHub OAuth for auth
- [ ] Scoped API tokens

**CLI additions:**
- [ ] `ozzy remote add <name> <url>` — configure remote registry
- [ ] `ozzy push [-m "message"]` — upload to remote
- [ ] `ozzy pull` — download from remote
- [ ] `ozzy fetch <owner/project/endpoint>` — download and execute locally
- [ ] `ozzy auth login` — GitHub OAuth flow
- [ ] `ozzy auth token create` — create API token
- [ ] `ozzy tag <name>` — create a tag pointing to current commit

**Storage:**
- [ ] Local NVMe storage on Hetzner (optionally sync to R2 for redundancy)
- [ ] Content-addressed blob storage for raw data, transforms, lockfiles

**Python client:**
- [ ] `ozzy.fetch("rileyleff/sapflux/corrected")` — download + execute locally
- [ ] `ozzy.fetch("rileyleff/sapflux/corrected@v1.0.0")` — fetch specific version

### Phase 3: Reproducibility & Validation (Weeks 10-14)

**Goal**: Full dependency pinning, determinism verification, multi-runtime support.

**Reproducibility:**
- [ ] Lockfile hash as part of transform identity (`uv.lock` required)
- [ ] `ozzy transform test <name> [--sample 1000]` — test on sample data
- [ ] Determinism report in `transform test` (detects nondeterministic indicators)
- [ ] `ozzy diff <ref1> <ref2>` — compare outputs between refs
- [ ] `ozzy status` — show local changes vs remote
- [ ] `ozzy lineage <endpoint>` — output reproducibility report (hashes, platform, lockfile)

**Multi-runtime support:**
- [ ] R transform support (renv)
- [ ] Julia transform support (Manifest.toml)
- [ ] Runtime-specific determinism defaults

**Semantic schema (optional):**
- [ ] Semantic type annotations (unit, domain, range)
- [ ] Schema evolution tracking

### Phase 4: Server-Side Compute (Weeks 14-20)

**Goal**: Optional server-side execution for users who want it.

**Compute engine:**
- [ ] Job queue (Postgres-backed with LISTEN/NOTIFY)
- [ ] gVisor container isolation for transform execution
- [ ] Environment caching on server (`~/.ozzy/envs/` equivalent)
- [ ] `compute=server` option in API and client

**API additions:**
- [ ] `202 Accepted` for async materializations
- [ ] `GET /jobs/{id}` — job status
- [ ] Webhook notifications on completion

**WASM support:**
- [ ] wasmtime integration for Rust/Go/C++ transforms
- [ ] WASM blob storage and caching

### Phase 5: Collaboration & Publishing (Weeks 20-28)

**Goal**: Multi-user, releases, DOIs.

- [ ] Project visibility (public/private/org)
- [ ] Organization management
- [ ] Collaborator permissions
- [ ] Audit logging
- [ ] Release tagging (`ozzy release create v1.0.0`)
- [ ] DataCite DOI minting
- [ ] DOI landing pages with lineage visualization
- [ ] DOI resolution in client libraries
- [ ] Cross-project dependencies

### Phase 6: Scale & Real-Time (Weeks 28-40)

**Goal**: Large datasets, streaming, production hardening.

- [ ] Chunked/streaming transform execution
- [ ] Data buffer and auto-commit for real-time sources
- [ ] Cache eviction policies (LRU, TTL, pin-on-DOI)
- [ ] Proactive materialization (re-materialize on transform update)
- [ ] Monitoring, alerting, rate limiting
- [ ] R and Julia client libraries

### Phase 7: Ecosystem (Week 40+)

- [ ] Public transform registry
- [ ] Semantic type registry
- [ ] Jupyter/RStudio integrations
- [ ] GitHub Actions integration
- [ ] Web UI: project page, DAG visualization, schema browser

---

## 18. Open Design Decisions

These are decisions that should be resolved during implementation:

1. **Container runtime**: gVisor vs Firecracker vs Nsjail for native runtime isolation. Firecracker is most secure but hardest to operate. gVisor is a good middle ground.

2. **Job queue backend**: Postgres-backed (simple, transactional) vs dedicated queue (Redis/NATS). Start with Postgres, migrate if needed.

3. **WASM compilation verification**: Should the server recompile from source to verify the WASM blob matches? This is expensive but eliminates tampering. Could be opt-in for DOI releases.

4. **Cross-dataset join semantics**: When a transform joins two datasets, how do we express the dependency? Likely: the transform takes named inputs, each resolving to an Ozzy ref.

5. **Pricing model**: Free for public projects (like GitHub)? Per-compute-minute for private? Storage-based? This affects architecture (metering, quotas).

6. **Federation**: Should OzzyDB instances be able to reference each other? (Like git remotes.) This would enable institutional deployments that interoperate with the public registry.

7. **Deterministic WASM compilation**: Rust's `cargo build --target wasm32-wasi` is not perfectly reproducible across machines. May need to standardize the build environment (Nix? Docker?).

8. **Transform discoverability**: How do users find useful public transforms? Tags, categories, search, "stars"? A package registry model (like crates.io) might emerge naturally.

9. **Pipeline diff visualization**: When comparing two pipelines (yours vs a collaborator's), show where they diverge. "We agree on cleaning, disagree on calibration" is high-value for scientific collaboration.

10. **Proactive materialization**: When a transform is updated, optionally kick off background re-materialization of all endpoints that depend on it. Like CI for data. The job queue should be designed to support this from the start.

11. **Platform fingerprint granularity**: How fine-grained should the platform fingerprint be? Too coarse = incorrect cache sharing. Too fine = no cache hits across minor differences. Need empirical testing to find the right balance.

12. **Merge commits**: Should the commit model support merge commits (multiple parents)? This enables collaborative workflows but adds complexity. Current design uses `parent_hashes` array to allow it.

13. **Nondeterministic transform policy for releases**: Should releases with `reproducible=False` transforms be allowed at all? Current design allows them but blocks DOI minting.

14. **Local-only vs remote refs**: How do local refs sync with remote refs? Git-style push/pull/fetch semantics, or something simpler?
