"""Resource-free proofs of the permanent signing policy and exact selector."""

from __future__ import annotations

import json
from pathlib import Path
import stat
import subprocess
from types import SimpleNamespace
import unittest
from unittest import mock

from deployment import signing


class SigningTests(unittest.TestCase):
    def setUp(self):
        self.policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                       "keychain": "/Users/fixture/Library/Keychains/login.keychain-db",
                       "identifier_namespace": "local.cell"}}

    def test_fingerprint_normalization_preserves_the_permanent_identifier(self):
        uppercase = {"schema": 1, "macos": {**self.policy["macos"], "certificate_sha1": "A" * 40}}
        self.assertEqual(signing.validate_policy(uppercase), self.policy)
        self.assertEqual(signing.validate_policy(uppercase), signing.validate_policy(self.policy))
        self.assertEqual(signing.identifier(self.policy, "annals", "annals-usage"), "local.cell.annals.annals-usage")
        self.assertEqual(signing.identifier(self.policy, "krisis", "krisis-install"), "local.cell.krisis.krisis-install")

    def test_unsupported_selection_and_duplicate_fields_have_no_fallback(self):
        for field, value in (("profile", "adhoc"), ("certificate_sha1", "auto"),
                             ("keychain", "relative/keychain"), ("identifier_namespace", "local"),
                             ("identifier_namespace", 'local.cell" or true')):
            with self.subTest(field=field):
                with self.assertRaises(signing.SigningError):
                    signing.validate_policy({"schema": 1, "macos": {**self.policy["macos"], field: value}})
        with self.assertRaisesRegex(signing.SigningError, "duplicate"):
            json.loads('{"schema":1,"schema":1}', object_pairs_hook=signing._object)

    def test_fixed_home_uses_the_system_account(self):
        with mock.patch.object(signing.os, "getuid", return_value=42), \
                mock.patch.object(signing.pwd, "getpwuid", return_value=SimpleNamespace(pw_dir="/Users/operator")), \
                mock.patch.dict(signing.os.environ, {"HOME": "/Users/another"}):
            self.assertEqual(signing.config_path(), Path("/Users/operator/Library/Application Support/Cell/signing.json"))

    def test_certificate_or_namespace_drift_stops_publication(self):
        for field, value in (("certificate_sha1", "b" * 40), ("identifier_namespace", "other.cell")):
            replacement = {"schema": 1, "macos": {**self.policy["macos"], field: value}}
            with self.subTest(field=field), mock.patch.object(signing, "load_policy", return_value=replacement):
                with self.assertRaisesRegex(signing.SigningError, "changed after admission"):
                    signing.assert_current(self.policy)

    def test_preflight_rejects_other_available_certificates(self):
        metadata = SimpleNamespace(st_mode=stat.S_IFREG | 0o600, st_uid=42)
        unrelated = subprocess.CompletedProcess([], 0, '1) ' + 'b' * 40 + ' "Other Identity"\n', '')
        with mock.patch.object(Path, "lstat", return_value=metadata), \
                mock.patch.object(signing.os, "getuid", return_value=42), \
                mock.patch.object(signing, "_run", return_value=unrelated) as run:
            with self.assertRaisesRegex(signing.SigningError, "missing, expired, or unavailable"):
                signing.preflight(self.policy)
            self.assertEqual(run.call_count, 1)

    def test_signing_pins_leaf_and_identifier_without_an_audit(self):
        with mock.patch.object(signing, "assert_current"), \
                mock.patch.object(signing, "is_native", return_value=True), \
                mock.patch.object(signing, "_run") as run:
            signing.sign(Path("/fixture/native"), self.policy, "nucleus", "nucleusd")
            run.assert_called_once()
            sign_command = run.call_args.args[0]
            self.assertIn(self.policy["macos"]["certificate_sha1"], sign_command)
            self.assertIn("--timestamp=none", sign_command)
            self.assertIn("=designated => " + signing.requirement(self.policy, "nucleus", "nucleusd"), sign_command)

    def test_signing_instruction_does_not_inspect_payload_contents(self):
        with mock.patch.object(signing, "is_native", side_effect=AssertionError("content inspection")), \
                mock.patch.object(signing, "_run") as run:
            signing.sign(Path("/fixture/payload"), self.policy, "nucleus", "nucleusd")
            run.assert_called_once()

    def test_structured_status_requires_explicit_json_selection(self):
        with mock.patch.object(signing.sys, "platform", "darwin"), \
                mock.patch.object(signing, "load_policy", return_value=self.policy), \
                mock.patch.object(signing, "preflight"), mock.patch("builtins.print") as output:
            self.assertEqual(signing.run_cli(["status"]), 0)
            self.assertTrue(output.call_args_list[0].args[0].startswith("Cell macOS signing ready:"))
            output.reset_mock()
            self.assertEqual(signing.run_cli(["status", "--json"]), 0)
            status = json.loads(output.call_args.args[0])
            self.assertTrue(status["ready"])
            self.assertEqual(status["policy"], self.policy)
            self.assertNotIn("policy_digest", status)


if __name__ == "__main__":
    unittest.main()
