"""
Integration tests for OzzyDB Python client.

These tests require a running OzzyDB server or the ozzy CLI.
They are skipped in normal test runs.
"""

import subprocess
from pathlib import Path

import pytest


def _ozzy_cli_available() -> bool:
    """Check if ozzy CLI is available."""
    if (Path.home() / ".cargo" / "bin" / "ozzy").exists():
        return True
    return subprocess.run(["which", "ozzy"], capture_output=True).returncode == 0


@pytest.mark.skipif(
    not _ozzy_cli_available(),
    reason="ozzy CLI not installed",
)
class TestCliIntegration:
    """Integration tests requiring the ozzy CLI."""

    def test_import_ozzydb(self):
        """Verify package imports correctly."""
        import ozzydb

        assert ozzydb.__version__ == "0.2.0"
        assert callable(ozzydb.fetch)
        assert callable(ozzydb.run)
        assert callable(ozzydb.inspect)
