"""
OzzyDB Python Client

Data management platform for scientific computing.
"""

__version__ = "0.2.0"

from .client import (
    download,
    download_dataframe,
    fetch,
    fetch_lazy,
    inspect,
    inspect_project,
    run,
    upload,
)
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
    # Functions
    "fetch",
    "fetch_lazy",
    "run",
    "inspect",
    "inspect_project",
    "upload",
    "download",
    "download_dataframe",
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
