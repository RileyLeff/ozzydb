"""
Type definitions for OzzyDB Python client.
"""

from dataclasses import dataclass, field
from typing import Any, Optional
import pyarrow as pa


@dataclass
class DataSourceMeta:
    """Metadata about a data source."""

    name: str
    path: str
    hash: Optional[str] = None
    schema_hash: Optional[str] = None
    row_count: Optional[int] = None
    byte_size: Optional[int] = None
    schema: Optional[pa.Schema] = None


@dataclass
class TransformMeta:
    """Metadata about a transform."""

    name: str
    hash: str
    runtime: str
    source_path: str
    function_name: str
    lockfile_hash: str
    params_schema: dict[str, Any] = field(default_factory=dict)
    reproducible: bool = True
    input_schema: Optional[dict[str, Any]] = None
    output_schema: Optional[dict[str, Any]] = None


@dataclass
class PipelineNode:
    """A node in the endpoint's DAG."""

    node_name: str
    transform_name: str
    params: dict[str, Any] = field(default_factory=dict)


@dataclass
class PipelineEdge:
    """An edge in the endpoint's DAG."""

    target_node: str
    input_name: str
    source_type: str  # "data_source", "node", "external"
    source_ref: str
    external_owner: Optional[str] = None
    external_project: Optional[str] = None
    external_endpoint: Optional[str] = None
    external_commit_hash: Optional[str] = None


@dataclass
class EndpointMeta:
    """Metadata about an endpoint."""

    name: str
    nodes: list[PipelineNode]
    edges: list[PipelineEdge]
    data_sources: dict[str, DataSourceMeta]
    transforms: dict[str, TransformMeta]
    schema: Optional[pa.Schema] = None
    hash: Optional[str] = None

    @property
    def dag(self) -> str:
        """Return ASCII representation of the DAG."""
        lines = []
        lines.append(f"Endpoint: {self.name}")
        lines.append("")

        # Build node -> inputs mapping
        node_inputs: dict[str, list[str]] = {}
        for edge in self.edges:
            if edge.target_node not in node_inputs:
                node_inputs[edge.target_node] = []
            if edge.source_type == "data_source":
                node_inputs[edge.target_node].append(f"[{edge.source_ref}]")
            else:
                node_inputs[edge.target_node].append(edge.source_ref)

        # Print each node
        for node in self.nodes:
            inputs = node_inputs.get(node.node_name, [])
            inputs_str = ", ".join(inputs) if inputs else "?"
            lines.append(f"  {inputs_str} -> {node.node_name} ({node.transform_name})")

        return "\n".join(lines)


@dataclass
class ProjectMeta:
    """Metadata about a project."""

    name: str
    owner: str
    root: str
    head: str
    data_sources: dict[str, DataSourceMeta]
    transforms: dict[str, TransformMeta]
    endpoints: list[str]
