"""Tests for OzzyDB v4 type deserialization."""

from ozzydb.types import (
    ArtifactConformance,
    ArtifactDetail,
    ArtifactManifest,
    ConformanceRecordDetail,
    EdgeDetail,
    EndpointDetail,
    EndpointSummary,
    TypeRefDetail,
    TypedPortDetail,
    _from_dict,
)


class TestFromDict:
    def test_endpoint_detail(self):
        data = {
            "name": "corrected",
            "description": "Apply QC corrections",
            "commit_sha": "abc123def456",
            "project_revision_id": "proj-rev-1",
            "registry_revision_id": "reg-rev-1",
            "terminal_node": "qc",
            "inputs": [{
                "name": "raw",
                "description": None,
                "type": {
                    "reference": "RawCsv@1",
                    "name": "RawCsv",
                    "version": "1",
                    "type_version_id": "type-1",
                    "canonical_type_key": "ct-1",
                    "expr": {"kind": "named"},
                },
            }],
            "params": [],
            "nodes": [{
                "name": "qc",
                "params": {},
                "transform": {
                    "authored_name": "apply_qc",
                    "versioned_name": "apply_qc@1",
                    "transform_version_id": "tx-1",
                    "description": None,
                    "source": None,
                    "command": None,
                    "network": False,
                    "secrets": [],
                    "environment": {
                        "versioned_name": "default@1",
                        "environment_version_id": "env-1",
                    },
                    "inputs": [],
                    "outputs": [],
                },
            }],
            "edges": [{"from": "input:raw", "to": "qc.raw"}],
        }
        result = _from_dict(EndpointDetail, data)
        assert result.project_revision_id == "proj-rev-1"
        assert result.inputs[0].type_ref.reference == "RawCsv@1"
        assert result.nodes[0].transform.versioned_name == "apply_qc@1"
        assert result.edges[0].from_ == "input:raw"

    def test_endpoint_summary(self):
        data = {
            "name": "filtered",
            "description": None,
            "inputs": [],
            "params": [],
            "node_count": 3,
            "edge_count": 2,
            "terminal_node": "sink",
        }
        result = _from_dict(EndpointSummary, data)
        assert result.name == "filtered"
        assert result.edge_count == 2
        assert result.terminal_node == "sink"

    def test_artifact_detail_bundle_manifest(self):
        data = {
            "id": "artifact-1",
            "artifact_kind": "manifest",
            "content_hash": None,
            "manifest": {
                "kind": "bundle",
                "entries": {"raw": {"artifact_id": "artifact-raw"}},
            },
            "source_invocation_id": None,
            "created_at": "2026-01-01T00:00:00Z",
        }
        result = _from_dict(ArtifactDetail, data)
        assert result.manifest is not None
        assert result.manifest.kind == "bundle"
        assert result.manifest.entries["raw"].artifact_id == "artifact-raw"

    def test_artifact_conformance(self):
        data = {
            "artifact_id": "artifact-1",
            "records": [{
                "id": "conf-1",
                "status": "verified",
                "type_version": {
                    "id": "type-1",
                    "name": "RawCsv",
                    "version": "1",
                    "canonical_type_key": "ct-1",
                    "expr": {"kind": "named"},
                },
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:01Z",
                "attempts": [],
            }],
        }
        result = _from_dict(ArtifactConformance, data)
        assert result.artifact_id == "artifact-1"
        assert result.records[0].status == "verified"
        assert result.records[0].type_version.name == "RawCsv"

    def test_edge_detail_maps_reserved_from_key(self):
        result = _from_dict(EdgeDetail, {"from": "input:raw", "to": "qc.raw"})
        assert result.from_ == "input:raw"
        assert result.to == "qc.raw"

    def test_simple_type_ref(self):
        result = _from_dict(TypeRefDetail, {
            "reference": "RawCsv@1",
            "name": "RawCsv",
            "version": "1",
            "type_version_id": "type-1",
            "canonical_type_key": "ct-1",
            "expr": {"kind": "named"},
        })
        assert result.reference == "RawCsv@1"
        assert result.version == "1"

    def test_typed_port_detail(self):
        result = _from_dict(TypedPortDetail, {
            "name": "raw",
            "description": "Input dataset",
            "type": {
                "reference": "RawCsv@1",
                "name": "RawCsv",
                "version": "1",
                "type_version_id": "type-1",
                "canonical_type_key": "ct-1",
                "expr": {"kind": "named"},
            },
        })
        assert result.name == "raw"
        assert result.type_ref.reference == "RawCsv@1"
