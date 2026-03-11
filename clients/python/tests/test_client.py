"""Tests for OzzyDB Python client functions."""

import io
from unittest.mock import MagicMock, patch

import polars as pl
import pytest

from ozzydb.client import (
    _infer_content_type,
    _parse_project_ref,
    _parse_remote_ref,
    _read_output,
    create_bundle_artifact,
    create_collection_artifact,
    declare_conformance,
    download_artifact,
    fetch,
    fetch_lazy,
    get_artifact,
    get_artifact_conformance,
    inspect,
    list_artifacts,
    list_endpoints,
    upload_artifact,
)
from ozzydb.http import OzzyApiError, OzzyClient


class TestRefParsing:
    def test_parse_remote_ref(self):
        owner, project, endpoint = _parse_remote_ref("alice/my-project/corrected")
        assert owner == "alice"
        assert project == "my-project"
        assert endpoint == "corrected"

    def test_parse_remote_ref_invalid(self):
        with pytest.raises(ValueError, match="owner/project/endpoint"):
            _parse_remote_ref("alice/proj")

    def test_parse_project_ref(self):
        owner, project = _parse_project_ref("alice/my-project")
        assert owner == "alice"
        assert project == "my-project"

    def test_parse_project_ref_invalid(self):
        with pytest.raises(ValueError, match="owner/project"):
            _parse_project_ref("alice/proj/extra")


class TestContentTypeHelpers:
    def test_infer_parquet(self):
        assert _infer_content_type("/tmp/data.parquet") == "application/vnd.apache.parquet"

    def test_infer_csv(self):
        assert _infer_content_type("/tmp/data.csv") == "text/csv"

    def test_read_parquet(self, tmp_path):
        df = pl.DataFrame({"a": [1, 2, 3]})
        path = tmp_path / "test.parquet"
        df.write_parquet(path)
        result = _read_output(str(path), "application/vnd.apache.parquet")
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 1)

    def test_read_binary_fallback(self, tmp_path):
        path = tmp_path / "test.bin"
        path.write_bytes(b"\x00\x01")
        assert _read_output(str(path), "application/octet-stream") == b"\x00\x01"


def _make_response(status_code: int = 200, *, content: bytes = b"", headers: dict | None = None, json_data=None):
    resp = MagicMock()
    resp.ok = 200 <= status_code < 400
    resp.status_code = status_code
    resp.headers = headers or {}
    resp.content = content
    resp.reason = "OK" if resp.ok else "Error"
    if json_data is not None:
        resp.json.return_value = json_data
        resp.headers.setdefault("content-length", str(len(str(json_data))))
    else:
        resp.headers.setdefault("content-length", str(len(content)))
    resp.iter_content = MagicMock(return_value=[content])
    return resp


def _mock_fetch_sequence(mock_client, output_bytes, content_type, cache_hit=True):
    job_id = "test-job-123"
    fetch_resp = _make_response(
        json_data={
            "job_id": job_id,
            "status": "done" if cache_hit else "queued",
            "output_url": f"/jobs/{job_id}/output" if cache_hit else None,
            "output_hash": None,
        }
    )
    output_resp = _make_response(content=output_bytes, headers={"content-type": content_type})
    if cache_hit:
        mock_client._session.request = MagicMock(side_effect=[fetch_resp, output_resp])
    else:
        poll_resp = _make_response(
            json_data={
                "status": "done",
                "node_status": {"step1": "done"},
                "output_hash": None,
                "error_message": None,
            }
        )
        mock_client._session.request = MagicMock(side_effect=[fetch_resp, poll_resp, output_resp])


class TestFetch:
    def test_fetch_parquet_cache_hit(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"id": [1, 2, 3]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        result = fetch("alice/proj/ep", client=mock_client)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 1)
        first_call = mock_client._session.request.call_args_list[0]
        assert first_call[0][0] == "POST"
        assert first_call[1]["json"] == {"params": {}, "inputs": {}}

    def test_fetch_with_params_and_inputs(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        fetch(
            "alice/proj/ep",
            client=mock_client,
            inputs={"raw": "123e4567-e89b-12d3-a456-426614174000"},
            threshold=50.0,
            species="oak",
        )

        payload = mock_client._session.request.call_args_list[0][1]["json"]
        assert payload["params"]["threshold"] == 50.0
        assert payload["params"]["species"] == "oak"
        assert payload["inputs"]["raw"] == "123e4567-e89b-12d3-a456-426614174000"

    def test_fetch_with_ref(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")
        fetch("alice/proj/ep", client=mock_client, ref_name="v1.0")
        payload = mock_client._session.request.call_args_list[0][1]["json"]
        assert payload["ref"] == "v1.0"

    def test_fetch_poll_until_done(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1, 2]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet", cache_hit=False)
        result = fetch("alice/proj/ep", client=mock_client, poll_interval=0.01)
        assert isinstance(result, pl.DataFrame)
        assert len(mock_client._session.request.call_args_list) == 3

    def test_fetch_job_error(self, mock_client):
        fetch_resp = _make_response(json_data={"job_id": "job-err", "status": "queued"})
        poll_resp = _make_response(
            json_data={
                "status": "failed",
                "node_status": {"step1": "running"},
                "output_hash": None,
                "error_message": "Transform crashed",
            }
        )
        mock_client._session.request = MagicMock(side_effect=[fetch_resp, poll_resp])
        with pytest.raises(RuntimeError, match="Transform crashed"):
            fetch("alice/proj/ep", client=mock_client, poll_interval=0.01)

    def test_fetch_auth_error(self, mock_client):
        mock_resp = MagicMock()
        mock_resp.ok = False
        mock_resp.status_code = 401
        mock_resp.reason = "Unauthorized"
        mock_resp.json.return_value = {"error": "unauthorized", "message": "Invalid token"}
        mock_client._session.request = MagicMock(return_value=mock_resp)
        with pytest.raises(OzzyApiError, match="unauthorized"):
            fetch("alice/proj/ep", client=mock_client)


class TestFetchLazy:
    def test_fetch_lazy_parquet(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1, 2, 3]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")
        result = fetch_lazy("alice/proj/ep", client=mock_client)
        assert isinstance(result, pl.LazyFrame)
        assert result.collect().shape == (3, 1)


class TestInspect:
    def test_inspect_endpoint(self, mock_client):
        endpoint_data = {
            "name": "corrected",
            "description": "QC corrections",
            "commit_sha": "abc123",
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
                    "source": "transforms/apply_qc.py:apply_qc",
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
        mock_client._session.request = MagicMock(return_value=_make_response(json_data=endpoint_data))
        result = inspect("alice/proj/corrected", client=mock_client)
        assert result.name == "corrected"
        assert result.project_revision_id == "proj-rev-1"
        assert result.nodes[0].transform.versioned_name == "apply_qc@1"
        assert result.edges[0].from_ == "input:raw"

    def test_list_endpoints(self, mock_client):
        data = [{
            "name": "corrected",
            "description": "QC corrections",
            "inputs": [],
            "params": [],
            "node_count": 1,
            "edge_count": 0,
            "terminal_node": "qc",
        }]
        mock_client._session.request = MagicMock(return_value=_make_response(json_data=data))
        result = list_endpoints("alice/proj", client=mock_client)
        assert len(result) == 1
        assert result[0].terminal_node == "qc"


class TestArtifacts:
    def test_upload_artifact(self, mock_client, tmp_path):
        test_file = tmp_path / "data.parquet"
        pl.DataFrame({"x": [1, 2, 3]}).write_parquet(test_file)
        mock_client._session.request = MagicMock(return_value=_make_response(json_data={
            "artifact_id": "artifact-1",
            "content_hash": "abc123",
            "content_type": "application/vnd.apache.parquet",
            "byte_size": test_file.stat().st_size,
            "deduplicated": False,
            "created_at": "2026-01-01T00:00:00Z",
        }))
        result = upload_artifact("alice/proj", test_file, client=mock_client)
        assert result.artifact_id == "artifact-1"
        assert result.content_hash == "abc123"

    def test_upload_artifact_missing_file(self, mock_client, tmp_path):
        with pytest.raises(FileNotFoundError):
            upload_artifact("alice/proj", tmp_path / "missing.parquet", client=mock_client)

    def test_list_artifacts(self, mock_client):
        mock_client._session.request = MagicMock(return_value=_make_response(json_data=[{
            "id": "artifact-1",
            "artifact_kind": "blob",
            "content_hash": "abc123",
            "source_invocation_id": None,
            "created_at": "2026-01-01T00:00:00Z",
        }]))
        result = list_artifacts("alice/proj", client=mock_client)
        assert len(result) == 1
        assert result[0].artifact_kind == "blob"

    def test_get_artifact(self, mock_client):
        mock_client._session.request = MagicMock(return_value=_make_response(json_data={
            "id": "artifact-1",
            "artifact_kind": "manifest",
            "content_hash": None,
            "manifest": {
                "kind": "bundle",
                "entries": {"raw": {"artifact_id": "artifact-raw"}},
            },
            "source_invocation_id": None,
            "created_at": "2026-01-01T00:00:00Z",
        }))
        result = get_artifact("alice/proj", "artifact-1", client=mock_client)
        assert result.manifest is not None
        assert result.manifest.kind == "bundle"
        assert result.manifest.entries["raw"].artifact_id == "artifact-raw"

    def test_get_artifact_conformance(self, mock_client):
        mock_client._session.request = MagicMock(return_value=_make_response(json_data={
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
        }))
        result = get_artifact_conformance("alice/proj", "artifact-1", client=mock_client)
        assert result.artifact_id == "artifact-1"
        assert result.records[0].status == "verified"

    def test_declare_conformance(self, mock_client):
        mock_client._session.request = MagicMock(return_value=_make_response(json_data={
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
        }))
        result = declare_conformance("alice/proj", "artifact-1", "RawCsv@1", client=mock_client)
        assert result.status == "verified"
        payload = mock_client._session.request.call_args[1]["json"]
        assert payload == {"type": "RawCsv@1", "verify": True}

    def test_create_bundle_artifact(self, mock_client):
        mock_client._session.request = MagicMock(return_value=_make_response(json_data={
            "id": "artifact-1",
            "artifact_kind": "manifest",
            "content_hash": None,
            "manifest": {"kind": "bundle", "entries": {}},
            "source_invocation_id": None,
            "created_at": "2026-01-01T00:00:00Z",
        }))
        result = create_bundle_artifact("alice/proj", {"raw": "artifact-raw"}, client=mock_client)
        assert result.artifact_kind == "manifest"
        payload = mock_client._session.request.call_args[1]["json"]
        assert payload["kind"] == "bundle"
        assert payload["entries"]["raw"]["artifact_id"] == "artifact-raw"

    def test_create_collection_artifact(self, mock_client):
        mock_client._session.request = MagicMock(return_value=_make_response(json_data={
            "id": "artifact-1",
            "artifact_kind": "manifest",
            "content_hash": None,
            "manifest": {"kind": "collection", "items": []},
            "source_invocation_id": None,
            "created_at": "2026-01-01T00:00:00Z",
        }))
        result = create_collection_artifact("alice/proj", ["artifact-a", "artifact-b"], client=mock_client)
        assert result.artifact_kind == "manifest"
        payload = mock_client._session.request.call_args[1]["json"]
        assert payload["kind"] == "collection"
        assert payload["items"][0]["artifact_id"] == "artifact-a"

    def test_download_artifact_dataframe(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1, 2, 3]})
        redirect = _make_response(status_code=302, headers={
            "location": "https://storage.example.com/object",
            "X-OzzyDB-Content-Type": "application/vnd.apache.parquet",
        })
        mock_client._session.request = MagicMock(return_value=redirect)
        final_resp = _make_response(content=parquet_bytes, headers={"content-type": "application/vnd.apache.parquet"})
        with patch("ozzydb.client.req_lib.get", return_value=final_resp):
            result = download_artifact("alice/proj", "artifact-1", client=mock_client)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 1)


class TestAuth:
    def test_load_credentials(self, credentials_dir):
        with patch.object(OzzyClient, "_credentials_path", staticmethod(lambda: credentials_dir)):
            client = OzzyClient()
        assert client.base_url == "https://custom.ozzydb.com"
        assert client.authenticated

    def test_no_credentials(self, tmp_path):
        fake_creds = tmp_path / "nonexistent" / "credentials.json"
        with patch.object(OzzyClient, "_credentials_path", staticmethod(lambda: fake_creds)):
            client = OzzyClient()
        assert client.base_url == "https://api.ozzydb.com"
        assert not client.authenticated


def _make_parquet_bytes(data: dict) -> bytes:
    df = pl.DataFrame(data)
    buf = io.BytesIO()
    df.write_parquet(buf)
    return buf.getvalue()
