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


def _parse_ref(ref: str) -> tuple[Path, str]:
    """
    Parse a reference string into project path and endpoint name.

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
    Fetch data from a local OzzyDB project endpoint.

    Args:
        ref: Reference to the endpoint in format "path/to/project/endpoint"
             Can be relative (./project/endpoint) or absolute (/path/project/endpoint)
        as_pandas: If True, return a pandas DataFrame instead of polars
        override_params: Dict of {transform_name: {param: value}} to override
        force: If True, ignore cache and re-execute all transforms

    Returns:
        DataFrame with the endpoint's output data

    Example:
        >>> import ozzydb as ozzy
        >>> df = ozzy.fetch("./my-project/corrected")
        >>> df = ozzy.fetch("~/data/sapflux/filtered", as_pandas=True)
    """
    project_path, endpoint_name = _parse_ref(ref)

    # Build command
    ozzy_bin = _find_ozzy_binary()
    cmd = [ozzy_bin, "run", endpoint_name]

    if force:
        cmd.append("--force")

    # Add parameter overrides
    if override_params:
        for transform_name, params in override_params.items():
            for key, value in params.items():
                # Format: --param key=value
                # Note: This applies globally, not per-transform
                # Future: support per-transform params
                cmd.extend(["--param", f"{key}={value}"])

    # Create temp file for output
    with tempfile.NamedTemporaryFile(suffix=".parquet", delete=False) as tmp:
        tmp_path = tmp.name

    try:
        cmd.extend(["--output", tmp_path])

        # Run the command
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

        # Read the output
        df = pl.read_parquet(tmp_path)

        if as_pandas:
            return df.to_pandas()
        return df

    finally:
        # Clean up temp file
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
    project_path, endpoint_name = _parse_ref(ref)

    project = Project(project_path)
    endpoint = project.get_endpoint(endpoint_name)

    # Try to get the output schema by finding the last node's transform
    if endpoint.nodes:
        last_node = endpoint.nodes[-1]
        transform = endpoint.transforms.get(last_node.transform_name)
        if transform and transform.output_schema:
            # Convert output_schema to Arrow schema if possible
            pass  # Future: convert JSON schema to pa.Schema

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
