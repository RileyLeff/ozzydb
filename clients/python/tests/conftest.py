"""Pytest configuration and fixtures for OzzyDB tests."""

import json
import tempfile
from pathlib import Path
from unittest.mock import MagicMock, patch

import pytest

from ozzydb.http import OzzyClient, reset_default_client


@pytest.fixture(autouse=True)
def _reset_client():
    """Reset the default client between tests."""
    reset_default_client()
    yield
    reset_default_client()


@pytest.fixture
def mock_client():
    """Create an OzzyClient with no credentials file and a mock session."""
    with patch.object(OzzyClient, "_load_credentials", return_value={}):
        client = OzzyClient(base_url="https://test.ozzydb.com", token="test-token")
    return client


@pytest.fixture
def credentials_dir(tmp_path):
    """Create a temp credentials file."""
    creds_dir = tmp_path / ".config" / "ozzy"
    creds_dir.mkdir(parents=True)
    creds_file = creds_dir / "credentials.json"
    creds_file.write_text(json.dumps({
        "registry_url": "https://custom.ozzydb.com",
        "token": "cred-file-token",
    }))
    return creds_file
