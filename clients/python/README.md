# OzzyDB Python Client

Python client for OzzyDB v4.

## Installation

```bash
cd clients/python
uv pip install -e .
```

## Quick Start

```python
import ozzydb as ozzy

# Fetch endpoint output
result = ozzy.fetch(
    "rileyleff/sapflux/corrected_readings",
    species="oak",
    threshold=12.0,
)

# Bind explicit artifact inputs to typed endpoint input ports
result = ozzy.fetch(
    "rileyleff/sapflux/corrected_readings",
    inputs={"raw": "123e4567-e89b-12d3-a456-426614174000"},
)

# Inspect an endpoint without executing it
endpoint = ozzy.inspect("rileyleff/sapflux/corrected_readings")
print(endpoint.terminal_node)
print(endpoint.inputs)
print(endpoint.nodes)

# Upload a blob artifact
uploaded = ozzy.upload_artifact("rileyleff/sapflux", "data/raw.parquet")
print(uploaded.artifact_id)

# Create manifest artifacts
bundle = ozzy.create_bundle_artifact(
    "rileyleff/sapflux",
    {"raw": uploaded.artifact_id},
)
collection = ozzy.create_collection_artifact(
    "rileyleff/sapflux",
    [uploaded.artifact_id],
)

# Inspect artifacts and conformance
artifact = ozzy.get_artifact("rileyleff/sapflux", uploaded.artifact_id)
conformance = ozzy.get_artifact_conformance("rileyleff/sapflux", uploaded.artifact_id)

# Declare conformance against a published version-pinned type
record = ozzy.declare_conformance(
    "rileyleff/sapflux",
    uploaded.artifact_id,
    "RawCsv@1",
)

# Download a blob artifact. Tabular formats are decoded to polars by default.
downloaded = ozzy.download_artifact("rileyleff/sapflux", uploaded.artifact_id)
```

## Public API

- `fetch(ref, *, inputs=None, ref_name=None, as_pandas=False, **params)`
- `fetch_lazy(ref, *, inputs=None, ref_name=None, **params)`
- `inspect(ref, *, ref_name=None)`
- `list_endpoints(project, *, ref_name=None)`
- `upload_artifact(project, file, *, content_type=None)`
- `list_artifacts(project)`
- `get_artifact(project, artifact_id)`
- `download_artifact(project, artifact_id, *, as_pandas=False)`
- `create_bundle_artifact(project, entries)`
- `create_collection_artifact(project, items)`
- `get_artifact_conformance(project, artifact_id)`
- `declare_conformance(project, artifact_id, type_ref, *, verify=True)`

## Notes

- This client targets the v4 API directly.
- It does not preserve the old data-atom / collection / project-inspection API surface.
- Endpoint parameter values are sent as JSON values, not query-string strings.
