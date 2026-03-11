"""OzzyDB Python client types for the v4 API."""

from __future__ import annotations

from dataclasses import dataclass, fields as dc_fields
from typing import Any


@dataclass
class ArtifactManifestEntry:
    artifact_id: str


@dataclass
class ArtifactManifest:
    kind: str
    entries: dict[str, ArtifactManifestEntry] | None = None
    items: list[ArtifactManifestEntry] | None = None


@dataclass
class UploadArtifactResult:
    artifact_id: str
    content_hash: str
    content_type: str
    byte_size: int
    deduplicated: bool
    created_at: str | None = None


@dataclass
class ArtifactSummary:
    id: str
    artifact_kind: str
    content_hash: str | None
    source_invocation_id: str | None
    created_at: str


@dataclass
class ArtifactDetail:
    id: str
    artifact_kind: str
    content_hash: str | None
    manifest: ArtifactManifest | None
    source_invocation_id: str | None
    created_at: str


@dataclass
class TypeRefDetail:
    reference: str
    name: str
    version: str
    type_version_id: str
    canonical_type_key: str
    expr: Any


@dataclass
class TypedPortDetail:
    name: str
    description: str | None
    type_ref: TypeRefDetail


@dataclass
class ParamSummary:
    name: str
    type: str
    description: str | None
    default: Any


@dataclass
class ParamDetail:
    name: str
    type: str
    description: str | None
    default: Any
    min: float | None
    max: float | None
    enum: list[Any] | None
    binds: str


@dataclass
class TransformEnvironmentRef:
    versioned_name: str
    environment_version_id: str


@dataclass
class TransformInspection:
    authored_name: str
    versioned_name: str
    transform_version_id: str
    description: str | None
    source: str | None
    command: str | None
    network: bool
    secrets: list[str]
    environment: TransformEnvironmentRef
    inputs: list[TypedPortDetail]
    outputs: list[TypedPortDetail]


@dataclass
class NodeDetail:
    name: str
    params: dict[str, Any]
    transform: TransformInspection


@dataclass
class EdgeDetail:
    from_: str
    to: str


@dataclass
class EndpointSummary:
    name: str
    description: str | None
    inputs: list[TypedPortDetail]
    params: list[ParamSummary]
    node_count: int
    edge_count: int
    terminal_node: str


@dataclass
class EndpointDetail:
    name: str
    description: str | None
    commit_sha: str
    project_revision_id: str
    registry_revision_id: str
    terminal_node: str
    inputs: list[TypedPortDetail]
    params: list[ParamDetail]
    nodes: list[NodeDetail]
    edges: list[EdgeDetail]


@dataclass
class DagResponse:
    format: str
    content: str


@dataclass
class TypeVersionDetail:
    id: str
    name: str
    version: str
    canonical_type_key: str
    expr: Any
    created_at: str | None = None


@dataclass
class VerificationAttemptDetail:
    id: str
    verifier: str
    attempt_kind: str
    verdict: str | None
    diagnostics: Any
    evidence: Any | None
    failure_error: str | None
    created_at: str


@dataclass
class ConformanceRecordDetail:
    id: str
    status: str
    type_version: TypeVersionDetail
    created_at: str
    updated_at: str
    attempts: list[VerificationAttemptDetail]


@dataclass
class ArtifactConformance:
    artifact_id: str
    records: list[ConformanceRecordDetail]


def _from_dict(cls, data: dict[str, Any]) -> Any:
    """Create a typed client object from API JSON."""
    if cls is ArtifactManifestEntry:
        return ArtifactManifestEntry(artifact_id=data["artifact_id"])
    if cls is ArtifactManifest:
        kind = data["kind"]
        if kind == "bundle":
            return ArtifactManifest(
                kind=kind,
                entries={
                    name: _from_dict(ArtifactManifestEntry, entry)
                    for name, entry in data.get("entries", {}).items()
                },
            )
        if kind == "collection":
            return ArtifactManifest(
                kind=kind,
                items=[
                    _from_dict(ArtifactManifestEntry, item)
                    for item in data.get("items", [])
                ],
            )
        return ArtifactManifest(kind=kind)
    if cls is ArtifactDetail:
        manifest = data.get("manifest")
        return ArtifactDetail(
            id=data["id"],
            artifact_kind=data["artifact_kind"],
            content_hash=data.get("content_hash"),
            manifest=_from_dict(ArtifactManifest, manifest) if manifest else None,
            source_invocation_id=data.get("source_invocation_id"),
            created_at=data["created_at"],
        )
    if cls is ArtifactSummary:
        return ArtifactSummary(**{k: data.get(k) for k in dc_fields_dict(ArtifactSummary)})
    if cls is TypeRefDetail:
        return TypeRefDetail(
            reference=data["reference"],
            name=data["name"],
            version=data["version"],
            type_version_id=data["type_version_id"],
            canonical_type_key=data["canonical_type_key"],
            expr=data.get("expr"),
        )
    if cls is TypedPortDetail:
        return TypedPortDetail(
            name=data["name"],
            description=data.get("description"),
            type_ref=_from_dict(TypeRefDetail, data["type"]),
        )
    if cls is ParamSummary:
        return ParamSummary(
            name=data["name"],
            type=data["type"],
            description=data.get("description"),
            default=data.get("default"),
        )
    if cls is ParamDetail:
        return ParamDetail(
            name=data["name"],
            type=data["type"],
            description=data.get("description"),
            default=data.get("default"),
            min=data.get("min"),
            max=data.get("max"),
            enum=data.get("enum"),
            binds=data["binds"],
        )
    if cls is TransformEnvironmentRef:
        return TransformEnvironmentRef(
            versioned_name=data["versioned_name"],
            environment_version_id=data["environment_version_id"],
        )
    if cls is TransformInspection:
        return TransformInspection(
            authored_name=data["authored_name"],
            versioned_name=data["versioned_name"],
            transform_version_id=data["transform_version_id"],
            description=data.get("description"),
            source=data.get("source"),
            command=data.get("command"),
            network=data["network"],
            secrets=list(data.get("secrets", [])),
            environment=_from_dict(TransformEnvironmentRef, data["environment"]),
            inputs=[_from_dict(TypedPortDetail, port) for port in data.get("inputs", [])],
            outputs=[_from_dict(TypedPortDetail, port) for port in data.get("outputs", [])],
        )
    if cls is NodeDetail:
        return NodeDetail(
            name=data["name"],
            params=data.get("params", {}),
            transform=_from_dict(TransformInspection, data["transform"]),
        )
    if cls is EdgeDetail:
        return EdgeDetail(from_=data["from"], to=data["to"])
    if cls is EndpointSummary:
        return EndpointSummary(
            name=data["name"],
            description=data.get("description"),
            inputs=[_from_dict(TypedPortDetail, port) for port in data.get("inputs", [])],
            params=[_from_dict(ParamSummary, param) for param in data.get("params", [])],
            node_count=data["node_count"],
            edge_count=data["edge_count"],
            terminal_node=data["terminal_node"],
        )
    if cls is EndpointDetail:
        return EndpointDetail(
            name=data["name"],
            description=data.get("description"),
            commit_sha=data["commit_sha"],
            project_revision_id=data["project_revision_id"],
            registry_revision_id=data["registry_revision_id"],
            terminal_node=data["terminal_node"],
            inputs=[_from_dict(TypedPortDetail, port) for port in data.get("inputs", [])],
            params=[_from_dict(ParamDetail, param) for param in data.get("params", [])],
            nodes=[_from_dict(NodeDetail, node) for node in data.get("nodes", [])],
            edges=[_from_dict(EdgeDetail, edge) for edge in data.get("edges", [])],
        )
    if cls is TypeVersionDetail:
        return TypeVersionDetail(
            id=data["id"],
            name=data["name"],
            version=data["version"],
            canonical_type_key=data["canonical_type_key"],
            expr=data.get("expr"),
            created_at=data.get("created_at"),
        )
    if cls is VerificationAttemptDetail:
        return VerificationAttemptDetail(
            id=data["id"],
            verifier=data["verifier"],
            attempt_kind=data["attempt_kind"],
            verdict=data.get("verdict"),
            diagnostics=data.get("diagnostics"),
            evidence=data.get("evidence"),
            failure_error=data.get("failure_error"),
            created_at=data["created_at"],
        )
    if cls is ConformanceRecordDetail:
        return ConformanceRecordDetail(
            id=data["id"],
            status=data["status"],
            type_version=_from_dict(TypeVersionDetail, data["type_version"]),
            created_at=data["created_at"],
            updated_at=data["updated_at"],
            attempts=[
                _from_dict(VerificationAttemptDetail, attempt)
                for attempt in data.get("attempts", [])
            ],
        )
    if cls is ArtifactConformance:
        return ArtifactConformance(
            artifact_id=data["artifact_id"],
            records=[_from_dict(ConformanceRecordDetail, record) for record in data.get("records", [])],
        )
    if cls is UploadArtifactResult:
        return UploadArtifactResult(
            artifact_id=data["artifact_id"],
            content_hash=data["content_hash"],
            content_type=data["content_type"],
            byte_size=data["byte_size"],
            deduplicated=data["deduplicated"],
            created_at=data.get("created_at"),
        )
    known = {f.name for f in dc_fields(cls)}
    return cls(**{k: v for k, v in data.items() if k in known})


def dc_fields_dict(cls) -> dict[str, Any]:
    return {f.name: f for f in dc_fields(cls)}
