"""OzzyDB type definitions — dataclasses matching v2 server API responses."""

from __future__ import annotations

from dataclasses import dataclass, field, fields as dc_fields
from typing import Any


# ── Projects ─────────────────────────────────────────────────────


@dataclass
class RefInfo:
    name: str
    ref_type: str
    commit_sha: str | None


@dataclass
class CollaboratorInfo:
    username: str
    role: str


@dataclass
class ProjectInfo:
    owner: str
    slug: str
    description: str | None
    visibility: str
    created_at: str
    updated_at: str


@dataclass
class ProjectDetail:
    owner: str
    slug: str
    description: str | None
    visibility: str
    created_at: str
    updated_at: str
    commit_count: int
    refs: list[RefInfo]
    collaborators: list[CollaboratorInfo]


# ── Data Atoms ───────────────────────────────────────────────────


@dataclass
class DataAtom:
    id: str
    name: str
    hash: str
    content_type: str
    byte_size: int
    yanked: bool
    created_at: str


@dataclass
class DataAtomDetail:
    id: str
    name: str
    hash: str
    content_type: str
    byte_size: int
    yanked: bool
    created_at: str
    uploaded_by: str
    yank_reason: str | None
    yanked_at: str | None
    metadata: dict[str, Any]


@dataclass
class MetadataEntry:
    field: str
    value: Any
    set_by: str
    created_at: str


@dataclass
class UploadResult:
    name: str
    hash: str
    content_type: str
    byte_size: int
    deduplicated: bool
    collection_version: int | None


# ── Collections ──────────────────────────────────────────────────


@dataclass
class MemberInfo:
    member_type: str
    member_ref: str
    member_hash: str
    ordinal: int


@dataclass
class VersionDetail:
    version_number: int
    hash: str
    created_by: str
    created_at: str
    members: list[MemberInfo]


@dataclass
class VersionLogEntry:
    version_number: int
    hash: str
    created_by: str
    created_at: str


@dataclass
class CollectionInfo:
    id: str
    name: str
    yanked: bool
    created_at: str
    latest_version: int | None
    member_count: int | None


@dataclass
class CollectionDetail:
    id: str
    name: str
    yanked: bool
    yank_reason: str | None
    yanked_at: str | None
    created_at: str
    version: VersionDetail | None


@dataclass
class FlattenedAtom:
    name: str
    hash: str
    path: list[str]


# ── Endpoints ────────────────────────────────────────────────────


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
class NodeDetail:
    transform: str
    params: dict[str, Any]
    machine: str | None


@dataclass
class EdgeDetail:
    from_node: str  # server JSON key is "from" — Python reserved word
    to: str


@dataclass
class EndpointSummary:
    name: str
    description: str | None
    params: list[ParamSummary]
    node_count: int


@dataclass
class EndpointDetail:
    name: str
    description: str | None
    commit_sha: str
    params: list[ParamDetail]
    nodes: dict[str, NodeDetail]
    edges: list[EdgeDetail]


@dataclass
class DagResponse:
    format: str
    content: str


# ── Commits ──────────────────────────────────────────────────────


@dataclass
class CommitSummary:
    id: str
    git_commit_sha: str
    git_provider: str
    git_repo: str
    message: str | None
    pushed_by: str
    created_at: str


@dataclass
class CommitDetail:
    id: str
    git_commit_sha: str
    git_provider: str
    git_repo: str
    message: str | None
    pushed_by: str
    created_at: str
    ozzy_toml_hash: str
    environments: dict[str, Any]
    transforms: dict[str, Any]
    endpoints: dict[str, Any]


# ── Secrets ──────────────────────────────────────────────────────


@dataclass
class SecretInfo:
    name: str
    version_id: str
    created_at: str
    updated_at: str


# ── Fetch Metadata ───────────────────────────────────────────────


@dataclass
class FetchMetadata:
    """Metadata extracted from fetch response headers."""

    hash: str | None = None
    cache: str | None = None
    verification: str | None = None
    content_type: str = "application/octet-stream"


# ── Deserialization helpers ──────────────────────────────────────


def _from_dict(cls, data: dict) -> Any:
    """Create a dataclass instance from a dict, handling nested types."""
    if cls is ProjectDetail:
        return ProjectDetail(
            owner=data["owner"],
            slug=data["slug"],
            description=data.get("description"),
            visibility=data["visibility"],
            created_at=data["created_at"],
            updated_at=data["updated_at"],
            commit_count=data["commit_count"],
            refs=[_from_dict(RefInfo, r) for r in data.get("refs", [])],
            collaborators=[
                _from_dict(CollaboratorInfo, c)
                for c in data.get("collaborators", [])
            ],
        )
    if cls is EndpointDetail:
        return EndpointDetail(
            name=data["name"],
            description=data.get("description"),
            commit_sha=data["commit_sha"],
            params=[_from_dict(ParamDetail, p) for p in data.get("params", [])],
            nodes={
                k: _from_dict(NodeDetail, v)
                for k, v in data.get("nodes", {}).items()
            },
            edges=[_from_dict(EdgeDetail, e) for e in data.get("edges", [])],
        )
    if cls is EndpointSummary:
        return EndpointSummary(
            name=data["name"],
            description=data.get("description"),
            params=[_from_dict(ParamSummary, p) for p in data.get("params", [])],
            node_count=data["node_count"],
        )
    if cls is CollectionDetail:
        version = data.get("version")
        return CollectionDetail(
            id=data["id"],
            name=data["name"],
            yanked=data["yanked"],
            yank_reason=data.get("yank_reason"),
            yanked_at=data.get("yanked_at"),
            created_at=data["created_at"],
            version=_from_dict(VersionDetail, version) if version else None,
        )
    if cls is VersionDetail:
        return VersionDetail(
            version_number=data["version_number"],
            hash=data["hash"],
            created_by=data["created_by"],
            created_at=data["created_at"],
            members=[
                _from_dict(MemberInfo, m) for m in data.get("members", [])
            ],
        )
    if cls is EdgeDetail:
        # Server JSON uses "from" which is a Python reserved word
        return EdgeDetail(from_node=data["from"], to=data["to"])
    if cls is CommitDetail:
        return CommitDetail(
            id=data["id"],
            git_commit_sha=data["git_commit_sha"],
            git_provider=data["git_provider"],
            git_repo=data["git_repo"],
            message=data.get("message"),
            pushed_by=data["pushed_by"],
            created_at=data["created_at"],
            ozzy_toml_hash=data["ozzy_toml_hash"],
            environments=data.get("environments", {}),
            transforms=data.get("transforms", {}),
            endpoints=data.get("endpoints", {}),
        )
    # Simple dataclasses — construct from dict, filtering to known fields
    known = {f.name for f in dc_fields(cls)}
    return cls(**{k: v for k, v in data.items() if k in known})
