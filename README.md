<p align="center">
  <img src="assets/ozzydb_face.png" width="200" alt="OzzyDB" />
</p>

<h1 align="center">OzzyDB</h1>

<p align="center">
  <strong>OzzyDB keeps track of what happens to scientific data.</strong><br/>
  Typed artifacts, versioned transforms, reproducible fetch.
</p>

<p align="center">
  <a href="https://ozzydb.com">Website</a> &middot;
  <a href="docs/getting_started.md">Getting Started</a> &middot;
  <a href="https://api.ozzydb.com/health">API Status</a>
</p>

---

## What OzzyDB Is

OzzyDB is a tool for keeping track of what happens to scientific data.

Scientific datasets change through scripts, models, calibrations, filters, unit
conversions, exclusions, merges, and format changes. OzzyDB records those changes
as versioned transforms over versioned artifacts. The transform code lives in
git. The data lives in OzzyDB. The relationship between them is stored as a
queryable provenance graph.

At a high level, an OzzyDB project looks like this:

```text
raw observation
  -> transform in a pinned environment
  -> derived artifact
  -> another transform
  -> named endpoint
```

A named endpoint is a pre-routed point in the graph. A user or script can fetch
that endpoint by binding concrete input artifacts:

```python
import ozzydb

artifact_id = "11111111-1111-1111-1111-111111111111"

df = ozzydb.fetch(
    "acme/sensor-qc/cleaned",
    inputs={"raw": artifact_id},
)
```

The important trick is simple: the data can be huge, while the instructions that
produce a derived version are usually small, and the instructions are already
text. We can store the instructions themselves in git, store the artifacts in
OzzyDB, and keep the relationship between them explicit.

## Why This Exists

My initial motivation for OzzyDB was that scientific data does not have the
right infrastructure for version control.

Git works beautifully for source code because source code is already a compact
description of how to produce behavior. It works less beautifully for a
one-billion-row CSV. If you convert a column from psi to MPa, the byte diff is
enormous, while the semantic change is tiny:

```text
value_mpa = value_psi * 0.00689476
```

The meaningful object is the transformation, the environment it ran in, the
input it consumed, and the claim it makes about the output. A normal file diff
throws away that structure and asks downstream readers to infer the operation
from context, naming conventions, metadata, or prose.

OzzyDB stores the recipe directly.

This is a more natural compression scheme for scientific data. Kolmogorov would
object to the formalism, since OzzyDB is not literally finding shortest programs,
but I think he would recognize the impulse: version the faithful description of
the change first, then materialize the changed bytes when they are worth storing.

That has a practical consequence. You can keep many logical versions of a
dataset without eagerly storing every materialized result. OzzyDB can cache the
outputs that are expensive or frequently requested, recompute cheap ones, and
move along the storage and compute tradeoff while keeping artifacts and recipes
in sync.

I tried early versions of this idea as GitHub Actions because the instructions
already live in git. That almost worked, which is why it was tempting. But
Actions does not make the relationship between code, environment, inputs, and
outputs into a durable object. I kept rebuilding that relationship out of
filenames, workflow YAML, cache keys, and conventions. That was exactly the
brittleness I was trying to remove.

OzzyDB exists because that relationship is the thing I wanted to version,
inspect, fetch, cache, and eventually cite.

## Fragmented Infrastructure

Scientific data infrastructure is extraordinarily fragmented.

Every organization eventually builds its own database. The narrower databases
often preserve more meaning, but only by enforcing brittle domain-specific
metadata standards. Broad repositories often become wrappers around CSVs in S3,
with trust-based metadata and reporting standards layered on top.

Researchers need infrastructure that can preserve more structure than a generic
file repository while still letting scientists bring their own tools. OzzyDB
tries to sit in that middle layer:

- Git owns source code.
- OzzyDB owns artifacts, transforms, environments, and provenance.
- Users bring their own tools.
- The system records enough structure for the work to be inspected, reused, and
  recomputed.

In principle, this also makes transforms publishable scientific objects. A DOI
should be able to point at the versioned operation that turns one scientific
object into another: the code, environment, input contract, output contract, and
evidence that it ran.

## How OzzyDB Works

OzzyDB is built around six objects:

1. `Artifact`: a concrete piece of data.
2. `TypeVersion`: a versioned contract over artifacts.
3. `TransformVersion`: versioned code with typed input and output ports.
4. `EnvironmentVersion`: the pinned execution environment for a transform.
5. `Invocation`: one concrete run of a transform on specific inputs.
6. `ConformanceRecord`: an explicit claim that an artifact satisfies a type.

Today, OzzyDB stores a project's transform code in a git repo and uses
`ozzy.toml` to define pre-routed pipelines as named endpoints. This works. It
is probably not the final ergonomic shape. I expect to replace this authoring
layer with something nicer as the model settles.

When you push, OzzyDB publishes a project revision: a pinned registry snapshot
of the types, transforms, environments, and endpoints for that commit. When
someone fetches an endpoint, OzzyDB resolves the endpoint against that published
revision, checks the typed input bindings, looks for cached outputs, and runs
any missing transforms in the right environment.

The result is an artifact with a recorded path back through the graph.

## Minimal `ozzy.toml`

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
lockfile = "requirements.txt"

[types]
RawReading = 'csv(delimiter=",", header=true)'
CleanReading = 'csv(delimiter=",", header=true)'

[transforms.clean]
source = "transforms/clean.py:quality_control"
environment = "default"

[transforms.clean.inputs.raw]
type = "RawReading"

[transforms.clean.outputs.result]
type = "CleanReading"

[endpoints.cleaned]
description = "Quality-controlled sensor readings"

[endpoints.cleaned.inputs.raw]
type = "RawReading"

[endpoints.cleaned.nodes]
qc = { transform = "clean" }

[[endpoints.cleaned.edges]]
from = "input:raw"
to = "qc.raw"
```

Then:

```bash
ozzy artifact upload readings.csv
ozzy push -m "publish sensor cleaning pipeline"
ozzy fetch acme/sensor-qc/cleaned \
  --input raw=11111111-1111-1111-1111-111111111111
```

## Inspect What Happened

OzzyDB is designed around inspection. You can ask what endpoints exist, what a
published endpoint requires, what artifact was produced, and what conformance
claims are attached to it.

```bash
ozzy endpoint ls
ozzy endpoint show cleaned
ozzy artifact ls
ozzy artifact show 11111111-1111-1111-1111-111111111111
```

The Python client exposes the same shape:

```python
import ozzydb

detail = ozzydb.inspect("acme/sensor-qc/cleaned")
print(detail.project_revision_id, detail.registry_revision_id)

artifact = ozzydb.upload_artifact("acme/sensor-qc", "readings.csv")
df = ozzydb.fetch(
    "acme/sensor-qc/cleaned",
    inputs={"raw": artifact.artifact_id},
)
```

## Hosted And Local Use

OzzyDB is live at [ozzydb.com](https://ozzydb.com), but I currently restrict
hosted access to my own GitHub username because I cannot cover arbitrary storage
and compute costs yet.

If you want to try it seriously, run it locally:

```bash
git clone https://github.com/RileyLeff/ozzydb
cd ozzydb
docker compose -f docker-compose.dev.yml up -d
```

The system is CLI driven and fairly agent-friendly. If you want help exploring
it locally, let your favorite coding agent read the codebase and docs for
context.

Main checks:

```bash
just test
just test-docker
just test-e2e
just test-all
```

## What I Learned Building It

I built OzzyDB because I wanted scientific data to carry its history more
faithfully.

Trying to use it for my own research made the next missing piece obvious:
provenance is necessary, and it still leaves a hard semantic problem. If
arbitrary scientific tools are allowed, the system also needs to understand what
information is preserved, destroyed, assumed, or made more expensive to recover
as data moves across formats, models, and representations.

A CSV, an Arrow table, a pandas DataFrame, an R tibble, a Parquet file, and a
domain-specific model object may contain overlapping scientific meaning. Moving
between them changes what can be recovered. The path can be lossless, lossy,
one-way, approximately reversible, cheap, expensive, or valid only under
assumptions.

OzzyDB currently records typed artifacts and typed transforms. The deeper system
needs a richer graph of scientific meaning: what a transform preserves, what it
forgets, what assumptions make it valid, and how a workflow should choose among
competing paths.

That is why I now think OzzyDB is one half of the tool I actually need.

## Myco And The Longer Arc

In parallel, I have been building [Myco](https://github.com/RileyLeff/myco), a
language and compiler for declarative scientific models.

OzzyDB is about proof by observation: artifacts, transforms, evidence, and
provenance. Myco is about proof by construction: executable scientific
structure, constraints, invertibility, overdetermination, lossiness, and
workflow-specific compilation.

I expect these projects to converge eventually, but I am intentionally avoiding
that merger for now.

Myco needs more time to develop its acausal, invertible core before every hard
external operation becomes an opaque escape hatch. OzzyDB needs more time as a
practical data and provenance layer. The shared future is probably a system
where OzzyDB stores and verifies the evidence, while Myco supplies a richer type
and process language for describing what scientific transformations mean.

The destination is a substrate where scientific data can move without shedding
its history at every step. For now, OzzyDB is the data layer: a working attempt
to keep the recipes, artifacts, environments, and evidence attached.

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

## Current Status

The v4 server, API, CLI, and Python client rewrite is implemented. The active
design baseline lives in:

- `planning/v4/architecture.md`
- `planning/v4/implementation_plan.md`
- `planning/v4/WORKFLOW_STATE.md`
- `planning/v4/soul.md`

Older v3 planning docs are background only unless a v4 document points back to
them.

## License

MIT
