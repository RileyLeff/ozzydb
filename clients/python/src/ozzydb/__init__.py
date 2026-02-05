"""
OzzyDB Python Client

Version control for data transformations.

Usage:
    import ozzydb as ozzy

    # Fetch data from a local project
    df = ozzy.fetch("./my-project/corrected")

    # Inspect project metadata
    meta = ozzy.inspect("./my-project/corrected")
    print(meta.schema)
    print(meta.dag)
"""

from ozzydb.client import fetch, fetch_lazy, inspect, inspect_project
from ozzydb.project import Project
from ozzydb.types import EndpointMeta, DataSourceMeta, TransformMeta, ProjectMeta

__version__ = "0.1.0"

__all__ = [
    "fetch",
    "fetch_lazy",
    "inspect",
    "inspect_project",
    "Project",
    "EndpointMeta",
    "DataSourceMeta",
    "TransformMeta",
    "ProjectMeta",
]
