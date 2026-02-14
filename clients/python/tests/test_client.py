"""Tests for OzzyDB Python client functions."""

import io
import json
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch, PropertyMock

import polars as pl
import pytest

import ozzydb
from ozzydb.client import (
    _parse_remote_ref,
    _parse_project_ref,
    _read_output,
    _infer_content_type,
    fetch,
    fetch_lazy,
    inspect,
    inspect_project,
    run,
    upload,
    download,
    download_dataframe,
)
from ozzydb.http import OzzyApiError, OzzyClient


# ── Reference parsing ────────────────────────────────────────────


class TestRefParsing:
    def test_parse_remote_ref(self):
        owner, project, endpoint = _parse_remote_ref("alice/my-project/corrected")
        assert owner == "alice"
        assert project == "my-project"
        assert endpoint == "corrected"

    def test_parse_remote_ref_strips_slashes(self):
        owner, project, endpoint = _parse_remote_ref("/alice/proj/ep/")
        assert owner == "alice"
        assert project == "proj"
        assert endpoint == "ep"

    def test_parse_remote_ref_invalid_two_parts(self):
        with pytest.raises(ValueError, match="owner/project/endpoint"):
            _parse_remote_ref("alice/proj")

    def test_parse_remote_ref_invalid_one_part(self):
        with pytest.raises(ValueError, match="owner/project/endpoint"):
            _parse_remote_ref("alice")

    def test_parse_project_ref(self):
        owner, project = _parse_project_ref("alice/my-project")
        assert owner == "alice"
        assert project == "my-project"

    def test_parse_project_ref_invalid(self):
        with pytest.raises(ValueError, match="owner/project"):
            _parse_project_ref("alice/proj/extra")

    def test_parse_remote_ref_empty_component(self):
        with pytest.raises(ValueError, match="owner/project/endpoint"):
            _parse_remote_ref("alice//endpoint")

    def test_parse_project_ref_empty_component(self):
        with pytest.raises(ValueError, match="owner/project"):
            _parse_project_ref("alice/")


# ── Content type helpers ─────────────────────────────────────────


class TestContentTypeHelpers:
    def test_infer_parquet(self):
        assert _infer_content_type("/tmp/data.parquet") == "application/vnd.apache.parquet"

    def test_infer_csv(self):
        assert _infer_content_type("/tmp/data.csv") == "text/csv"

    def test_infer_unknown(self):
        assert _infer_content_type("/tmp/data.xyz") == "application/octet-stream"

    def test_read_parquet(self, tmp_path):
        df = pl.DataFrame({"a": [1, 2, 3], "b": [4.0, 5.0, 6.0]})
        path = tmp_path / "test.parquet"
        df.write_parquet(path)

        result = _read_output(str(path), "application/vnd.apache.parquet")
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 2)

    def test_read_csv(self, tmp_path):
        df = pl.DataFrame({"x": [10, 20], "y": ["a", "b"]})
        path = tmp_path / "test.csv"
        df.write_csv(path)

        result = _read_output(str(path), "text/csv")
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (2, 2)

    def test_read_parquet_as_pandas(self, tmp_path):
        import pandas as pd

        df = pl.DataFrame({"a": [1, 2, 3]})
        path = tmp_path / "test.parquet"
        df.write_parquet(path)

        result = _read_output(str(path), "application/vnd.apache.parquet", as_pandas=True)
        assert isinstance(result, pd.DataFrame)
        assert len(result) == 3

    def test_read_binary_fallback(self, tmp_path):
        path = tmp_path / "test.bin"
        path.write_bytes(b"\x00\x01\x02\x03")

        result = _read_output(str(path), "application/octet-stream")
        assert isinstance(result, bytes)
        assert result == b"\x00\x01\x02\x03"


# ── Mock response helper ────────────────────────────────────────


def _make_response(
    status_code: int = 200,
    content: bytes = b"",
    headers: dict | None = None,
    json_data: dict | None = None,
):
    """Create a mock requests.Response."""
    resp = MagicMock()
    resp.ok = 200 <= status_code < 300
    resp.status_code = status_code
    resp.headers = headers or {}
    resp.content = content
    resp.reason = "OK" if resp.ok else "Error"

    if json_data is not None:
        resp.json.return_value = json_data
        resp.headers.setdefault("content-length", str(len(json.dumps(json_data))))
    else:
        resp.headers.setdefault("content-length", str(len(content)))

    # For streaming
    resp.iter_content = MagicMock(return_value=[content])

    return resp


# ── Fetch tests ──────────────────────────────────────────────────


def _mock_fetch_sequence(mock_client, output_bytes, content_type, cache_hit=True):
    """Set up mock responses for the POST + GET /output fetch flow.

    For cache hits: POST returns done, then GET /output returns bytes.
    For cache misses: POST returns queued, GET /jobs/{id} returns done, then GET /output.
    """
    job_id = "test-job-123"

    # POST /fetch response
    fetch_resp = _make_response(
        json_data={
            "job_id": job_id,
            "status": "done" if cache_hit else "queued",
            "output_url": f"/v1/jobs/{job_id}/output" if cache_hit else None,
            "output_hash": "hash123" if cache_hit else None,
        }
    )

    # GET /jobs/{id}/output response
    output_resp = _make_response(
        content=output_bytes,
        headers={"content-type": content_type},
    )

    if cache_hit:
        # POST fetch, then GET output
        mock_client._session.request = MagicMock(side_effect=[fetch_resp, output_resp])
    else:
        # POST fetch, then GET /jobs/{id} poll, then GET output
        poll_resp = _make_response(
            json_data={
                "status": "done",
                "node_status": {"step1": "done"},
                "output_hash": "hash123",
                "error_message": None,
            }
        )
        mock_client._session.request = MagicMock(
            side_effect=[fetch_resp, poll_resp, output_resp]
        )


class TestFetch:
    def test_fetch_parquet_cache_hit(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"id": [1, 2, 3], "value": [10.0, 20.0, 30.0]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        result = fetch("alice/proj/ep", client=mock_client)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 2)
        assert result["id"].to_list() == [1, 2, 3]

        # Verify POST was used for the first request
        first_call = mock_client._session.request.call_args_list[0]
        assert first_call[0][0] == "POST"

    def test_fetch_csv(self, mock_client):
        csv_bytes = b"a,b\n1,2\n3,4\n"
        _mock_fetch_sequence(mock_client, csv_bytes, "text/csv")

        result = fetch("alice/proj/ep", client=mock_client)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (2, 2)

    def test_fetch_with_params(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        fetch("alice/proj/ep", client=mock_client, threshold=50.0, limit=10)

        # First call is POST /fetch with params
        first_call = mock_client._session.request.call_args_list[0]
        params = first_call[1].get("params", {})
        assert params["threshold"] == "50.0"
        assert params["limit"] == "10"

    def test_fetch_with_ref(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        fetch("alice/proj/ep", client=mock_client, ref_name="v1.0")

        first_call = mock_client._session.request.call_args_list[0]
        params = first_call[1].get("params", {})
        assert params["ref"] == "v1.0"

    def test_fetch_as_pandas(self, mock_client):
        import pandas as pd

        parquet_bytes = _make_parquet_bytes({"x": [1, 2, 3]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        result = fetch("alice/proj/ep", client=mock_client, as_pandas=True)
        assert isinstance(result, pd.DataFrame)

    def test_fetch_binary_fallback(self, mock_client):
        _mock_fetch_sequence(mock_client, b"\x00\x01\x02", "image/png")

        result = fetch("alice/proj/ep", client=mock_client)
        assert isinstance(result, bytes)

    def test_fetch_auth_error(self, mock_client):
        mock_resp = MagicMock()
        mock_resp.ok = False
        mock_resp.status_code = 401
        mock_resp.reason = "Unauthorized"
        mock_resp.json.return_value = {"error": "unauthorized", "message": "Invalid token"}

        mock_client._session.request = MagicMock(return_value=mock_resp)

        with pytest.raises(OzzyApiError, match="unauthorized"):
            fetch("alice/proj/ep", client=mock_client)

    def test_fetch_not_found(self, mock_client):
        mock_resp = MagicMock()
        mock_resp.ok = False
        mock_resp.status_code = 404
        mock_resp.reason = "Not Found"
        mock_resp.json.return_value = {"error": "not_found", "message": "Endpoint not found"}

        mock_client._session.request = MagicMock(return_value=mock_resp)

        with pytest.raises(OzzyApiError) as exc_info:
            fetch("alice/proj/ep", client=mock_client)
        assert exc_info.value.status == 404

    def test_fetch_poll_until_done(self, mock_client):
        """Test the poll loop: POST returns queued, poll returns done."""
        parquet_bytes = _make_parquet_bytes({"x": [1, 2]})
        _mock_fetch_sequence(
            mock_client, parquet_bytes, "application/vnd.apache.parquet",
            cache_hit=False,
        )

        result = fetch("alice/proj/ep", client=mock_client, poll_interval=0.01)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (2, 1)

        # Verify: POST, GET poll, GET output
        assert len(mock_client._session.request.call_args_list) == 3

    def test_fetch_job_error(self, mock_client):
        """Test that job errors are raised as RuntimeError."""
        fetch_resp = _make_response(
            json_data={"job_id": "job-err", "status": "queued", "output_url": None, "output_hash": None}
        )
        poll_resp = _make_response(
            json_data={
                "status": "error",
                "node_status": {"step1": "running"},
                "output_hash": None,
                "error_message": "Transform crashed",
            }
        )
        mock_client._session.request = MagicMock(side_effect=[fetch_resp, poll_resp])

        with pytest.raises(RuntimeError, match="Transform crashed"):
            fetch("alice/proj/ep", client=mock_client, poll_interval=0.01)


class TestFetchLazy:
    def test_fetch_lazy_parquet(self, mock_client):
        parquet_bytes = _make_parquet_bytes({"x": [1, 2, 3], "y": [4.0, 5.0, 6.0]})
        _mock_fetch_sequence(mock_client, parquet_bytes, "application/vnd.apache.parquet")

        result = fetch_lazy("alice/proj/ep", client=mock_client)
        assert isinstance(result, pl.LazyFrame)

        df = result.collect()
        assert df.shape == (3, 2)


# ── Inspect tests ────────────────────────────────────────────────


class TestInspect:
    def test_inspect_endpoint(self, mock_client):
        endpoint_data = {
            "name": "corrected",
            "description": "QC corrections",
            "commit_sha": "abc123",
            "params": [
                {
                    "name": "threshold",
                    "type": "float",
                    "description": "QC threshold",
                    "default": 50.0,
                    "min": 0.0,
                    "max": 100.0,
                    "enum": None,
                    "binds": "qc.threshold",
                }
            ],
            "nodes": {
                "qc": {"transform": "apply_qc", "params": {}, "machine": None}
            },
            "edges": [{"from": "data:raw", "to": "qc.main"}],
        }
        mock_resp = _make_response(json_data=endpoint_data)
        mock_client._session.request = MagicMock(return_value=mock_resp)

        result = inspect("alice/proj/corrected", client=mock_client)
        assert result.name == "corrected"
        assert len(result.params) == 1
        assert result.params[0].min == 0.0
        assert result.edges[0].from_node == "data:raw"

    def test_inspect_project(self, mock_client):
        project_data = {
            "owner": "alice",
            "slug": "my-proj",
            "description": "A project",
            "visibility": "public",
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-02T00:00:00Z",
            "commit_count": 10,
            "refs": [
                {"name": "main", "ref_type": "branch", "commit_sha": "abc123"}
            ],
            "collaborators": [],
        }
        mock_resp = _make_response(json_data=project_data)
        mock_client._session.request = MagicMock(return_value=mock_resp)

        result = inspect_project("alice/my-proj", client=mock_client)
        assert result.owner == "alice"
        assert result.commit_count == 10
        assert len(result.refs) == 1


# ── Upload tests ─────────────────────────────────────────────────


class TestUpload:
    def test_upload_file(self, mock_client, tmp_path):
        # Create test file
        test_file = tmp_path / "data.parquet"
        df = pl.DataFrame({"x": [1, 2, 3]})
        df.write_parquet(test_file)

        upload_resp = {
            "name": "data",
            "hash": "abc123",
            "content_type": "application/vnd.apache.parquet",
            "byte_size": test_file.stat().st_size,
            "deduplicated": False,
            "collection_version": None,
        }
        mock_resp = _make_response(json_data=upload_resp)
        mock_client._session.request = MagicMock(return_value=mock_resp)

        result = upload("alice/proj", test_file, client=mock_client)
        assert result.name == "data"
        assert result.hash == "abc123"
        assert not result.deduplicated

    def test_upload_with_collection(self, mock_client, tmp_path):
        test_file = tmp_path / "raw.csv"
        test_file.write_text("a,b\n1,2\n")

        upload_resp = {
            "name": "raw",
            "hash": "def456",
            "content_type": "text/csv",
            "byte_size": 8,
            "deduplicated": False,
            "collection_version": 1,
        }
        mock_resp = _make_response(json_data=upload_resp)
        mock_client._session.request = MagicMock(return_value=mock_resp)

        result = upload(
            "alice/proj",
            test_file,
            name="raw",
            collection="my-collection",
            client=mock_client,
        )
        assert result.collection_version == 1

    def test_upload_missing_file(self, mock_client, tmp_path):
        with pytest.raises(FileNotFoundError):
            upload("alice/proj", tmp_path / "nonexistent.parquet", client=mock_client)


# ── Download tests ───────────────────────────────────────────────


class TestDownload:
    def test_download_bytes(self, mock_client):
        content = b"raw binary data"
        mock_resp = _make_response(content=content)
        mock_client._session.request = MagicMock(return_value=mock_resp)

        result = download("alice/proj", "raw", client=mock_client)
        assert result == content

    def test_download_dataframe(self, mock_client):
        df = pl.DataFrame({"x": [1, 2, 3]})
        parquet_bytes = _make_parquet_bytes({"x": [1, 2, 3]})

        mock_resp = _make_response(
            content=parquet_bytes,
            headers={"content-type": "application/vnd.apache.parquet"},
        )
        mock_client._session.request = MagicMock(return_value=mock_resp)

        result = download_dataframe("alice/proj", "raw", client=mock_client)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 1)


# ── Run tests ────────────────────────────────────────────────────


class TestRun:
    @patch("ozzydb.client.subprocess.run")
    @patch("ozzydb.client.shutil.which", return_value="/usr/bin/ozzy")
    def test_run_subprocess(self, mock_which, mock_subprocess, tmp_path):
        # Create a parquet file that the "CLI" would produce
        output_df = pl.DataFrame({"result": [10, 20, 30]})
        output_file = tmp_path / "output.parquet"
        output_df.write_parquet(output_file)

        def side_effect(*args, **kwargs):
            # Copy the output to wherever the client asks
            import shutil
            cmd = args[0]
            output_idx = cmd.index("--output") + 1
            output_path = cmd[output_idx]
            shutil.copy(output_file, output_path)
            result = MagicMock()
            result.returncode = 0
            result.stderr = ""
            return result

        mock_subprocess.side_effect = side_effect

        result = run("my-endpoint", cwd=tmp_path, threshold=50)
        assert isinstance(result, pl.DataFrame)
        assert result.shape == (3, 1)

        # Verify CLI args
        call_args = mock_subprocess.call_args[0][0]
        assert "run" in call_args
        assert "my-endpoint" in call_args
        assert "--param" in call_args
        assert "threshold=50" in call_args

    @patch("ozzydb.client.shutil.which", return_value=None)
    def test_run_missing_cli(self, mock_which, tmp_path):
        with patch("ozzydb.client.Path.home") as mock_home:
            mock_home.return_value = tmp_path  # No .cargo/bin/ozzy exists
            with pytest.raises(RuntimeError, match="ozzy CLI not found"):
                run("ep", cwd=tmp_path)

    @patch("ozzydb.client.subprocess.run")
    @patch("ozzydb.client.shutil.which", return_value="/usr/bin/ozzy")
    def test_run_cli_failure(self, mock_which, mock_subprocess):
        mock_subprocess.return_value = MagicMock(
            returncode=1,
            stderr="Error: endpoint 'missing' not found",
        )
        with pytest.raises(RuntimeError, match="ozzy run failed"):
            run("missing")


# ── Auth tests ───────────────────────────────────────────────────


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

    def test_explicit_token(self):
        with patch.object(OzzyClient, "_load_credentials", return_value={}):
            client = OzzyClient(
                base_url="https://custom.example.com",
                token="my-token",
            )
        assert client.base_url == "https://custom.example.com"
        assert client.authenticated
        assert client._session.headers["Authorization"] == "Bearer my-token"


# ── Helper ───────────────────────────────────────────────────────


def _make_parquet_bytes(data: dict) -> bytes:
    """Create parquet bytes from a dict."""
    df = pl.DataFrame(data)
    buf = io.BytesIO()
    df.write_parquet(buf)
    return buf.getvalue()
