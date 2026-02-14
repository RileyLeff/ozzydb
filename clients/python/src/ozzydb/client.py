"""OzzyDB client functions — fetch, inspect, run, upload, download."""

from __future__ import annotations

import io
import shutil
import subprocess
import tempfile
import time
import warnings
from pathlib import Path
from typing import Any
from urllib.parse import quote

import polars as pl

from .http import OzzyClient, OzzyApiError, get_default_client
from .types import (
    EndpointDetail,
    FetchMetadata,
    ProjectDetail,
    UploadResult,
    _from_dict,
)


# ── Reference parsing ────────────────────────────────────────────


def _parse_remote_ref(ref: str) -> tuple[str, str, str]:
    """Parse 'owner/project/endpoint' into (owner, project, endpoint).

    Raises ValueError if the format is invalid.
    """
    parts = ref.strip("/").split("/")
    if len(parts) != 3 or not all(parts):
        raise ValueError(
            f"Invalid reference '{ref}': expected 'owner/project/endpoint'"
        )
    return parts[0], parts[1], parts[2]


def _parse_project_ref(ref: str) -> tuple[str, str]:
    """Parse 'owner/project' into (owner, project).

    Raises ValueError if the format is invalid.
    """
    parts = ref.strip("/").split("/")
    if len(parts) != 2 or not all(parts):
        raise ValueError(
            f"Invalid reference '{ref}': expected 'owner/project'"
        )
    return parts[0], parts[1]


# ── Remote fetch ─────────────────────────────────────────────────


def fetch(
    ref: str,
    *,
    as_pandas: bool = False,
    ref_name: str | None = None,
    poll_interval: float = 2.0,
    timeout: float = 600.0,
    verbose: bool = False,
    client: OzzyClient | None = None,
    **params: Any,
) -> Any:
    """Fetch endpoint output from the OzzyDB registry.

    Submits a compute job via POST, polls for completion, then downloads
    the result. Cache hits return immediately without polling.

    Args:
        ref: Remote reference in "owner/project/endpoint" format.
        as_pandas: If True, return a pandas DataFrame instead of polars.
        ref_name: Git ref (branch/tag) to resolve against.
        poll_interval: Seconds between status polls (default 2).
        timeout: Maximum seconds to wait for job completion (default 600).
        verbose: If True, print job progress to stderr.
        client: Optional OzzyClient instance (uses default if not provided).
        **params: Endpoint parameters passed as query string.

    Returns:
        polars.DataFrame, pandas.DataFrame, or bytes depending on content type.
    """
    owner, project, endpoint = _parse_remote_ref(ref)
    c = client or get_default_client()

    # Build query params
    query: dict[str, str] = {k: str(v) for k, v in params.items()}
    if ref_name:
        query["ref"] = ref_name

    # POST to trigger job
    fetch_resp = c.json_request(
        "POST",
        f"/fetch/{quote(owner, safe='')}/{quote(project, safe='')}/{quote(endpoint, safe='')}",
        params=query,
    )

    job_id = fetch_resp["job_id"]
    status = fetch_resp.get("status", "queued")
    output_url = fetch_resp.get("output_url")
    output_hash = fetch_resp.get("output_hash")

    # Cache hit — download immediately
    if status == "done" and output_url:
        if verbose:
            import sys
            print("Cache hit", file=sys.stderr)
        return _download_job_output(
            c, job_id, output_url, output_hash, as_pandas=as_pandas
        )

    # Poll for completion
    start = time.monotonic()
    while True:
        if time.monotonic() - start > timeout:
            raise TimeoutError(
                f"Job {job_id} did not complete within {timeout}s"
            )
        time.sleep(poll_interval)

        job = c.json_request("GET", f"/jobs/{job_id}")
        job_status = job.get("status", "unknown")

        if verbose:
            import sys
            node_status = job.get("node_status", {})
            parts = []
            for name in sorted(node_status):
                s = node_status[name]
                sym = "+" if s == "done" else "~" if s == "running" else "."
                parts.append(f"[{sym}]{name}")
            print(f"\r{' '.join(parts)}", end="", file=sys.stderr, flush=True)

        if job_status == "done":
            if verbose:
                print(f"\rDone{' ' * 40}", file=sys.stderr)
            job_output_url = f"/jobs/{job_id}/output"
            return _download_job_output(
                c, job_id, job_output_url, job.get("output_hash"),
                as_pandas=as_pandas,
            )
        elif job_status == "error":
            if verbose:
                print("", file=sys.stderr)
            msg = job.get("error_message", "unknown error")
            raise RuntimeError(f"Job {job_id} failed: {msg}")


def fetch_lazy(
    ref: str,
    *,
    ref_name: str | None = None,
    poll_interval: float = 2.0,
    timeout: float = 600.0,
    client: OzzyClient | None = None,
    **params: Any,
) -> pl.LazyFrame:
    """Fetch endpoint output as a polars LazyFrame.

    Only works for parquet outputs. For other formats, use fetch() instead.
    Uses the same async job model as fetch().

    Args:
        ref: Remote reference in "owner/project/endpoint" format.
        ref_name: Git ref (branch/tag) to resolve against.
        poll_interval: Seconds between status polls (default 2).
        timeout: Maximum seconds to wait (default 600).
        client: Optional OzzyClient instance.
        **params: Endpoint parameters.

    Returns:
        polars.LazyFrame
    """
    # Reuse fetch() to get the result, then convert to lazy
    result = fetch(
        ref,
        as_pandas=False,
        ref_name=ref_name,
        poll_interval=poll_interval,
        timeout=timeout,
        client=client,
        **params,
    )

    if isinstance(result, pl.DataFrame):
        return result.lazy()
    raise ValueError(
        f"Cannot create LazyFrame: fetch returned {type(result).__name__}"
    )


# ── Job output download ──────────────────────────────────────────


def _download_job_output(
    c: OzzyClient,
    job_id: str,
    output_url: str,
    output_hash: str | None,
    *,
    as_pandas: bool = False,
) -> Any:
    """Download output from a completed job.

    Handles both redirect (presigned URL) and direct proxy responses.
    """
    import requests as req_lib

    resp = c.request("GET", output_url, stream=True, allow_redirects=False)

    # Handle redirect to presigned URL
    if resp.status_code in (302, 307):
        location = resp.headers.get("location")
        resp.close()
        if not location:
            raise RuntimeError("Redirect response missing Location header")
        resp = req_lib.get(location, stream=True)
        resp.raise_for_status()

    content_type = resp.headers.get("content-type", "application/octet-stream")

    # Stream to temp file
    tmp = tempfile.NamedTemporaryFile(
        delete=False, suffix=_ext_for_type(content_type)
    )
    tmp_path = tmp.name
    try:
        for chunk in resp.iter_content(chunk_size=8192):
            tmp.write(chunk)
        tmp.close()
    except Exception:
        tmp.close()
        Path(tmp_path).unlink(missing_ok=True)
        raise
    finally:
        resp.close()

    try:
        result = _read_output(tmp_path, content_type, as_pandas=as_pandas)
        # Attach metadata
        if hasattr(result, "attrs"):
            result.attrs["ozzydb"] = {
                "hash": output_hash,
                "content_type": content_type,
                "job_id": job_id,
            }
        return result
    finally:
        Path(tmp_path).unlink(missing_ok=True)


# ── Inspect ──────────────────────────────────────────────────────


def inspect(
    ref: str,
    *,
    ref_name: str | None = None,
    client: OzzyClient | None = None,
) -> EndpointDetail:
    """Inspect an endpoint's metadata without executing it.

    Args:
        ref: Remote reference in "owner/project/endpoint" format.
        ref_name: Git ref (branch/tag) to resolve against.
        client: Optional OzzyClient instance.

    Returns:
        EndpointDetail with params, nodes, edges, etc.
    """
    owner, project, endpoint = _parse_remote_ref(ref)
    c = client or get_default_client()

    query: dict[str, str] = {}
    if ref_name:
        query["ref"] = ref_name

    data = c.json_request(
        "GET",
        f"/endpoints/{quote(owner, safe='')}/{quote(project, safe='')}/{quote(endpoint, safe='')}",
        params=query or None,
    )
    return _from_dict(EndpointDetail, data)


def inspect_project(
    ref: str,
    *,
    client: OzzyClient | None = None,
) -> ProjectDetail:
    """Inspect a project's metadata.

    Args:
        ref: Project reference in "owner/project" format.
        client: Optional OzzyClient instance.

    Returns:
        ProjectDetail with refs, collaborators, commit count, etc.
    """
    owner, project = _parse_project_ref(ref)
    c = client or get_default_client()

    data = c.json_request(
        "GET",
        f"/projects/{quote(owner, safe='')}/{quote(project, safe='')}",
    )
    return _from_dict(ProjectDetail, data)


# ── Local execution ──────────────────────────────────────────────


def run(
    endpoint: str,
    *,
    cwd: str | Path | None = None,
    as_pandas: bool = False,
    force: bool = False,
    **params: Any,
) -> Any:
    """Execute an endpoint locally via the ozzy CLI.

    Args:
        endpoint: Endpoint name (from local ozzy.toml).
        cwd: Working directory (defaults to current directory).
        as_pandas: Return pandas DataFrame instead of polars.
        force: Force re-execution, ignoring cache.
        **params: Endpoint parameters.

    Returns:
        polars.DataFrame, pandas.DataFrame, or bytes.
    """
    ozzy_bin = shutil.which("ozzy")
    if ozzy_bin is None:
        cargo_bin = Path.home() / ".cargo" / "bin" / "ozzy"
        if cargo_bin.exists():
            ozzy_bin = str(cargo_bin)
        else:
            raise RuntimeError(
                "ozzy CLI not found. Install it or add it to PATH."
            )

    with tempfile.NamedTemporaryFile(delete=False) as tmp:
        output_path = tmp.name

    cmd = [ozzy_bin, "run", endpoint, "--output", output_path]
    if force:
        cmd.append("--force")
    for key, value in params.items():
        cmd.extend(["--param", f"{key}={value}"])

    try:
        result = subprocess.run(
            cmd,
            cwd=cwd or Path.cwd(),
            capture_output=True,
            text=True,
            timeout=600,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"ozzy run failed (exit {result.returncode}): {result.stderr.strip()}"
            )

        # Infer content type from output file
        content_type = _infer_content_type(output_path)
        return _read_output(output_path, content_type, as_pandas=as_pandas)
    finally:
        Path(output_path).unlink(missing_ok=True)


# ── Data management ──────────────────────────────────────────────


def upload(
    project: str,
    file: str | Path,
    *,
    name: str | None = None,
    content_type: str | None = None,
    collection: str | None = None,
    client: OzzyClient | None = None,
) -> UploadResult:
    """Upload a data atom to the registry.

    Args:
        project: Project reference in "owner/project" format.
        file: Path to the file to upload.
        name: Atom name (defaults to filename stem).
        content_type: MIME type (inferred from extension if omitted).
        collection: Add to this collection after upload.
        client: Optional OzzyClient instance.

    Returns:
        UploadResult with name, hash, byte_size, etc.
    """
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()

    file_path = Path(file)
    if not file_path.exists():
        raise FileNotFoundError(f"File not found: {file_path}")

    # Build multipart form
    with open(file_path, "rb") as fh:
        files = {"file": (file_path.name, fh)}
        form_data: dict[str, str] = {"project": f"{owner}/{slug}"}
        if name:
            form_data["name"] = name
        if content_type:
            form_data["content_type"] = content_type
        if collection:
            form_data["collection"] = collection

        data = c.json_request("POST", "/data/upload", files=files, data=form_data)

    return _from_dict(UploadResult, data)


def download(
    project: str,
    name: str,
    *,
    client: OzzyClient | None = None,
) -> bytes:
    """Download a data atom's content as bytes.

    Args:
        project: Project reference in "owner/project" format.
        name: Atom name.
        client: Optional OzzyClient instance.

    Returns:
        Raw bytes of the atom content.
    """
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()

    resp = c.request(
        "GET",
        f"/data/{quote(owner, safe='')}/{quote(slug, safe='')}/{quote(name, safe='')}/download",
    )
    return resp.content


def download_dataframe(
    project: str,
    name: str,
    *,
    as_pandas: bool = False,
    client: OzzyClient | None = None,
) -> Any:
    """Download a data atom and read it as a DataFrame.

    Args:
        project: Project reference in "owner/project" format.
        name: Atom name.
        as_pandas: Return pandas DataFrame instead of polars.
        client: Optional OzzyClient instance.

    Returns:
        polars.DataFrame or pandas.DataFrame.
    """
    owner, slug = _parse_project_ref(project)
    c = client or get_default_client()

    resp = c.request(
        "GET",
        f"/data/{quote(owner, safe='')}/{quote(slug, safe='')}/{quote(name, safe='')}/download",
        stream=True,
    )

    content_type = resp.headers.get("content-type", "application/octet-stream")

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
    finally:
        resp.close()

    try:
        return _read_output(tmp_path, content_type, as_pandas=as_pandas)
    finally:
        Path(tmp_path).unlink(missing_ok=True)


# ── Helpers ──────────────────────────────────────────────────────


def _ext_for_type(content_type: str) -> str:
    """Return a file extension for a content type."""
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
    """Infer content type from file extension or magic bytes."""
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

    # Check magic bytes for common formats
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
        # Text format heuristics: check if content is valid UTF-8
        try:
            text = header.decode("utf-8")
            # JSON starts with { or [
            if text.lstrip().startswith(("{", "[")):
                return "application/json"
            # CSV: has commas and likely a header row
            if "," in text and "\n" in text:
                return "text/csv"
            # TSV: has tabs and likely a header row
            if "\t" in text and "\n" in text:
                return "text/tab-separated-values"
        except (UnicodeDecodeError, ValueError):
            pass
    except (OSError, IOError):
        pass

    return "application/octet-stream"


def _read_output(path: str, content_type: str, *, as_pandas: bool = False) -> Any:
    """Read an output file based on content type."""
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
            import pandas as pd
            import pyarrow.ipc as ipc
            with open(path, "rb") as f:
                reader = ipc.open_stream(f)
                table = reader.read_all()
            return table.to_pandas()
        return pl.read_ipc_stream(path)

    if "arrow" in ct:
        if as_pandas:
            import pandas as pd
            import pyarrow.ipc as ipc
            with open(path, "rb") as f:
                reader = ipc.open_file(f)
                table = reader.read_all()
            return table.to_pandas()
        return pl.read_ipc(path)

    # Binary fallback — return raw bytes
    return Path(path).read_bytes()
