"""Integration tests for OzzyDB Python client."""

import pytest


class TestPackageIntegration:
    def test_import_ozzydb(self):
        import ozzydb

        assert ozzydb.__version__ == "0.3.0"
        assert callable(ozzydb.fetch)
        assert callable(ozzydb.inspect)
        assert callable(ozzydb.upload_artifact)
        assert callable(ozzydb.list_artifacts)
