# OzzyDB v2 Implementation Details

Companion to `v2_architecture.md`. This document covers the concrete implementation details for items marked "designed but needs detail."

---

## Table of Contents

1. [Object storage (R2)](#1-object-storage-r2)
2. [Postgres schema](#2-postgres-schema)
3. [API wire protocols](#3-api-wire-protocols)
4. [Runner implementations](#4-runner-implementations)
5. [`ozzy init` experience](#5-ozzy-init-experience)
6. [Local vs remote execution](#6-local-vs-remote-execution)
7. [Frontend changes](#7-frontend-changes)

---

## 1. Object storage (R2)

### Why R2

Cloudflare R2 is S3-compatible with zero egress fees. Since OzzyDB serves data to consumers (potentially large blobs, frequently), egress costs would be significant on S3/GCS. R2 eliminates this.

### Bucket structure

Single bucket, prefixed by object type:

```
ozzydb-store/
├── data/
│   └── {blake3_hash}                      # data atoms (raw bytes)
│
├── cache/
│   └── {materialized_hash}                # cached transform outputs
│
├── source/
│   └── {provider}/{repo}/{sha}.tar.gz     # cached git source tarballs
│
├── collections/
│   └── {collection_version_hash}.json     # collection manifests
│
└── build-logs/
    └── {env_hash}.txt                     # environment build logs
```

### Access patterns

**Data atoms:** Write-once, read-many. Deduplicated by hash (same bytes = same key = stored once). Never deleted (yanking is a metadata flag, not a deletion).

**Cache entries:** Write-once per materialized hash, read-many. Can be evicted under storage pressure (LRU by last_accessed). Re-computable, so loss is a performance hit, not data loss.

**Source tarballs:** Write-once per commit SHA, read on execution. Immutable (a commit SHA always refers to the same content). Evictable and re-fetchable from git provider.

### Fly Machine access via presigned URLs

Fly Machines need to read inputs from R2 and write outputs to R2. Rather than giving containers R2 credentials:

1. OzzyDB server generates **presigned GET URLs** for each input blob (time-limited, e.g., 1 hour)
2. OzzyDB server generates a **presigned PUT URL** for the output location
3. These URLs are passed to the Fly Machine as environment variables
4. The Fly Machine's init script downloads inputs from the presigned URLs before running the transform
5. After the transform completes, the init script uploads the output via the presigned PUT URL

This means:
- No R2 credentials in the container
- URLs are scoped to specific objects and time-limited
- The transform code never touches R2 directly — the init script handles I/O

**Init script** (injected by OzzyDB, runs before the transform):

```bash
#!/bin/bash
set -euo pipefail

# Download inputs from presigned URLs
for input_spec in $OZZY_INPUT_SPECS; do
    name=$(echo "$input_spec" | cut -d'=' -f1)
    url=$(echo "$input_spec" | cut -d'=' -f2-)
    curl -sf "$url" -o "/workspace/inputs/$name"
done

# Run the transform (runner script or command)
$OZZY_TRANSFORM_CMD

# Upload output via presigned URL
for output_file in /workspace/output/*; do
    curl -sf -X PUT -T "$output_file" "$OZZY_OUTPUT_UPLOAD_URL"
done
```

### R2 configuration

```
R2_ACCOUNT_ID     = <cloudflare account id>
R2_ACCESS_KEY_ID  = <access key>
R2_SECRET_ACCESS_KEY = <secret key>
R2_BUCKET_NAME    = ozzydb-store
R2_ENDPOINT       = https://<account_id>.r2.cloudflarestorage.com
```

The server uses the `aws-sdk-s3` Rust crate (or equivalent) with the R2 endpoint. v1's `ContentStorage` abstraction can be adapted — it already has the `from_config()` and `from_config_with_prefix()` pattern with R2-primary and local fallback.

For development/testing, local filesystem storage should remain as a fallback (same as v1). The storage trait:

```rust
trait ObjectStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Vec<u8>>;
    async fn get_stream(&self, key: &str) -> Result<impl AsyncRead>;
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn presigned_get(&self, key: &str, expires: Duration) -> Result<String>;
    async fn presigned_put(&self, key: &str, expires: Duration) -> Result<String>;
}
```

Local storage implements presigned URLs as direct server proxy URLs (the server streams the file itself). R2 storage generates real presigned URLs. The interface is the same.

---

## 2. Postgres schema

### DDL

```sql
-- ============================================================
-- Users (carried from v1, minor additions)
-- ============================================================
CREATE TABLE users (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    github_id       BIGINT UNIQUE,
    gitlab_id       BIGINT UNIQUE,
    username        TEXT NOT NULL UNIQUE,
    display_name    TEXT,
    avatar_url      TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- API tokens (carried from v1)
-- ============================================================
CREATE TABLE api_tokens (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash      TEXT NOT NULL UNIQUE,
    name            TEXT NOT NULL,
    scope           TEXT NOT NULL,              -- "account" | "project:{owner}/{slug}"
    project_id      UUID,                       -- NULL for account tokens
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at      TIMESTAMPTZ,
    last_used_at    TIMESTAMPTZ
);

-- ============================================================
-- Projects
-- ============================================================
CREATE TABLE projects (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    owner_id        UUID NOT NULL REFERENCES users(id),
    slug            TEXT NOT NULL,              -- url-safe project name
    description     TEXT,
    visibility      TEXT NOT NULL DEFAULT 'private' CHECK (visibility IN ('public', 'private')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (owner_id, slug)
);

CREATE TABLE project_collaborators (
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            TEXT NOT NULL DEFAULT 'read' CHECK (role IN ('read', 'write', 'admin')),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, user_id)
);

-- ============================================================
-- Commits (git-referenced)
-- ============================================================
CREATE TABLE commits (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    git_provider    TEXT NOT NULL,              -- "github" | "gitlab"
    git_repo        TEXT NOT NULL,              -- "rileyleff/sapflux-analysis"
    git_commit_sha  TEXT NOT NULL,              -- full 40-char SHA
    ozzy_toml_hash  TEXT NOT NULL,              -- blake3 of ozzy.toml content
    pushed_by       UUID NOT NULL REFERENCES users(id),
    message         TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, git_commit_sha)
);

-- Parsed + cached ozzy.toml content (avoid re-fetching from git on every request)
CREATE TABLE commit_state (
    commit_id       UUID PRIMARY KEY REFERENCES commits(id) ON DELETE CASCADE,
    ozzy_toml_raw   TEXT NOT NULL,              -- raw ozzy.toml content
    environments    JSONB NOT NULL,             -- parsed [environments] section
    transforms      JSONB NOT NULL,             -- parsed [transforms] section
    endpoints       JSONB NOT NULL,             -- parsed [endpoints] section
    project_meta    JSONB NOT NULL,             -- parsed [project] + [git] + [remote]
    parsed_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Refs (branches and tags)
CREATE TABLE refs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    ref_name        TEXT NOT NULL,              -- "main", "v1.0", etc.
    ref_type        TEXT NOT NULL CHECK (ref_type IN ('branch', 'tag')),
    commit_id       UUID NOT NULL REFERENCES commits(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, ref_name)
);

-- ============================================================
-- Data atoms
-- ============================================================
CREATE TABLE data_atoms (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,              -- human-readable, url-safe
    hash            TEXT NOT NULL,              -- blake3 of raw bytes
    content_type    TEXT NOT NULL,              -- MIME type
    byte_size       BIGINT NOT NULL,
    r2_key          TEXT NOT NULL,              -- key in R2 bucket (data/{hash})
    uploaded_by     UUID NOT NULL REFERENCES users(id),
    yanked          BOOLEAN NOT NULL DEFAULT false,
    yank_reason     TEXT,
    yanked_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

-- Content deduplication across projects.
-- Multiple data_atoms can reference the same r2_key if they have the same hash.
CREATE TABLE content_refs (
    hash            TEXT PRIMARY KEY,
    r2_key          TEXT NOT NULL,
    content_type    TEXT NOT NULL,
    byte_size       BIGINT NOT NULL,
    ref_count       INT NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Data metadata (append-only log)
-- ============================================================
CREATE TABLE data_metadata_log (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    data_atom_id    UUID NOT NULL REFERENCES data_atoms(id) ON DELETE CASCADE,
    field           TEXT NOT NULL,              -- "description", "tags", "license", "schema"
    value           JSONB NOT NULL,
    set_by          UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Index for fast "latest metadata" lookups
CREATE INDEX idx_metadata_latest
    ON data_metadata_log (data_atom_id, field, created_at DESC);

-- ============================================================
-- Collections
-- ============================================================
CREATE TABLE collections (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,              -- human-readable, url-safe
    created_by      UUID NOT NULL REFERENCES users(id),
    yanked          BOOLEAN NOT NULL DEFAULT false,
    yank_reason     TEXT,
    yanked_at       TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

-- Each membership change creates a new version
CREATE TABLE collection_versions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id   UUID NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    version_number  INT NOT NULL,
    hash            TEXT NOT NULL,              -- blake3(sorted member hashes, recursive)
    created_by      UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (collection_id, version_number)
);

-- Members of a specific collection version.
-- member_type + member_ref identify what the member is.
-- member_hash is the resolved hash at the time the version was created.
CREATE TABLE collection_members (
    id                      UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_version_id   UUID NOT NULL REFERENCES collection_versions(id) ON DELETE CASCADE,
    member_type             TEXT NOT NULL CHECK (member_type IN ('data', 'endpoint', 'collection')),
    member_ref              TEXT NOT NULL,      -- atom name, endpoint ref, or collection name
    member_hash             TEXT NOT NULL,      -- resolved hash at version creation time
    ordinal                 INT NOT NULL,       -- ordering within collection
    UNIQUE (collection_version_id, ordinal)
);

-- ============================================================
-- Endpoint yanking
-- (Endpoint definitions live in ozzy.toml / commit_state.
--  This table tracks yanked endpoint versions specifically.)
-- ============================================================
CREATE TABLE endpoint_yanks (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    endpoint_name   TEXT NOT NULL,
    commit_id       UUID NOT NULL REFERENCES commits(id),  -- which version was yanked
    yank_reason     TEXT NOT NULL,
    yanked_by       UUID NOT NULL REFERENCES users(id),
    yanked_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, endpoint_name, commit_id)
);

-- ============================================================
-- Secrets (encrypted, per-project)
-- ============================================================
CREATE TABLE secrets (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name            TEXT NOT NULL,              -- "GEMINI_API_KEY"
    encrypted_value BYTEA NOT NULL,            -- encrypted with server-side key
    rotation_counter INT NOT NULL DEFAULT 1,   -- incremented on every set (even if value unchanged)
                                                -- included in materialized hash to invalidate cache on rotation
    set_by          UUID NOT NULL REFERENCES users(id),
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, name)
);

-- ============================================================
-- Environment images (tracking built environments)
-- ============================================================
CREATE TABLE environment_images (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    env_hash        TEXT NOT NULL UNIQUE,       -- blake3(base_image_digest + lockfile_hash)
                                                -- or blake3(dockerfile_content)
    image_ref       TEXT NOT NULL,              -- full container registry reference
    build_type      TEXT NOT NULL CHECK (build_type IN ('base_lockfile', 'dockerfile', 'prebuilt')),
    base_image      TEXT,                       -- e.g., "ozzydb/python:3.12" (null for prebuilt)
    build_log_r2_key TEXT,                      -- build log in R2 (null for prebuilt)
    built_at        TIMESTAMPTZ,
    build_duration_ms INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ============================================================
-- Source cache (cached git tarballs)
-- ============================================================
CREATE TABLE source_cache (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    git_provider    TEXT NOT NULL,
    git_repo        TEXT NOT NULL,
    git_commit_sha  TEXT NOT NULL,
    r2_key          TEXT NOT NULL,
    byte_size       BIGINT NOT NULL,
    cached_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_accessed   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (git_provider, git_repo, git_commit_sha)
);

-- ============================================================
-- Materialized cache (transform outputs)
-- ============================================================
CREATE TABLE materialized_cache (
    materialized_hash   TEXT PRIMARY KEY,       -- blake3(inputs + transform + params + platform)
    project_id          UUID NOT NULL REFERENCES projects(id),
    commit_id           UUID NOT NULL REFERENCES commits(id),
    endpoint_name       TEXT NOT NULL,
    node_name           TEXT NOT NULL,
    transform_name      TEXT NOT NULL,
    output_hash         TEXT NOT NULL,          -- blake3 of output bytes
    output_r2_key       TEXT NOT NULL,
    output_content_type TEXT NOT NULL,
    output_byte_size    BIGINT NOT NULL,
    platform            TEXT NOT NULL,          -- platform fingerprint string
    verification_tier   INT NOT NULL DEFAULT 1, -- 1=server-verified, 2=client-computed
    computed_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_accessed       TIMESTAMPTZ NOT NULL DEFAULT now(),
    access_count        INT NOT NULL DEFAULT 1
);
```

### Key design notes

**commit_state caches parsed ozzy.toml:** When a commit is pushed, the server fetches ozzy.toml from git, parses it, and stores the parsed sections as JSONB. This avoids re-fetching and re-parsing on every request. The raw TOML is also stored for debugging.

**collection_members stores resolved hashes:** When a version is created, each member's hash is resolved and stored. For data atoms, that's the atom hash. For endpoint outputs, that's the materialized hash of the endpoint at that moment. For sub-collections, that's the sub-collection's version hash. This makes the collection version hash deterministic and immutable.

**endpoint_yanks is separate from endpoint definitions:** Endpoints are defined in ozzy.toml (stored in commit_state). Yanking is an out-of-band operation that marks a specific endpoint at a specific commit as retracted. This avoids modifying the commit record.

**content_refs enables cross-project deduplication:** If two projects upload the same file (same bytes = same blake3 hash), it's stored once in R2. The ref_count tracks how many data_atoms reference it.

### Indexes

```sql
-- Fast lookups for common queries
CREATE INDEX idx_commits_project     ON commits (project_id, created_at DESC);
CREATE INDEX idx_data_atoms_project  ON data_atoms (project_id);
CREATE INDEX idx_data_atoms_hash     ON data_atoms (hash);
CREATE INDEX idx_collections_project ON collections (project_id);
CREATE INDEX idx_coll_versions       ON collection_versions (collection_id, version_number DESC);
CREATE INDEX idx_coll_members        ON collection_members (collection_version_id);
CREATE INDEX idx_cache_project       ON materialized_cache (project_id, endpoint_name);
CREATE INDEX idx_cache_accessed      ON materialized_cache (last_accessed);  -- for LRU eviction
CREATE INDEX idx_refs_project        ON refs (project_id);
CREATE INDEX idx_endpoint_yanks      ON endpoint_yanks (project_id, endpoint_name);
```

---

## 3. API wire protocols

### Authentication

Carried from v1. GitHub OAuth device flow for login. Bearer token (JWT) for API calls. Project-scoped tokens for CI/automation.

```
POST   /v1/auth/device_start        → { device_code, user_code, verification_uri }
GET    /v1/auth/device_poll          → { token } or { pending }
GET    /v1/auth/me                   → { user }
POST   /v1/auth/token/create         → { token }
GET    /v1/auth/token/list           → [{ name, scope, created_at }]
DELETE /v1/auth/token/revoke/:id
```

### Push

Registers a git commit with the OzzyDB registry.

```
POST /v1/push
  Authorization: Bearer <token>
  Content-Type: application/json

  {
    "project": "rileyleff/sapflux-analysis",
    "git_provider": "github",
    "git_repo": "rileyleff/sapflux-analysis",
    "git_commit_sha": "a1b2c3d4e5f6...",
    "ref": "main",                     // optional: update this ref
    "message": "Updated QC threshold"  // optional
  }

  Server-side steps:
    1. Verify user has write access to the project (create if first push)
    2. Fetch ozzy.toml from git provider API at the commit SHA
    3. Parse and validate ozzy.toml (environments, transforms, endpoints)
    4. Verify all referenced source files exist at the commit
    5. Cache source tarball in R2
    6. Build or locate environment images (see section 4)
    7. Insert commit + commit_state records
    8. Upsert ref if specified
    9. Return response

  Response 200:
  {
    "commit_id": "uuid",
    "git_commit_sha": "a1b2c3d4e5f6...",
    "environments": [
      { "name": "scipy-stack", "env_hash": "abc123", "status": "cached" },
      { "name": "geo", "env_hash": "def456", "status": "building" }
    ],
    "source_cached": true
  }
```

### Fetch (endpoint execution)

Resolves an endpoint, executes uncached transforms, returns the result.

```
GET /v1/fetch/{owner}/{project}/{endpoint_name}
  ?ref=main                        // optional: resolve ref (default: latest)
  &qc_threshold=50                 // optional: endpoint params
  &format=parquet                  // optional: output format hint
  Authorization: Bearer <token>    // required for private projects

  Server-side steps:
    1. Resolve project → ref → commit
    2. Load commit_state (parsed ozzy.toml)
    3. Find endpoint definition
    4. Check endpoint_yanks — if yanked, return 410 Gone with reason
    5. Validate consumer params against declared min/max/enum
    6. Resolve all data: and collection: references
       - Check data_atoms exist and are not yanked
       - Resolve collection to latest version, recursively resolve members
       - Resolve endpoint: references (recursive fetch)
    7. Compute materialized hash chain for each node in the DAG
    8. Check materialized_cache at each node
    9. For uncached nodes, build execution plan:
       a. Group consecutive same-environment nodes for batching
       b. Generate presigned R2 URLs for inputs
       c. Dispatch to Fly Machine (see section 8 of architecture doc)
       d. On completion, verify output, store in R2, insert cache record
    10. Stream final node's output from R2

  Response 200:
    Body: raw output bytes (parquet, image, JSON, etc.)
    Headers:
      Content-Type: application/vnd.apache.parquet
      X-OzzyDB-Hash: <materialized_hash>
      X-OzzyDB-Verification: server-verified | client-computed | uploaded
      X-OzzyDB-Cache: hit | miss

  Response 410 (yanked):
  {
    "error": "yanked",
    "message": "This endpoint has been yanked.",
    "reason": "Based on yanked input data. Use v2.",
    "yanked_at": "2024-03-15T14:30:00Z"
  }

  Response 400 (param validation):
  {
    "error": "invalid_params",
    "message": "Parameter 'qc_threshold' out of range.",
    "details": { "param": "qc_threshold", "got": 100.0, "min": 0.0, "max": 20.0 }
  }
```

### Data upload

```
POST /v1/data/upload
  Authorization: Bearer <token>
  Content-Type: multipart/form-data

  Fields:
    file: <binary>                   // required
    project: "rileyleff/sapflux"     // required
    name: "raw_readings"             // optional (default: filename stem)
    description: "..."               // optional
    content_type: "parquet"          // optional (inferred from extension)
    tags: "raw,sap-flux,2024"        // optional (comma-separated)
    collection: "all_readings"       // optional (add to collection after upload)

  Server-side steps:
    1. Receive file, compute blake3 hash
    2. Check content_refs — if hash exists, skip R2 upload (dedup)
    3. If new, upload to R2 at data/{hash}
    4. Upsert content_refs (increment ref_count if exists)
    5. Insert data_atoms record
    6. If metadata provided, insert data_metadata_log entries
    7. If collection specified, add atom to collection (creates new version)
    8. Return response

  Response 200:
  {
    "name": "raw_readings",
    "hash": "abc123...",
    "content_type": "application/vnd.apache.parquet",
    "byte_size": 1048576,
    "deduplicated": false,
    "collection_version": 3           // if collection was specified
  }
```

### Data management

```
GET    /v1/data/{owner}/{project}                    → list data atoms
GET    /v1/data/{owner}/{project}/{name}             → atom metadata
DELETE /v1/data/{owner}/{project}/{name}              → soft delete (yank)
POST   /v1/data/{owner}/{project}/{name}/yank
         { "reason": "Sensor miscalibration." }      → yank with reason
POST   /v1/data/{owner}/{project}/{name}/describe
         { "field": "description", "value": "..." }  → update metadata
GET    /v1/data/{owner}/{project}/{name}/metadata     → full metadata log
GET    /v1/data/{owner}/{project}/{name}/download     → presigned URL or stream
```

### Collection management

```
POST   /v1/collections/{owner}/{project}
         { "name": "all_readings" }                   → create collection

GET    /v1/collections/{owner}/{project}               → list collections

GET    /v1/collections/{owner}/{project}/{name}        → current version + members

GET    /v1/collections/{owner}/{project}/{name}/log    → version history

GET    /v1/collections/{owner}/{project}/{name}/flatten → all leaf-level atoms

POST   /v1/collections/{owner}/{project}/{name}/add
         { "members": ["data:readings_jan", "endpoint:canonical-2020", "collection:raw-data"] }
         → add members, creates new version
         Server validates: no circular references, all refs exist

POST   /v1/collections/{owner}/{project}/{name}/remove
         { "members": ["data:readings_jan"] }
         → remove members, creates new version

POST   /v1/collections/{owner}/{project}/{name}/yank
         { "reason": "..." }                           → yank collection
```

### Secret management

```
POST   /v1/secrets/{owner}/{project}
         { "name": "GEMINI_API_KEY", "value": "sk-..." }
         → encrypt and store. Value never returned after this.

GET    /v1/secrets/{owner}/{project}
         → [{ "name": "GEMINI_API_KEY", "created_at": "...", "updated_at": "..." }]
         Names only. Never values.

DELETE /v1/secrets/{owner}/{project}/{name}
         → delete secret
```

### Endpoint inspection (no execution)

```
GET /v1/endpoints/{owner}/{project}
      ?ref=main
      → list endpoints with descriptions, params, verification status

GET /v1/endpoints/{owner}/{project}/{name}
      ?ref=main
      → endpoint detail: DAG structure, params with types/defaults/ranges,
        verification tier, data dependencies, last computed time

GET /v1/endpoints/{owner}/{project}/{name}/dag
      ?ref=main&format=json|mermaid|svg
      → DAG visualization
```

---

## 4. Runner implementations

The runner is a small script generated by OzzyDB and injected into the container. It bridges the container I/O contract (files + env vars) to a language-specific function call. The user never sees or writes the runner.

### Python runner

Handles: `source = "path/to/file.py:function_name"`

```python
#!/usr/bin/env python3
"""OzzyDB Python runner. Auto-generated — do not edit."""
import sys, os, json

sys.path.insert(0, '/workspace/source')

# --- Load params ---
params = json.loads(os.environ.get("OZZY_PARAMS", "{}"))

# --- Load inputs ---
# Input manifest is a JSON dict: { input_name: { path, content_type } }
input_manifest = json.loads(os.environ.get("OZZY_INPUT_MANIFEST", "{}"))
inputs = {}

for name, spec in input_manifest.items():
    path = spec["path"]
    content_type = spec["content_type"]
    is_collection = spec.get("is_collection", False)

    if is_collection:
        # Collection: load each member
        member_manifest = json.loads(open(spec["manifest_path"]).read())
        members = []
        for member in member_manifest:
            members.append(_load_item(member["path"], member["content_type"]))
        inputs[name] = members
    else:
        inputs[name] = _load_item(path, content_type)

def _load_item(path, content_type):
    if "parquet" in content_type:
        import polars as pl
        return pl.read_parquet(path)
    elif content_type.startswith("image/"):
        return open(path, "rb").read()  # raw bytes
    elif content_type == "application/json":
        return json.loads(open(path).read())
    elif content_type.startswith("text/"):
        return open(path).read()
    else:
        return open(path, "rb").read()  # fallback: raw bytes

# --- Import and call the user's function ---
from {module} import {function}
result = {function}(inputs, params)

# --- Handle output ---
output_dir = "/workspace/output"

if isinstance(result, list):
    # Collection output: write each item
    manifest = []
    for i, item in enumerate(result):
        out_path = os.path.join(output_dir, f"item_{i:06d}")
        _write_item(item, out_path)
        manifest.append({{"index": i, "path": out_path}})
    with open(os.path.join(output_dir, "manifest.json"), "w") as f:
        json.dump(manifest, f)
else:
    # Single output
    out_path = os.path.join(output_dir, "result")
    _write_item(result, out_path)

def _write_item(item, path):
    if hasattr(item, 'collect'):
        item = item.collect()  # LazyFrame → DataFrame
    if hasattr(item, 'write_parquet'):
        item.write_parquet(path + ".parquet")
    elif isinstance(item, (bytes, bytearray)):
        with open(path, "wb") as f:
            f.write(item)
    elif isinstance(item, str):
        with open(path, "w") as f:
            f.write(item)
    elif isinstance(item, dict):
        with open(path + ".json", "w") as f:
            json.dump(item, f)
    else:
        raise TypeError(f"Unsupported output type: {{type(item)}}")
```

**Note:** The `{module}` and `{function}` placeholders are filled by OzzyDB from the `source = "path:function"` field. The runner is generated per-transform, not generic.

### R runner

Handles: `source = "path/to/file.R:function_name"`

```r
#!/usr/bin/env Rscript
# OzzyDB R runner. Auto-generated.
library(jsonlite)
library(arrow)

# Load params
params <- fromJSON(Sys.getenv("OZZY_PARAMS", "{}"))

# Load input manifest
input_manifest <- fromJSON(Sys.getenv("OZZY_INPUT_MANIFEST", "{}"))
inputs <- list()

for (name in names(input_manifest)) {
  spec <- input_manifest[[name]]
  if (grepl("parquet", spec$content_type)) {
    inputs[[name]] <- read_parquet(spec$path)
  } else if (spec$content_type == "text/csv") {
    inputs[[name]] <- read.csv(spec$path)
  } else {
    inputs[[name]] <- readBin(spec$path, "raw", file.info(spec$path)$size)
  }
}

# Source the user's file and call the function
source("/workspace/source/{source_path}")
result <- {function_name}(inputs, params)

# Write output
output_dir <- "/workspace/output"
if (inherits(result, "data.frame") || inherits(result, "ArrowTabular")) {
  write_parquet(result, file.path(output_dir, "result.parquet"))
} else {
  # Fallback: serialize as RDS (not ideal, but safe)
  saveRDS(result, file.path(output_dir, "result.rds"))
}
```

### Command runner

No runner needed. OzzyDB substitutes system-controlled template variables in the `command` field and executes it directly in the container shell:

1. Parse `${input.NAME}` → replace with `/workspace/inputs/NAME`
2. Parse `${output}` → replace with `/workspace/output/result`
3. Execute via `/bin/sh -c "<substituted command>"`

**Parameters are NOT template-substituted.** Param values come from users (consumers calling `fetch()`) and could contain shell metacharacters. Instead, params are accessible only via environment variables (`$OZZY_PARAM_*`, `$OZZY_PARAMS`) and the params file (`/workspace/params.json`). The shell expands `$OZZY_PARAM_epsg` as a single token, preventing injection.

All env vars (`OZZY_PARAMS`, `OZZY_PARAM_*`, `OZZY_INPUT_*`, `OZZY_OUTPUT`) are set before the command runs.

### Input manifest

Rather than a separate env var per input, the runner uses `OZZY_INPUT_MANIFEST` — a JSON blob describing all inputs:

```json
{
  "readings": {
    "path": "/workspace/inputs/readings.parquet",
    "content_type": "application/vnd.apache.parquet",
    "is_collection": false
  },
  "all_readings": {
    "path": "/workspace/inputs/all_readings/",
    "content_type": "collection",
    "is_collection": true,
    "manifest_path": "/workspace/inputs/all_readings/manifest.json"
  }
}
```

For collections, the manifest file lists all members:

```json
[
  { "index": 0, "name": "readings_jan", "path": "/workspace/inputs/all_readings/0.parquet", "content_type": "application/vnd.apache.parquet", "hash": "abc123" },
  { "index": 1, "name": "readings_feb", "path": "/workspace/inputs/all_readings/1.parquet", "content_type": "application/vnd.apache.parquet", "hash": "def456" }
]
```

The individual `OZZY_INPUT_*` env vars are still set for convenience (especially for command-based transforms), but the manifest is the complete source of truth.

---

## 5. `ozzy init` experience

```bash
$ cd my-research-project
$ ozzy init
```

**Steps:**

1. **Detect git repo:**
   ```
   Detected git repo: github.com/rileyleff/my-research-project
   ```
   If not in a git repo, warn and ask if the user wants to proceed without git integration.

2. **Detect language/runtime:**
   Scan for lockfiles and project files:
   - `uv.lock` / `pyproject.toml` → Python + uv
   - `poetry.lock` → Python + poetry
   - `renv.lock` → R + renv
   - `Manifest.toml` → Julia
   - `Cargo.lock` → Rust
   - `Dockerfile` → container

   ```
   Detected: Python project (pyproject.toml + uv.lock)
   ```

3. **Generate `ozzy.toml`:**

   ```toml
   [project]
   name = "my-research-project"
   owner = "rileyleff"

   [git]
   provider = "github"
   repo = "rileyleff/my-research-project"

   [remote]
   url = "https://api.ozzydb.com"

   [environments.default]
   base = "ozzydb/python:3.12"
   lockfile = "uv.lock"

   # Add transforms here:
   # [transforms.my_transform]
   # source = "transforms/my_transform.py:my_function"
   # environment = "default"
   # inputs.data = "parquet"
   # output = "parquet"

   # Add endpoints here:
   # [endpoints.my_endpoint]
   # description = "..."
   # [endpoints.my_endpoint.nodes]
   # step = { transform = "my_transform" }
   # edges = [
   #   { from = "data:my_data", to = "step.data" },
   # ]
   ```

4. **Print next steps:**
   ```
   Created ozzy.toml

   Next steps:
     1. Upload data:       ozzy data upload <file>
     2. Add transforms:    edit ozzy.toml [transforms] section
     3. Define endpoints:  edit ozzy.toml [endpoints] section
     4. Test locally:      ozzy run <endpoint>
     5. Push to registry:  ozzy push
   ```

### `ozzy transform scaffold`

Helper command to generate transform boilerplate:

```bash
$ ozzy transform scaffold quality_control --lang python
```

Creates `transforms/quality_control.py`:

```python
def quality_control(inputs, params):
    """TODO: Implement transform logic.

    Args:
        inputs: dict of input data (keys match ozzy.toml input declarations)
        params: dict of parameters (keys match ozzy.toml param declarations)

    Returns:
        Output data (DataFrame, bytes, list for collections, etc.)
    """
    raise NotImplementedError("Implement this transform")
```

And prints suggested TOML to add:

```
Add this to your ozzy.toml:

[transforms.quality_control]
source = "transforms/quality_control.py:quality_control"
environment = "default"
inputs.data = "parquet"
output = "parquet"
# params.threshold = { type = "float" }
```

---

## 6. Local vs remote execution

### `ozzy run <endpoint>` — local development

Runs an endpoint locally using Docker. Same container, same I/O contract, same determinism — just different orchestrator.

```bash
$ ozzy run corrected_readings --param qc_threshold=12.0
```

**Steps:**

1. Parse `ozzy.toml` from local working directory
2. Resolve data references:
   - `data:raw_readings` → fetch from OzzyDB registry to local cache (`~/.ozzy/cache/data/{hash}`)
   - `collection:all_readings` → fetch collection manifest + member data
   - `endpoint:other/project/name` → recursive remote fetch
3. Resolve endpoint params (apply user overrides + defaults)
4. Build execution plan (topological sort, cache check at each node)
5. For each uncached node:
   a. Pull environment image (from GHCR or local Docker cache)
   b. Mount inputs from local cache
   c. Run container via `docker run` with:
      - `--network none` (unless `network = true`)
      - Determinism env vars
      - Input mounts + output volume
      - Runner script
   d. Collect output to local cache (`~/.ozzy/cache/materialized/{hash}`)
6. Display or write final output

**Local cache layout:**

```
~/.ozzy/cache/
├── data/
│   └── {hash}                     # downloaded data atoms
├── materialized/
│   └── {materialized_hash}        # cached transform outputs
└── index.db                       # SQLite index (same concept as v1)
```

### `ozzy fetch <owner/project/endpoint>` — remote execution

Fetches from the registry. Server resolves, executes on Fly, streams result.

```bash
$ ozzy fetch rileyleff/sapflux/corrected_readings(qc_threshold=12.0)
```

**Steps:**

1. Parse the reference (owner, project, endpoint, params, optional ref)
2. Call `GET /v1/fetch/{owner}/{project}/{endpoint}?ref=...&qc_threshold=12.0`
3. Server handles everything (see API protocol above)
4. Stream response to stdout or write to file

```bash
# Write to file
$ ozzy fetch rileyleff/sapflux/corrected_readings -o output.parquet

# Pipe to Python
$ ozzy fetch rileyleff/sapflux/corrected_readings | python analyze.py
```

### Python client

```python
import ozzydb as ozzy

# Remote fetch (calls the API)
df = ozzy.fetch("rileyleff/sapflux/corrected_readings", qc_threshold=12.0)

# Local run (calls ozzy run via subprocess)
df = ozzy.run("./corrected_readings", qc_threshold=12.0)

# Inspect without executing
meta = ozzy.inspect("rileyleff/sapflux/corrected_readings")
print(meta.params)       # available params with types and defaults
print(meta.dag)          # DAG structure
print(meta.verification) # verification tier
```

---

## 7. Frontend changes

### New pages

**Data browser** (`/{owner}/{project}/data`)
- List all data atoms with name, content type, size, upload date
- Upload button → drag-and-drop modal with metadata form
- Click atom → detail view with metadata, schema (if parquet), download button
- Yanked atoms shown with strikethrough and reason

**Collection browser** (`/{owner}/{project}/collections`)
- List all collections
- Click collection → tree view of members (expandable for nested collections)
- Version history timeline
- "Add members" and "Remove members" actions
- Flatten view (all leaf-level atoms)

**Endpoint explorer** (`/{owner}/{project}/endpoints/{name}`)
- Description
- Params form: input fields for each param with type, default, min/max, description
- "Run" button → executes fetch with current params, shows result
- DAG visualization (mermaid or custom SVG)
- Verification badge
- Cache status (hit/miss, last computed)
- Yank status

**Secrets management** (`/{owner}/{project}/settings/secrets`)
- List secrets (names only)
- Add/delete secrets

### Updated pages

**Project overview** (`/{owner}/{project}`)
- Summary cards: N data atoms, N collections, N endpoints, N commits
- Recent activity (uploads, pushes, fetches)
- Quick links to data, collections, endpoints

**Commit detail** (`/{owner}/{project}/commits/{sha}`)
- Git commit info (SHA, message, author, date)
- Link to git provider (GitHub commit page)
- Diff of ozzy.toml (what changed in transforms/endpoints/environments)

### Carried forward

- Auth flow (GitHub OAuth device flow → login page)
- Project list / user profile
- Theme (tuxedo cat: black/white + pink #e8657a + green #a3b86c)
- SvelteKit 5 SPA with `@sveltejs/adapter-static`
- Caddy serving static files for `ozzydb.com`, reverse proxy for `api.ozzydb.com`
