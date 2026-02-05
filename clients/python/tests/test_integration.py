"""
End-to-end integration tests for OzzyDB Python client.

These tests create a real OzzyDB project using the CLI and verify
that the Python client can fetch and inspect data correctly.
"""

import json
import subprocess
import tempfile
from pathlib import Path

import polars as pl
import pytest

import ozzydb as ozzy


def _ozzy_cli_available() -> bool:
    """Check if ozzy CLI is available."""
    if (Path.home() / ".cargo" / "bin" / "ozzy").exists():
        return True
    return subprocess.run(["which", "ozzy"], capture_output=True).returncode == 0


def _get_ozzy_path() -> str:
    """Get the path to the ozzy CLI."""
    cargo_path = Path.home() / ".cargo" / "bin" / "ozzy"
    if cargo_path.exists():
        return str(cargo_path)
    result = subprocess.run(["which", "ozzy"], capture_output=True, text=True)
    if result.returncode == 0:
        return result.stdout.strip()
    raise FileNotFoundError("ozzy CLI not found")


@pytest.fixture
def ozzy_cli():
    """Get the ozzy CLI path."""
    return _get_ozzy_path()


@pytest.fixture
def test_project(tmp_path, ozzy_cli):
    """Create a test project with data, transform, and endpoint."""
    project_dir = tmp_path / "test-project"
    project_dir.mkdir()

    # Initialize project
    subprocess.run(
        [ozzy_cli, "init", "--name", "test-project", "--owner", "test-user"],
        cwd=project_dir,
        check=True,
        capture_output=True,
    )

    # Create test data
    data_dir = project_dir / "data"
    data_dir.mkdir(exist_ok=True)
    test_df = pl.DataFrame({
        "id": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        "value": [10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0, 90.0, 100.0],
        "category": ["A", "B", "A", "B", "A", "B", "A", "B", "A", "B"],
    })
    test_df.write_parquet(data_dir / "raw.parquet")

    # Add data source
    result = subprocess.run(
        [ozzy_cli, "data", "add", "data/raw.parquet", "--name", "raw"],
        cwd=project_dir,
        capture_output=True,
        text=True,
    )
    # Ignore if it already exists (from previous runs)
    if result.returncode != 0 and "already exists" not in result.stderr:
        raise RuntimeError(f"Failed to add data source: {result.stderr}")

    # Create transform
    transforms_dir = project_dir / "transforms"
    transforms_dir.mkdir(exist_ok=True)

    transform_code = '''
import polars as pl

class ozzy:
    @staticmethod
    def transform(**kwargs):
        def decorator(func):
            return func
        return decorator

@ozzy.transform(
    params={"threshold": float},
)
def filter_values(inputs, params):
    """Filter values above threshold."""
    df = inputs["main"]
    threshold = getattr(params, "threshold", 25.0)
    return df.filter(pl.col("value") > threshold)
'''
    (transforms_dir / "filter.py").write_text(transform_code)

    # Create uv.lock (minimal)
    (transforms_dir / "uv.lock").write_text("# uv lockfile\n")

    # Add transform
    subprocess.run(
        [ozzy_cli, "transform", "add", "transforms/filter.py:filter_values"],
        cwd=project_dir,
        check=True,
        capture_output=True,
    )

    # Create endpoint
    subprocess.run(
        [ozzy_cli, "endpoint", "create", "filtered",
         "--input", "raw",
         "--transforms", "filter_values"],
        cwd=project_dir,
        check=True,
        capture_output=True,
    )

    return project_dir


@pytest.mark.skipif(not _ozzy_cli_available(), reason="ozzy CLI not installed")
class TestEndToEnd:
    """End-to-end integration tests."""

    def test_fetch_endpoint(self, test_project, ozzy_cli):
        """Test fetching data from an endpoint."""
        # Fetch using Python client
        df = ozzy.fetch(f"{test_project}/filtered")

        # Verify the result
        assert isinstance(df, pl.DataFrame)
        # With default threshold of 25.0, values > 25 are kept (30-100)
        assert len(df) == 8
        assert df["value"].min() > 25.0

    def test_fetch_as_pandas(self, test_project, ozzy_cli):
        """Test fetching data as pandas DataFrame."""
        import pandas as pd

        df = ozzy.fetch(f"{test_project}/filtered", as_pandas=True)

        assert isinstance(df, pd.DataFrame)
        assert len(df) == 8

    def test_fetch_lazy(self, test_project, ozzy_cli):
        """Test lazy fetching."""
        lf = ozzy.fetch_lazy(f"{test_project}/filtered")

        assert isinstance(lf, pl.LazyFrame)

        # Apply additional filter
        result = lf.filter(pl.col("value") > 50).collect()
        assert len(result) == 5

    def test_fetch_with_force(self, test_project, ozzy_cli):
        """Test force re-execution."""
        # First fetch (populates cache)
        df1 = ozzy.fetch(f"{test_project}/filtered")

        # Force re-execution
        df2 = ozzy.fetch(f"{test_project}/filtered", force=True)

        assert len(df1) == len(df2)
        assert df1.equals(df2)

    def test_inspect_endpoint(self, test_project, ozzy_cli):
        """Test inspecting an endpoint."""
        meta = ozzy.inspect(f"{test_project}/filtered")

        assert meta.name == "filtered"
        assert len(meta.nodes) == 1
        assert meta.nodes[0].transform_name == "filter_values"
        assert "raw" in meta.data_sources

    def test_inspect_project(self, test_project, ozzy_cli):
        """Test inspecting a project."""
        meta = ozzy.inspect_project(str(test_project))

        assert meta.name == "test-project"
        assert meta.owner == "test-user"
        assert "raw" in meta.data_sources
        assert "filter_values" in meta.transforms
        assert "filtered" in meta.endpoints

    def test_project_class(self, test_project, ozzy_cli):
        """Test the Project class."""
        project = ozzy.Project(test_project)

        assert project.name == "test-project"
        assert project.owner == "test-user"

        # Check data sources
        sources = project.data_sources
        assert "raw" in sources
        assert sources["raw"].row_count == 10

        # Check transforms
        transforms = project.transforms
        assert "filter_values" in transforms

        # Check endpoints
        endpoints = project.endpoints
        assert "filtered" in endpoints

    def test_dag_visualization(self, test_project, ozzy_cli):
        """Test DAG visualization."""
        meta = ozzy.inspect(f"{test_project}/filtered")
        dag = meta.dag

        assert "Endpoint: filtered" in dag
        assert "filter_values" in dag
        assert "raw" in dag
