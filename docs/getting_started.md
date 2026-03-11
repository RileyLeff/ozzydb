# Getting Started with OzzyDB

This guide walks through the v4 flow:

1. initialize a project
2. upload input artifacts
3. define typed transforms and typed endpoint inputs
4. publish the current git commit
5. fetch a result by binding concrete artifact IDs

## Prerequisites

- **Python 3.10+** with `uv` or `pip`
- **Git** (your transforms live in a git repo)
- A **GitHub account** (for authentication and push/fetch against hosted OzzyDB)

## 1. Install the CLI and Python client

```bash
# Install the Python client
uv add ozzydb
# — or —
pip install ozzydb
```

The CLI (`ozzy`) is a standalone Rust binary. Download it from the releases
page, or build from source:

```bash
cargo install --path crates/ozzy-cli
```

## 2. Sign in

```bash
ozzy auth login
```

This starts a GitHub device flow. You'll get a code to enter at
[github.com/login/device](https://github.com/login/device). Once authorized,
your credentials are saved to `~/.config/ozzy/credentials.json`.

## 3. Initialize a project

Create a git repo (or use an existing one), then initialize OzzyDB:

```bash
mkdir sensor-qc && cd sensor-qc
git init
ozzy init
```

This creates an `ozzy.toml` file. OzzyDB auto-detects your git remote and
runtime. The file will look something like:

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
lockfile = "uv.lock"

# [types]
# RawCsv = 'csv(delimiter=",", header=true) & table<{ value: float64 }>'
```

## 4. Upload an artifact

Upload a file as a first-class artifact:

```bash
ozzy artifact upload readings.csv
```

OzzyDB prints the artifact UUID. Keep it; you will bind it to an endpoint input
when you fetch.

Inspect what is now stored in the project:

```bash
ozzy artifact ls
ozzy artifact show <artifact-uuid>
```

## 5. Write a transform

Create a transform scaffold:

```bash
ozzy transform scaffold clean --lang python
```

Then edit `transforms/clean.py`:

```python
def quality_control(inputs, params):
    """Return a value matching the declared output port type."""
    raw = inputs["raw"]
    min_val = params.get("min_value", 0.0)
    return raw.filter(raw["value"] >= min_val)
```

The function signature is still `(inputs, params)`, but in v4 the important
thing is that those ports are declared and typed in `ozzy.toml`.

## 6. Define the pipeline in `ozzy.toml`

Add typed aliases, typed transform ports, and typed endpoint inputs:

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
lockfile = "uv.lock"

[types]
RawRow = '{ id: int64, value: float64, timestamp: datetime }'
RawReadings = 'csv(delimiter=",", header=true) & table<RawRow>'
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
description = "Cleaned sensor readings"

[endpoints.cleaned.inputs.raw]
type = "RawReadings"

[endpoints.cleaned.params.min_value]
type = "float"
default = 0.0
binds = "qc.min_value"
description = "Minimum valid reading"

[endpoints.cleaned.nodes]
qc = { transform = "clean" }

[[endpoints.cleaned.edges]]
from = "input:raw"
to = "qc.raw"
```

## 7. Push to the registry

Commit your code, push to GitHub, then publish to OzzyDB:

```bash
git add .
git commit -m "initial pipeline"
git push origin main

ozzy push -m "initial pipeline"
```

`ozzy push` tells OzzyDB: “at this git commit, here is my typed project graph.”
OzzyDB reads `ozzy.toml`, validates it, resolves types/transforms/environments,
and publishes a new project revision.

## 8. Fetch results

Fetch by binding concrete artifact IDs to the endpoint's typed inputs:

```bash
ozzy fetch your-username/sensor-qc/cleaned \
  --input raw=<artifact-uuid> \
  --param min_value=10
```

Or from Python:

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
source hash, params, and secrets provenance reuse the cached output.

## 9. Inspect what was published

```bash
ozzy endpoint ls
ozzy endpoint show cleaned
ozzy artifact show <artifact-uuid>
ozzy artifact conformance <artifact-uuid> --type your-username/RawReadings@1
```

The Python client also exposes endpoint inspection, artifact download, bundle
creation, collection creation, and conformance inspection.
