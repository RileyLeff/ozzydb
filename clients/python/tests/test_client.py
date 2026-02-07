"""
Tests for OzzyDB Python client.
"""

import os
import subprocess
import tempfile
from pathlib import Path

import polars as pl
import pytest

import ozzydb as ozzy


def create_test_project(tmp_path: Path) -> Path:
    """Create a minimal test OzzyDB project."""
    project_dir = tmp_path / "test-project"
    project_dir.mkdir()

    # Create ozzy.toml
    (project_dir / "ozzy.toml").write_text("""
[project]
name = "test-project"
owner = "test-user"

[refs]
head = "refs/heads/main"
""")

    # Create .ozzy directory structure
    ozzy_dir = project_dir / ".ozzy"
    ozzy_dir.mkdir()
    (ozzy_dir / "commits").mkdir()
    (ozzy_dir / "refs" / "heads").mkdir(parents=True)
    (ozzy_dir / "staged_data").mkdir()
    (ozzy_dir / "staged_transforms").mkdir()
    (ozzy_dir / "staged_endpoints").mkdir()

    # Create test data
    data_dir = project_dir / "data"
    data_dir.mkdir()

    test_df = pl.DataFrame({
        "id": [1, 2, 3, 4, 5],
        "value": [10.0, 20.0, 30.0, 40.0, 50.0],
        "category": ["A", "B", "A", "B", "A"],
    })
    test_df.write_parquet(data_dir / "raw.parquet")

    # Create staged data source metadata
    import json
    (ozzy_dir / "staged_data" / "raw.json").write_text(json.dumps({
        "name": "raw",
        "hash": "abc123",
        "schema_hash": "def456",
        "path": "data/raw.parquet",
        "row_count": 5,
        "byte_size": 1024,
    }))

    return project_dir


def create_committed_endpoint(project_dir: Path, endpoint_name: str = "ep") -> str:
    """Create a minimal committed endpoint and point HEAD to it."""
    import json

    commit_hash = "a" * 64
    commit = {
        "hash": commit_hash,
        "parent_hashes": [],
        "author": "test-user",
        "message": "seed",
        "timestamp": "2026-02-07T00:00:00Z",
        "data_sources": {},
        "transforms": {},
        "endpoints": {
            endpoint_name: {
                "name": endpoint_name,
                "nodes": [],
                "edges": [],
                "description": None,
            }
        },
    }

    commits_dir = project_dir / ".ozzy" / "commits"
    commits_dir.mkdir(parents=True, exist_ok=True)
    (commits_dir / f"{commit_hash}.json").write_text(json.dumps(commit))
    (project_dir / ".ozzy" / "refs" / "heads" / "main").write_text(f"{commit_hash}\n")
    return commit_hash


class TestProject:
    """Tests for Project class."""

    def test_load_project(self, tmp_path):
        """Test loading a project."""
        project_dir = create_test_project(tmp_path)
        project = ozzy.Project(project_dir)

        assert project.name == "test-project"
        assert project.owner == "test-user"
        assert project.head == "refs/heads/main"

    def test_project_not_found(self, tmp_path):
        """Test error when project doesn't exist."""
        with pytest.raises(FileNotFoundError, match="ozzy.toml not found"):
            ozzy.Project(tmp_path / "nonexistent")

    def test_data_sources(self, tmp_path):
        """Test loading data sources."""
        project_dir = create_test_project(tmp_path)
        project = ozzy.Project(project_dir)

        sources = project.data_sources
        assert "raw" in sources
        assert sources["raw"].name == "raw"
        assert sources["raw"].row_count == 5

    def test_get_data_path(self, tmp_path):
        """Test getting data source path."""
        project_dir = create_test_project(tmp_path)
        project = ozzy.Project(project_dir)

        path = project.get_data_path("raw")
        assert path.exists()
        assert path.name == "raw.parquet"

    def test_staged_endpoint_deletion_hidden(self, tmp_path):
        """Committed endpoints staged for deletion should be hidden."""
        project_dir = create_test_project(tmp_path)
        create_committed_endpoint(project_dir, "deleted_ep")

        staged_dir = project_dir / ".ozzy" / "staged_endpoints"
        (staged_dir / "deleted_ep.deleted").write_text("deleted\n")

        project = ozzy.Project(project_dir)
        assert "deleted_ep" not in project.endpoints

        with pytest.raises(KeyError, match="staged for deletion"):
            project.get_endpoint("deleted_ep")


class TestInspect:
    """Tests for inspect functions."""

    def test_inspect_project(self, tmp_path):
        """Test inspecting a project."""
        project_dir = create_test_project(tmp_path)
        meta = ozzy.inspect_project(str(project_dir))

        assert meta.name == "test-project"
        assert meta.owner == "test-user"
        assert "raw" in meta.data_sources


class TestParseRef:
    """Tests for reference parsing."""

    def test_parse_relative_ref(self, tmp_path):
        """Test parsing relative reference."""
        from ozzydb.client import _parse_local_ref

        project_dir = create_test_project(tmp_path)

        # Change to parent directory
        old_cwd = os.getcwd()
        try:
            os.chdir(tmp_path)
            project_path, endpoint = _parse_local_ref("test-project/endpoint1")
            assert project_path == project_dir
            assert endpoint == "endpoint1"
        finally:
            os.chdir(old_cwd)

    def test_parse_absolute_ref(self, tmp_path):
        """Test parsing absolute reference."""
        from ozzydb.client import _parse_local_ref

        project_dir = create_test_project(tmp_path)

        project_path, endpoint = _parse_local_ref(f"{project_dir}/myendpoint")
        assert project_path == project_dir
        assert endpoint == "myendpoint"


class TestEndpointMeta:
    """Tests for EndpointMeta."""

    def test_dag_representation(self):
        """Test DAG string representation."""
        from ozzydb.types import EndpointMeta, PipelineNode, PipelineEdge

        endpoint = EndpointMeta(
            name="test",
            nodes=[
                PipelineNode("step1", "transform1", {}),
                PipelineNode("step2", "transform2", {}),
            ],
            edges=[
                PipelineEdge("step1", "main", "data_source", "raw"),
                PipelineEdge("step2", "main", "node", "step1"),
            ],
            data_sources={},
            transforms={},
        )

        dag = endpoint.dag
        assert "Endpoint: test" in dag
        assert "step1" in dag
        assert "step2" in dag


# Integration tests (require ozzy CLI to be built)
def _ozzy_cli_available() -> bool:
    """Check if ozzy CLI is available."""
    if (Path.home() / ".cargo" / "bin" / "ozzy").exists():
        return True
    return subprocess.run(["which", "ozzy"], capture_output=True).returncode == 0


@pytest.mark.skipif(
    not _ozzy_cli_available(),
    reason="ozzy CLI not installed"
)
class TestFetchIntegration:
    """Integration tests requiring the ozzy CLI."""

    def test_fetch_requires_endpoint(self, tmp_path):
        """Test that fetch fails gracefully for missing endpoint."""
        project_dir = create_test_project(tmp_path)

        # This should fail because there's no endpoint named "nonexistent"
        with pytest.raises(RuntimeError, match="ozzy run failed"):
            ozzy.fetch(f"{project_dir}/nonexistent")
