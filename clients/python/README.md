# OzzyDB Python Client

Python client for OzzyDB - the data management platform for scientific computing.

## Installation

```bash
# Using uv (recommended)
uv pip install ozzydb

# Or from source
cd clients/python
uv pip install -e .
```

## Quick Start

```python
import ozzydb as ozzy

# Fetch data from the registry
df = ozzy.fetch("rileyleff/sapflux/corrected_readings", qc_threshold=12.0)

# Fetch as pandas DataFrame
df = ozzy.fetch("rileyleff/sapflux/corrected_readings", as_pandas=True)

# Lazy fetch (returns polars LazyFrame, parquet only)
lf = ozzy.fetch_lazy("rileyleff/sapflux/corrected_readings")
result = lf.filter(pl.col("year") == 2024).collect()

# Inspect endpoint metadata without executing
meta = ozzy.inspect("rileyleff/sapflux/corrected_readings")
print(meta.params)
print(meta.nodes)

# Inspect a project
project = ozzy.inspect_project("rileyleff/sapflux")
print(project.refs)
print(project.commit_count)

# Upload data
result = ozzy.upload("rileyleff/sapflux", "data/raw.parquet", name="raw")
print(result.hash)

# Download data
data = ozzy.download("rileyleff/sapflux", "raw")
# Or as a DataFrame:
df = ozzy.download_dataframe("rileyleff/sapflux", "raw")

# Local execution (requires ozzy CLI)
df = ozzy.run("corrected_readings", qc_threshold=12.0)
```

## API Reference

### `ozzy.fetch(ref, *, as_pandas=False, ref_name=None, **params)`

Fetch endpoint output from the OzzyDB registry.

**Arguments:**
- `ref`: Remote reference in `"owner/project/endpoint"` format
- `as_pandas`: If True, return a pandas DataFrame instead of polars
- `ref_name`: Git ref (branch/tag) to resolve against
- `**params`: Endpoint parameters

**Returns:** `polars.DataFrame`, `pandas.DataFrame`, or `bytes`

### `ozzy.fetch_lazy(ref, *, ref_name=None, **params)`

Fetch endpoint output as a polars LazyFrame (parquet only).

**Returns:** `polars.LazyFrame`

### `ozzy.inspect(ref, *, ref_name=None)`

Inspect endpoint metadata without executing it.

**Returns:** `EndpointDetail` with params, nodes, edges

### `ozzy.inspect_project(ref)`

Inspect a project's metadata.

**Returns:** `ProjectDetail` with refs, collaborators, commit count

### `ozzy.run(endpoint, *, cwd=None, as_pandas=False, force=False, **params)`

Execute an endpoint locally via the ozzy CLI.

**Arguments:**
- `endpoint`: Endpoint name from local `ozzy.toml`
- `cwd`: Working directory (defaults to current directory)
- `as_pandas`: Return pandas DataFrame instead of polars
- `force`: Force re-execution, ignoring cache
- `**params`: Endpoint parameters

**Returns:** `polars.DataFrame`, `pandas.DataFrame`, or `bytes`

### `ozzy.upload(project, file, *, name=None, content_type=None, collection=None)`

Upload a data atom to the registry.

**Returns:** `UploadResult` with name, hash, byte_size

### `ozzy.download(project, name)`

Download a data atom as raw bytes.

**Returns:** `bytes`

### `ozzy.download_dataframe(project, name, *, as_pandas=False)`

Download a data atom and read it as a DataFrame.

**Returns:** `polars.DataFrame` or `pandas.DataFrame`

### `ozzy.OzzyClient(base_url=None, token=None)`

HTTP client for the OzzyDB API. Uses credentials from `~/.config/ozzy/credentials.json` if available.

## Authentication

The client reads credentials from `~/.config/ozzy/credentials.json` (created by `ozzy auth login`). You can also pass a token explicitly:

```python
client = ozzy.OzzyClient(token="your-api-token")
df = ozzy.fetch("owner/project/endpoint", client=client)
```

## Requirements

- Python >= 3.10
- polars >= 1.0.0
- pyarrow >= 15.0.0
- requests >= 2.28.0
- ozzy CLI (only for `run()` — not needed for remote operations)

## Development

```bash
cd clients/python
uv sync --all-groups
uv run pytest -v
```
