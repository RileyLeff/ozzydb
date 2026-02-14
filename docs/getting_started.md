# Getting Started with OzzyDB

This guide walks you through creating your first OzzyDB project: uploading data, writing a transform, and fetching results.

## Prerequisites

- **Python 3.10+** with `uv` or `pip`
- **Git** (your transforms live in a git repo)
- A **GitHub account** (for authentication)

## 1. Install the CLI and Python client

```bash
# Install the Python client
uv add ozzydb
# — or —
pip install ozzydb
```

The CLI (`ozzy`) is a standalone Rust binary. Download it from the [releases page](https://github.com/RileyLeff/ozzydb/releases), or build from source:

```bash
cargo install --path crates/ozzy-cli
```

## 2. Sign in

```bash
ozzy auth login
```

This starts a GitHub device flow. You'll get a code to enter at [github.com/login/device](https://github.com/login/device). Once authorized, your credentials are saved to `~/.config/ozzy/credentials.json`.

You can also sign in at [ozzydb.com/login](https://ozzydb.com/login).

## 3. Initialize a project

Create a git repo (or use an existing one), then initialize OzzyDB:

```bash
mkdir sensor-qc && cd sensor-qc
git init
ozzy init
```

This creates an `ozzy.toml` file. OzzyDB auto-detects your git remote and language. The file will look something like:

```toml
[project]
name = "sensor-qc"
owner = "your-username"

[git]
provider = "github"
repo = "your-username/sensor-qc"

[remote]
url = "https://api.ozzydb.com"
```

## 4. Upload some data

Upload a data file to OzzyDB. Data atoms are the raw inputs to your pipeline.

```bash
# Upload a CSV file
ozzy data upload readings.csv --name raw_readings --description "Raw sensor readings from field station"
```

You can upload any file format: CSV, Parquet, Arrow, images, PDFs, whatever your transforms need.

```bash
# List your data
ozzy data ls

# View details
ozzy data show raw_readings
```

## 5. Write a transform

A transform is just a Python function. Create one:

```bash
ozzy transform scaffold clean --language python
```

This creates `transforms/clean.py` with a template. Edit it:

```python
# transforms/clean.py
import csv
import json
import os


def quality_control(inputs, params):
    """Remove rows with missing or out-of-range values."""

    # Read input (OzzyDB passes file paths via the inputs dict)
    with open(inputs["raw_readings"]) as f:
        reader = csv.DictReader(f)
        rows = list(reader)

    # Get params (with defaults)
    min_val = params.get("min_value", 0)
    max_val = params.get("max_value", 100)

    # Filter
    cleaned = [
        row for row in rows
        if min_val <= float(row["value"]) <= max_val
    ]

    # Write output (OzzyDB expects output at the path in OZZY_OUTPUT_PATH)
    output_path = os.environ["OZZY_OUTPUT_PATH"]
    with open(output_path, "w", newline="") as f:
        if cleaned:
            writer = csv.DictWriter(f, fieldnames=cleaned[0].keys())
            writer.writeheader()
            writer.writerows(cleaned)
```

The function signature is always `(inputs, params)`. Inputs is a dict mapping input names to file paths. Params is a dict of parameter values.

## 6. Define the pipeline in ozzy.toml

Add the environment, transform, and endpoint to your `ozzy.toml`:

```toml
[project]
name = "sensor-qc"
owner = "your-username"

[git]
provider = "github"
repo = "your-username/sensor-qc"

[remote]
url = "https://api.ozzydb.com"

# Environment: use the base Python image (no extra deps needed for stdlib)
[environments.default]
image = "python:3.12-slim"

# Transform: point to the function
[transforms.clean]
source = "transforms/clean.py:quality_control"
environment = "default"
inputs = ["raw_readings"]
params = { min_value = { type = "float", default = 0 }, max_value = { type = "float", default = 100 } }

# Endpoint: wire data to transforms
[endpoints.cleaned]
nodes = [
  { name = "qc", transform = "clean", edges = [
    { input = "raw_readings", source = "data:raw_readings" }
  ]}
]
terminal = "qc"
params = [
  { name = "min_value", type = "float", default = 0, description = "Minimum valid reading" },
  { name = "max_value", type = "float", default = 100, description = "Maximum valid reading" }
]
```

## 7. Push to the registry

Commit your code, push to GitHub, then register with OzzyDB:

```bash
git add .
git commit -m "initial pipeline"
git push origin main

ozzy push -m "initial pipeline"
```

`ozzy push` tells OzzyDB "at this git commit, here's what my pipeline looks like." OzzyDB reads your `ozzy.toml` from the git repo, validates it, and registers the commit.

## 8. Fetch results

Now anyone can fetch your endpoint:

```bash
# Via CLI
ozzy fetch your-username/sensor-qc/cleaned

# With custom parameters
ozzy fetch your-username/sensor-qc/cleaned -p min_value=10 -p max_value=90
```

Or from Python:

```python
import ozzydb

# Returns a Polars DataFrame
df = ozzydb.fetch("your-username/sensor-qc/cleaned")
print(df)

# With parameters
df = ozzydb.fetch("your-username/sensor-qc/cleaned", params={"min_value": 10})
```

The first fetch runs the pipeline and caches the result. Subsequent fetches with the same inputs and parameters return the cached result instantly.

## 9. Inspect on the web

Visit [ozzydb.com/your-username/sensor-qc](https://ozzydb.com) to see your project: endpoints, transforms, data, commit history, and the DAG visualization.

---

## Key concepts

**Data atoms** are raw, immutable inputs. Upload once, reference by name. Content-addressed (same bytes = same hash = stored once).

**Transforms** are functions that take data + parameters and produce output. Written in Python (R support coming). They run in sandboxed Docker containers.

**Endpoints** are DAGs of transforms wired to data. They're the "API" of your project. Fetch an endpoint to get a result.

**Collections** group related data atoms with versioning. Like a mutable pointer to an immutable set of data.

**The materialized hash** is the cache key: `blake3(inputs + transform + params + platform)`. Same inputs + same code + same parameters = same result = cache hit.

## CLI reference

```
ozzy init                          Initialize a project
ozzy data upload/ls/show/yank      Manage data atoms
ozzy collection create/add/ls      Manage collections
ozzy transform scaffold            Scaffold a new transform
ozzy push                          Register a commit with the registry
ozzy fetch <owner/project/ep>      Fetch a remote endpoint
ozzy auth login/logout             Authentication
ozzy cache ls/size/clear           Local cache management
ozzy secret set/ls/rm              Project secrets
```
