"""OzzyDB client functions — fetch, inspect, run, upload, download."""

from __future__ import annotations

import io
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any
from urllib.parse import quote

import polars as pl

from .http import OzzyClient, get_default_client
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
    if len(parts) != 3:
        raise ValueError(
            f"Invalid reference '{ref}': expected 'owner/project/endpoint'"
        )
    return parts[0], parts[1], parts[2]


def _parse_project_ref(ref: str) -> tuple[str, str]:
    """Parse 'owner/project' into (owner, project).

    Raises ValueError if the format is invalid.
    """
    parts = ref.strip("/").split("/")
    if len(parts) != 2:
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
    client: OzzyClient | None = None,
    **params: Any,
) -> Any:
    """Fetch endpoint output from the OzzyDB registry.

    Args:
        ref: Remote reference in "owner/project/endpoint" format.
        as_pandas: If True, return a pandas DataFrame instead of polars.
        ref_name: Git ref (branch/tag) to resolve against.
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

    resp = c.request(
        "GET",
        f"/fetch/{quote(owner, safe='')}/{quote(project, safe='')}/{quote(endpoint, safe='')}",
        params=query,
        stream=True,
    )

    content_type = resp.headers.get("content-type", "application/octet-stream")
    meta = FetchMetadata(
        hash=resp.headers.get("x-ozzydb-hash"),
        cache=resp.headers.get("x-ozzydb-cache"),
        verification=resp.headers.get("x-ozzydb-verification"),
        content_type=content_type,
    )

    # Stream to a temp file to avoid loading everything into memory
    with tempfile.NamedTemporaryFile(delete=False, suffix=_ext_for_type(content_type)) as tmp:
        tmp_path = tmp.name
        for chunk in resp.iter_content(chunk_size=8192):
            tmp.write(chunk)

    try:
        result = _read_output(tmp_path, content_type, as_pandas=as_pandas)
        # Attach metadata to the result if it's a DataFrame
        if hasattr(result, "attrs"):
            result.attrs["ozzydb"] = {
                "hash": meta.hash,
                "cache": meta.cache,
                "verification": meta.verification,
                "content_type": meta.content_type,
            }
        return result
    finally:
        Path(tmp_path).unlink(missing_ok=True)


def fetch_lazy(
    ref: str,
    *,
    ref_name: str | None = None,
    client: OzzyClient | None = None,
    **params: Any,
) -> pl.LazyFrame:
    """Fetch endpoint output as a polars LazyFrame.

    Only works for parquet outputs. For other formats, use fetch() instead.

    Args:
        ref: Remote reference in "owner/project/endpoint" format.
        ref_name: Git ref (branch/tag) to resolve against.
        client: Optional OzzyClient instance.
        **params: Endpoint parameters.

    Returns:
        polars.LazyFrame
    """
    owner, project, endpoint = _parse_remote_ref(ref)
    c = client or get_default_client()

    query: dict[str, str] = {k: str(v) for k, v in params.items()}
    if ref_name:
        query["ref"] = ref_name

    resp = c.request(
        "GET",
        f"/fetch/{quote(owner, safe='')}/{quote(project, safe='')}/{quote(endpoint, safe='')}",
        params=query,
        stream=True,
    )

    content_type = resp.headers.get("content-type", "application/octet-stream")

    # Write to temp file — for lazy scanning we need it to persist
    tmp = tempfile.NamedTemporaryFile(delete=False, suffix=".parquet")
    tmp_path = tmp.name
    for chunk in resp.iter_content(chunk_size=8192):
        tmp.write(chunk)
    tmp.close()

    if "parquet" in content_type:
        return pl.scan_parquet(tmp_path)
    else:
        # Fall back: read into DataFrame and convert to lazy
        try:
            df = _read_output(tmp_path, content_type, as_pandas=False)
            if isinstance(df, pl.DataFrame):
                return df.lazy()
            raise ValueError(
                f"Cannot create LazyFrame from content type: {content_type}"
            )
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
    files = {"file": (file_path.name, open(file_path, "rb"))}
    form_data: dict[str, str] = {"project": f"{owner}/{slug}"}
    if name:
        form_data["name"] = name
    if content_type:
        form_data["content_type"] = content_type
    if collection:
        form_data["collection"] = collection

    try:
        data = c.json_request("POST", "/data/upload", files=files, data=form_data)
    finally:
        files["file"][1].close()

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
        stream=True,
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

    with tempfile.NamedTemporaryFile(delete=False, suffix=_ext_for_type(content_type)) as tmp:
        tmp_path = tmp.name
        for chunk in resp.iter_content(chunk_size=8192):
            tmp.write(chunk)

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
            header = f.read(8)
        if header[:4] == b"PAR1":
            return "application/vnd.apache.parquet"
        if header[:8] == b"ARROW1\x00\x00":
            return "application/vnd.apache.arrow.file"
        if header[:4] == b"\x89PNG":
            return "image/png"
        if header[:2] == b"\xff\xd8":
            return "image/jpeg"
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
