# OzzyDB Python Client

Python client for OzzyDB - version control for data transformations.

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

# Fetch data from a local project
df = ozzy.fetch("./my-project/corrected")

# Fetch as pandas DataFrame
df = ozzy.fetch("./my-project/corrected", as_pandas=True)

# Lazy fetch (returns polars LazyFrame)
lf = ozzy.fetch_lazy("./my-project/corrected")
result = lf.filter(pl.col("year") == 2024).collect()

# Inspect endpoint metadata
meta = ozzy.inspect("./my-project/corrected")
print(meta.schema)
print(meta.dag)

# Inspect full project
project_meta = ozzy.inspect_project("./my-project")
print(project_meta.endpoints)
```

## API Reference

### `ozzy.fetch(ref, *, as_pandas=False, override_params=None, force=False)`

Fetch data from a local OzzyDB project endpoint.

**Arguments:**
- `ref`: Reference to the endpoint in format `path/to/project/endpoint`
- `as_pandas`: If True, return a pandas DataFrame instead of polars
- `override_params`: Dict of `{transform_name: {param: value}}` to override
- `force`: If True, ignore cache and re-execute all transforms

**Returns:** `polars.DataFrame` or `pandas.DataFrame`

### `ozzy.fetch_lazy(ref, *, override_params=None, force=False)`

Fetch data lazily from a local OzzyDB project endpoint.

**Returns:** `polars.LazyFrame`

### `ozzy.inspect(ref)`

Inspect metadata for a local OzzyDB project endpoint.

**Returns:** `EndpointMeta` with schema, DAG, and lineage information

### `ozzy.inspect_project(path)`

Inspect a full OzzyDB project.

**Returns:** `ProjectMeta` with all data sources, transforms, and endpoints

### `ozzy.Project(path)`

Load an OzzyDB project from a directory.

```python
project = ozzy.Project("./my-project")
print(project.name)
print(project.data_sources)
print(project.transforms)
print(project.endpoints)
```

## Requirements

- Python >= 3.10
- polars >= 1.0.0
- pyarrow >= 15.0.0
- ozzy CLI (must be installed and in PATH)

## Development

```bash
cd clients/python

# Create virtual environment
uv venv

# Install with dev dependencies
uv pip install -e ".[dev]"

# Run tests
uv run pytest
```
