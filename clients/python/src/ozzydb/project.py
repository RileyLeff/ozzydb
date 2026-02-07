"""
Project loading and management for OzzyDB Python client.
"""

import json
import os
from pathlib import Path
from typing import Any, Optional

import pyarrow.parquet as pq
import toml

from ozzydb.types import (
    DataSourceMeta,
    EndpointMeta,
    PipelineEdge,
    PipelineNode,
    ProjectMeta,
    TransformMeta,
)


class Project:
    """
    Represents a local OzzyDB project.

    Usage:
        project = Project("./my-project")
        print(project.name)
        print(project.endpoints)
    """

    def __init__(self, path: str | Path):
        """
        Load a project from a directory.

        Args:
            path: Path to the project directory (containing ozzy.toml)
        """
        self.root = Path(path).expanduser().resolve()

        if not (self.root / "ozzy.toml").exists():
            raise FileNotFoundError(
                f"Not an OzzyDB project: {self.root} (ozzy.toml not found)"
            )

        self._config = toml.load(self.root / "ozzy.toml")
        self._commit: Optional[dict[str, Any]] = None

    @property
    def name(self) -> str:
        """Project name."""
        return self._config["project"]["name"]

    @property
    def owner(self) -> str:
        """Project owner."""
        return self._config["project"]["owner"]

    @property
    def head(self) -> str:
        """Current HEAD ref."""
        return self._config.get("refs", {}).get("head", "refs/heads/main")

    @property
    def ozzy_dir(self) -> Path:
        """Path to .ozzy directory."""
        return self.root / ".ozzy"

    def _load_latest_commit(self) -> Optional[dict[str, Any]]:
        """Load the latest commit from the refs."""
        if self._commit is not None:
            return self._commit

        # Read the HEAD ref
        head = self.head
        if head.startswith("refs/"):
            ref_path = self.ozzy_dir / head
        else:
            ref_path = self.ozzy_dir / "refs" / "heads" / head

        if not ref_path.exists():
            return None

        commit_hash = ref_path.read_text().strip()
        commit_path = self.ozzy_dir / "commits" / f"{commit_hash}.json"

        if not commit_path.exists():
            return None

        self._commit = json.loads(commit_path.read_text())
        return self._commit

    def _load_staged_data_sources(self) -> dict[str, DataSourceMeta]:
        """Load staged (uncommitted) data sources from data/ directory.

        The CLI scans the data/ directory directly for parquet files,
        so we do the same here for consistency.
        """
        data_dir = self.root / "data"
        if not data_dir.exists():
            return {}

        sources = {}
        for parquet_file in data_dir.glob("*.parquet"):
            name = parquet_file.stem
            # Get file metadata
            stat = parquet_file.stat()
            try:
                # Try to get row count from parquet metadata
                parquet_meta = pq.read_metadata(parquet_file)
                row_count = parquet_meta.num_rows
            except Exception:
                row_count = None

            sources[name] = DataSourceMeta(
                name=name,
                hash=None,  # Hash is computed at commit time
                schema_hash=None,
                path=f"data/{parquet_file.name}",
                row_count=row_count,
                byte_size=stat.st_size,
            )
        return sources

    def _load_staged_transforms(self) -> dict[str, TransformMeta]:
        """Load staged (uncommitted) transforms from transforms/ directory.

        The CLI scans the transforms/ directory for @ozzy.transform decorated
        functions, so we do the same here for consistency.
        """
        transforms_dir = self.root / "transforms"
        if not transforms_dir.exists():
            return {}

        transforms = {}
        for py_file in transforms_dir.glob("*.py"):
            # Parse the file to find @ozzy.transform decorated functions
            try:
                content = py_file.read_text()
                found_transforms = self._parse_transform_file(content, py_file)
                transforms.update(found_transforms)
            except Exception:
                continue

        return transforms

    def _parse_transform_file(
        self, content: str, file_path: Path
    ) -> dict[str, TransformMeta]:
        """Parse a Python file for @ozzy.transform decorated functions."""
        import re

        transforms = {}
        lines = content.split("\n")

        in_decorator = False
        for i, line in enumerate(lines):
            stripped = line.strip()
            if stripped.startswith("@ozzy.transform"):
                in_decorator = True
            elif in_decorator and stripped.startswith("def "):
                # Extract function name
                match = re.match(r"def\s+(\w+)\s*\(", stripped)
                if match:
                    func_name = match.group(1)
                    rel_path = f"transforms/{file_path.name}"
                    transforms[func_name] = TransformMeta(
                        name=func_name,
                        hash=None,  # Hash is computed at commit time
                        runtime="python",
                        source_path=rel_path,
                        function_name=func_name,
                        lockfile_hash=None,
                        params_schema={},
                        reproducible=True,
                        input_schema=None,
                        output_schema=None,
                    )
                in_decorator = False

        return transforms

    def _load_staged_endpoints(self) -> dict[str, dict[str, Any]]:
        """Load staged (uncommitted) endpoints."""
        staged_dir = self.ozzy_dir / "staged_endpoints"
        if not staged_dir.exists():
            return {}

        endpoints = {}
        for f in staged_dir.glob("*.json"):
            data = json.loads(f.read_text())
            endpoints[f.stem] = data
        return endpoints

    def _load_staged_endpoint_deletions(self) -> set[str]:
        """Load endpoint names staged for deletion."""
        staged_dir = self.ozzy_dir / "staged_endpoints"
        if not staged_dir.exists():
            return set()

        deletions = set()
        for marker in staged_dir.glob("*.deleted"):
            deletions.add(marker.stem)
        return deletions

    @property
    def data_sources(self) -> dict[str, DataSourceMeta]:
        """All data sources (committed + staged)."""
        sources = {}

        # Load from latest commit
        commit = self._load_latest_commit()
        if commit:
            for name, data in commit.get("data_sources", {}).items():
                sources[name] = DataSourceMeta(
                    name=data["name"],
                    hash=data["hash"],
                    schema_hash=data["schema_hash"],
                    path=data["path"],
                    row_count=data.get("row_count"),
                    byte_size=data.get("byte_size"),
                )

        # Override with staged
        sources.update(self._load_staged_data_sources())
        return sources

    @property
    def transforms(self) -> dict[str, TransformMeta]:
        """All transforms (committed + staged)."""
        transforms = {}

        # Load from latest commit
        commit = self._load_latest_commit()
        if commit:
            for name, data in commit.get("transforms", {}).items():
                transforms[name] = TransformMeta(
                    name=data["name"],
                    hash=data["hash"],
                    runtime=data["runtime"],
                    source_path=data["source_path"],
                    function_name=data["function_name"],
                    lockfile_hash=data["lockfile_hash"],
                    params_schema=data.get("params_schema", {}),
                    reproducible=data.get("reproducible", True),
                    input_schema=data.get("input_schema"),
                    output_schema=data.get("output_schema"),
                )

        # Override with staged
        transforms.update(self._load_staged_transforms())
        return transforms

    @property
    def endpoints(self) -> list[str]:
        """List of endpoint names."""
        endpoint_names = set()
        staged = self._load_staged_endpoints()
        staged_deletions = self._load_staged_endpoint_deletions()

        # From commit
        commit = self._load_latest_commit()
        if commit:
            endpoint_names.update(commit.get("endpoints", {}).keys())

        # From staged
        endpoint_names.update(staged.keys())

        # Staged deletions hide committed endpoints, unless the endpoint is
        # simultaneously restaged with a JSON definition.
        endpoint_names.difference_update(staged_deletions - set(staged.keys()))

        return sorted(endpoint_names)

    def get_endpoint(self, name: str) -> EndpointMeta:
        """
        Get metadata for a specific endpoint.

        Args:
            name: Endpoint name

        Returns:
            EndpointMeta with full DAG information
        """
        # Check staged first
        staged = self._load_staged_endpoints()
        staged_deletions = self._load_staged_endpoint_deletions()
        if name in staged:
            endpoint_data = staged[name]
        elif name in staged_deletions:
            raise KeyError(f"Endpoint '{name}' is staged for deletion")
        else:
            # Check committed
            commit = self._load_latest_commit()
            if not commit or name not in commit.get("endpoints", {}):
                raise KeyError(f"Endpoint '{name}' not found")
            endpoint_data = commit["endpoints"][name]

        # Parse nodes
        nodes = [
            PipelineNode(
                node_name=n["node_name"],
                transform_name=n["transform_name"],
                params=n.get("params", {}),
            )
            for n in endpoint_data.get("nodes", [])
        ]

        # Parse edges
        edges = [
            PipelineEdge(
                target_node=e["target_node"],
                input_name=e["input_name"],
                source_type=e["source_type"],
                source_ref=e["source_ref"],
                external_owner=e.get("external_owner"),
                external_project=e.get("external_project"),
                external_endpoint=e.get("external_endpoint"),
                external_commit_hash=e.get("external_commit_hash"),
            )
            for e in endpoint_data.get("edges", [])
        ]

        return EndpointMeta(
            name=name,
            nodes=nodes,
            edges=edges,
            data_sources=self.data_sources,
            transforms=self.transforms,
            hash=endpoint_data.get("dag_hash"),
        )

    def get_data_path(self, source_name: str) -> Path:
        """Get the full path to a data source file."""
        source = self.data_sources.get(source_name)
        if not source:
            raise KeyError(f"Data source '{source_name}' not found")
        return self.root / source.path

    def get_schema(self, source_name: str):
        """Get the Arrow schema of a data source."""
        path = self.get_data_path(source_name)
        return pq.read_schema(path)

    def meta(self) -> ProjectMeta:
        """Get full project metadata."""
        return ProjectMeta(
            name=self.name,
            owner=self.owner,
            root=str(self.root),
            head=self.head,
            data_sources=self.data_sources,
            transforms=self.transforms,
            endpoints=self.endpoints,
        )
