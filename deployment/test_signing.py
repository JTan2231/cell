"""Signing-policy proof without production keys, configuration, or trust changes."""

from __future__ import annotations

import contextlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

from deployment import signing


class SigningTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        patch = mock.patch.object(signing, "home", return_value=self.root)
        patch.start()
        self.addCleanup(patch.stop)
        patch = mock.patch.object(signing.sys, "platform", "darwin")
        patch.start()
        self.addCleanup(patch.stop)
        signing.config_path().parent.mkdir(parents=True, mode=0o700)
        self.policy = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "a" * 40,
                       "keychain": str(self.root / "login.keychain-db"), "identifier_namespace": "local.cell"}}

    def test_missing_policy_never_selects_an_available_identity(self):
        with mock.patch.object(signing, "_run") as run:
            with self.assertRaisesRegex(signing.SigningError, "not configured"):
                signing.load_policy()
            run.assert_not_called()

    def test_policy_roundtrip_is_private_and_namespace_is_stable(self):
        signing._write_policy(self.policy)
        self.assertEqual(signing.load_policy(), self.policy)
        self.assertEqual(signing.config_path().stat().st_mode & 0o777, 0o600)
        self.assertEqual(signing.identifier(self.policy, "nucleus", "nucleusd"), "local.cell.nucleus.nucleusd")
        signing.assert_current(self.policy)

    def test_explicit_identity_change_stops_old_publication(self):
        signing._write_policy(self.policy)
        replacement = json.loads(json.dumps(self.policy))
        replacement["macos"]["certificate_sha1"] = "b" * 40
        signing._write_policy(replacement)
        with self.assertRaisesRegex(signing.SigningError, "changed after admission"):
            signing.assert_current(self.policy)

    def test_duplicate_fields_and_symbolic_config_are_rejected(self):
        signing.config_path().write_text('{"schema":1,"schema":1,"macos":{}}')
        signing.config_path().chmod(0o600)
        with self.assertRaisesRegex(signing.SigningError, "duplicate"):
            signing.load_policy()
        signing.config_path().unlink()
        elsewhere = self.root / "elsewhere"
        elsewhere.write_bytes(signing.json_bytes(self.policy))
        elsewhere.chmod(0o600)
        signing.config_path().symlink_to(elsewhere)
        with self.assertRaisesRegex(signing.SigningError, "non-symbolic"):
            signing.load_policy()

    def test_preflight_does_not_fall_back_to_another_certificate(self):
        keychain = Path(self.policy["macos"]["keychain"])
        keychain.touch()
        unrelated = subprocess.CompletedProcess([], 0, '1) ' + 'b' * 40 + ' "Other Identity"\n', '')
        with mock.patch.object(signing, "_run", return_value=unrelated) as run:
            with self.assertRaisesRegex(signing.SigningError, "missing, expired, or unavailable"):
                signing.preflight(self.policy)
            self.assertEqual(run.call_count, 1)

    def test_signing_requires_native_code_and_verifies_external_policy(self):
        signing._write_policy(self.policy)
        artifact = self.root / "native"
        artifact.write_bytes(bytes.fromhex("cffaedfe") + b"fixture")
        with mock.patch.object(signing, "_run") as run:
            signing.sign(artifact, self.policy, "nucleus", "nucleusd")
            first, second = (call.args[0] for call in run.call_args_list)
            self.assertIn(self.policy["macos"]["certificate_sha1"], first)
            self.assertIn("--timestamp=none", first)
            self.assertIn("=designated => " + signing.requirement(self.policy, "nucleus", "nucleusd"), first)
            self.assertIn("--all-architectures", second)
            self.assertIn("=" + signing.requirement(self.policy, "nucleus", "nucleusd"), second)
        artifact.write_text("#!/bin/sh\nexit 0\n")
        with mock.patch.object(signing, "_run") as run:
            with self.assertRaisesRegex(signing.SigningError, "not native"):
                signing.sign(artifact, self.policy, "nucleus", "nucleusd")
            run.assert_not_called()

    @unittest.skipUnless(sys.platform == "darwin", "requires Apple's verifier")
    def test_real_native_code_with_wrong_signer_is_rejected(self):
        with self.assertRaises(signing.SigningError):
            signing.verify(Path("/usr/bin/true"), self.policy, "nucleus", "nucleusd")

    def test_create_local_does_not_replace_a_configured_identity(self):
        signing._write_policy(self.policy)
        with mock.patch.object(signing, "configuration_lock", return_value=contextlib.nullcontext()), mock.patch.object(signing, "_run") as run:
            with self.assertRaisesRegex(signing.SigningError, "already configured"):
                signing.create_local()
            run.assert_not_called()
        self.assertEqual(signing.load_policy(), self.policy)

    def test_create_local_refuses_existing_certificate_or_unknown_keychain_state(self):
        for outcome in (subprocess.CompletedProcess([], 0, "", ""),
                        subprocess.CompletedProcess([], 1, "", ""),
                        subprocess.TimeoutExpired("security", 30)):
            with self.subTest(outcome=type(outcome).__name__), mock.patch.object(
                    signing, "configuration_lock", return_value=contextlib.nullcontext()), mock.patch.object(
                    signing.subprocess, "run", **({"side_effect": outcome} if isinstance(outcome, Exception)
                                                 else {"return_value": outcome})):
                with self.assertRaises(signing.SigningError):
                    signing.create_local()
                self.assertFalse(signing.config_path().exists())
                self.assertFalse((signing.config_path().parent / "signing-backup").exists())

    @unittest.skipUnless(Path("/usr/bin/openssl").exists(), "requires local certificate tooling")
    def test_local_creation_retains_encrypted_recovery_and_cleans_plaintext_key(self):
        original = signing._run
        def isolated_run(command, **kwargs):
            if command[0] == "/usr/bin/security":
                return subprocess.CompletedProcess(command, 0, "", "")
            return original(command, **kwargs)
        real_run = subprocess.run
        def isolated_subprocess(command, **kwargs):
            if command[:2] == ["/usr/bin/security", "find-certificate"]:
                return subprocess.CompletedProcess(command, 44, "", "")
            return real_run(command, **kwargs)
        with mock.patch.object(signing, "configuration_lock", return_value=contextlib.nullcontext()), mock.patch.object(signing, "_run", side_effect=isolated_run), mock.patch.object(signing, "preflight"), mock.patch.object(signing.subprocess, "run", side_effect=isolated_subprocess):
            result = signing.create_local()
        backup = Path(result["recovery_directory"])
        self.assertEqual(signing.load_policy(), result["policy"])
        self.assertTrue((backup / "identity.p12").is_file())
        self.assertTrue((backup / "recovery-password").is_file())
        self.assertEqual((backup / "identity.p12").stat().st_mode & 0o777, 0o600)
        self.assertFalse(list(signing.config_path().parent.glob(".signing-setup-*")))
        self.assertFalse(list(signing.config_path().parent.rglob("key.pem")))
        with mock.patch.object(signing, "configuration_lock", return_value=contextlib.nullcontext()):
            signing.config_path().unlink()
            with self.assertRaisesRegex(signing.SigningError, "recovery export exists"):
                signing.create_local()

    def test_configuration_refuses_unsettled_queue(self):
        from ci_manager import storage, workspace
        state = self.root / "external/ci-manager"
        store = storage.Store(state, create=True)
        self.addCleanup(store.db.close)
        store.set("paused", False)
        with mock.patch.object(workspace, "directory", side_effect=lambda value: self.root / "external" / value):
            with self.assertRaisesRegex(signing.SigningError, "pause and settle"):
                with signing.configuration_lock():
                    self.fail("an admitting queue must block configuration")


if __name__ == "__main__":
    unittest.main()
