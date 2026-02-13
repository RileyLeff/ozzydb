"""
OzzyDB Python Client

Data management platform for scientific computing.
"""

__version__ = "0.2.0"

from .http import OzzyApiError, OzzyClient
from .types import (
    CollaboratorInfo,
    CollectionDetail,
    CollectionInfo,
    CommitDetail,
    CommitSummary,
    DagResponse,
    DataAtom,
    DataAtomDetail,
    EdgeDetail,
    EndpointDetail,
    EndpointSummary,
    FetchMetadata,
    FlattenedAtom,
    MemberInfo,
    MetadataEntry,
    NodeDetail,
    ParamDetail,
    ParamSummary,
    ProjectDetail,
    ProjectInfo,
    RefInfo,
    SecretInfo,
    UploadResult,
    VersionDetail,
    VersionLogEntry,
)

__all__ = [
    # Client
    "OzzyClient",
    "OzzyApiError",
    # Types
    "CollaboratorInfo",
    "CollectionDetail",
    "CollectionInfo",
    "CommitDetail",
    "CommitSummary",
    "DagResponse",
    "DataAtom",
    "DataAtomDetail",
    "EdgeDetail",
    "EndpointDetail",
    "EndpointSummary",
    "FetchMetadata",
    "FlattenedAtom",
    "MemberInfo",
    "MetadataEntry",
    "NodeDetail",
    "ParamDetail",
    "ParamSummary",
    "ProjectDetail",
    "ProjectInfo",
    "RefInfo",
    "SecretInfo",
    "UploadResult",
    "VersionDetail",
    "VersionLogEntry",
]
