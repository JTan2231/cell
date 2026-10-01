"""Candidate inventory and native-signature admission without content hashes."""

import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from deployment import candidate, signing


POLICY = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
          "keychain": "/Users/fixture/login.keychain-db", "identifier_namespace": "local.cell"}}


class CandidateTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "bin").mkdir()
        self.binary = self.root / "bin/alpha"
        self.binary.write_bytes(b"native executable fixture")
        self.manifest = {"schema": 1, "product": "alpha", "candidate_id": "uuid:fixture",
                         "signing_policy": POLICY,
                         "binaries": {"alpha": {"path": "bin/alpha",
                                      "code_identifier": "local.cell.alpha.alpha"}}}
        self.current = mock.patch.object(signing, "assert_current").start()
        self.addCleanup(mock.patch.stopall)
        self.verify = mock.patch.object(signing, "verify").start()

    def write_manifest(self):
        (self.root / "candidate.json").write_text(json.dumps(self.manifest))

    def test_declared_signed_candidate_is_accepted(self):
        self.write_manifest()
        self.assertEqual(candidate.verify(self.root, signing_policy=POLICY), self.manifest)
        self.verify.assert_called_once_with(self.binary, POLICY, "alpha", "alpha")

    def test_old_hash_fields_are_opaque_and_need_no_recomputation(self):
        self.manifest.update(candidate_id="sha256:old-release", signing_policy_digest="obsolete")
        self.manifest["binaries"]["alpha"]["sha256"] = "obsolete"
        self.write_manifest()
        candidate.verify(self.root, signing_policy=POLICY)
        self.verify.assert_called_once()

    def test_missing_or_extra_executable_stops_admission(self):
        self.write_manifest()
        self.binary.unlink()
        with self.assertRaises(candidate.CandidateError):
            candidate.verify(self.root, signing_policy=POLICY)
        self.binary.write_bytes(b"fixture")
        (self.root / "bin/undeclared").write_bytes(b"fixture")
        with self.assertRaises(candidate.CandidateError):
            candidate.verify(self.root, signing_policy=POLICY)
        self.verify.assert_not_called()

    def test_symbolic_executable_is_rejected(self):
        self.write_manifest()
        self.binary.unlink()
        self.binary.symlink_to(self.root / "candidate.json")
        with self.assertRaises(candidate.CandidateError):
            candidate.verify(self.root, signing_policy=POLICY)
        self.verify.assert_not_called()

    def test_wrong_policy_or_identifier_stops_before_signature_check(self):
        self.manifest["signing_policy"] = {}
        self.write_manifest()
        with self.assertRaises(candidate.CandidateError):
            candidate.verify(self.root, signing_policy=POLICY)
        self.manifest["signing_policy"] = POLICY
        self.manifest["binaries"]["alpha"]["code_identifier"] = "other.cell.alpha.alpha"
        self.write_manifest()
        with self.assertRaises(candidate.CandidateError):
            candidate.verify(self.root, signing_policy=POLICY)
        self.verify.assert_not_called()

    def test_native_signature_failure_remains_a_hard_stop(self):
        self.write_manifest()
        self.verify.side_effect = signing.SigningError("wrong certificate")
        with self.assertRaisesRegex(signing.SigningError, "wrong certificate"):
            candidate.verify(self.root, signing_policy=POLICY)


if __name__ == "__main__":
    unittest.main()
