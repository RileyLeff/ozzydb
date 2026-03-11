<p align="center">
  <img src="assets/ozzydb_face.png" width="200" alt="OzzyDB" />
</p>

<h1 align="center">OzzyDB</h1>

<p align="center">
  <strong>Typed artifacts, typed transforms, reproducible fetch.</strong><br/>
  A registry and execution layer for auditable scientific computation.
</p>

<p align="center">
  <a href="https://ozzydb.com">Website</a> &middot;
  <a href="docs/getting_started.md">Getting Started</a> &middot;
  <a href="https://api.ozzydb.com/health">API Status</a>
</p>

---

OzzyDB is a provenance system over typed computation.

You upload first-class artifacts, publish typed transforms and environments from
git, and fetch versioned endpoints by binding concrete input artifacts.

```python
import ozzydb

artifact_id = "11111111-1111-1111-1111-111111111111"
df = ozzydb.fetch(
    "acme/sensor-qc/cleaned",
    inputs={"raw": artifact_id},
)
```

That fetch runs against a pinned project revision and registry snapshot. The
output is cached by typed input artifact identities, transform version,
environment version, source hash, params, and secrets provenance.

## What OzzyDB owns

OzzyDB is a switchboard, not a monolith.

| What | Where it lives | Why |
|------|---------------|-----|
| Source code | **GitHub** | versioned, reviewable, forkable |
| Environment definitions | **OzzyDB registry** | versioned, typed, resolved from authored content |
| Environment realization | **Container / compute backend** | provider-specific build and execution |
| Raw and derived artifacts | **OzzyDB** | content-addressed, typed, inspectable |
| Orchestration | **OzzyDB** | binds typed transforms to typed artifacts |
| Cached results | **OzzyDB** | deduplicated and reproducible |

## v4 model

The v4 runtime is built around six first-class objects:

1. `TypeVersion`
2. `EnvironmentVersion`
3. `TransformVersion`
4. `Artifact`
5. `Invocation`
6. `ConformanceRecord`

The important consequences are:

- `ozzy.toml` is authored declaration, not runtime control-plane JSON
- fetch is driven by published project revisions and pinned registry snapshots
- artifacts replace the old data/collection split
- conformance is explicit: `declared`, `verified`, `rejected`
- provider realization is internal, not part of the public API contract

## Example `ozzy.toml`

```toml
[project]
name = "sensor-qc"
owner = "acme"

[git]
repo = "acme/sensor-qc"

[remote]
url = "https://api.ozzydb.com"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "uv.lock"

[types]
RawReading = '{
  id: int64,
  value: float64,
  timestamp: datetime
}'
RawReadings = 'csv(delimiter=",", header=true) & table<RawReading>'
CleanReadings = 'csv(delimiter=",", header=true) & table<{
  id: int64,
  value: float64 & min(0),
  timestamp: datetime
}>'

[transforms.clean]
source = "transforms/clean.py:quality_control"
environment = "default"

[transforms.clean.inputs.raw]
type = "RawReadings"

[transforms.clean.outputs.result]
type = "CleanReadings"

[transforms.clean.params.min_value]
type = "float"
default = 0.0

[endpoints.cleaned]
description = "Quality-controlled sensor readings"

[endpoints.cleaned.inputs.raw]
type = "RawReadings"

[endpoints.cleaned.params.min_value]
type = "float"
default = 0.0
binds = "qc.min_value"
description = "Minimum valid value"

[endpoints.cleaned.nodes]
qc = { transform = "clean" }

[[endpoints.cleaned.edges]]
from = "input:raw"
to = "qc.raw"
```

## CLI flow

Initialize a repo:

```bash
ozzy init
```

Upload source artifacts:

```bash
ozzy artifact upload readings.csv
ozzy artifact ls
```

Publish the current git commit:

```bash
git add .
git commit -m "initial pipeline"
git push origin main
ozzy push -m "initial pipeline"
```

Fetch with explicit typed input bindings:

```bash
ozzy fetch acme/sensor-qc/cleaned \
  --input raw=11111111-1111-1111-1111-111111111111 \
  --param min_value=10
```

Inspect the published graph:

```bash
ozzy endpoint show cleaned
ozzy artifact show 11111111-1111-1111-1111-111111111111
```

## Python client

```python
import ozzydb

artifact_id = "11111111-1111-1111-1111-111111111111"

detail = ozzydb.inspect("acme/sensor-qc/cleaned")
print(detail.project_revision_id, detail.registry_revision_id)

df = ozzydb.fetch(
    "acme/sensor-qc/cleaned",
    inputs={"raw": artifact_id},
    min_value=10,
)
```

The Python client also exposes artifact upload, artifact manifests, and
conformance inspection/declaration.

## Architecture

```text
crates/
  ozzy-types/      v4 type system: syntax, canonicalization, relations, verification
  ozzy-core/       shared core: hashing, manifests, ozzy.toml parsing
  ozzy-cli/        CLI binary
  ozzy-server/     registry server, DB, orchestration, storage
clients/
  python/          Python client
frontend/          deferred relative to the v4 API/server work
```

## Local development

Spin up the local stack:

```bash
docker compose -f docker-compose.dev.yml up -d
docker compose -f docker-compose.dev.yml logs server | grep "Auth token"
```

Then run the main checks:

```bash
just test
just test-docker
just test-e2e
just test-all
```

## Current status

The v4 server/API/client rewrite is implemented. The active design baseline
lives in:

- `planning/v4/architecture.md`
- `planning/v4/implementation_plan.md`
- `planning/v4/WORKFLOW_STATE.md`
- `planning/v4/soul.md`

Older v3 planning docs are background only unless a v4 document points back to
them.

## License

MIT
