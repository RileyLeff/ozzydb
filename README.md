<p align="center">
  <img src="assets/ozzydb_face.png" width="200" alt="OzzyDB" />
</p>

<h1 align="center">OzzyDB</h1>

<p align="center">
  <strong>Data as functions, not files.</strong><br/>
  A content-addressed database for reproducible data pipelines.
</p>

<p align="center">
  <a href="https://ozzydb.com">Website</a> &middot;
  <a href="docs/getting_started.md">Getting Started</a> &middot;
  <a href="https://api.ozzydb.com/health">API Status</a>
</p>

---

OzzyDB stores data, wires it to your code, runs the pipeline, and caches the result. Every output is identified by a single hash: `blake3(inputs + transform + params + platform)`. Same inputs, same code, same answer. Always.

```python
import ozzydb

df = ozzydb.fetch("acme/sensor-qc/cleaned")
```

That one line resolves the endpoint, checks the cache, runs the pipeline if needed, and returns a Polars DataFrame. The entire methodology is inspectable: what data went in, what code ran, what parameters were used.

## How it works

OzzyDB is a **switchboard**. It doesn't try to own everything:

| What | Where it lives | Why |
|------|---------------|-----|
| Source code | **Git** (GitHub) | Already versioned, reviewable, forkable |
| Environments | **Container registries** | Cached, immutable, reusable |
| Raw data | **OzzyDB** | Content-addressed, versioned, yankable |
| Orchestration | **OzzyDB** | Wires code + data + environments together |
| Cached results | **OzzyDB** | Deduplicated, re-computable on demand |

Your git repo contains an `ozzy.toml` that declares the pipeline:

```toml
[project]
name = "sensor-qc"
owner = "acme"

[git]
provider = "github"
repo = "acme/sensor-qc"

[environments.default]
base = "python"
lockfile = "requirements.txt"

[transforms.clean]
source = "transforms/clean.py:quality_control"
environment = "default"
inputs = ["raw_readings"]

[transforms.calibrate]
source = "transforms/calibrate.py:apply_calibration"
environment = "default"
inputs = ["cleaned"]
params = { offset = { type = "float", default = 0.0 } }

[endpoints.cleaned]
nodes = [
  { name = "qc", transform = "clean", edges = [{ input = "raw_readings", source = "data:raw_readings" }] }
]
terminal = "qc"

[endpoints.calibrated]
nodes = [
  { name = "qc", transform = "clean", edges = [{ input = "raw_readings", source = "data:raw_readings" }] },
  { name = "cal", transform = "calibrate", edges = [{ input = "cleaned", source = "qc" }] }
]
terminal = "cal"
params = [{ name = "offset", type = "float", default = 0.0, description = "Calibration offset" }]
```

When someone fetches `acme/sensor-qc/calibrated`, OzzyDB:

1. Resolves the endpoint DAG from the latest commit
2. Checks the materialized cache (hash of inputs + transform + params + platform)
3. On cache miss: spins up a sandboxed container, mounts the data, runs the transform chain
4. Stores the output content-addressed, returns it

## Architecture

```
crates/
  ozzy-core/       Core library: hashing, schema, ozzy.toml parser
  ozzy-cli/        CLI binary (Rust + clap)
  ozzy-server/     Registry server (Axum, PostgreSQL, R2 storage)
clients/
  python/          Python client library (ozzydb on PyPI)
frontend/          Web UI (SvelteKit 5)
```

**~20k lines of Rust**, ~4.7k lines of Svelte, ~1.5k lines of Python.

## Key design decisions

- **BLAKE3** for all hashing. Fast, parallel, and not SHA.
- **Content-addressed everything.** Same bytes = same hash = stored once.
- **Deterministic execution.** `PYTHONHASHSEED=0`, single-threaded BLAS, pinned thread counts. Same code + same data = same output, byte-for-byte.
- **No DSL.** Write normal Python functions. Use polars, numpy, scipy, whatever. Your code runs outside OzzyDB just fine.
- **Sandboxed compute.** Transforms run in Docker containers with `--network=none` and gVisor isolation. No internet access, no side effects.
- **Declarative pipelines.** `ozzy.toml` in your git repo defines the DAG. Push to register, fetch to execute.

## Self-hosting

OzzyDB is designed to be self-hostable. The server runs as a Docker Compose stack:

```bash
cd crates/ozzy-server/docker
cp .env.example .env.prod
# Edit .env.prod with your values (Postgres password, GitHub OAuth app, etc.)
docker compose -f docker-compose.prod.yml --env-file .env.prod up -d
```

Requirements:
- Docker with Docker Compose
- A GitHub OAuth App (for authentication)
- PostgreSQL 17 (included in the Compose stack)
- Optional: Cloudflare R2 bucket (for scalable storage; local filesystem works too)
- Optional: gVisor (runsc) for sandboxed compute

## License

MIT
