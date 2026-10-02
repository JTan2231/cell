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
        for directory in ("alpha", "infrastructure/alpha"):
            with self.subTest(directory=directory), tempfile.TemporaryDirectory() as name:
                root = Path(name).resolve()
                source = root / "source"
                descriptors = source / "pipeline/products"
                descriptors.mkdir(parents=True)
                (source / directory).mkdir(parents=True)
                (descriptors / "alpha.sh").write_text(f"PRODUCT_ID=alpha\nPRODUCT_DIR={directory}\n")
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
                self.assertEqual(manifest["product"], "alpha")
                self.assertEqual(manifest["binaries"]["alpha"]["version"], "alpha 1.2.3")
                self.assertEqual((output / "bin/alpha").read_bytes(), b"opaque executable")
                self.assertEqual(candidate.read_manifest(output), manifest)
                candidate.remove_tree(output)

    def test_staging_rejects_a_declared_directory_outside_the_source(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name).resolve()
            source = root / "source"
            descriptors = source / "pipeline/products"
            descriptors.mkdir(parents=True)
            external = root / "external"
            external.mkdir()
            (source / "infrastructure").symlink_to(external, target_is_directory=True)
            (external / "alpha").mkdir()
            (descriptors / "alpha.sh").write_text("PRODUCT_ID=alpha\nPRODUCT_DIR=infrastructure/alpha\n")
            with mock.patch.object(candidate, "git", return_value=b"a" * 40), \
                    mock.patch.object(signing, "assert_current"), \
                    mock.patch.object(signing, "sign") as sign:
                with self.assertRaisesRegex(candidate.CandidateError, "product source directory is unavailable"):
                    candidate.stage_build(source, "alpha", root / "candidate",
                        "main|target/release/alpha|alpha", target=root / "target",
                        source_key="a" * 40, signing_policy=POLICY)
            sign.assert_not_called()
            self.assertFalse((root / "candidate").exists())


if __name__ == "__main__":
    unittest.main()
