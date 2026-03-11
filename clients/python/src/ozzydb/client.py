"""OzzyDB Python client functions — fetch, inspect, and artifact operations."""

from __future__ import annotations

import tempfile
import time
from pathlib import Path
from typing import Any
from urllib.parse import quote

import polars as pl
import requests as req_lib

from .http import OzzyClient, get_default_client
from .types import (
    ArtifactConformance,
    ArtifactDetail,
    ArtifactManifest,
    ArtifactManifestEntry,
    ArtifactSummary,
    EndpointDetail,
    EndpointSummary,
    UploadArtifactResult,
    _from_dict,
)


def _parse_remote_ref(ref: str) -> tuple[str, str, str]:
    parts = ref.strip("/").split("/")
    if len(parts) != 3 or not all(parts):
        raise ValueError(f"Invalid reference '{ref}': expected 'owner/project/endpoint'")
    return parts[0], parts[1], parts[2]


def _parse_project_ref(ref: str) -> tuple[str, str]:
    parts = ref.strip("/").split("/")
    if len(parts) != 2 or not all(parts):
        raise ValueError(f"Invalid reference '{ref}': expected 'owner/project'")
    return parts[0], parts[1]


def fetch(
    ref: str,
    *,
    inputs: dict[str, str] | None = None,
    as_pandas: bool = False,
    ref_name: str | None = None,
    poll_interval: float = 2.0,
    timeout: float = 600.0,
    verbose: bool = False,
    client: OzzyClient | None = None,
    **params: Any,
) -> Any:
    owner, project, endpoint = _parse_remote_ref(ref)
    c = client or get_default_client()

    payload: dict[str, Any] = {
        "params": params,
        "inputs": {name: str(artifact_id) for name, artifact_id in (inputs or {}).items()},
    }
    if ref_name:
        payload["ref"] = ref_name

    fetch_resp = c.json_request(
        "POST",
        f"/fetch/{quote(owner, safe='')}/{quote(project, safe='')}/{quote(endpoint, safe='')}",
        json=payload,
    )

    job_id = fetch_resp["job_id"]
    status = fetch_resp.get("status", "queued")
    output_url = fetch_resp.get("output_url")
    output_hash = fetch_resp.get("output_hash")

    if status == "done" and output_url:
        if verbose:
            print("Cache hit")
        return _download_job_output(c, job_id, output_url, output_hash, as_pandas=as_pandas)

    start = time.monotonic()
    while True:
        if time.monotonic() - start > timeout:
            raise TimeoutError(f"Job {job_id} did not complete within {timeout}s")
        time.sleep(poll_interval)

        job = c.json_request("GET", f"/jobs/{job_id}")
        job_status = job.get("status", "unknown")

        if verbose:
            node_status = job.get("node_status", {})
            parts = []
            for name in sorted(node_status):
                state = node_status[name]
                symbol = "+" if state == "done" else "~" if state == "running" else "!" if state == "failed" else "."
                parts.append(f"[{symbol}]{name}")
            print("\r" + " ".join(parts), end="", flush=True)

        if job_status == "done":
            if verbose:
                print("\rDone")
            return _download_job_output(
                c,
                job_id,
                f"/jobs/{job_id}/output",
                job.get("output_hash"),
                as_pandas=as_pandas,
            )
        if job_status == "failed":
            if verbose:
                print("")
            raise RuntimeError(f"Job {job_id} failed: {job.get('error_message', 'unknown error')}")


def fetch_lazy(
    ref: str,
    *,
    inputs: dict[str, str] | None = None,
    ref_name: str | None = None,
    poll_interval: float = 2.0,
    timeout: float = 600.0,
    client: OzzyClient | None = None,
    **params: Any,
) -> pl.LazyFrame:
    result = fetch(
        ref,
        inputs=inputs,
        as_pandas=False,
        ref_name=ref_name,
        poll_interval=poll_interval,
        timeout=timeout,
        client=client,
        **params,
    )
    if isinstance(result, pl.DataFrame):
        return result.lazy()
    raise ValueError(f"Cannot create LazyFrame: fetch returned {type(result).__name__}")


def inspect(ref: str, *, ref_name: str | None = None, client: OzzyClient | None = None) -> EndpointDetail:
    owner, project, endpoint = _parse_remote_ref(ref)
    c = client or get_default_client()

    query = {"ref": ref_name} if ref_name else None
    data = c.json_request(
        "GET",
        f"/endpoints/{quote(owner, safe='')}/{quote(project, safe='')}/{quote(endpoint, safe='')}",
        params=query,
    )
    return _from_dict(EndpointDetail, data)


def list_endpoints(project: str, *, ref_name: str | None = None, client: OzzyClient | None = None) -> list[EndpointSummary]:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    query = {"ref": ref_name} if ref_name else None
    data = c.json_request(
        "GET",
        f"/endpoints/{quote(owner, safe='')}/{quote(slug, safe='')}",
        params=query,
    )
    return [_from_dict(EndpointSummary, item) for item in data]


def upload_artifact(
    project: str,
    file: str | Path,
    *,
    content_type: str | None = None,
    client: OzzyClient | None = None,
) -> UploadArtifactResult:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()

    file_path = Path(file)
    if not file_path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")

    with open(file_path, "rb") as fh:
        files = {"file": (file_path.name, fh)}
        form_data: dict[str, str] = {}
        if content_type is not None:
            form_data["content_type"] = content_type
        data = c.json_request(
            "POST",
            f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}/upload",
            files=files,
            data=form_data,
        )
    return _from_dict(UploadArtifactResult, data)


def list_artifacts(project: str, *, client: OzzyClient | None = None) -> list[ArtifactSummary]:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    data = c.json_request("GET", f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}")
    return [_from_dict(ArtifactSummary, item) for item in data]


def get_artifact(project: str, artifact_id: str, *, client: OzzyClient | None = None) -> ArtifactDetail:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    data = c.json_request(
        "GET",
        f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}/{quote(artifact_id, safe='')}",
    )
    return _from_dict(ArtifactDetail, data)


def get_artifact_conformance(
    project: str,
    artifact_id: str,
    *,
    client: OzzyClient | None = None,
) -> ArtifactConformance:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    data = c.json_request(
        "GET",
        f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}/{quote(artifact_id, safe='')}/conformance",
    )
    return _from_dict(ArtifactConformance, data)


def declare_conformance(
    project: str,
    artifact_id: str,
    type_ref: str,
    *,
    verify: bool = True,
    client: OzzyClient | None = None,
):
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    data = c.json_request(
        "POST",
        f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}/{quote(artifact_id, safe='')}/conformance",
        json={"type": type_ref, "verify": verify},
    )
    from .types import ConformanceRecordDetail
    return _from_dict(ConformanceRecordDetail, data)


def create_bundle_artifact(
    project: str,
    entries: dict[str, str],
    *,
    client: OzzyClient | None = None,
) -> ArtifactDetail:
    manifest = {
        "kind": "bundle",
        "entries": {name: {"artifact_id": str(artifact_id)} for name, artifact_id in entries.items()},
    }
    return _create_manifest_artifact(project, manifest, client=client)


def create_collection_artifact(
    project: str,
    items: list[str],
    *,
    client: OzzyClient | None = None,
) -> ArtifactDetail:
    manifest = {
        "kind": "collection",
        "items": [{"artifact_id": str(artifact_id)} for artifact_id in items],
    }
    return _create_manifest_artifact(project, manifest, client=client)


def download_artifact(
    project: str,
    artifact_id: str,
    *,
    as_pandas: bool = False,
    client: OzzyClient | None = None,
) -> Any:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    resp = c.request(
        "GET",
        f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}/{quote(artifact_id, safe='')}/download",
        stream=True,
        allow_redirects=False,
    )

    if resp.status_code not in (302, 307):
        content_type = resp.headers.get("content-type", "application/octet-stream")
        return _read_streaming_response(resp, content_type, as_pandas=as_pandas)

    location = resp.headers.get("location")
    if not location:
        resp.close()
        raise RuntimeError("Redirect response missing Location header")
    content_type = resp.headers.get("X-OzzyDB-Content-Type", "application/octet-stream")
    resp.close()

    redirected = req_lib.get(location, stream=True)
    redirected.raise_for_status()
    return _read_streaming_response(redirected, content_type, as_pandas=as_pandas)


def _create_manifest_artifact(project: str, manifest: dict[str, Any], *, client: OzzyClient | None) -> ArtifactDetail:
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()
    data = c.json_request(
        "POST",
        f"/artifacts/{quote(owner, safe='')}/{quote(slug, safe='')}/manifest",
        json=manifest,
    )
    return _from_dict(ArtifactDetail, data)


def _download_job_output(
    c: OzzyClient,
    job_id: str,
    output_url: str,
    output_hash: str | None,
    *,
    as_pandas: bool = False,
) -> Any:
    resp = c.request("GET", output_url, stream=True, allow_redirects=False)

    if resp.status_code in (302, 307):
        location = resp.headers.get("location")
        resp.close()
        if not location:
            raise RuntimeError("Redirect response missing Location header")
        resp = req_lib.get(location, stream=True)
        resp.raise_for_status()

    content_type = resp.headers.get("content-type", "application/octet-stream")
    try:
        result = _read_streaming_response(resp, content_type, as_pandas=as_pandas)
    finally:
        resp.close()

    if output_hash is not None and isinstance(result, bytes):
        actual_hash = __import__("blake3").blake3(result).hexdigest()
        if actual_hash != output_hash:
            raise RuntimeError(f"Output hash mismatch: expected {output_hash}, got {actual_hash}")

    if hasattr(result, "attrs"):
        result.attrs["ozzydb"] = {
            "hash": output_hash,
            "content_type": content_type,
            "job_id": job_id,
        }
    return result


def _read_streaming_response(resp, content_type: str, *, as_pandas: bool = False) -> Any:
    tmp = tempfile.NamedTemporaryFile(delete=False, suffix=_ext_for_type(content_type))
    tmp_path = tmp.name
    try:
        for chunk in resp.iter_content(chunk_size=8192):
            tmp.write(chunk)
        tmp.close()
    except Exception:
        tmp.close()
        Path(tmp_path).unlink(missing_ok=True)
        raise

    try:
        return _read_output(tmp_path, content_type, as_pandas=as_pandas)
    finally:
        Path(tmp_path).unlink(missing_ok=True)


def _ext_for_type(content_type: str) -> str:
    ct = content_type.lower()
    if "parquet" in ct:
        return ".parquet"
    if "csv" in ct:
        return ".csv"
    if "json" in ct:
        return ".json"
    if "arrow" in ct:
        return ".arrow"
    return ".bin"


def _infer_content_type(path: str) -> str:
    p = Path(path)
    ext = p.suffix.lower()
    ext_mapping = {
        ".parquet": "application/vnd.apache.parquet",
        ".csv": "text/csv",
        ".tsv": "text/tab-separated-values",
        ".json": "application/json",
        ".arrow": "application/vnd.apache.arrow.file",
        ".ipc": "application/vnd.apache.arrow.stream",
        ".png": "image/png",
        ".jpg": "image/jpeg",
        ".jpeg": "image/jpeg",
    }
    if ext in ext_mapping:
        return ext_mapping[ext]

    try:
        with open(p, "rb") as f:
            header = f.read(64)
        if header[:4] == b"PAR1":
            return "application/vnd.apache.parquet"
        if header[:8] == b"ARROW1\x00\x00":
            return "application/vnd.apache.arrow.file"
        if header[:4] == b"\x89PNG":
            return "image/png"
        if header[:2] == b"\xff\xd8":
            return "image/jpeg"
        try:
            text = header.decode("utf-8")
            if text.lstrip().startswith(("{", "[")):
                return "application/json"
            if "," in text and "\n" in text:
                return "text/csv"
            if "\t" in text and "\n" in text:
                return "text/tab-separated-values"
        except (UnicodeDecodeError, ValueError):
            pass
    except (OSError, IOError):
        pass

    return "application/octet-stream"


def _read_output(path: str, content_type: str, *, as_pandas: bool = False) -> Any:
    ct = content_type.lower()

    if "parquet" in ct:
        if as_pandas:
            import pandas as pd
            return pd.read_parquet(path)
        return pl.read_parquet(path)

    if "csv" in ct:
        if as_pandas:
            import pandas as pd
            return pd.read_csv(path)
        return pl.read_csv(path)

    if "tab-separated" in ct:
        if as_pandas:
            import pandas as pd
            return pd.read_csv(path, sep="\t")
        return pl.read_csv(path, separator="\t")

    if "arrow.stream" in ct:
        if as_pandas:
            import pyarrow.ipc as ipc
            with open(path, "rb") as f:
                reader = ipc.open_stream(f)
                table = reader.read_all()
            return table.to_pandas()
        return pl.read_ipc_stream(path)

    if "arrow" in ct:
        if as_pandas:
            import pyarrow.ipc as ipc
            with open(path, "rb") as f:
                reader = ipc.open_file(f)
                table = reader.read_all()
            return table.to_pandas()
        return pl.read_ipc(path)

    return Path(path).read_bytes()
