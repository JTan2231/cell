"""Packaging signs declared files without running or auditing the payload."""

from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

from deployment import candidate, signing

POLICY = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
          "keychain": "/Users/fixture/login.keychain-db", "identifier_namespace": "local.cell"}}


class CandidateTests(unittest.TestCase):
    def test_staging_signs_once_and_uses_declared_versions_without_executing_payload(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            source = root / "source"
            descriptors = source / "pipeline/products"
            descriptors.mkdir(parents=True)
            (descriptors / "alpha.sh").write_text("PRODUCT_ID=alpha\nPRODUCT_DIR=alpha\n")
            target = root / "target"
            (target / "release").mkdir(parents=True)
            (target / "release/alpha").write_bytes(b"opaque executable")
            output = root / "candidate"
            with mock.patch.object(candidate, "git", return_value=b"a" * 40), \
                    mock.patch.object(signing, "assert_current"), \
                    mock.patch.object(signing, "sign") as sign, \
                    mock.patch.object(signing, "verify") as verify, \
                    mock.patch.object(subprocess, "run", side_effect=AssertionError("payload executed")):
                manifest = candidate.stage_build(source, "alpha", output,
                    "main|target/release/alpha|alpha", target=target, source_key="a" * 40,
                    versions={"alpha": "alpha 1.2.3"}, signing_policy=POLICY)
            sign.assert_called_once()
            verify.assert_not_called()
            self.assertEqual(manifest["binaries"]["alpha"]["version"], "alpha 1.2.3")
            self.assertEqual((output / "bin/alpha").read_bytes(), b"opaque executable")
            self.assertEqual(candidate.read_manifest(output), manifest)
            candidate.remove_tree(output)


if __name__ == "__main__":
    unittest.main()
