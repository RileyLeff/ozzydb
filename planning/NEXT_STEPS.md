# OzzyDB Next Steps

Status as of 2026-02-06. Phases 1-3 (local workflow, S3 cache, registry server)
are implemented. This document covers what's left to ship a deployed, publicly
usable platform — "GitHub for scientific data."

See `ozzydb_soul.md` for the design principles that guide every decision below.

---

## Architecture

```
                          +-------------------+
                          |   Cloudflare R2   |
                          |  (single storage) |
                          +--------+----------+
                                   |
                    +--------------+--------------+
                    |                             |
              source content              materialized cache
          content/{h[0:2]}/...        materialized/{platform}/{hash}.parquet
          (parquet, .py, lockfiles)    (transform outputs, regenerable)
                    |                             |
                    +-------------+---------------+
                                  |
                  +---------------+---------------+
                  |         ozzy-server           |
                  | Axum API + Svelte 5 frontend  |
                  |     gVisor compute sandbox    |
                  +---------------+---------------+
                                  |
                          +-------+-------+
                          |  PostgreSQL   |
                          | (metadata,    |
                          |  users, orgs, |
                          |  refs, cache  |
                          |  index)       |
                          +---------------+
```

### Storage: R2-only

R2 is the single source of truth for all content. No NVMe tier, no local
filesystem state to manage.

**Layout:**
```
ozzy-content/
  content/{hash[0:2]}/{hash[2:4]}/{hash}.parquet   -- source data
  content/{hash[0:2]}/{hash[2:4]}/{hash}.py         -- transform source
  content/{hash[0:2]}/{hash[2:4]}/{hash}.lock       -- lockfiles
  materialized/{platform_hash}/{mat_hash}.parquet   -- cached outputs
```

Source content (data, transforms, lockfiles) is permanent — tied to commits,
never deleted unless a project is deleted. Materialized outputs are cache entries
that can be regenerated from source if evicted.

**Why R2-only:**
- One storage tier. No local/remote divergence problem.
- R2 has free egress (Cloudflare). Reads cost nothing.
- $0.015/GB/month for storage. 100GB of cached outputs = $1.50/month.
- No eviction daemon, no capacity management.
- MinIO in Docker Compose is a faithful test of R2 in production.

**Changes needed:**
- `ContentStorage::store()` — write to R2, local disk is optional write-through
- `Config` — `R2Config` required, `local_storage_path` becomes optional cache dir
- New `materialized/` key prefix for transform outputs (separate from source)
- Postgres `materialized_cache` table tracks: hash, endpoint, last_accessed,
  size_bytes, pinned (for optional future eviction)

### Compute: gVisor-sandboxed Docker containers

Server executes transforms on fetch. Each transform runs in a gVisor-sandboxed
container with **no network access** and strict resource limits.

**Network isolation is load-bearing for reproducibility.** Transforms run with
`--network=none` always. They cannot phone home, download data at runtime, or
depend on external APIs. If your transform needs external data, that data must
be declared as an input. This is not a security feature bolted on — it's a
design constraint that makes the hash guarantee meaningful. A transform that
can reach the internet is not a pure function.

**Execution flow:**
```
Client: GET /owner/project/endpoint@main  (or click "Download" in web UI)
  1. Compute materialized hash (server platform + inputs + transforms + params)
  2. Check R2: materialized/{platform}/{hash}.parquet
  3. HIT  -> stream from R2
  4. MISS -> execute pipeline:
     a. Pull source data + transforms from R2
     b. For each transform in topo order:
        - Look up (or build) Docker image for lockfile hash
        - docker run --runtime=runsc --network=none --memory=4g --cpus=2
          --read-only --tmpfs /tmp:size=1G
          -v /input:/in:ro -v /output:/out
          ozzy-env:{lockfile_hash} python /in/transform.py
        - Verify output, check for timeout (300s default)
     c. Store final output to R2: materialized/{platform}/{hash}.parquet
     d. Stream result to client
```

**Determinism enforcement:** Between `PYTHONHASHSEED=0`, `OMP_NUM_THREADS=1`,
`MKL_NUM_THREADS=1`, and `--network=none`, most sources of non-determinism are
eliminated. For stochastic algorithms (random forests, sampling), the correct
approach is a seed parameter: `params={"seed": 42}`. Different seed = different
cache key = different cache entry. At commit time, warn if we detect unseeded
randomness (e.g., `np.random` calls without a prior `np.random.seed`). No
`reproducible=false` flag, no propagation logic — enforce determinism, don't
build infrastructure around non-determinism.

**Why gVisor:**
- Same Dockerfiles, same `docker run` as plain Docker. Zero extra cost.
- Syscall interception in userspace — kernel exploits don't escape.
- numpy, polars, arrow, scipy all work.
- ptrace mode on any Linux box (no KVM). Works on any Hetzner VPS.

**Image management:**
- One Docker image per unique lockfile hash: `ozzy-env:{lockfile_hash}`
- Built on first use: `FROM python:3.12-slim` + `pip install` from lockfile
- Cached locally by Docker (Docker manages its own image storage/GC)

**Resource limits (configurable per-deployment):**
- Memory: 4GB default
- CPUs: 2 default
- Timeout: 300s default
- Disk (tmpfs): 1GB default
- Network: none (always)

### Cache eviction (if ever needed)

R2 storage is cheap enough that eviction is optional. If it becomes necessary:

- Postgres `materialized_cache` table tracks `last_accessed` per entry
- Background job evicts lowest-scoring entries above a storage threshold
- Score: `access_count * rebuild_time_ms / (size_bytes * age_hours)`
- Endpoints can be marked `pinned` (never evict, with per-project quota)
- Content-addressing handles invalidation — changed transforms produce new
  hashes, old entries age out naturally

---

## Concepts

### Projects

Owned by a user or organization. Contains data sources, transforms, endpoints,
commits, refs. Same as today — `owner/project` namespace where `owner` is
either a username or org slug.

### Organizations

Groups of users with shared ownership. An org can own projects and curations
just like a user can. Members have roles (owner, admin, member) that determine
what they can do within the org's namespace.

**Data model:**
```sql
CREATE TABLE organizations (
    id UUID PRIMARY KEY,
    slug VARCHAR(255) UNIQUE NOT NULL,
    display_name TEXT NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE org_members (
    org_id UUID REFERENCES organizations(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(20) NOT NULL CHECK (role IN ('owner', 'admin', 'member')),
    created_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (org_id, user_id)
);
```

Projects and curations reference an `owner_user_id` OR `owner_org_id` (one
must be non-null). The `owner/slug` namespace resolves against both users
and orgs.

**CLI:**
```
ozzy org create <slug> --name "VCR LTER"
ozzy org invite <slug> <username> --role member|admin
ozzy org rm-member <slug> <username>
ozzy org ls
ozzy org show <slug>
```

**API routes:**
```
POST   /api/v1/orgs                          { slug, display_name, description }
GET    /api/v1/orgs/{slug}
PUT    /api/v1/orgs/{slug}                   { display_name, description }
DELETE /api/v1/orgs/{slug}
POST   /api/v1/orgs/{slug}/members           { username, role }
DELETE /api/v1/orgs/{slug}/members/{username}
GET    /api/v1/orgs/{slug}/members
```

### Curations

A curated list of projects, maintained by a user or org. Like a Spotify playlist
for datasets — references projects, doesn't own them. A project can appear in
multiple curations. Public curations don't require permission from project owners.

**Use cases:**
- A researcher creates a curation bundling the datasets used in a paper. The
  curation URL goes in the data availability statement.
- An LTER site (as an org) maintains a living curation that grows as datasets
  are contributed by different researchers.
- A grad student creates a curation that remixes existing public datasets for a
  specific analysis.

**Data model:**
```sql
CREATE TABLE curations (
    id UUID PRIMARY KEY,
    slug VARCHAR(255) NOT NULL,
    owner_user_id UUID REFERENCES users(id),
    owner_org_id UUID REFERENCES organizations(id),
    name TEXT NOT NULL,
    description TEXT,
    visibility VARCHAR(20) DEFAULT 'public',
    created_at TIMESTAMPTZ DEFAULT NOW(),
    CHECK (
        (owner_user_id IS NOT NULL AND owner_org_id IS NULL) OR
        (owner_user_id IS NULL AND owner_org_id IS NOT NULL)
    ),
    UNIQUE(owner_user_id, slug),
    UNIQUE(owner_org_id, slug)
);

CREATE TABLE curation_maintainers (
    curation_id UUID REFERENCES curations(id) ON DELETE CASCADE,
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    role VARCHAR(20) CHECK (role IN ('owner', 'editor')),
    PRIMARY KEY (curation_id, user_id)
);

CREATE TABLE curation_entries (
    curation_id UUID REFERENCES curations(id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    added_by UUID REFERENCES users(id),
    note TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    added_at TIMESTAMPTZ DEFAULT NOW(),
    PRIMARY KEY (curation_id, project_id)
);
```

**CLI:**
```
ozzy curation create <slug> --name "VCR LTER Datasets"
ozzy curation add <slug> <owner/project> [--note "Sap flux data from 2019-2024"]
ozzy curation rm <slug> <owner/project>
ozzy curation ls
ozzy curation show <owner/slug>
```

**API routes:**
```
POST   /api/v1/curations                           { slug, name, description }
GET    /api/v1/curations/{owner}/{slug}
PUT    /api/v1/curations/{owner}/{slug}             { name, description }
DELETE /api/v1/curations/{owner}/{slug}
POST   /api/v1/curations/{owner}/{slug}/entries     { project: "owner/project", note }
DELETE /api/v1/curations/{owner}/{slug}/entries/{owner}/{project}
GET    /api/v1/curations/{owner}/{slug}/entries
POST   /api/v1/curations/{owner}/{slug}/maintainers { username, role }
DELETE /api/v1/curations/{owner}/{slug}/maintainers/{username}
```

### Deprecation and Yanking

Tags and endpoints can be marked deprecated or yanked. This communicates
lifecycle state to downstream consumers without breaking content-addressing.

**Deprecation** is a warning — the data still works, but consumers are nudged
to update:
```
ozzy tag deprecate v1.0 --successor v2.0 --reason "Bug in outlier detection"
ozzy endpoint deprecate old_cleaned --successor cleaned
```

**Yanking** is a hard block — the tag can't be fetched by name (you'd need the
raw commit hash). For when the data is actively wrong:
```
ozzy tag yank v1.0 --reason "Critical calibration error, values are wrong"
```

**Data model** (additions to existing `refs` table):
```sql
ALTER TABLE refs ADD COLUMN deprecated_at TIMESTAMPTZ;
ALTER TABLE refs ADD COLUMN deprecation_message TEXT;
ALTER TABLE refs ADD COLUMN successor_ref VARCHAR(255);
ALTER TABLE refs ADD COLUMN yanked BOOLEAN NOT NULL DEFAULT FALSE;
```

Endpoints get similar columns in the `endpoints` table for endpoint-level
deprecation (separate from ref-level).

**CLI behavior:**
- `ozzy fetch ...@deprecated-tag` prints warning, proceeds
- `ozzy fetch ...@yanked-tag` prints error, refuses (use `--force` or raw hash)
- `ozzy run` with deprecated dependencies prints warning summary
- `ozzy dep ls` shows deprecation/yank status
- Web UI shows a banner on deprecated/yanked endpoint pages

### Cross-Project Dependencies

When a project consumes another project's endpoint as an input, the dependency
should be explicit and pinned by default.

**In `ozzy.toml`:**
```toml
[dependencies]
cleaned_flux = {
  remote = "rileyleff/sap-flux/cleaned",
  ref = "v1.0",
  commit = "abc123...",    # recorded at add time
}
```

Dependencies are always pinned to a specific commit hash. The ref is a
human-readable label recorded for convenience, but `ozzy run` uses the commit
hash. To update a dependency, run `ozzy dep update` explicitly — no floating
dependencies, no silent changes between runs.

**CLI:**
```
ozzy dep add rileyleff/sap-flux/cleaned@v1.0    # records ref + commit hash
ozzy dep update cleaned_flux                      # re-resolves ref, updates hash
ozzy dep update cleaned_flux --ref v2.0          # switch to new ref
ozzy dep ls                                       # show deps, pin state, warnings
ozzy dep rm cleaned_flux
```

**Runtime behavior:**
- On `ozzy run`, check each dependency's deprecation/yank status
- If deprecated: warn with successor info
- If yanked: error, require `ozzy dep update` to a non-yanked ref

**API routes:**
```
GET  /api/v1/{owner}/{project}/{endpoint}@{ref}/status
  -> { deprecated, yanked, successor, deprecation_message }
```

---

## Web UI

### Tech

Svelte 5 SPA served by the same Axum server (static files + API). SvelteKit
for SSR and routing. Consumes the same REST API the CLI uses — no special
server-side rendering paths. If it's not in the API, it's not in the UI.

### Pages

**Homepage** (`/`)
Discovery surface. Featured curations, recent public projects, search bar.
Not a dashboard — a front door. Scientists land here from Google, from paper
links, from colleagues.

**User profile** (`/{username}`)
Projects, curations maintained by this user.

**Org profile** (`/{orgslug}`)
Projects, curations owned by the org. Member list. Same layout as user profile
with org-specific controls (invite, manage members).

**Project page** (`/{owner}/{project}`)
The GitHub repo equivalent. README at top. Then:
- **Endpoints** as the primary content (not raw files). Each shows: name,
  description, schema summary, row count, size, "Download" button.
- **DAG** tab — interactive mermaid rendering. Click a node to see schema.
- **Data sources** tab — raw inputs with schema.
- **Transforms** tab — code with syntax highlighting.
- **History** tab — commit log.

**Endpoint detail** (`/{owner}/{project}/{endpoint}`)
The download page. Where most visitors end up.
- Schema table: column names, types, descriptions, units.
- Data preview: first 20 rows rendered as a table.
- **Download button** — parquet by default. Dropdown option for CSV.
  CSV conversion happens server-side (read parquet from R2, stream as CSV).
- Code snippets: Python client, CLI, curl.
- Metadata: last materialized, size, commit hash.

**Curation page** (`/{owner}/~{slug}` or `/curations/{owner}/{slug}`)
- Description/README at top.
- Project list with: name, owner, short description, endpoint count, curator's
  note for each entry.
- Maintained by: list of maintainers.

**Search** (`/search?q=...`)
Full-text search across projects, endpoints, curations, users, orgs. Faceted
by type.

### The Download Flow

The critical UX path. A scientist follows a link from a paper:

1. Lands on endpoint page. Sees schema: "timestamp, sap_flux, species, site_id"
2. Sees preview table — first 20 rows. "This looks right."
3. Clicks **Download** (parquet by default, CSV from dropdown).
4. Server checks materialized cache. Hit -> stream. Miss -> execute -> cache ->
   stream. During execution, the user sees a progress indicator (SSE or
   WebSocket for real-time pipeline status).
5. Gets a parquet file. Or CSV if selected.

For the programmatic equivalent:
```python
import ozzydb
df = ozzydb.fetch("rileyleff/sap-flux/cleaned@v1.0")
```
Same pipeline, different interface.

### Design Language

Named after Ozzy, a black and white tuxedo cat with a pink nose and pink toes.
Polite, chatty, playful.

**Palette:**
- Primary: black and white, high contrast
- Accent: pink (CTAs, active states, links, interactive elements)
- Neutral: warm grays

**Typography:**
- Mix a softer/rounder display font (headings, the logo, empty states) with a
  clean sans-serif body font. The contrast between playful headings and
  professional body text captures Ozzy's personality without undermining
  seriousness.

**Organic details, used sparingly:**
- Section dividers or hero backgrounds use soft, wavy SVG paths where black
  meets white — evoking tuxedo fur patterns. Not everywhere. Maybe the homepage
  hero and the footer. Enough to be distinctive, not enough to feel whimsical.
- Cards and containers use standard border-radius. The organic shapes are
  accents, not the structural language.

**Voice:**
- Empty states are friendly: "No endpoints yet. Create one to get started."
- Error messages are helpful: "That endpoint doesn't exist. Did you mean
  `cleaned_flux`?"
- Loading states during compute: characterful but not cutesy.
- Small Ozzy cat mark in the logo. Not a mascot that appears everywhere.

**What it's NOT:**
- Not the cold blue of Snowflake/Databricks/BigQuery.
- Not whimsical or unserious. It's a serious data platform that happens to have
  personality.

---

## Implementation Plan

### 0. Remove dead code from Phase 2 [DONE]

The Phase 2 tiered cache (local L1 + S3 L2) is obsoleted by the server's R2
materialized cache. Remove the remote cache subsystem and its CLI commands.

**Delete:**
- `crates/ozzy-core/src/cache/backend.rs` — CacheBackend trait
- `crates/ozzy-core/src/cache/config.rs` — RemoteCacheConfig, TieredCacheConfig
- `crates/ozzy-core/src/cache/remote.rs` — RemoteCache (S3 L2)
- `crates/ozzy-core/src/cache/tiered.rs` — TieredCache (L1/L2 composition)

**Remove CLI commands:**
- `ozzy cache push` / `ozzy cache pull` / `ozzy cache sync` / `ozzy cache status`

**Keep:**
- Local SQLite cache for `ozzy run` (the L1 layer)
- `ozzy cache ls` / `ozzy cache size` / `ozzy cache clear`

**Remove from `ozzy.toml` schema:**
- `[cache.remote]` section and `[cache.remote.policy]`

**Also remove from `Cargo.toml` if no longer needed elsewhere:**
- `async-trait` (check if still used)

**Scope:** Net deletion. ~400 lines removed.

### 1. R2-only storage [DONE]

Flip `ContentStorage` from local-first to R2-primary.

**Files:**
- `crates/ozzy-server/src/storage/content.rs` — R2 writes first, local optional
- `crates/ozzy-server/src/config.rs` — `R2Config` required, drop `local_storage_path`
- `crates/ozzy-server/docker/docker-compose.prod.yml` — clean up env vars

**Scope:** ~100 lines changed.

### 2. Server-side transform execution [DONE]

Add `compute` module to `ozzy-server`.

**New files:**
- `crates/ozzy-server/src/compute/mod.rs`
- `crates/ozzy-server/src/compute/executor.rs` — Docker/gVisor execution
- `crates/ozzy-server/src/compute/image.rs` — image build from lockfile
- `crates/ozzy-server/src/compute/pipeline.rs` — DAG execution, cache checks

**Integration:**
- `fetch_endpoint` handler checks materialized cache, runs pipeline on miss
- Config: `COMPUTE_ENABLED`, `COMPUTE_MEMORY_LIMIT`, `COMPUTE_CPU_LIMIT`,
  `COMPUTE_TIMEOUT_SECONDS`
- Postgres: `materialized_cache` table
- All containers run with `--network=none` — no exceptions

**Scope:** ~500-800 lines.

### 3. End-to-end tests [DONE]

Automated tests against Docker Compose (postgres + minio + gvisor).

**Approach:**
- `tests/e2e/` directory, `just e2e` target
- Mock GitHub OAuth via test-only auth bypass (`X-Test-User` header)
- Skip if `docker compose` not available

**Key scenarios:**
- Push -> pull roundtrip
- Push deduplication
- Fetch endpoint -> server-side execution -> cached result
- Cache hit on second fetch
- Tag create -> pull by tag
- Auth token scoping
- Collaborator access control
- Private project visibility

### 4. Deploy to Hetzner [DONE]

Deployed to Hetzner CX22 (2 vCPU, 4GB RAM) at `46.225.111.110`.
- Docker + gVisor (runsc) installed
- PostgreSQL 17 + Caddy + ozzy-server in Docker Compose
- TLS via Caddy auto-provisioning (Let's Encrypt)
- API at `https://api.ozzydb.com`
- Local-only storage (no R2 yet)
- Registration restricted via `ALLOWED_LOGINS=rileyleff`
- Server-side compute enabled with gVisor runtime
- GitHub OAuth device flow working

**To update:** `cd /opt/ozzydb && git pull && cd crates/ozzy-server/docker && docker compose -f docker-compose.prod.yml --env-file .env.prod build server && docker compose -f docker-compose.prod.yml --env-file .env.prod up -d`

**Ongoing ops:** `pg_dump` -> R2 backups, `/health` monitoring, `docker logs`.

### 5. Web UI

SvelteKit app, served from the same domain. Consumes the REST API.

**Pages (in build order):**
1. Endpoint detail + download (the highest-value page)
2. Project page (endpoints list, README)
3. Homepage (search, featured curations)
4. Curation page
5. User / org profile
6. Search
7. Settings / auth

**CSV export:** Server-side parquet-to-CSV conversion for the download dropdown.
Arrow's CSV writer handles this — read parquet from R2, stream CSV rows.

**Compute progress:** SSE or WebSocket endpoint for real-time pipeline status
during server-side execution. The web UI shows a progress indicator; the CLI
can optionally show a progress bar.

**Scope:** This is the largest single work item. Build iteratively — endpoint
detail page first (the money page), then expand outward.

### 6. Curations

Data model, API routes, CLI commands.

**New files:**
- `crates/ozzy-server/migrations/003_curations.sql`
- `crates/ozzy-server/src/api/v1/curations.rs`
- `crates/ozzy-server/src/db/` — curation queries
- `crates/ozzy-cli/src/commands/curation.rs`

**Scope:** ~400 lines server, ~150 lines CLI.

### 7. Organizations + Collaborator CLI

Org model and collaborator management.

**Current state:** `project_collaborators` table exists (migration 002),
permissions enforced in `push_pull.rs`. Missing: org model, CLI commands,
and API routes for both.

**New files:**
- `crates/ozzy-server/migrations/004_organizations.sql`
- `crates/ozzy-server/src/api/v1/orgs.rs`
- `crates/ozzy-cli/src/commands/org.rs`
- `crates/ozzy-cli/src/commands/collab.rs`

**New CLI (collaborators):**
```
ozzy collab add <username> --permission read|write|admin
ozzy collab rm <username>
ozzy collab ls
```

**Scope:** ~400 lines server (orgs + collab API), ~250 lines CLI.

### 8. Deprecation, yanking, and dependencies

Lifecycle metadata and cross-project dependency management.

**Database changes:**
- Migration adding `deprecated_at`, `deprecation_message`, `successor_ref`,
  `yanked` columns to `refs` table
- Similar columns on `endpoints` table for endpoint-level deprecation
- New `project_dependencies` table linking projects to remote endpoints with
  pinned commit hashes

**New CLI commands:**
- `ozzy tag deprecate/yank` — lifecycle management
- `ozzy endpoint deprecate` — endpoint-level deprecation
- `ozzy dep add/update/ls/rm` — dependency management

**Server changes:**
- Deprecation/yank status included in fetch and resolve responses
- `/status` endpoint for checking lifecycle state
- Yank enforcement: yanked refs return 410 Gone unless `?force=true`

**Scope:** ~300 lines server, ~200 lines CLI.

### 9. Python client improvements

- `fetch()` for remote endpoints
- Better error messages when CLI not found
- Publish to PyPI (`ozzydb`)
- Later: PyO3 native bindings

### 10. LLM skills

A `.md` skill file documenting how to use the CLI and Python client. Written
for LLM consumption — structured, example-heavy, covering every command.
Comes last, after the interfaces stabilize.

### Git integration note

`ozzy init` should generate a `.gitignore` that excludes `data/*.parquet` and
`.ozzy/`. Transforms, `ozzy.toml`, and requirements files stay in git. Git
versions your development history (how did this code evolve?). OzzyDB versions
your pipeline state (what exact computation produced this output?). Both are
useful, neither replaces the other.

---

## Roadmap (post-launch)

### DOIs and Releases

Once the platform is deployed and working in production, add DOI minting via
DataCite. Immutable commit hashes already provide the citing guarantee described
in the soul doc — DOIs add discoverability and integration with academic
publishing infrastructure.

**Planned approach:**
- `ozzy release create v1.0 --title "..." --description "..."`
- A release is a named, immutable snapshot (commit hash + metadata)
- DataCite API integration mints a DOI pointing to the release

This is deferred until the core platform is running and integrated into real
workflows, but it's a first-class goal.

### Other deferred items

- **R/Julia/WASM runtimes** — runtime.rs extension; DAG/cache layers are
  language-agnostic
- **Federation** — cross-registry discovery; not needed until multiple registries
- **Cache eviction** — R2 is cheap; add when storage costs matter

---

## Remaining Review Items

From `codex_review_2.md`:

| Item | Status | Action |
|------|--------|--------|
| `org` visibility = "any authenticated" | Addressed | Org model planned (step 7) |
| Transform source-path fidelity | Known | Transforms round-trip as `{name}.py` |
| DAG SVG output | Not implemented | Low priority |
| Lockfile-based runtime envs | Done | `ensure_env_from_lockfile()` |
| `--registry` flag on fetch | Done | Wired through |

---

## Priority Order

0. **Remove Phase 2 tiered cache** — delete dead code first
1. **R2-only storage** — small change, big simplification
2. **Server-side compute** — core value prop of the hosted registry
3. **E2E tests** — confidence before deploying
4. **Deploy to Hetzner** — make it real
5. **Web UI** — the front door for scientists
6. **Curations** — the key differentiator from "just another data store"
7. **Orgs + Collaborator CLI** — enable team usage
8. **Deprecation, yanking, and dependencies** — lifecycle management
9. **Python client** — better DX
10. **LLM skills** — last, after interfaces stabilize
