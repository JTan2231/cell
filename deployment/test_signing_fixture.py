"""Fixture signing uses the current policy and leaves script mocks unchanged."""

from pathlib import Path
import sys
import unittest
from unittest import mock

from deployment import signing_fixture


class SigningFixtureTests(unittest.TestCase):
    def run_fixture(self):
        arguments = ["fixture-signing", "annals", "annals", "/fixture/annals"]
        with mock.patch.object(sys, "argv", arguments), mock.patch.object(sys, "platform", "darwin"):
            return signing_fixture.main()

    def test_script_mock_requires_no_signing_configuration(self):
        with mock.patch.object(signing_fixture.signing, "is_native", return_value=False), \
                mock.patch.object(signing_fixture.signing, "load_policy") as load:
            self.assertEqual(self.run_fixture(), 0)
        load.assert_not_called()

    def test_native_copy_uses_current_policy_then_verifies(self):
        policy = {"fixture": "policy"}
        with mock.patch.object(signing_fixture.signing, "is_native", return_value=True), \
                mock.patch.object(signing_fixture.signing, "load_policy", return_value=policy) as load, \
                mock.patch.object(signing_fixture.signing, "sign") as sign, \
                mock.patch.object(signing_fixture.signing, "verify") as verify:
            self.assertEqual(self.run_fixture(), 0)
        load.assert_called_once_with()
        sign.assert_called_once_with(Path("/fixture/annals"), policy, "annals", "annals")
        verify.assert_called_once_with(Path("/fixture/annals"), policy, "annals", "annals")

    def test_unavailable_current_identity_fails_fixture_setup(self):
        with mock.patch.object(signing_fixture.signing, "is_native", return_value=True), \
                mock.patch.object(signing_fixture.signing, "load_policy",
                                  side_effect=signing_fixture.signing.SigningError("identity unavailable")), \
                mock.patch.object(signing_fixture.signing, "sign") as sign, \
                mock.patch("builtins.print"):
            self.assertEqual(self.run_fixture(), 1)
        sign.assert_not_called()


if __name__ == "__main__":
    unittest.main()
