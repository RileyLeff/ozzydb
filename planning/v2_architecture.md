# OzzyDB v2 Architecture

**OzzyDB is a switchboard. Git owns code. Container registries own environments. OzzyDB owns data, wires everything together, orchestrates compute, and caches results.**

This document captures the design decisions for OzzyDB v2, derived from the v1 implementation, the soul document, and a series of design conversations. It is intended to be concrete enough to code against.

---

## Table of Contents

1. [Soul (unchanged)](#1-soul-unchanged)
2. [Architecture overview](#2-architecture-overview)
3. [Data plane](#3-data-plane)
4. [Compute plane (ozzy.toml)](#4-compute-plane-ozzytoml)
5. [Git integration](#5-git-integration)
6. [Execution model](#6-execution-model)
7. [Verification and trust](#7-verification-and-trust)
8. [Compute infrastructure](#8-compute-infrastructure)
9. [What survives from v1](#9-what-survives-from-v1)
10. [What we're dropping and why](#10-what-were-dropping-and-why)
11. [Open items / deferred](#11-open-items--deferred)

---

## 1. Soul (unchanged)

The eleven principles from `ozzydb_soul.md` are load-bearing. They survived v1 intact and carry forward to v2 without modification:

1. **Kolmogorov's revenge** — The shortest description of a derived dataset is usually the program that produces it. Store the function, materialize on demand, cache the result.
2. **The hash is the truth** — `blake3(inputs + transform + params + deps + platform)` is the identity. Immutable, verifiable, unforgeable.
3. **Data is a function call, not a file** — Endpoints are pure functions with parameters, history, and versions. They're alive, not frozen corpses.
4. **Write normal code** — No DSL, no drag-and-drop GUI. Write a Python function, use polars, numpy, scipy. Your code runs outside OzzyDB just fine.
5. **The schema is the code** — The interface declaration and the implementation are tightly coupled. They can't drift apart.
6. **Sunlight on the black box** — Every step is code. Every parameter is recorded. Every intermediate result is inspectable. You can't hide what you did.
7. **Shared methodology, not just shared memoization** — When Lab B uses Lab A's transform, that's a verifiable methodological statement, not just a cache hit.
8. **Science is a DAG** — Transforms chain. Endpoints reference other endpoints. The computation graph is explicit and navigable.
9. **Cite the exact thing** — A DOI points to exactly what was used: the dataset, cleaned this way, calibrated with these constants, at this commit.
10. **Scientists shouldn't have to build infrastructure** — OzzyDB is the shared platform that eliminates duplicated data-hosting effort.
11. **Equally accessible to humans and LLMs** — Same API, same auth, same guarantees. CLI and Python client are first-class, not afterthoughts.

If you had to rebuild OzzyDB on a completely different stack, you'd keep these eleven things. Everything else is scaffolding.

---

## 2. Architecture overview

### The switchboard model

v1 was a monolith: OzzyDB stored source code, stored lockfiles, stored data, built environments from scratch on every execution, and ran transforms. That's five systems duct-taped together.

v2 is a switchboard:

```
Git (GitHub, GitLab)          → source of truth for code
Container registries (GHCR)   → source of truth for environments
OzzyDB                        → source of truth for data
                              → wires code + environments + data together
                              → orchestrates compute
                              → caches results
                              → serves endpoints
```

OzzyDB has two halves:

**Data plane (imperative):** You upload data, create collections, append, yank. This state lives in OzzyDB. Managed via CLI, GUI, or API.

**Compute plane (declarative):** Your git repo contains transforms, environments, and endpoint definitions in `ozzy.toml`. The TOML says "given data X, do Y." It doesn't claim to own X.

The TOML never asserts that data exists. It references data by name. If the data doesn't exist in OzzyDB, the endpoint fails at resolution time with a clear error. No state sync problem.

### What lives where

| Artifact | Lives in | Why |
|----------|----------|-----|
| Raw data (parquet, images, PDFs, etc.) | OzzyDB (R2/storage) | Too large for git, needs content addressing |
| Transform source code | Git | Already versioned, reviewable, forkable |
| Lockfiles, pyproject.toml, etc. | Git | Part of the codebase |
| ozzy.toml | Git | Declarative config, versioned with code |
| Built environment images | Container registry (GHCR) | Cached, reusable, immutable |
| Cached transform outputs | OzzyDB (R2/storage) | Content-addressed, deduplicated |
| Collection membership | OzzyDB (Postgres) | Mutable state, versioned |
| Metadata (descriptions, tags) | OzzyDB (Postgres) | Mutable annotations |
| Secrets (API keys) | OzzyDB (encrypted) | Per-project, injected at runtime |

---

## 3. Data plane

### Terminology note

**"Data atom"** is the internal/architectural term for a single immutable blob. In user-facing contexts (CLI output, error messages, docs), prefer **"dataset"** — shorter, familiar, no jargon. The CLI command is `ozzy data`, not `ozzy atom`. In the database schema and API paths the table/resource is called `data_atoms` for precision (distinguishing from collections). This document uses "data atom" because it's the architecture doc.

### 3.1 Data atoms

A **data atom** (user-facing: **dataset**) is the fundamental unit: a single, immutable, content-addressed blob.

**Properties:**
- **Hash:** `blake3(raw bytes)` — its identity
- **Content type:** Inferred from file extension or declared explicitly (e.g., `application/vnd.apache.parquet`, `image/tiff`, `application/pdf`)
- **Schema:** Optional. Arrow schema for tabular data, dimensions/dtype for images, etc.
- **Name:** Human-readable identifier within a project
- **Metadata:** Description, tags, license, etc. (mutable — see 3.3)

An atom is the unit of:
- **Storage** — one blob in object storage
- **Deduplication** — same bytes = same hash = stored once
- **Fetching** — individually addressable
- **Caching** — individually cacheable

### 3.2 Collections

A **collection** is a named, versioned, untyped set of references. It's a semantic grouping — nothing more.

**Members can be:**
- Data atoms — `data:name`
- Endpoint outputs — `endpoint:name` or `endpoint:owner/project/name`
- Other collections — `collection:name`

**Properties:**
- **No type constraints.** A collection can contain parquets, TIFFs, CSVs, PDFs, and sub-collections. Type compatibility is checked at the transform boundary, not the collection boundary.
- **Content-addressed:** `hash = blake3(sorted member reference hashes)`, resolved recursively for sub-collections.
- **Versioned:** Adding or removing members creates a new version (new hash).
- **Multi-membership:** An atom, endpoint output, or collection can appear in multiple collections.
- **No circular references.** Validated at `collection add` time via DFS with a visited set. When adding `collection:B` to collection A, the server walks B's membership graph recursively. If A appears anywhere in B's transitive membership, the add is rejected with a clear error: `"Circular reference: adding 'B' to 'A' would create a cycle (A → ... → B → A)."` Same check runs when adding `collection:A` to B.
- **Nested:** Collections can contain other collections, giving JSON-like hierarchical organization.

**Why untyped:** Real scientific data is messy. Five undergrads produce five years of shrub survey data in five different formats. That's one collection: "raw shrub data." The collection groups them semantically. A transform that needs them in a canonical format will validate types at its input boundary.

**CLI operations:**

```bash
ozzy collection create <name>
ozzy collection add <name> <ref...>       # data:x, endpoint:y, collection:z
ozzy collection rm <name> <ref...>        # remove members (new version)
ozzy collection ls                        # list all collections
ozzy collection ls <name>                 # list members (tree view)
ozzy collection log <name>               # version history
ozzy collection flatten <name>           # show all leaf-level atoms
```

**Example: the shrub survey**

```bash
# Upload raw data (various formats from various undergrads)
ozzy data upload shrub_2020.pdf
ozzy data upload shrub_2021.csv
ozzy data upload shrub_2022.parquet
ozzy data upload shrub_2023.json
ozzy data upload shrub_2024.parquet

# Group semantically
ozzy collection create raw-shrub-data
ozzy collection add raw-shrub-data shrub_2020_pdf shrub_2021_csv \
  shrub_2022_parquet shrub_2023_json shrub_2024_parquet

# After defining adapter endpoints in ozzy.toml (see section 4),
# collect the canonical outputs:
ozzy collection create canonical-shrub-data
ozzy collection add canonical-shrub-data \
  endpoint:canonical-2020 endpoint:canonical-2021 \
  endpoint:canonical-2022 endpoint:canonical-2023 \
  endpoint:canonical-2024

# Hierarchical grouping
ozzy collection create shrub-data
ozzy collection add shrub-data \
  collection:raw-shrub-data collection:canonical-shrub-data
```

### 3.3 Metadata

Data is immutable. Metadata is mutable.

Metadata includes: description, tags, license, schema annotations, units, constraints. It lives in an **append-only log** in Postgres. Latest entry wins for display. History is viewable for audit.

Metadata does NOT affect the hash. It's annotation, not identity. If you fuck up the description, just update it. The data blob doesn't change.

```bash
ozzy data describe raw_readings --set-description "Raw sap flux, Jan-Mar 2024"
ozzy data describe raw_readings --set-description "Raw sap flux, Jan-Mar 2024 (corrected timezone labels)"
ozzy data describe raw_readings --history
# 2024-03-15 14:30: "Raw sap flux, Jan-Mar 2024 (corrected timezone labels)"
# 2024-03-01 09:00: "Raw sap flux, Jan-Mar 2024"
```

### 3.4 Uploading data

**CLI:**

```bash
# Upload a single file (name defaults to filename stem)
ozzy data upload readings.parquet

# Upload with explicit name and metadata
ozzy data upload readings.parquet --name raw_readings \
  --description "Raw sap flux sensor readings, VCR LTER 2024"

# Upload with metadata sidecar
ozzy data upload readings.parquet --meta readings.ozzy.toml

# Bulk upload
ozzy data upload data/*.parquet

# Upload and add to collection in one shot
ozzy data upload new_batch.parquet --collection all_readings
```

**Metadata sidecar** (optional, consumed at upload time):

```toml
# readings.ozzy.toml
name = "raw_readings"
description = "Raw sap flux sensor readings, battery voltage unfiltered"
content_type = "parquet"
tags = ["raw", "sap-flux", "vcr-lter"]
license = "CC-BY-4.0"

[schema]
columns = [
  { name = "timestamp", type = "timestamp[us]" },
  { name = "flux", type = "float64" },
  { name = "battery_v", type = "float64" },
]
```

**GUI:** Drag and drop onto the project page. Form fields for name, description, tags. Schema auto-detected from parquet metadata and shown for confirmation.

**HTTP API:** For programmatic/automated uploads (e.g., sensor data cron jobs):

```
POST /v1/data/upload
  Authorization: Bearer $TOKEN
  file: <multipart>
  name: "weather_hourly_20240301_14"
  collection: "weather_readings"     # optional: upload + add in one call
```

### 3.5 Yanking

Yank doesn't delete. It marks data or endpoints as retracted with a reason. The hash still exists. The DAG still shows it. But fetching returns a hard error.

```bash
ozzy data yank readings_jan --reason "Sensor miscalibration. Use readings_jan_v2."
ozzy endpoint yank corrected_readings@v1 --reason "Based on yanked input data."
```

```
$ ozzy fetch rileyleff/sapflux/corrected_readings@v1
Error: This endpoint has been yanked.
Reason: "Based on yanked input data."
```

Downstream endpoints that depend on yanked data automatically fail with a clear message pointing to the yanked dependency. This is a hard error, not something transforms handle gracefully. Yanked means "this is wrong, stop using it."

---

## 4. Compute plane (ozzy.toml)

The `ozzy.toml` file lives in the git repo. It declares environments, transforms, and endpoints. It does NOT declare data — data is managed imperatively in OzzyDB.

### 4.1 Full annotated spec

```toml
# ================================================================
# Project
# ================================================================
[project]
name = "sapflux-analysis"
owner = "rileyleff"                     # required for push
description = "Sap flux processing for VCR LTER"

# ================================================================
# Git (auto-detected from .git remote, can override)
# ================================================================
[git]
provider = "github"                     # "github" | "gitlab"
repo = "rileyleff/sapflux-analysis"     # inferred from git remote

# ================================================================
# Remote registry
# ================================================================
[remote]
url = "https://api.ozzydb.com"

# ================================================================
# Environments
#
# All explicit. No defaults. Reproducibility is paramount.
# Three tiers:
#   1. Base image + lockfile (most users)
#   2. Custom Dockerfile in repo (power users)
#   3. Pre-built image (CI-integrated, max control)
# ================================================================

# Tier 1: OzzyDB base image + user's lockfile
[environments.scipy-stack]
base = "ozzydb/python:3.12"             # OzzyDB-maintained base image
lockfile = "uv.lock"                    # installed on top of base

# Tier 2: Custom Dockerfile from the repo
[environments.geo]
dockerfile = "envs/geo.Dockerfile"

# Tier 3: Pre-built image from a container registry
[environments.legacy]
image = "ghcr.io/rileyleff/legacy-fortran:v2.1"

# OzzyDB-maintained base images:
#   ozzydb/python:3.11, ozzydb/python:3.12, ozzydb/python:3.13
#   ozzydb/r:4.3, ozzydb/r:4.4
#   ozzydb/julia:1.10, ozzydb/julia:1.11
#   ozzydb/base:latest (minimal Debian for raw commands)

# ================================================================
# Transforms
#
# A transform is: environment + source/command + interface.
# No decorators. The function is just a function.
# ================================================================

# --- Function-based transform (Python) ---
[transforms.quality_control]
source = "transforms/qc.py:quality_control"
environment = "scipy-stack"
description = "Filter readings by battery voltage threshold"
inputs.readings = "parquet"
inputs.metadata = "parquet"
output = "parquet"
params.threshold = { type = "float", description = "Minimum battery voltage" }
params.method = { type = "string", enum = ["voltage", "range"] }

# Optional: declare output schema for validation and metadata propagation
[transforms.quality_control.output_schema]
columns = [
  { name = "timestamp", type = "timestamp[us]" },
  { name = "flux", type = "float64", constraints = { min = 0.0 }, unit = "g/m2/s" },
  { name = "qc_flag", type = "utf8" },
]

# --- Function-based transform operating on a collection ---
[transforms.aggregate_canonical]
source = "transforms/aggregate.py:aggregate_canonical"
environment = "scipy-stack"
inputs.data = "collection<parquet>"         # explicitly takes a collection
output = "parquet"                          # collection in, single item out (reduce)

# --- Function-based transform producing a collection ---
[transforms.clean_collection]
source = "transforms/clean.py:clean_readings"
environment = "scipy-stack"
inputs.readings = "collection<parquet>"     # takes a collection
output = "collection<parquet>"              # produces a collection
params.threshold = { type = "float" }

# --- Command-based transform (any language, any tool) ---
# Params accessed via env vars ($OZZY_PARAM_*), NOT template substitution.
[transforms.reproject]
command = "ogr2ogr -f GeoJSON -t_srs EPSG:$OZZY_PARAM_epsg ${output} ${input.boundaries}"
environment = "geo"
inputs.boundaries = "application/geo+json"
output = "application/geo+json"
params.epsg = { type = "int" }

# --- Network-enabled transform (for LLM APIs, etc.) ---
[transforms.extract_from_pdf]
source = "transforms/extract.py:extract_from_pdf"
environment = "ml-stack"
network = true                              # opt-in network access
secrets = ["GEMINI_API_KEY"]                # injected as env vars at runtime
inputs.pdf = "application/pdf"
output = "parquet"
params.model = { type = "string", default = "gemini-2.5-pro" }

# --- R transform ---
[transforms.spatial_join]
source = "scripts/spatial.R:join_sites"
environment = "r-env"
inputs.readings = "parquet"
inputs.boundaries = "application/geo+json"
output = "parquet"

# ================================================================
# Endpoints
#
# Endpoints are named, parameterized DAGs. They are what consumers
# fetch(). Each endpoint exposes parameters with defaults, declares
# nodes (transform invocations) and edges (data flow).
#
# Unified DAG syntax. Linear pipelines are just single-path DAGs.
# ================================================================

[endpoints.corrected_readings]
description = "QC'd and calibrated sap flux data"

# Parameters exposed to consumers.
# `binds` routes the param to the correct node.param.
# `min`/`max`/`enum` validated at the API boundary before execution.
[endpoints.corrected_readings.params]
qc_threshold = { type = "float", default = 11.5,
                 binds = "qc.threshold",
                 min = 0.0, max = 20.0,
                 description = "Battery voltage cutoff" }
cal_method   = { type = "string", default = "leff_2024",
                 binds = "cal.method",
                 enum = ["leff_2024", "smith_2023"] }

# Nodes: named transform invocations.
# Hardcoded params go here. Exposed params are injected via bindings above.
[endpoints.corrected_readings.nodes]
qc  = { transform = "quality_control", params = { method = "voltage" } }
cal = { transform = "apply_calibration" }

# Edges: explicit data flow. "from" -> "to" (node.input_name).
# Sources: data:<name>, collection:<name>, <node_name>, endpoint:<ref>
edges = [
  { from = "data:raw_readings",  to = "qc.readings" },
  { from = "data:site_metadata", to = "qc.metadata" },
  { from = "qc",                 to = "cal.data" },
]

# --- Endpoint with machine config ---
[endpoints.heavy_analysis]
description = "GPU-accelerated model training"

[endpoints.heavy_analysis.nodes]
train   = { transform = "train_model", machine = "gpu-large" }
process = { transform = "process_results" }
# `process` gets default machine (cpu-small)

edges = [
  { from = "collection:training_data", to = "train.data" },
  { from = "train",                    to = "process.model_output" },
]

# --- Endpoint referencing cross-project data ---
[endpoints.full_analysis]
description = "Analysis using shared calibration data"

[endpoints.full_analysis.nodes]
qc  = { transform = "quality_control" }
cal = { transform = "apply_calibration" }

edges = [
  { from = "data:raw_readings",                             to = "qc.readings" },
  { from = "data:site_metadata",                            to = "qc.metadata" },
  { from = "qc",                                            to = "cal.data" },
  { from = "endpoint:vcr-lter/shared/calibration_constants@v1.0", to = "cal.constants" },
]
```

### 4.2 Naming rules

All names (data, collections, transforms, endpoints, params) must match `[a-zA-Z0-9_-]`. No commas, parentheses, dots, colons, slashes, or whitespace. These characters are reserved for the addressing scheme.

### 4.3 Endpoint parameter syntax

Consumers can pass parameters using function-call syntax or kwargs:

```bash
# CLI
ozzy fetch rileyleff/sapflux/corrected_readings(qc_threshold=50)
ozzy fetch rileyleff/sapflux/corrected_readings --param qc_threshold=50
```

```python
# Python client
ozzy.fetch("rileyleff/sapflux/corrected_readings(qc_threshold=50)")
ozzy.fetch("rileyleff/sapflux/corrected_readings", qc_threshold=50)
```

Parameters are validated against `min`/`max`/`enum` at the API boundary before any execution. Out-of-range values return an error immediately:

```
Error: Parameter 'qc_threshold' out of range.
  Got: 100.0
  Allowed: [0.0, 20.0]
```

Each unique parameter combination produces a different materialized hash and cache entry.

### 4.4 Edge syntax

Edges are an array of `{from, to}` objects defining data flow through the DAG.

**`to`** is always `node_name.input_name` — which node and which input slot receives the data.

**`from`** is always a source. Prefixes indicate the source type:

| Prefix | Meaning | Example |
|--------|---------|---------|
| `data:` | A data atom in OzzyDB | `data:raw_readings` |
| `collection:` | A collection in OzzyDB | `collection:all_readings` |
| `endpoint:` | Another endpoint's output (pinned) | `endpoint:vcr-lter/shared/constants@v1.0` |
| *(no prefix)* | A node in this DAG | `qc` (output of the qc node) |

**Cross-project endpoint pinning:**

`endpoint:` references to other projects MUST be pinned to a specific commit SHA or tag. Unpinned cross-project references are rejected at `ozzy push` validation time.

```toml
# Good: pinned to a tag
{ from = "endpoint:vcr-lter/shared/calibration_constants@v1.0", to = "cal.constants" }

# Good: pinned to a commit SHA
{ from = "endpoint:vcr-lter/shared/calibration_constants@a1b2c3d", to = "cal.constants" }

# Bad: unpinned cross-project reference (rejected at push time)
{ from = "endpoint:vcr-lter/shared/calibration_constants", to = "cal.constants" }
```

Same-project `endpoint:` references (no `/` in the ref) are resolved against the current commit and don't need explicit pinning.

Why: Without pinning, builds are not reproducible. If Lab A pushes a new version of their calibration constants, Lab B's endpoint silently changes behavior. Pinning makes cross-project dependencies explicit and immutable. To adopt a new upstream version, you update the pin in `ozzy.toml` and commit — producing a visible, reviewable change in git.

**Validation rules:**
- Every node input must have exactly one incoming edge
- No cycles
- Source content types must be compatible with the transform's declared input types
- All `data:` and `collection:` references must exist in OzzyDB at execution time
- All cross-project `endpoint:` references must be pinned to a commit SHA or tag

**Example reading:**

```toml
edges = [
  { from = "data:raw_readings",  to = "qc.readings" },
  # "The 'readings' input of node 'qc' comes from the data atom 'raw_readings' in OzzyDB."

  { from = "qc",                 to = "cal.data" },
  # "The 'data' input of node 'cal' comes from the output of node 'qc'."
]
```

### 4.5 Transform vs Endpoint: the distinction

**Transform** = a reusable function definition. "Here is a tool called `quality_control`. It takes a parquet and some params, and produces a parquet." It's the **verb**.

**Endpoint** = a named, fetchable result. "Take `raw_readings`, run it through `quality_control` with threshold 11.5, then through `apply_calibration`." It's the **sentence**.

Why they're separate:
- Same transform, different endpoints (different params, different data)
- Endpoint is the unit of consumption — what consumers `fetch()`
- Endpoint is the unit of citation — DOI points here
- Transform is the unit of sharing — "we used the same QC procedure as Lab A"

---

## 5. Git integration

### 5.1 Provider abstraction

OzzyDB uses a provider-agnostic trait for git operations. GitHub is the first implementation. GitLab and others can be added later.

```rust
trait GitProvider: Send + Sync {
    /// Fetch a tarball of the repo at a specific commit
    async fn fetch_archive(&self, repo: &str, commit_sha: &str) -> Result<Vec<u8>>;

    /// Fetch a single file at a specific commit
    async fn get_file(&self, repo: &str, commit_sha: &str, path: &str) -> Result<Vec<u8>>;

    /// Resolve a ref (branch, tag) to a commit SHA
    async fn resolve_ref(&self, repo: &str, ref_name: &str) -> Result<String>;
}
```

### 5.2 GitHub App

Auth for repo access uses a **GitHub App**. The user installs the OzzyDB GitHub App on their repos. The app receives per-installation tokens that can fetch repo content. No deploy keys, no PATs, no user token forwarding.

For private repos, the GitHub App installation grants access. For public repos, no installation needed (public API).

### 5.3 Push flow

```bash
$ ozzy push
```

1. CLI reads `git rev-parse HEAD` to get the current commit SHA
2. CLI reads `ozzy.toml` and validates it
3. CLI sends to the registry: `{provider: "github", repo: "user/repo", commit_sha: "abc123"}`
4. Registry calls the git provider API to fetch `ozzy.toml` at that commit
5. Registry parses `ozzy.toml`, validates transform sources exist, records the commit
6. Registry fetches and caches the source tarball (for execution)
7. Registry builds or locates environment images (see section 6). **Environment builds are asynchronous** — push returns immediately with a `"status": "building"` for any environments that need to be built. The first `ozzy fetch` after a lockfile change will block until the environment image is ready. This keeps push fast (seconds) while front-loading the build work.

The canonical source is always git. The cached tarball is for performance. If the cache is evicted, the registry re-fetches from the git provider.

**Dirty state and local dev:** `ozzy push` requires a clean git state (all changes committed). It reads the HEAD commit SHA and registers that exact snapshot. For iterating on transforms locally without committing, use `ozzy run` — see implementation details, section 6.

### 5.4 Source caching

When `ozzy push` registers a commit, the server fetches and caches the repo tarball at that commit SHA. This is keyed by `{provider, repo, commit_sha}` and is immutable (a commit SHA always refers to the same content). The cache can be evicted under memory pressure and re-fetched.

---

## 6. Execution model

### 6.1 Container I/O contract

Every transform — function-based or command-based — runs inside a container with a standardized filesystem layout:

```
/workspace/
├── inputs/                    # mounted read-only
│   ├── readings.parquet       # named after declared inputs
│   └── metadata.parquet
├── output/                    # transform writes results here
├── params.json                # transform parameters as JSON
└── source/                    # transform source code from git (read-only)
    └── transforms/
        └── qc.py
```

**Environment variables set by OzzyDB:**

```
OZZY_PARAMS = '{"threshold": 11.5, "method": "voltage"}'
OZZY_PARAM_threshold = '11.5'
OZZY_PARAM_method = 'voltage'
OZZY_INPUT_readings = '/workspace/inputs/readings.parquet'
OZZY_INPUT_metadata = '/workspace/inputs/metadata.parquet'
OZZY_OUTPUT = '/workspace/output/'
PYTHONHASHSEED = '0'
OMP_NUM_THREADS = '1'
MKL_NUM_THREADS = '1'
OPENBLAS_NUM_THREADS = '1'
NUMEXPR_NUM_THREADS = '1'
```

**Hard constraints:**
- **No network access by default.** The container has no outbound connectivity. All inputs are mounted as files. All outputs are written to files. Enforced at the container runtime level.
- **Network opt-in:** If `network = true`, the container gets outbound access. This caps verification at Tier 2 (hollow badge).
- **Secrets:** If `secrets` is declared, the listed secrets are injected as environment variables. Secrets are NOT part of the hash.
- **Determinism:** `PYTHONHASHSEED=0`, `OMP_NUM_THREADS=1`, etc. are always enforced.

### 6.2 Function-based transforms

For function-based transforms (`source = "file:function"`), OzzyDB generates a language-specific runner script that bridges the container I/O contract to a function call.

**Python runner** (generated by OzzyDB, user never writes this):

```python
import sys, os, json
sys.path.insert(0, '/workspace/source')

import polars as pl
from transforms.qc import quality_control

# Load inputs from mounted files
inputs = {
    "readings": pl.read_parquet("/workspace/inputs/readings.parquet"),
    "metadata": pl.read_parquet("/workspace/inputs/metadata.parquet"),
}

# Load params from environment
params = json.loads(os.environ["OZZY_PARAMS"])

# Call the user's function
result = quality_control(inputs, params)

# Handle LazyFrame
if hasattr(result, 'collect'):
    result = result.collect()

# Write output
result.write_parquet("/workspace/output/result.parquet")
```

**R runner:**

```r
library(jsonlite)
library(arrow)

inputs <- list(
  readings = read_parquet("/workspace/inputs/readings.parquet")
)
params <- fromJSON(Sys.getenv("OZZY_PARAMS"))

source("/workspace/source/scripts/analysis.R")
result <- run_analysis(inputs, params)

write_parquet(result, "/workspace/output/result.parquet")
```

**The user's function knows nothing about OzzyDB:**

```python
# transforms/qc.py
import polars as pl

def quality_control(inputs, params):
    readings = inputs["readings"]
    return readings.filter(pl.col("battery_v") > params["threshold"])
```

No imports from OzzyDB. No decorators. Testable in complete isolation:

```python
df = pl.read_parquet("test_data.parquet")
result = quality_control({"readings": df}, {"threshold": 11.5})
assert result.height > 0
```

### 6.3 Command-based transforms

For command-based transforms (`command = "..."`), OzzyDB substitutes template variables and runs the command inside the container.

Template variables (system-controlled only):
- `${input.NAME}` — path to the named input file
- `${output}` — path to the output directory/file

Parameters are accessed via environment variables, NOT template substitution. This prevents shell injection from user-supplied param values:
- `$OZZY_PARAM_epsg` — individual param env var
- `$OZZY_PARAMS` — full JSON blob
- `/workspace/params.json` — params file

```toml
[transforms.reproject]
command = "ogr2ogr -f GeoJSON -t_srs EPSG:$OZZY_PARAM_epsg ${output} ${input.boundaries}"
environment = "geo"
inputs.boundaries = "application/geo+json"
output = "application/geo+json"
params.epsg = { type = "int" }
```

After substitution with `params = { epsg = 4326 }`:

```bash
ogr2ogr -f GeoJSON -t_srs EPSG:4326 /workspace/output/result.geojson /workspace/inputs/boundaries.geojson
```

Why only `${input.*}` and `${output}` are template-substituted: These are system-controlled paths (`/workspace/inputs/...`, `/workspace/output/...`) that cannot be influenced by the consumer. Param values come from users and could contain shell metacharacters. By routing params through env vars only, the shell never interpolates untrusted content during command parsing — the variable is expanded by the shell as a single token, not parsed as shell syntax.

The command can also read `$OZZY_PARAMS` or `/workspace/params.json` for complex cases. Same filesystem layout, same env vars, same no-network constraint.

### 6.4 Collection handling in transforms

Transforms handle collections explicitly. There is no inferred map mode or implicit iteration.

**A transform that processes a collection iterates internally:**

```python
# transforms/clean.py
import polars as pl

def clean_readings(inputs, params):
    """Process each reading in the collection."""
    readings = inputs["readings"]  # list of DataFrames
    results = []
    for df in readings:
        cleaned = df.filter(pl.col("battery_v") > params["threshold"])
        results.append(cleaned)
    return results  # list → becomes a collection
```

```toml
[transforms.clean_readings]
source = "transforms/clean.py:clean_readings"
environment = "scipy-stack"
inputs.readings = "collection<parquet>"
output = "collection<parquet>"
params.threshold = { type = "float" }
```

**A reduce (collection in, single item out):**

```python
def monthly_means(inputs, params):
    all_dfs = inputs["readings"]  # list of DataFrames
    combined = pl.concat(all_dfs)
    return combined.group_by(pl.col("month")).agg(pl.col("flux").mean())
```

```toml
[transforms.monthly_means]
inputs.readings = "collection<parquet>"
output = "parquet"
```

**Type checking at the transform boundary:**

When an endpoint wires a collection to a typed input like `collection<parquet>`, OzzyDB resolves all leaf-level members (recursively through sub-collections and endpoint outputs) and validates their content types match. If any leaf doesn't match, execution fails with a clear error before the transform runs.

If the transform declares `inputs.data = "collection"` (no type parameter), it accepts any collection regardless of member types. The function handles type dispatch internally.

### 6.5 Pseudorandom processes

For transforms with stochastic behavior, the random seed MUST be a declared parameter:

```toml
params.seed = { type = "int" }
```

Without a fixed seed, the output is non-deterministic and caching is meaningless. OzzyDB should warn if a transform doesn't declare a seed param but produces different output hashes on repeated runs with the same inputs.

### 6.6 Environment building

**Tier 1 (base + lockfile):** OzzyDB generates a Dockerfile internally:

```dockerfile
FROM ozzydb/python:3.12
COPY uv.lock pyproject.toml ./
RUN uv sync --frozen
```

Built server-side and cached by `blake3(base_image_digest + lockfile_hash)`. The user never writes a Dockerfile.

**Tier 2 (custom Dockerfile):** OzzyDB fetches the Dockerfile from git, builds it, and caches by `blake3(dockerfile_hash + build_context_hash)`.

**Tier 3 (pre-built image):** OzzyDB pulls from the registry. No build.

**Package caching:** The server maintains global caches for pip/uv, CRAN, cargo, etc. When building environments, packages are pulled from cache instead of the internet. First build of `polars==1.0` takes seconds; subsequent environments that need polars get it from cache in milliseconds.

---

## 7. Verification and trust

### 7.1 Three tiers

| Tier | Badge | Meaning |
|------|-------|---------|
| **1: Server-verified** | Filled green checkmark | Server fetched source from git, built the environment, executed the transform, hashed the output. Full chain of custody. Zero trust required. |
| **2: Reproducible, client-computed** | Hollow/outline checkmark | User ran locally and uploaded the result. Transform, environment, inputs, and params are all recorded. Server *could* re-run to verify but hasn't yet. |
| **3: Uploaded data** | No badge | Primary source data uploaded by user. No transform chain. Nothing to verify — it's ground truth. |

### 7.2 Network transforms

Transforms with `network = true` are **capped at Tier 2.** Even if the server executes them, the result may not be reproducible (LLM non-determinism, API changes, etc.). Re-running might produce a different hash.

This incentivizes isolating network-dependent steps. The pattern:

```
PDF → [extract_from_pdf, network=true, hollow] → structured.parquet
structured.parquet → [clean, network=false, filled] → clean.parquet
```

The non-determinism is contained to the extraction step. Everything downstream is fully reproducible given the extraction output.

### 7.3 Verification cascade

An endpoint's verification level = the **weakest link** in its pipeline:

| Pipeline | Endpoint badge |
|----------|---------------|
| All Tier 1 | Filled |
| Any Tier 2 (network or client-computed) | Hollow |

### 7.4 Re-verification

For Tier 2 endpoints, anyone can request server re-verification. OzzyDB re-runs the pipeline server-side. If the output hash matches the client-computed result → promoted to Tier 1 (filled badge). If it doesn't match (e.g., network transform produced different output) → stays Tier 2 with a note about the discrepancy.

### 7.5 Secrets and hashing

Secret values are **NOT part of the hash** — the hash includes the transform source, lockfile, params, and inputs, but not the API key itself. Secret values are sensitive and must never appear in hashes or logs.

However, **secret rotation must invalidate the cache.** If a transform calls an LLM API and you rotate the API key to point at a different account or model version, cached results from the old key may be stale.

Solution: the materialized hash for transforms that declare `secrets = [...]` includes `blake3(sorted secret names + secret version_id)`. The `version_id` is a UUID that is regenerated every time `ozzy secret set` is called (even if the value happens to be the same). This means:
- Setting a new secret value → new version_id → new materialized hash → cache miss → re-execution
- Same secret names, same version_ids → same hash → cache hit
- No secret values ever appear in hashes

Why UUID instead of an integer counter: if a secret is deleted and recreated with the same name, an integer counter would reset to 1, potentially colliding with a previous version's cached results. A UUID is globally unique and collision-proof.

The `version_id` lives in the `secrets` table. Transforms without `secrets = [...]` are unaffected.

---

## 8. Compute infrastructure

### 8.1 Pluggable backend

Compute is abstracted behind a trait. Fly Machines is the first implementation.

```rust
trait ComputeBackend: Send + Sync {
    async fn run(&self, request: ComputeRequest) -> Result<ComputeResult>;
    fn available_machines(&self) -> Vec<MachineConfig>;
}

struct ComputeRequest {
    image: String,           // container image to run
    command: Vec<String>,    // command + args
    inputs: Vec<InputMount>, // files to mount
    machine: MachineConfig,  // compute resources
    timeout: Duration,
    network: bool,           // false by default
    env_vars: HashMap<String, String>,
}

struct ComputeResult {
    output_blobs: Vec<Blob>,
    exit_code: i32,
    stdout: String,
    stderr: String,
    duration: Duration,
}
```

### 8.2 Machine menu

Named tiers, provider-agnostic:

| Name | CPU | Memory | GPU | Example use |
|------|-----|--------|-----|-------------|
| `cpu-small` | 2 | 4 GB | - | Simple transforms, data cleaning |
| `cpu-medium` | 4 | 16 GB | - | Moderate analysis, joins |
| `cpu-large` | 8 | 64 GB | - | Large dataset processing |
| `gpu-small` | 4 | 16 GB | L40S | Light ML inference |
| `gpu-large` | 8 | 64 GB | A100 | Model training, heavy ML |

Default is `cpu-small`. Specified per-node in the endpoint:

```toml
[endpoints.analysis.nodes]
train   = { transform = "train_model", machine = "gpu-large" }
process = { transform = "process_results" }  # defaults to cpu-small
```

The names are stable across providers. Switching from Fly to AWS remaps `gpu-large` to the equivalent instance type.

### 8.3 Fly Machines

First compute backend. Key properties:
- **Firecracker micro-VMs** — each job runs in its own VM. Stronger isolation than gVisor. No additional sandboxing needed.
- **GPU support** — L40S, A100 available.
- **Pay-per-second** — no idle costs.
- **Scale to zero** — when nobody's running transforms, cost is zero.
- **Docker native** — `fly machine run <image> <command>`. No SDK, no vendor-specific abstractions in transforms.

### 8.4 Execution flow

When a consumer fetches an endpoint:

1. **Resolve DAG** — parse endpoint definition, resolve all data/collection/endpoint references
2. **Validate types** — check that all edge types match transform input declarations
3. **Validate params** — check min/max/enum constraints on consumer-provided params
4. **Compute hash chain** — for each node, compute `blake3(input_hashes + transform_hash + params_hash + platform_hash)`
5. **Check cache** — at each node, look for a cached result with the materialized hash
6. **Build execution plan** — identify the frontier (first uncached nodes), group by environment for batch efficiency
7. **Execute** — for each uncached node:
   a. Pull environment image (from cache or registry)
   b. Mount input files (from previous node outputs or OzzyDB data storage)
   c. Run transform in Fly Machine
   d. Collect output, verify against declared output schema
   e. Cache result by materialized hash
8. **Return** — serve the final node's output to the consumer

### 8.5 Environment grouping optimization

When consecutive nodes in the DAG share the same environment, they can be batched into a single container invocation. This avoids redundant container startup and Python import overhead:

```
Nodes: qc(scipy-stack) → cal(scipy-stack) → format(scipy-stack)

Without batching: 3 container starts, ~9s overhead
With batching:    1 container start, ~3s overhead
```

The runner script chains the transforms within a single process, passing data in memory between steps. This is an optimization, not a semantic change — the results are identical either way.

---

## 9. What survives from v1

### Keep as-is or with minor changes

| Component | Why it survives |
|-----------|----------------|
| `hash.rs` — BLAKE3 infrastructure | Core concept unchanged. `hash_bytes`, `hash_reader`, `hash_file` all reusable. |
| `platform.rs` — PlatformFingerprint | Still needed for materialized hashes. os, arch, libc, cpu_features, blas, python_version. |
| `canon.rs` — Canonicalization | UTF-8, LF endings, sorted JSON keys. Still needed for deterministic hashing. |
| `schema.rs` — Arrow schema parsing | Parquet is still the dominant tabular format. Schema validation logic reusable. |
| Auth system — GitHub OAuth device flow, JWT, API tokens | Solid, battle-tested. AccountAuthUser extractor carries forward. |
| Server scaffolding — Axum router, middleware, Postgres pool | Good foundation. Routes change, but the infrastructure stays. |
| R2/local storage abstraction — ContentStorage | Content-addressed storage works the same. verify_content_hash flag useful. |
| Frontend skeleton — SvelteKit 5 SPA | Pages need redesign for new data model, but auth flow, routing, and theme survive. |
| CLI scaffolding — clap command structure | Command names change, but the infrastructure stays. |
| Python client — subprocess pattern | The shell-out-to-CLI pattern is simple and correct. API surface changes. |
| Deterministic env vars | PYTHONHASHSEED=0, OMP_NUM_THREADS=1, etc. Still enforced. |

### Keep the concept, rewrite the implementation

| Component | What changes |
|-----------|-------------|
| `cache/` — local SQLite cache | Concept is the same (materialized hash → cached file). Implementation needs update for new hash scheme and content types beyond parquet. |
| Commit model | Now references git commits instead of storing source blobs. Much simpler. |
| Push/pull protocol | Push registers a git commit reference. Pull is replaced by git clone + data fetch. |

---

## 10. What we're dropping and why

| v1 feature | Why we're dropping it |
|------------|----------------------|
| `@ozzy.transform` decorator | The TOML is the single source of truth for the interface. Functions should have zero dependency on OzzyDB. Decorators couple the code to the platform. |
| Source code storage in R2 | Git is the canonical source. OzzyDB caches source for execution but doesn't own it. Eliminates the "code in git AND code in OzzyDB" sync problem. |
| `transforms/` directory convention | Transforms can live anywhere in the repo. The TOML `source` field points to them. |
| `data/` directory with parquet files | Data lives in OzzyDB, not in the local filesystem. Transforms operate on data already in OzzyDB. No state sync between local files and remote storage. |
| Parquet-only data model | v2 supports typed blobs: parquet, images, PDFs, GeoJSON, anything. Content type is tracked per atom. |
| uv-only Python runtime | v2 uses containers. The base image includes the package manager. uv is recommended but not required. Poetry, conda, pip, renv all work. |
| Staged endpoint JSON files (`.ozzy/staged_endpoints/`) | Endpoints are declared in ozzy.toml and versioned with git. No separate staging state. |
| `.ozzy/` directory (most of it) | Local state minimized. Commits are git commits. Refs are git refs. The `.ozzy/` directory is no longer needed for most operations. Local cache at `~/.ozzy/cache/` may still exist but is just a cache, not state. |
| `collect_data_sources()` scanning | No automatic file scanning. Data is uploaded explicitly. |
| `parse_python_transforms()` decorator scanning | No Python file scanning. Transforms are declared in TOML. |
| Environment building from lockfile at runtime | Environments are pre-built as container images and cached. No rebuilding from lockfile on every execution. |

---

## 11. Open items / deferred

### Designed but needs detail during implementation

- **Object storage (R2)** — Essential infrastructure. Data atoms, cached outputs, environment build artifacts all need to live in Cloudflare R2 (or equivalent), not on the Hetzner box's local disk. Local disk is limited, not redundant, and not accessible from Fly Machines where compute happens. v1 has R2 support in the code (`ContentStorage` with R2-primary, local fallback) but never configured a bucket. v2 requires it from day one.
- **Postgres schema for v2** — Tables for: projects, commits (git-referenced), data atoms, collections, collection membership, collection versions, metadata log, endpoints, materialized cache, secrets. Needs concrete DDL.
- **Push/fetch wire protocols** — Exact HTTP API contract for push (register commit), fetch (resolve + execute + stream result), data upload, collection management.
- **Runner implementations** — Python runner is designed. R runner is sketched. Julia, command-based need implementation. Each language needs its own bridge from the container I/O contract to function call.
- **Frontend changes** — Project page needs: data management UI (upload, browse, collections), endpoint explorer with param inputs, verification badges, DAG visualization. Auth and routing can carry forward.
- **`ozzy init` experience** — What happens when a new user runs `ozzy init` in their repo? Scaffolding: generate ozzy.toml template, detect existing lockfiles, suggest environment config.
- **Local vs remote execution** — `ozzy run` for local dev (runs in local Docker), `ozzy fetch` for remote (server dispatches to Fly). Same transform, same container, different orchestrator.

### Deferred to post-v2

- **DOI minting** — Wait until the architecture is stable. DOIs are hard to change once minted. The design supports them (endpoint + commit hash + params = exact citation) but implementation is deferred.
- **Billing and quotas** — Storage metering, compute metering, tier enforcement. Deferred until there are users.
- **Streaming / buffers** — High-frequency sensor data ingestion. Deferred; the sensor operator batches and uploads via API.
- **Incremental map optimization** — Caching per-item results within a collection transform to avoid reprocessing unchanged items. The interface doesn't need to change; this is a cache-layer optimization that can be added later.
- **Annotated tags** — Tag with message/metadata (like git annotated tags).
- **GitHub Actions integration** — `ozzydb/push-action` for CI-driven push. Nice to have, not essential.
- **Site licenses / academic pricing** — Business model deferred until product-market fit.
- **SSO / SAML** — Enterprise auth. Build when someone is willing to pay for it.
- **Nix-based environments** — Power-user option for maximum reproducibility. The learning curve makes it impractical as a default.

---

## Appendix: Content addressing scheme

All hashing uses BLAKE3.

```
Data atom hash       = blake3(raw bytes)

Collection hash      = blake3(sorted member reference hashes)
                       (recursive for sub-collections)

Transform hash       = blake3(
                         source_hash +
                         function_name +
                         lockfile_hash +
                         environment_image_hash +
                         params_schema_hash
                       )

Materialized hash    = blake3(
                         sorted(input_name, input_hash) pairs +
                         transform_hash +
                         params_hash +
                         platform_hash +
                         secrets_hash           # only if transform declares secrets
                       )

Secrets hash         = blake3(sorted(secret_name + version_id) pairs)
                       # empty/zero if no secrets declared

Platform fingerprint = blake3(os + arch + libc + cpu_features + blas + runtime_version)
```

The materialized hash is the cache key. If two users fetch the same endpoint with the same params and the same input data on the same platform, they get the same hash and a cache hit. The first execution computes; every subsequent fetch serves from cache.

For server-side execution, the platform is the server's platform — consistent for all users. This makes the cache hit rate much higher than local execution (where every user has a different platform).
