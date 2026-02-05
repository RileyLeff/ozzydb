"""
Main client functions for OzzyDB Python client.
"""

import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Literal, Optional, Union, overload

import polars as pl
import pyarrow as pa
import pyarrow.parquet as pq

from ozzydb.project import Project
from ozzydb.types import EndpointMeta, ProjectMeta

# For type hints only
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pandas as pd


def _find_ozzy_binary() -> str:
    """Find the ozzy CLI binary."""
    # Check if ozzy is in PATH
    ozzy_path = shutil.which("ozzy")
    if ozzy_path:
        return ozzy_path

    # Check common locations
    common_paths = [
        Path.home() / ".cargo" / "bin" / "ozzy",
        Path("/usr/local/bin/ozzy"),
        Path("/usr/bin/ozzy"),
    ]

    for path in common_paths:
        if path.exists():
            return str(path)

    raise FileNotFoundError(
        "Could not find 'ozzy' CLI. Please install it or add it to your PATH."
    )


def _is_local_ref(ref: str) -> bool:
    """Check if a ref points to a local project (path) vs a remote registry."""
    # Explicit path prefixes are always local
    if ref.startswith(("./", "../", "~/", "/")):
        return True
    # Remote refs follow "owner/project/endpoint[@ref]" format (exactly 2 slashes)
    # Local paths should only be detected via explicit prefixes above
    # Do NOT probe the filesystem, as "owner/project" could accidentally match a local dir
    return False


def _parse_local_ref(ref: str) -> tuple[Path, str]:
    """
    Parse a local reference string into project path and endpoint name.

    Formats:
        "./path/to/project/endpoint"
        "~/data/project/endpoint"
        "/absolute/path/project/endpoint"

    Returns:
        (project_path, endpoint_name)
    """
    path = Path(ref).expanduser()

    # The endpoint is the last component, project is the parent
    endpoint_name = path.name
    project_path = path.parent

    # Resolve to absolute
    if not project_path.is_absolute():
        project_path = Path.cwd() / project_path

    project_path = project_path.resolve()

    # Verify it's a valid project
    if not (project_path / "ozzy.toml").exists():
        raise FileNotFoundError(
            f"Not an OzzyDB project: {project_path} (ozzy.toml not found)"
        )

    return project_path, endpoint_name


@overload
def fetch(
    ref: str,
    *,
    as_pandas: Literal[True],
    override_params: Optional[dict[str, dict]] = None,
    force: bool = False,
) -> "pd.DataFrame": ...


@overload
def fetch(
    ref: str,
    *,
    as_pandas: Literal[False] = False,
    override_params: Optional[dict[str, dict]] = None,
    force: bool = False,
) -> pl.DataFrame: ...


@overload
def fetch(
    ref: str,
    *,
    as_pandas: bool = False,
    override_params: Optional[dict[str, dict]] = None,
    force: bool = False,
) -> Union[pl.DataFrame, "pd.DataFrame"]: ...


def fetch(
    ref: str,
    *,
    as_pandas: bool = False,
    override_params: Optional[dict[str, dict]] = None,
    force: bool = False,
) -> Union[pl.DataFrame, "pd.DataFrame"]:
    """
    Fetch data from an OzzyDB endpoint (local or remote).

    Args:
        ref: Reference to the endpoint. Can be:
             - Local: "./project/endpoint", "~/data/project/endpoint"
             - Remote: "owner/project/endpoint", "owner/project/endpoint@v1.0"
        as_pandas: If True, return a pandas DataFrame instead of polars
        override_params: Dict of {transform_name: {param: value}} to override
        force: If True, ignore cache and re-execute all transforms

    Returns:
        DataFrame with the endpoint's output data

    Example:
        >>> import ozzydb as ozzy
        >>> df = ozzy.fetch("./my-project/corrected")
        >>> df = ozzy.fetch("rileyleff/sapflux/corrected@v1.0")
    """
    if _is_local_ref(ref):
        return _fetch_local(ref, as_pandas=as_pandas, override_params=override_params, force=force)
    else:
        return _fetch_remote(ref, as_pandas=as_pandas, override_params=override_params)


def _fetch_local(
    ref: str,
    *,
    as_pandas: bool = False,
    override_params: Optional[dict[str, dict]] = None,
    force: bool = False,
) -> Union[pl.DataFrame, "pd.DataFrame"]:
    """Fetch from a local project using `ozzy run`."""
    project_path, endpoint_name = _parse_local_ref(ref)

    ozzy_bin = _find_ozzy_binary()
    cmd = [ozzy_bin, "run", endpoint_name]

    if force:
        cmd.append("--force")

    if override_params:
        for transform_name, params in override_params.items():
            for key, value in params.items():
                cmd.extend(["--param", f"{transform_name}.{key}={value}"])

    with tempfile.NamedTemporaryFile(suffix=".parquet", delete=False) as tmp:
        tmp_path = tmp.name

    try:
        cmd.extend(["--output", tmp_path])

        result = subprocess.run(
            cmd,
            cwd=str(project_path),
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            raise RuntimeError(
                f"ozzy run failed:\n{result.stderr}\n{result.stdout}"
            )

        df = pl.read_parquet(tmp_path)

        if as_pandas:
            return df.to_pandas()
        return df

    finally:
        if os.path.exists(tmp_path):
            os.unlink(tmp_path)


def _fetch_remote(
    ref: str,
    *,
    as_pandas: bool = False,
    override_params: Optional[dict[str, dict]] = None,
) -> Union[pl.DataFrame, "pd.DataFrame"]:
    """Fetch from a remote registry using `ozzy fetch`."""
    ozzy_bin = _find_ozzy_binary()
    cmd = [ozzy_bin, "fetch", ref]

    if override_params:
        for transform_name, params in override_params.items():
            for key, value in params.items():
                cmd.extend(["--param", f"{transform_name}.{key}={value}"])

    with tempfile.NamedTemporaryFile(suffix=".parquet", delete=False) as tmp:
        tmp_path = tmp.name

    try:
        cmd.extend(["--output", tmp_path])

        result = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
        )

        if result.returncode != 0:
            raise RuntimeError(
                f"ozzy fetch failed:\n{result.stderr}\n{result.stdout}"
            )

        df = pl.read_parquet(tmp_path)

        if as_pandas:
            return df.to_pandas()
        return df

    finally:
        if os.path.exists(tmp_path):
            os.unlink(tmp_path)


def fetch_lazy(
    ref: str,
    *,
    override_params: Optional[dict[str, dict]] = None,
    force: bool = False,
) -> pl.LazyFrame:
    """
    Fetch data lazily from a local OzzyDB project endpoint.

    This executes the pipeline but returns a LazyFrame, allowing
    for additional operations before collecting the final result.

    Args:
        ref: Reference to the endpoint in format "path/to/project/endpoint"
        override_params: Dict of {transform_name: {param: value}} to override
        force: If True, ignore cache and re-execute all transforms

    Returns:
        LazyFrame with the endpoint's output data

    Example:
        >>> import ozzydb as ozzy
        >>> lf = ozzy.fetch_lazy("./my-project/corrected")
        >>> result = lf.filter(pl.col("year") == 2024).collect()
    """
    # For now, fetch eagerly then convert to lazy
    # Future optimization: support streaming from cache
    df = fetch(ref, override_params=override_params, force=force)
    return df.lazy()


def _ozzy_type_to_arrow(type_str: str) -> pa.DataType:
    """Convert an OzzyDB type string to a PyArrow DataType."""
    type_map = {
        "null": pa.null(),
        "bool": pa.bool_(),
        "int8": pa.int8(),
        "int16": pa.int16(),
        "int32": pa.int32(),
        "int64": pa.int64(),
        "uint8": pa.uint8(),
        "uint16": pa.uint16(),
        "uint32": pa.uint32(),
        "uint64": pa.uint64(),
        "float16": pa.float16(),
        "float32": pa.float32(),
        "float64": pa.float64(),
        "utf8": pa.utf8(),
        "large_utf8": pa.large_utf8(),
        "binary": pa.binary(),
        "large_binary": pa.large_binary(),
        "date32": pa.date32(),
        "date64": pa.date64(),
    }
    if type_str in type_map:
        return type_map[type_str]
    if type_str.startswith("timestamp["):
        inner = type_str[10:-1]
        parts = inner.split(", ", 1)
        tz = parts[1] if len(parts) > 1 else None
        return pa.timestamp(parts[0], tz=tz)
    if type_str.startswith("list<"):
        inner = type_str[5:-1]
        return pa.list_(_ozzy_type_to_arrow(inner))
    # Fallback
    return pa.utf8()


def _json_schema_to_arrow(output_schema: dict) -> Optional[pa.Schema]:
    """
    Convert an OzzyDB output_schema dict to a PyArrow Schema.

    Supports:
        {"fields": [{"name": "col", "type": "float64", "nullable": true}]}
        {"adds": ["col1", "col2"]}  (fields will be utf8 type)
    """
    if not output_schema:
        return None

    fields = []

    if "fields" in output_schema:
        for f in output_schema["fields"]:
            name = f.get("name", "unknown")
            dtype = _ozzy_type_to_arrow(f.get("type", "utf8"))
            nullable = f.get("nullable", True)
            fields.append(pa.field(name, dtype, nullable=nullable))
    elif "adds" in output_schema:
        for col_name in output_schema["adds"]:
            fields.append(pa.field(col_name, pa.utf8(), nullable=True))

    return pa.schema(fields) if fields else None


def inspect(ref: str) -> EndpointMeta:
    """
    Inspect metadata for a local OzzyDB project endpoint.

    Args:
        ref: Reference to the endpoint in format "path/to/project/endpoint"

    Returns:
        EndpointMeta with schema, DAG, and lineage information

    Example:
        >>> import ozzydb as ozzy
        >>> meta = ozzy.inspect("./my-project/corrected")
        >>> print(meta.schema)
        >>> print(meta.dag)
    """
    project_path, endpoint_name = _parse_local_ref(ref)

    project = Project(project_path)
    endpoint = project.get_endpoint(endpoint_name)

    # Try to infer the output schema from the last transform's output_schema
    if endpoint.nodes:
        last_node = endpoint.nodes[-1]
        transform = endpoint.transforms.get(last_node.transform_name)
        if transform and transform.output_schema:
            schema = _json_schema_to_arrow(transform.output_schema)
            if schema:
                endpoint.schema = schema

    return endpoint


def inspect_project(path: str) -> ProjectMeta:
    """
    Inspect a full OzzyDB project.

    Args:
        path: Path to the project directory

    Returns:
        ProjectMeta with all data sources, transforms, and endpoints

    Example:
        >>> import ozzydb as ozzy
        >>> meta = ozzy.inspect_project("./my-project")
        >>> print(meta.endpoints)
    """
    project = Project(path)
    return project.meta()
