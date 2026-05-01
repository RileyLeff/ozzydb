# Getting Started with OzzyDB

This guide walks through the current v4 flow:

1. initialize a project
2. define typed transforms and typed endpoint inputs
3. publish the current git commit
4. upload input artifacts
5. declare artifact conformance
6. fetch a result by binding concrete artifact IDs

Hosted OzzyDB is live at [ozzydb.com](https://ozzydb.com), but hosted access is
currently restricted to Riley's GitHub username while storage and compute costs
are still personally funded. The commands below describe the current product
shape. If you are trying OzzyDB yourself right now, the recommended path is to
run the stack locally with Docker Compose.

## Prerequisites

- **Rust** if you want to build the CLI from this repo
- **Python 3.10+** if you want the Python client
- **Git** and a GitHub repo for your transform code
- The **OzzyDB GitHub App** installed for the repo when using hosted push

For local development:

```bash
git clone https://github.com/RileyLeff/ozzydb
cd ozzydb
docker compose -f docker-compose.dev.yml up -d
docker compose -f docker-compose.dev.yml logs server | grep "Auth token"
```

The system is CLI driven, so it is fairly agent-friendly. Let your coding agent
read the repo and docs for context if you want help exploring it locally.

## 1. Install the CLI and Python client

Build the CLI from the repo:

```bash
cargo install --path crates/ozzy-cli
```

Install the Python client:

```bash
uv add ozzydb
# or
pip install ozzydb
```

For editable local Python client development:

```bash
cd clients/python
uv pip install -e .
```

## 2. Sign in

```bash
ozzy auth login
```

This starts a GitHub device flow. You'll get a code to enter at
[github.com/login/device](https://github.com/login/device). Once authorized,
your credentials are saved to `~/.config/ozzy/credentials.json`.

## 3. Initialize a project

Create a git repo, or use an existing one, then initialize OzzyDB:

```bash
mkdir sensor-qc
cd sensor-qc
git init
git remote add origin git@github.com:your-username/sensor-qc.git
ozzy init
```

This creates an `ozzy.toml` file and a `transforms/` directory. OzzyDB
auto-detects your git remote and runtime when it can. The generated file is a
starting point, not the final authoring experience.

## 4. Write a transform

Create a transform scaffold:

```bash
ozzy transform scaffold clean --lang python
```

Then edit `transforms/clean.py`:

```python
def clean(inputs, params):
    """Return a value matching the declared output port type."""
    raw = inputs["raw"]
    min_value = params.get("min_value", 0.0)
    return raw.filter(raw["value"] >= min_value)
```

For Python transforms, OzzyDB loads each input according to the artifact's
content type. CSV and Parquet inputs are passed as Polars DataFrames. Returned
Polars DataFrames are written as Parquet outputs.

Make sure the environment lockfile exists and includes what your transform
imports. For this example:

```bash
printf "polars>=1.0.0\n" > requirements.txt
```

## 5. Define the pipeline in `ozzy.toml`

Replace the generated placeholders with typed aliases, typed transform ports,
and a typed endpoint input:

```toml
[project]
name = "sensor-qc"
owner = "your-username"

[git]
repo = "your-username/sensor-qc"

[remote]
url = "https://api.ozzydb.com"

[environments.default]
base = "ozzydb/python:3.12"
lockfile = "requirements.txt"

[types]
RawRow = '{ id: int64, value: float64, timestamp: datetime }'
RawReadings = 'csv(delimiter=",", header=true) & table<RawRow>'
CleanReadings = 'parquet & table<{
  id: int64,
  value: float64 & min(0),
  timestamp: datetime
}>'

[transforms.clean]
source = "transforms/clean.py:clean"
environment = "default"

[transforms.clean.inputs.raw]
type = "RawReadings"

[transforms.clean.outputs.result]
type = "CleanReadings"

[transforms.clean.params.min_value]
type = "float"
default = 0.0

[endpoints.cleaned]
description = "Cleaned sensor readings"

[endpoints.cleaned.inputs.raw]
type = "RawReadings"

[endpoints.cleaned.params.min_value]
type = "float"
default = 0.0
binds = "clean.min_value"
description = "Minimum valid reading"

[endpoints.cleaned.nodes]
clean = { transform = "clean" }

[[endpoints.cleaned.edges]]
from = "input:raw"
to = "clean.raw"
```

Today, `ozzy.toml` defines pre-routed pipelines as named endpoints. This works,
but it is not the final ergonomic shape.

## 6. Publish the project graph

Commit your code, push it to GitHub, then publish the commit to OzzyDB:

```bash
git add .
git commit -m "initial pipeline"
git push origin main

ozzy push -m "initial pipeline"
```

`ozzy push` tells OzzyDB: at this git commit, here is my typed project graph.
The server fetches `ozzy.toml` and transform source from GitHub, validates the
graph, resolves types, transforms, and environments, and publishes a new project
revision.

This is also what creates the project in OzzyDB on first push.

## 7. Upload and type an input artifact

After the project and its type versions exist, upload a file:

```bash
ozzy artifact upload readings.csv
```

OzzyDB prints the artifact UUID. Bind that artifact to the endpoint input when
you fetch.

Fetch requires the input artifact to have a non-rejected conformance record for
the endpoint input type, so declare and verify conformance before running:

```bash
ozzy artifact conformance <artifact-uuid> --type RawReadings@1
```

Inspect what is stored in the project:

```bash
ozzy artifact ls
ozzy artifact show <artifact-uuid>
```

## 8. Fetch results

Fetch by binding concrete artifact IDs to the endpoint's typed inputs:

```bash
ozzy fetch your-username/sensor-qc/cleaned \
  --input raw=<artifact-uuid> \
  --param min_value=10
```

Parameter values are JSON values. Strings need quotes:

```bash
ozzy fetch your-username/sensor-qc/cleaned \
  --input raw=<artifact-uuid> \
  --param label='"oak"'
```

Or fetch from Python:

```python
import ozzydb

df = ozzydb.fetch(
    "your-username/sensor-qc/cleaned",
    inputs={"raw": "<artifact-uuid>"},
    min_value=10,
)
```

The first fetch runs the pipeline and caches the result. Subsequent fetches with
the same typed input artifact bindings, transform version, environment version,
source hash, params, and secrets provenance can reuse the cached output.

## 9. Inspect what was published

```bash
ozzy endpoint ls
ozzy endpoint show cleaned
ozzy endpoint dag cleaned
ozzy artifact show <artifact-uuid>
```

The Python client also exposes endpoint inspection, artifact upload/download,
bundle creation, collection creation, and conformance inspection/declaration.

```python
import ozzydb

detail = ozzydb.inspect("your-username/sensor-qc/cleaned")
print(detail.project_revision_id, detail.registry_revision_id)

artifact = ozzydb.get_artifact("your-username/sensor-qc", "<artifact-uuid>")
conformance = ozzydb.get_artifact_conformance(
    "your-username/sensor-qc",
    "<artifact-uuid>",
)
```
