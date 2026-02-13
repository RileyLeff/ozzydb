"""OzzyDB HTTP client — handles auth, base URL, error parsing."""

from __future__ import annotations

import json
import threading
from pathlib import Path
from typing import Any

import requests


class OzzyApiError(Exception):
    """Error returned by the OzzyDB API."""

    def __init__(self, status: int, code: str, message: str):
        super().__init__(f"{code}: {message}")
        self.status = status
        self.code = code
        self.error_message = message


class OzzyClient:
    """HTTP client for the OzzyDB registry API."""

    DEFAULT_BASE_URL = "https://api.ozzydb.com"

    def __init__(
        self,
        base_url: str | None = None,
        token: str | None = None,
    ):
        self._session = requests.Session()
        self._token = token
        self._base_url: str | None = base_url

        # If no explicit token/url, try loading from credentials file
        if self._token is None or self._base_url is None:
            creds = self._load_credentials()
            if self._token is None:
                self._token = creds.get("token")
            if self._base_url is None:
                self._base_url = creds.get("registry_url")

        # Final fallback for base_url
        if self._base_url is None:
            self._base_url = self.DEFAULT_BASE_URL

        # Strip trailing slash
        self._base_url = self._base_url.rstrip("/")

        # Set auth header if we have a token
        if self._token:
            self._session.headers["Authorization"] = f"Bearer {self._token}"

    @property
    def base_url(self) -> str:
        return self._base_url  # type: ignore[return-value]

    @property
    def authenticated(self) -> bool:
        return self._token is not None

    def request(
        self,
        method: str,
        path: str,
        *,
        stream: bool = False,
        **kwargs: Any,
    ) -> requests.Response:
        """Make an HTTP request to the API. Raises OzzyApiError on non-2xx."""
        url = f"{self._base_url}/api/v1{path}"
        resp = self._session.request(method, url, stream=stream, **kwargs)

        if not resp.ok:
            try:
                body = resp.json()
                code = body.get("error", "unknown")
                message = body.get("message", resp.reason or "Unknown error")
            except (json.JSONDecodeError, ValueError):
                code = "unknown"
                message = resp.reason or f"HTTP {resp.status_code}"
            raise OzzyApiError(resp.status_code, code, message)

        return resp

    def json_request(self, method: str, path: str, **kwargs: Any) -> Any:
        """Make an HTTP request and return parsed JSON."""
        resp = self.request(method, path, **kwargs)
        if resp.status_code == 204 or resp.headers.get("content-length") == "0":
            return None
        return resp.json()

    def close(self) -> None:
        """Close the underlying HTTP session."""
        self._session.close()

    def __enter__(self) -> "OzzyClient":
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    @staticmethod
    def _credentials_path() -> Path:
        """Compute credentials path at runtime (not import time)."""
        return Path.home() / ".config" / "ozzy" / "credentials.json"

    def _load_credentials(self) -> dict[str, str]:
        """Load credentials from ~/.config/ozzy/credentials.json."""
        try:
            with open(self._credentials_path()) as f:
                return json.load(f)
        except (FileNotFoundError, json.JSONDecodeError, PermissionError):
            return {}


# Module-level default client (lazy singleton, thread-safe)
_default_client: OzzyClient | None = None
_default_client_lock = threading.Lock()


def get_default_client() -> OzzyClient:
    """Get or create the default OzzyClient instance."""
    global _default_client
    if _default_client is None:
        with _default_client_lock:
            if _default_client is None:
                _default_client = OzzyClient()
    return _default_client


def reset_default_client() -> None:
    """Reset the default client (useful for testing)."""
    global _default_client
    with _default_client_lock:
        if _default_client is not None:
            _default_client.close()
        _default_client = None
