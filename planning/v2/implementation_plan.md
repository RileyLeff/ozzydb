# OzzyDB v2 Implementation Plan

Derived from `planning/v2_architecture.md` and `planning/v2_implementation_details.md`.

**Current state:** v1 code gutted. Surviving infrastructure: BLAKE3 hashing, canonicalization, platform fingerprint, Arrow schema parsing, auth routes (GitHub OAuth device flow, API tokens), content storage (R2 + local), frontend landing + login, CLI command stubs. DB schema is v1-flavored and needs full replacement.

---

## Phase 1: Foundation

Replace the database schema, build the `ozzy.toml` parser, and update core hashing — the three pillars everything else depends on.

### Step 1.1: Replace Postgres schema with v2 DDL

Drop all v1 migrations. Write a single fresh `001_v2_initial.sql` containing the full v2 schema from the implementation doc section 2:

- `users` (carried forward, minor tweaks)
- `api_tokens` (scope TEXT singular, project_id nullable)
- `projects` (owner_id, slug, visibility)
- `project_collaborators` (role: read/write/admin)
- `commits` (git_provider, git_repo, git_commit_sha, ozzy_toml_hash)
- `commit_state` (ozzy_toml_raw, environments/transforms/endpoints JSONB)
- `refs` (ref_name, ref_type branch/tag, commit_id)
- `data_atoms` (name, hash, content_type, byte_size, r2_key, yanked)
- `content_refs` (hash dedup across projects)
- `data_metadata_log` (append-only metadata)
- `collections` + `collection_versions` + `collection_members`
- `endpoint_yanks`
- `secrets` (encrypted, version_id for cache invalidation)
- `environment_images`
- `source_cache`
- `materialized_cache`
- `github_installations`
- All indexes from implementation doc

Update DB models (`db/models.rs`) and queries (`db/queries.rs`) to match the new schema. Update existing auth queries that touch `users` and `api_tokens`.

**Tests:** DB migration runs cleanly. Existing auth tests pass against new schema.

### Step 1.2: Build the `ozzy.toml` parser

New module: `crates/ozzy-core/src/toml_spec.rs`

Implement all structs from implementation doc section 8:
- `OzzyToml`, `ProjectSection`, `GitSection`, `RemoteSection`
- `EnvironmentDef` (enum: BaseLockfile / Dockerfile / Prebuilt)
- `TransformDef`, `ParamDef`
- `EndpointDef`, `EndpointParamDef`, `NodeDef`, `EdgeDef`
- `OutputSchemaDef`

Implement `OzzyToml::validate()` with all 11 validation rules:
1. Name format (`[a-zA-Z0-9_-]+`)
2. Environment refs exist
3. Transform exclusivity (source XOR command)
4. Node transform refs exist
5. Edge targets are valid `node.input`
6. Edge sources are valid (`data:`, `collection:`, `endpoint:`, bare node)
7. Input coverage (every input has exactly one edge)
8. No cycles (Kahn's algorithm)
9. Param binds reference valid `node.param`
10. Cross-project pinning (`endpoint:owner/project/name` must have `@ref`)
11. Content type compatibility

Return `Vec<ValidationError>` with location info and suggestions.

**Tests:** Parse valid TOML, reject invalid TOML (each validation rule), fuzzy name matching for "did you mean?" suggestions.

### Step 1.3: Update core hash functions

Update `crates/ozzy-core/src/hash.rs`:

- `transform_hash()` — add `environment_image_hash` parameter per v2 spec: `blake3(source_hash + function_name + lockfile_hash + environment_image_hash + params_schema_hash)`
- `secrets_hash()` — new: `blake3(sorted(secret_name + version_id) pairs)`
- `materialized_hash()` — update to include optional `secrets_hash`
- `collection_hash()` — new: `blake3(sorted member reference hashes)` with recursive resolution

**Tests:** Hash stability tests (golden values), secrets_hash with empty/single/multiple secrets.

### Step 1.4: Update server AppState and DB layer

- Update `AppState` to prepare for new fields (git provider client, compute backend will be added in later phases)
- Rewrite `db/models.rs` with v2 FromRow structs for all new tables
- Rewrite `db/queries.rs` with v2 query functions:
  - Project CRUD (create, get by owner/slug, list by user, update, delete)
  - Commit operations (insert, get by SHA, list by project)
  - Commit state (insert parsed ozzy.toml, get by commit)
  - Ref operations (upsert, resolve, list, delete)
  - Data atom operations (insert, get by name, list, yank)
  - Content ref operations (upsert with ref_count, dedup check)
  - Collection operations (create, version, add/remove members, flatten)
  - Secret operations (set, list names, delete)
  - Environment image operations (insert, get by hash)
  - Source cache operations (insert, get, update last_accessed)
  - Materialized cache operations (insert, get, update access tracking)
  - GitHub installation operations (upsert, lookup by login)

**Tests:** Query tests for each operation against real Postgres (testcontainers).

---

## Phase 2: Data Plane

The imperative half — upload data, manage collections, handle metadata.

### Step 2.1: Data upload API + CLI

**Server:**
- `POST /v1/data/upload` — multipart upload with name, description, content_type, tags, collection
- Server-side: receive file, blake3 hash, dedup via content_refs, store in R2, insert data_atom, optional metadata log entries, optional collection add
- `GET /v1/data/{owner}/{project}` — list data atoms
- `GET /v1/data/{owner}/{project}/{name}` — atom detail + latest metadata
- `GET /v1/data/{owner}/{project}/{name}/download` — presigned URL or stream
- `DELETE /v1/data/{owner}/{project}/{name}` — yank (not delete)
- `POST /v1/data/{owner}/{project}/{name}/yank` — yank with reason
- `POST /v1/data/{owner}/{project}/{name}/describe` — update metadata
- `GET /v1/data/{owner}/{project}/{name}/metadata` — full metadata log

**CLI:**
- `ozzy data upload <file> [--name] [--description] [--collection]`
- `ozzy data ls`
- `ozzy data show <name>`
- `ozzy data describe <name> --set-description "..."`
- `ozzy data yank <name> --reason "..."`
- `ozzy data download <name> [-o file]`

**Tests:** Upload + dedup, metadata append-only log, yank blocks download, bulk upload.

### Step 2.2: Collections API + CLI

**Server:**
- `POST /v1/collections/{owner}/{project}` — create collection
- `GET /v1/collections/{owner}/{project}` — list collections
- `GET /v1/collections/{owner}/{project}/{name}` — current version + members
- `GET /v1/collections/{owner}/{project}/{name}/log` — version history
- `GET /v1/collections/{owner}/{project}/{name}/flatten` — leaf-level atoms
- `POST /v1/collections/{owner}/{project}/{name}/add` — add members (new version)
- `POST /v1/collections/{owner}/{project}/{name}/remove` — remove members (new version)
- `POST /v1/collections/{owner}/{project}/{name}/yank` — yank collection

Cycle detection: DFS with visited set when adding `collection:` members.
Member hash resolution: resolve data atom hashes, endpoint materialized hashes, sub-collection version hashes at add time.

**CLI:**
- `ozzy collection create <name>`
- `ozzy collection add <name> <ref...>`
- `ozzy collection rm <name> <ref...>`
- `ozzy collection ls [name]`
- `ozzy collection log <name>`
- `ozzy collection flatten <name>`

**Tests:** Create, add members, remove members, version history, circular reference rejection, flatten with nested collections.

### Step 2.3: Secrets API + CLI

**Server:**
- `POST /v1/secrets/{owner}/{project}` — set secret (encrypt with AES-256-GCM)
- `GET /v1/secrets/{owner}/{project}` — list names only
- `DELETE /v1/secrets/{owner}/{project}/{name}` — delete secret

Encryption key from `SECRETS_ENCRYPTION_KEY` env var. version_id regenerated on every set.

**CLI:**
- `ozzy secret set <name>` (reads value from stdin or prompt)
- `ozzy secret ls`
- `ozzy secret rm <name>`

**Tests:** Set/list/delete, version_id changes on re-set, value never returned in list.

---

## Phase 3: Git Integration & Push

Wire the compute plane's entry point: registering git commits with the registry.

### Step 3.1: GitHub App integration

New module: `crates/ozzy-server/src/git/`

- `GitProvider` trait: `fetch_archive()`, `get_file()`, `resolve_ref()`
- `GitHubProvider` implementation:
  - JWT signing with App private key
  - Installation token acquisition (POST /app/installations/{id}/access_tokens)
  - File fetch (GET /repos/{owner}/{repo}/contents/{path}?ref={sha})
  - Tarball fetch (GET /repos/{owner}/{repo}/tarball/{sha})
  - Ref resolution (GET /repos/{owner}/{repo}/git/ref/heads/{ref})
- Webhook handler: `POST /v1/webhooks/github` for installation events
- `github_installations` table queries
- Fallback: unauthenticated API for public repos

Add `GitHubProvider` to `AppState`.

**Tests:** Mock GitHub API responses, token flow, webhook handling.

### Step 3.2: Push endpoint

**Server:**
- `POST /v1/push` — register git commit
  1. Verify write access
  2. Create project if first push
  3. Fetch ozzy.toml from git provider at commit SHA
  4. Parse + validate ozzy.toml (using toml_spec parser from Phase 1)
  5. Verify referenced source files exist at commit
  6. Cache source tarball in R2
  7. Trigger environment builds (async — push returns immediately)
  8. Insert commit + commit_state records
  9. Upsert ref if specified

**CLI:**
- `ozzy push [--ref main] [--message "..."]`
  - Requires clean git state
  - Reads HEAD SHA via `git rev-parse HEAD`
  - Reads ozzy.toml and validates locally first
  - Sends push request to registry

**Tests:** Push with valid TOML, push with dirty state rejected, push creates project on first push, ref upsert.

### Step 3.3: Endpoint inspection + project API

**Server:**
- `GET /v1/endpoints/{owner}/{project}` — list endpoints
- `GET /v1/endpoints/{owner}/{project}/{name}` — endpoint detail (DAG, params, verification)
- `GET /v1/endpoints/{owner}/{project}/{name}/dag` — DAG visualization (json, mermaid, svg)
- `GET /v1/projects/{owner}/{project}` — project detail
- `GET /v1/projects/{owner}` — list user's projects

**CLI:**
- `ozzy endpoint ls`
- `ozzy endpoint show <name>`
- `ozzy endpoint dag <name> [--format json|mermaid|svg]`

**Tests:** Endpoint listing after push, DAG rendering in multiple formats.

### Step 3.4: CLI init + transform scaffold

**CLI:**
- `ozzy init` — detect git repo, detect language/runtime, generate ozzy.toml with scaffolded sections
- `ozzy transform scaffold <name> [--lang python|r]` — generate transform file + print TOML to add

**Tests:** Init in git repo, init detects Python project, scaffold creates file.

### Step 3.5: Auth CLI commands

Wire up the existing auth server endpoints to the CLI:
- `ozzy auth login` — GitHub device flow (poll for token, save to `~/.config/ozzy/credentials.json`)
- `ozzy auth logout` — remove credentials
- `ozzy auth status` — show current user
- `ozzy auth token create <name> [--scope]`
- `ozzy auth token ls`
- `ozzy auth token revoke <id>`

**Tests:** Login flow (mocked), token CRUD.

---

## Phase 4: Execution

Local and remote execution of endpoint DAGs.

### Step 4.1: Environment building

New module: `crates/ozzy-server/src/environments/`

- Tier 1 (base + lockfile): generate Dockerfile, build, push to GHCR as `ghcr.io/ozzydb/envs/{env_hash}`
- Tier 2 (custom Dockerfile): fetch from git, build, push
- Tier 3 (pre-built): pull from user's registry, verify exists
- Async build: push returns immediately, first fetch blocks until ready
- Package caching: mount global pip/uv cache during builds
- `environment_images` table tracking

**Tests:** Tier 1 build (mock Docker), image hash computation, dedup (same lockfile = same image).

### Step 4.2: Runner generation

New module: `crates/ozzy-server/src/runners/`

- Python runner: generate from template, handles parquet/image/json/text I/O, collection support
- R runner: generate from template, parquet/csv I/O
- Command runner: template substitution (`${input.NAME}`, `${output}`), NO param substitution
- Init script: download inputs via presigned URLs, run transform, tar + upload output

**Tests:** Runner generation for each language, template substitution safety (no param injection).

### Step 4.3: Compute backend trait + Fly Machines

New module: `crates/ozzy-server/src/compute/`

- `ComputeBackend` trait: `run(ComputeRequest) -> ComputeResult`, `available_machines()`
- `FlyComputeBackend`:
  - Create Fly Machine via API
  - Poll for completion (wait?state=stopped)
  - Fetch logs on failure
  - Machine tier mapping (cpu-small through gpu-large)
  - auto_destroy, no restart, timeout enforcement
  - Orphan cleanup periodic job
- `DockerComputeBackend` (for `ozzy run` local execution):
  - `docker run` with --network none, determinism env vars, bind mounts
  - Same I/O contract as Fly

**Tests:** Mock Fly API, Docker backend integration test (requires Docker).

### Step 4.4: Server fetch endpoint (DAG execution)

**Server:**
- `GET /v1/fetch/{owner}/{project}/{endpoint}` — the big one
  1. Resolve project → ref → commit
  2. Load commit_state
  3. Find endpoint, check yanks
  4. Validate consumer params (min/max/enum)
  5. Resolve all data:/collection:/endpoint: references
  6. Compute materialized hash chain
  7. Check cache at each node
  8. Build execution plan (topological sort, environment grouping)
  9. For uncached nodes: generate presigned URLs, dispatch to Fly Machine
  10. Collect output, verify schema, cache result
  11. Stream final output

**Tests:** Full execution flow with mock compute, cache hit path, yanked endpoint returns 410, param validation.

### Step 4.5: CLI run + fetch

**CLI:**
- `ozzy run <endpoint> [--param key=value...] [--local-data name=path...] [-o file]`
  - Read ozzy.toml from local filesystem (not git)
  - Resolve data (local overrides or registry fetch)
  - Execute via Docker locally
- `ozzy fetch <owner/project/endpoint[@ref]> [--param key=value...] [-o file]`
  - Call server fetch API
  - Stream response to stdout or file

**Tests:** Local run with --local-data, fetch from server (mocked).

### Step 4.6: Cache management CLI

- `ozzy cache ls` — list cached items
- `ozzy cache size` — total cache size
- `ozzy cache clear [--older-than 30d]` — clear cache

**Tests:** Cache operations on local SQLite index.

---

## Phase 5: Frontend

Build all v2 frontend pages.

### Step 5.1: API client + types

Update `frontend/src/lib/api.ts` and `types.ts` for all v2 endpoints. Add typed fetch wrappers for data, collections, endpoints, projects, secrets.

### Step 5.2: Project overview page

`/{owner}/{project}` — summary cards (N atoms, N collections, N endpoints, N commits), recent activity, quick links.

### Step 5.3: Data browser

`/{owner}/{project}/data` — list atoms, upload modal (drag-and-drop + metadata form), atom detail view with schema, download button, yank status.

### Step 5.4: Collection browser

`/{owner}/{project}/collections` — list, tree view of members, version timeline, add/remove actions, flatten view.

### Step 5.5: Endpoint explorer

`/{owner}/{project}/endpoints/{name}` — description, param form, "Run" button, DAG visualization, verification badge, cache status.

### Step 5.6: Commit detail

`/{owner}/{project}/commits/{sha}` — git info, link to GitHub, ozzy.toml diff.

### Step 5.7: Secrets management

`/{owner}/{project}/settings/secrets` — list names, add/delete.

### Step 5.8: User profile update

`/{owner}` — user's projects, public activity.

---

## Phase 6: Python Client

### Step 6.1: Core client

- `ozzy.fetch(ref, **params)` — call server API, return polars DataFrame or bytes
- `ozzy.fetch_lazy(ref, **params)` — return LazyFrame
- `ozzy.inspect(ref)` — endpoint metadata without execution
- `ozzy.inspect_project(ref)` — project overview

### Step 6.2: Local execution

- `ozzy.run(endpoint, **params)` — subprocess to `ozzy run`, parse output
- `Project` context manager for local project operations

### Step 6.3: Data management

- `ozzy.upload(project, file, name=..., ...)` — upload data
- `ozzy.download(project, name)` — download data atom

**Tests:** Unit tests with mocked API, integration tests with real server.

---

## Phase 7: Deployment & Integration

### Step 7.1: Docker Compose update

Update production Docker Compose for v2:
- Fresh Postgres 17 (v2 schema, drop v1 DB)
- Caddy config unchanged
- Server container with new env vars (GitHub App credentials, Fly API token, secrets encryption key)
- R2 bucket configuration

### Step 7.2: End-to-end tests

Full E2E flow: init → data upload → write transform → push → fetch → verify cache hit → yank → verify yank error.

### Step 7.3: Deploy to production

- Drop v1 database
- Run v2 migrations
- Deploy new server
- Rebuild frontend
- Restart Caddy
- Smoke test

---

## Phase Dependencies

```
Phase 1 (Foundation)
  ├── Phase 2 (Data Plane) ← needs DB + models
  ├── Phase 3 (Git + Push) ← needs DB + toml_spec
  │     └── Phase 4 (Execution) ← needs push + environments
  ├── Phase 5 (Frontend) ← needs server APIs from 2, 3, 4
  └── Phase 6 (Python Client) ← needs server APIs from 2, 3, 4
Phase 7 (Deploy) ← needs everything
```

Phases 2 and 3 can be partially parallelized (data plane is independent of git integration). Phase 5 and 6 can start as soon as their backing APIs exist.
