"""Tests for OzzyDB type deserialization."""

from ozzydb.types import (
    CollaboratorInfo,
    CommitDetail,
    EdgeDetail,
    EndpointDetail,
    EndpointSummary,
    NodeDetail,
    ParamDetail,
    ParamSummary,
    ProjectDetail,
    RefInfo,
    UploadResult,
    VersionDetail,
    MemberInfo,
    CollectionDetail,
    _from_dict,
)


class TestFromDict:
    def test_simple_dataclass(self):
        data = {"username": "alice", "role": "admin"}
        result = _from_dict(CollaboratorInfo, data)
        assert result.username == "alice"
        assert result.role == "admin"

    def test_project_detail(self):
        data = {
            "owner": "alice",
            "slug": "my-project",
            "description": "A test project",
            "visibility": "public",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
            "commit_count": 5,
            "refs": [
                {"name": "main", "ref_type": "branch", "commit_sha": "abc123"},
                {"name": "v1.0", "ref_type": "tag", "commit_sha": "def456"},
            ],
            "collaborators": [
                {"username": "bob", "role": "write"},
            ],
        }
        result = _from_dict(ProjectDetail, data)
        assert result.owner == "alice"
        assert result.commit_count == 5
        assert len(result.refs) == 2
        assert result.refs[0].name == "main"
        assert result.refs[0].ref_type == "branch"
        assert result.collaborators[0].username == "bob"

    def test_endpoint_detail(self):
        data = {
            "name": "corrected",
            "description": "Apply QC corrections",
            "commit_sha": "abc123def456",
            "params": [
                {
                    "name": "threshold",
                    "type": "float",
                    "description": "QC threshold",
                    "default": 50.0,
                    "min": 0.0,
                    "max": 100.0,
                    "enum": None,
                    "binds": "qc_node.threshold",
                }
            ],
            "nodes": {
                "qc_node": {
                    "transform": "apply_qc",
                    "params": {"threshold": 50.0},
                    "machine": None,
                }
            },
            "edges": [
                {"from": "data:raw", "to": "qc_node.main"},
            ],
        }
        result = _from_dict(EndpointDetail, data)
        assert result.name == "corrected"
        assert len(result.params) == 1
        assert result.params[0].min == 0.0
        assert result.params[0].binds == "qc_node.threshold"
        assert "qc_node" in result.nodes
        assert result.nodes["qc_node"].transform == "apply_qc"
        assert len(result.edges) == 1
        assert result.edges[0].from_node == "data:raw"
        assert result.edges[0].to == "qc_node.main"

    def test_endpoint_summary(self):
        data = {
            "name": "filtered",
            "description": None,
            "params": [
                {"name": "limit", "type": "int", "description": None, "default": 100}
            ],
            "node_count": 3,
        }
        result = _from_dict(EndpointSummary, data)
        assert result.name == "filtered"
        assert result.node_count == 3
        assert result.params[0].name == "limit"

    def test_edge_detail_from_reserved_word(self):
        """Server JSON uses 'from' which is a Python reserved word."""
        data = {"from": "data:raw", "to": "node.input"}
        result = _from_dict(EdgeDetail, data)
        assert result.from_node == "data:raw"
        assert result.to == "node.input"

    def test_commit_detail(self):
        data = {
            "id": "uuid-123",
            "git_commit_sha": "abc123",
            "git_provider": "github",
            "git_repo": "alice/my-project",
            "message": "Initial commit",
            "pushed_by": "alice",
            "created_at": "2026-01-01T00:00:00Z",
            "ozzy_toml_hash": "hash123",
            "environments": {"default": {"base": "python:3.12"}},
            "transforms": {"apply_qc": {}},
            "endpoints": {"corrected": {}},
        }
        result = _from_dict(CommitDetail, data)
        assert result.git_provider == "github"
        assert result.ozzy_toml_hash == "hash123"
        assert "default" in result.environments

    def test_upload_result(self):
        data = {
            "name": "raw",
            "hash": "abc123",
            "content_type": "application/vnd.apache.parquet",
            "byte_size": 1024,
            "deduplicated": False,
            "collection_version": None,
        }
        result = _from_dict(UploadResult, data)
        assert result.name == "raw"
        assert not result.deduplicated
        assert result.collection_version is None

    def test_collection_detail_with_version(self):
        data = {
            "id": "uuid-456",
            "name": "my-collection",
            "yanked": False,
            "yank_reason": None,
            "yanked_at": None,
            "created_at": "2026-01-01T00:00:00Z",
            "version": {
                "version_number": 3,
                "hash": "vhash789",
                "created_by": "alice",
                "created_at": "2026-01-03T00:00:00Z",
                "members": [
                    {
                        "member_type": "data",
                        "member_ref": "raw",
                        "member_hash": "mhash123",
                        "ordinal": 0,
                    }
                ],
            },
        }
        result = _from_dict(CollectionDetail, data)
        assert result.name == "my-collection"
        assert result.version is not None
        assert result.version.version_number == 3
        assert len(result.version.members) == 1
        assert result.version.members[0].member_type == "data"

    def test_extra_keys_ignored(self):
        """Server adding new fields should not crash the client."""
        data = {"username": "alice", "role": "admin", "new_field": "surprise"}
        result = _from_dict(CollaboratorInfo, data)
        assert result.username == "alice"
        assert result.role == "admin"

    def test_collection_detail_without_version(self):
        data = {
            "id": "uuid-456",
            "name": "empty-collection",
            "yanked": False,
            "yank_reason": None,
            "yanked_at": None,
            "created_at": "2026-01-01T00:00:00Z",
            "version": None,
        }
        result = _from_dict(CollectionDetail, data)
        assert result.version is None
