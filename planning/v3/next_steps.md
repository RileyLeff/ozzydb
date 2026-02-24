# Next Steps

## Immediate (do now)

### GitHub App setup
Configure the existing GitHub App for private repo access. Currently only public repos work — the server logs `GitHub App not configured — only public repos accessible`.

### ~~End-to-end smoke test~~ DONE
Completed 2026-02-14. Full pipeline verified: push → data upload → fetch → Fly compute → R2 output → cache hit. Six bugs fixed during testing (see AGENT_WHITEBOARD.md).

### Automated pg_dump to R2
Cron job: `pg_dump | gzip | upload to R2`. No backup strategy exists today.

### Rethink GitHub auth flow for LLM/CLI ergonomics
**STATUS: Actively being reworked.** Riley is implementing a new auth solution that will replace the current GitHub-coupled flow. The GitHub username rename issue (where renaming a GitHub account breaks `owner/project` references in downstream `ozzy.toml` files) will be addressed as part of this work.

The current auth flow has high friction for LLM agents and automated workflows:
- `ozzy auth login` requires interactive browser-based GitHub device flow
- GitHub App installation requires visiting a web UI — no API/CLI path available
- Both steps block autonomous operation entirely
- GitHub username renames silently break `owner/project` URLs via the `ON CONFLICT (github_id) DO UPDATE SET username = EXCLUDED.username` upsert in `db/queries.rs`

Consider alternatives:
- Personal access token auth (paste a PAT, skip device flow)
- Server-side API key auth (bypass GitHub entirely for CLI usage)
- `gh` CLI token reuse (detect existing GitHub auth)
- Make GitHub App optional (support public repos without it, allow PAT-based private repo access)
- Decouple OzzyDB identity from GitHub username (pin on first login, or use redirect/alias table)

### BUG: `PlatformFingerprint::detect()` hashes the server, not the container

`PlatformFingerprint::detect()` is called in `orchestrator.rs:74` and `fetch.rs:319` on the **API server process**, but it's used to compute materialized cache keys for transforms that execute inside **compute containers**. This means:

1. **Wrong platform in cache key:** If the server runs Alpine (musl) but compute containers run `python:3.12-slim` (glibc), the materialized hash records the server's platform, not the container's.
2. **Mass cache invalidation on server migration:** Moving the API server from x86_64 to aarch64 (or changing its OS) would invalidate the entire materialized cache globally, even though compute containers haven't changed.
3. **Currently masked:** Single-server Docker setup means server and containers share the same kernel/arch, so the hash is coincidentally close enough. Breaks as soon as compute moves to Fly or a different-arch host.

**Fix options:**
- **Infer from environment definition:** The `ozzy.toml` environment spec (base image, runtime) determines the container platform. Derive the fingerprint from the image manifest (os/arch/variant from the OCI image index).
- **Probe on first use:** Run a tiny detection script inside the container image once, cache the result keyed by image digest. Adds ~1s latency on first use of an image.
- **Hardcode per-provider:** Docker backend → server's platform (correct today). Fly backend → Fly machine config (region/arch). Less dynamic but simpler.

**Impact:** Medium-term. Harmless in the current single-server Docker setup, but must be fixed before enabling Fly compute or multi-arch deployments.

### Container initialization robustness
The current init script (shell bootstrap that runs inside compute containers) has fragile assumptions:
- Requires `/workspace/` to exist (fixed with `mkdir -p`, but other dirs may be missing)
- Requires `python3` for all HTTP I/O (downloads, uploads via urllib) — works for Python transforms but breaks for R/Julia/WASM
- Requires `base64` and `tar` to be available in the image
- Shell script embedded as base64 env var — hard to debug, no structured logging back to the server
- No way to stream logs from the container back to the server in real-time (only get exit code + events after the fact)

For v4, consider a statically-linked "ozzy-agent" binary injected into the container (via bind mount or sidecar). This would:
- Handle all presigned URL I/O without depending on Python/curl/wget
- Stream structured logs back to the server via a callback URL
- Create the workspace directory structure reliably
- Support any runtime (Python, R, Julia, WASM) without per-language init script variants

## Near-term optimizations

### Batch init for sequential same-environment nodes
Architecture spec (v3) describes grouping sequential DAG nodes that share an environment into a single machine dispatch, eliminating cold starts for chains like A → B → C. Currently each node gets its own machine. Meaningful cost/latency win for multi-step pipelines.

### Compute tier selection
Per-node `machine` field in ozzy.toml maps to named providers, but no tier differentiation within a provider (cpu-small vs cpu-large). Fly supports different `guest` configs.

### Per-project compute config in ozzy.toml
Let projects specify default machine/tier preferences.

## Design Problems to Solve (v4 architecture)

### The Boundaries Problem: Transform I/O

**Status quo:** The runner pre-loads inputs into language-native objects (Polars DataFrame for parquet, string for text/csv, etc.) and type-checks outputs (must return DataFrame, str, bytes, or dict). This hard-codes OzzyDB's opinions about data libraries into every transform.

**The problem:** Data is messy, variable, and comes in dozens of formats (parquet, CSV, HDF5, NetCDF, images, GeoTIFF, protocol buffers, etc.). OzzyDB shouldn't pick winners among data libraries. A user who wants PyArrow or pandas or DuckDB for parquet shouldn't be forced through Polars. An R user shouldn't need the `arrow` package just because OzzyDB decided that's how parquet works.

**Three-layer model (proposed):**

1. **Transport layer** (server) — blobs in, blobs out. Content-addressed. The server moves bytes and knows content types, but never deserializes data. This is what OzzyDB owns.

2. **Adapter layer** (inside container) — bridges blobs ↔ typed objects. Currently baked into the runner. Should be separated. Options:
   - **Convention-based (simplest):** Runner provides file paths + manifest (content types, sizes). Transform reads/writes files itself. Maximum flexibility, slightly more boilerplate.
   - **Declared adapters:** ozzy.toml declares input/output formats, adapter code is generated per-language. Requires (language × format) matrix of adapter implementations.
   - **LLM-generated adapters:** Declare the schema, an LLM generates the glue code that loads the file into the right type. Wild but viable for common cases — cheap model like Gemini Flash could generate boilerplate on push.
   - **WASM WIT:** Standardized interface types for cross-language interop. Promising but ecosystem isn't there yet for data science.

3. **Transform** (user code) — pure function. `f(typed_inputs) -> typed_output`. The user's business logic.

**DX goal:** `library(ozzydb); fetch("rileyleff/water_potential/hsm@latest")` → data.frame. The magic deserialization happens in the **client library**, not the runner. Server returns blob + content-type header. R client sees `application/vnd.apache.parquet`, calls `arrow::read_parquet()`. Python client does the same with whatever library the user prefers.

**Recommendation:** Start with convention-based (paths + manifest). Runner generates NO deserialization code. Transform gets `inputs = {"names": "/workspace/inputs/names"}` and `params = {...}`. User does `pd.read_csv(inputs["names"])` or `pl.read_parquet(inputs["data"])` or `Image.open(inputs["photo"])`. Client libraries handle the output-side deserialization for `fetch()`.

### The Environment Problem

**Status quo:** Three tiers that conflate different concerns:
1. Base + lockfile (OzzyDB base image + `pip install -r requirements.txt`) — Python-pip only
2. Dockerfile (user writes a Dockerfile) — maximum flexibility, most work
3. Prebuilt image (user provides an image ref) — maximum flexibility, requires separate image hosting

**The problem:** The lockfile tier is a convenience shortcut that only works for Python-pip. Adding conda, npm, cargo, renv each requires new install logic. We're embedding package manager opinions into OzzyDB.

**What "environment" actually means:** An environment is a container image where a transform can run. The question is: who builds it, and from what spec?

**Options:**
- **Prebuilt-only:** Kill the lockfile tier entirely. Users provide Docker images. Push complexity to the user but OzzyDB stays language-agnostic. Could provide example Dockerfiles.
- **Auto-detection:** Scan the user's GitHub repo for environment signals (requirements.txt, pyproject.toml, renv.lock, package.json, Cargo.toml, etc.) and auto-generate a Dockerfile. Fuzzy inference — could be LLM-assisted (Gemini Flash: "here are the files in this repo, generate a Dockerfile").
- **Environment-as-code:** Users define environments declaratively (not as Dockerfiles) using a format OzzyDB owns. OzzyDB translates to Dockerfiles. Risk: reinventing Nix/Guix/Docker.
- **Hybrid:** Support prebuilt images (flexible) + auto-detection (convenient). If auto-detection fails, fall back to "please provide a Dockerfile or image."

**Key insight:** The environment problem and the boundaries problem are connected. If transforms handle their own I/O (convention-based adapter model), then the environment just needs to have the right libraries installed. The user's requirements.txt or renv.lock already lists those libraries. OzzyDB doesn't need to know what they are — it just needs to build an image that has them.

**Validation opportunity:** On push, OzzyDB could verify the environment is viable: scan transform source for imports, check they're available in the declared environment. Fail fast with "your transform imports `polars` but your environment doesn't have it" instead of failing at compute time.

### Determinism: Per-Transform Control

**Status quo:** Every container gets `PYTHONHASHSEED=0`, `OMP_NUM_THREADS=1`, etc. These are Python-centric, always-on, and sometimes harmful (single-threading hurts safely-parallel workloads).

**Proposal:** Move determinism settings to ozzy.toml per-transform (or per-environment):
```toml
[transforms.greet]
determinism = "strict"  # PYTHONHASHSEED=0, single-threaded (default for Python)

[transforms.heavy_compute]
determinism = "relaxed"  # No thread pinning, user manages reproducibility

[transforms.r_analysis]
determinism = "none"     # No determinism env vars (R/Julia/command transforms)
```

Or: detect runtime and apply language-appropriate determinism vars. Python gets PYTHONHASHSEED. R gets `set.seed()` injection. Commands get nothing.

### Schema: What Does the Consumer Need to Know?

The hard part isn't schema validation inside the pipeline — it's: how does a consumer of `fetch()` know what they're getting back?

**Current state:** Schema is declared in ozzy.toml but loosely enforced. The Python/R client gets back a blob and guesses based on content type.

**The DX goal:** A non-technical PI runs `fetch("rileyleff/water_potential/hsm@latest")` and gets a data.frame with columns they understand. They never think about parquet, content types, or adapters.

**What needs to happen:**
- Endpoint declares its output schema (column names, types, description)
- `inspect()` returns this schema without running compute
- `fetch()` returns data + schema metadata
- Client library uses schema to present a clean typed object
- Schema violations at compute time produce clear errors, not corrupted data

**Open question:** Schema for non-tabular outputs (images, models, nested structures). Maybe endpoints declare an output *type* (tabular, image, blob, collection) and schema only applies to tabular?

### First-Class Types: A Scientific Type System

**Core idea (2026-02-15 design session):** Types and constraints — both encodings and semantic types — should be first-class objects in OzzyDB. Not just metadata annotations, but registered, named, referenceable, composable entities.

**Two orthogonal axes:**
- **Encoding** describes how data is organized on disk (csv, parquet, png, pickle, arrow_ipc, rds, ...). Mechanical, boring, but necessary.
- **Semantics** describes what the data means and what invariants hold (water_potential is a float in pressure units that's ≤ 0, percent_composition is 0-100, etc.). This is where domain knowledge lives.

These are independent. A CSV of water potentials and a Parquet of water potentials have different encodings but identical semantics. A NumPyro model and a Stan model have different encodings but the same semantic capability (posterior predictive inference over the same domain).

**What first-class means:**
- Core ships common encodings (csv, parquet, json, png, etc.) and base semantic primitives (float, int, string, bool, units)
- Users define domain-specific semantic types: `water_potential`, `stomatal_conductance`, `hydraulic_safety_margin`
- Types compose: `table<{leaf_wp: water_potential<MPa>, species: string}>`
- Types are generic over units (Rust-style): `water_potential<U: Pressure>` → `water_potential<MPa>`, `water_potential<Bar>`
- Types are validated: structural compatibility at push time, value constraints at compute time

**Example syntax:**
```toml
[types.water_potential]
base = "float"
unit = "pressure"
constraints = ["<= 0"]
description = "Xylem water potential — always non-positive"

[transforms.hsm.inputs.survey]
encoding = "csv"
columns = [
  { name = "leaf_wp", type = "water_potential<MPa>" },
  { name = "soil_wp", type = "water_potential<MPa>" },
]
invariants = ["leaf_wp <= soil_wp"]
```

**Levels of semantic description (all optional, declare as much as you know):**
- Level 0: "bytes" (universal, unconstrained)
- Level 1: "table with columns" (structural)
- Level 2: column names and base types (schema)
- Level 3: units on columns (dimensional)
- Level 4: per-column constraints (value ranges, patterns)
- Level 5: cross-column invariants (relational constraints)

More declaration = more static checking at push time. Less declaration = more flexibility, less safety.

**Capability types for opaque data:**
Models, custom objects, etc. declare what they can DO, not what they ARE:
```toml
[transforms.train.output]
structure = "opaque"
encoding = "pickle"
capability = "predict"
predict.inputs = { depth = "float<meters>" }
predict.outputs = { density_mean = "float<g/cm3>", density_sd = "float<g/cm3>" }
```
A NumPyro model and a Stan model with identical capability declarations are substitutable. The consumer depends on the capability, not the framework.

**Possible standalone project:** This type system might be general enough to exist independently of OzzyDB — a "scientific type system" crate/library that both OzzyDB and the e-graph constraint modeling project (floco-adjacent) could build on. Core ideas: encoding vs semantic as orthogonal axes, generic units, refinement constraints, capability types for opaque objects, optional progressive declaration.

**Connection to floco / e-graph work:** The type system is the practical subset of a full constraint propagation system. OzzyDB uses it for DAG validation and runtime checking. The e-graph system could use the same type definitions for symbolic reasoning, forward/backward constraint propagation, and proving properties about scientific models. Shared vocabulary, different solvers.

**Strictness over optionality (2026-02-15 follow-up):**
Leaning toward strict, not optional. Rust's power comes from enforcement. Every port declares a type — if you don't know, say `Bytes` explicitly. No implicit untyped edges. This is like Rust requiring type annotations on fn signatures.

**Encoding in Rust's actual type system:**
Proc macros could generate real Rust types from declarations:
```rust
#[ozzy_type(unit = "pressure", constraint = "lte(0.0)")]
struct WaterPotential(f64);
```
Macro generates: newtype, validation, serialization, unit conversion traits, schema metadata. Incompatible port connections become compile errors in the orchestrator. Cross-language bridge: Rust types are source of truth → derive JSON/TOML schema descriptions → Python/R runners validate against derived schemas.

**Strictness enables LLM adapters:** Strict types constrain what valid adapter code looks like. An adapter between encoding A and encoding B for type T is a well-defined generation problem. The generated code can be VERIFIED against type constraints. Sloppy types give the LLM too much rope; strict types make correctness checkable.

**Triad: Rust types define the contract, the server enforces it, LLMs generate encoding adapters — types make the glue code verifiable.**

**Key design questions still open:**
- Constraint language boundary: range checks and patterns are easy, cross-column invariants are medium, arbitrary predicates are a rabbit hole. Where to draw the line?
- Type sharing: project-local only, or a shareable type registry across projects?
- Encoding negotiation: when producer and consumer share a semantic type but not an encoding, who transcodes?
- Versioning: if a type definition changes, what happens to data tagged with the old version?
- LLM integration: can a cheap model generate adapter code when encoding conversion is needed? Strictness makes this MORE feasible.
- Standalone crate? Could this type system exist independently, usable by both OzzyDB and the e-graph constraint system?

---

## v4 (when demand exists)

### Managed Postgres
Neon, Supabase, or Fly Postgres. pg_dump to R2 is the interim.

### Multi-server redundancy
Single VPS is fine for now. Scale when needed.

### Job queue (Redis / Postgres SKIP LOCKED)
In-memory orchestration sufficient at current scale.

### Modal GPU integration
No GPU demand yet. ComputeBackend trait is ready for it.

### CI pipeline
`just test` runs locally but no GitHub Actions. Tests should gate merges.
