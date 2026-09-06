"""Offline proofs for installed release identity and maintenance recovery."""
from __future__ import annotations

import hashlib
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

from deployment.adapter_support import MaintainedAdapter, ProductAdapter, Stopped, command, digest, failure_reason, tree_digest


ROOT = Path(__file__).resolve().parent.parent


class AdapterProofTests(unittest.TestCase):
    def test_command_failure_keeps_safe_reason_without_private_messages_or_arguments(self):
        cases = [
            ('{"error":{"code":"maintenance_held","message":"private document body"}}',
             'authorization token=secret', "product error code: maintenance_held"),
            ('private document body', 'fixture user deploy: unexpected version: private document body token=secret',
             "product version check failed"),
            ('private document body', 'clockwork version 1: private document body token=secret', None),
        ]
        for stdout, stderr, reason in cases:
            with self.subTest(reason=reason):
                response = subprocess.CompletedProcess([], 23, stdout, stderr)
                with mock.patch("deployment.adapter_support.subprocess.run", return_value=response):
                    with self.assertRaises(Stopped) as failed:
                        command(["/private/path/product", "private-argument"])
                detail = str(failed.exception)
                self.assertIn("exit 23", detail)
                self.assertNotIn("private", detail)
                self.assertNotIn("secret", detail)
                self.assertEqual(failure_reason(stdout, stderr), reason)
                if reason:
                    self.assertIn(reason, detail)

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name)
        self.home = self.base / "home"
        self.source = self.base / "source"
        self.binary = self.base / "candidate/bin/fixture"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_text("tested executable\n")
        self.binary.chmod(0o755)
        (self.source / "fixture/packaging").mkdir(parents=True)
        (self.source / "fixture/packaging/deploy.sh").write_text("selected deployer\n")
        (self.source / "fixture/provider").mkdir()
        (self.source / "fixture/provider/provider.json").write_text(json.dumps({"provider": {"release": "1.0.0"}}))
        self.spec = {"schema": 1, "product": "fixture", "application": "Fixture",
                     "proofs": {"binary_sha256": "bin/fixture", "deployer_sha256": "package/deploy-user.sh"},
                     "candidate_proofs": {"binary_sha256": "fixture"},
                     "source_proofs": {"deployer_sha256": "fixture/packaging/deploy.sh"},
                     "bundle_source": "fixture/provider"}
        self.request = {"schema": 1, "product": "fixture", "source_root": str(self.source),
                        "run_dir": str(self.base / "run"), "run_id": "run-a",
                        "candidate_dir": str(self.binary.parent.parent), "prior": None,
                        "candidate": {"schema": 1, "product": "fixture", "binaries": {"fixture": {"sha256": digest(self.binary)}},
                                      "source_inputs": {path.relative_to(self.source).as_posix(): digest(path)
                                                        for path in self.source.rglob("*") if path.is_file()}}}
        self.install()

    def tearDown(self):
        self.temporary.cleanup()

    def install(self, *, deployer="selected deployer\n", provider=None):
        stage = self.base / "install-stage"
        stage.mkdir()
        (stage / "bin").mkdir()
        shutil.copyfile(self.binary, stage / "bin/fixture")
        (stage / "package").mkdir()
        (stage / "package/deploy-user.sh").write_text(deployer)
        bundle = stage / "share/chancery/fixture"
        bundle.mkdir(parents=True)
        (bundle / "provider.json").write_text(provider or (self.source / "fixture/provider/provider.json").read_text())
        hashes = [digest(stage / "bin/fixture"), digest(stage / "package/deploy-user.sh"), tree_digest(bundle)]
        identity = hashlib.sha256("".join(value + "\n" for value in hashes).encode()).hexdigest()
        fields = {"format": "1", "product": "fixture", "release_id": identity, "version": "1.0.0",
                  "binary_sha256": hashes[0], "deployer_sha256": hashes[1], "chancery_sha256": hashes[2]}
        (stage / "manifest.txt").write_text("".join(f"{name}={value}\n" for name, value in fields.items()))
        install = self.home / "Library/Application Support/Fixture/install"
        releases = install / "releases"
        releases.mkdir(parents=True, exist_ok=True)
        stage.rename(releases / identity)
        current = install / "current"
        current.unlink(missing_ok=True)
        current.symlink_to("releases/" + identity)
        cli = self.home / ".local/bin/fixture"
        cli.parent.mkdir(parents=True, exist_ok=True)
        cli.unlink(missing_ok=True)
        cli.symlink_to(current / "bin/fixture")
        provider_link = self.home / "Library/Application Support/Chancery/providers/fixture"
        provider_link.parent.mkdir(parents=True, exist_ok=True)
        provider_link.unlink(missing_ok=True)
        provider_link.symlink_to(current / "share/chancery/fixture")

    def adapter(self, kind=ProductAdapter):
        with mock.patch.object(Path, "home", return_value=self.home):
            return kind(self.request, self.spec)

    def test_exact_candidate_and_source_are_accepted(self):
        adapter = self.adapter()
        with mock.patch("deployment.adapter_support.command", return_value="fixture 1.0.0"):
            self.assertEqual(adapter.verify()["status"], "verified")

    def test_same_binary_with_other_valid_provider_is_not_the_candidate(self):
        self.install(provider=json.dumps({"provider": {"release": "1.0.0"}, "different": "valid other source"}))
        adapter = self.adapter()
        self.assertIsNotNone(adapter.installed()["release_id"])
        with self.assertRaisesRegex(Stopped, "provider differs"):
            adapter.verify()

    def test_same_binary_with_other_valid_deployer_is_not_the_candidate(self):
        self.install(deployer="another valid product-owned deployer\n")
        adapter = self.adapter()
        self.assertIsNotNone(adapter.installed()["release_id"])
        with self.assertRaisesRegex(Stopped, "deployer_sha256 differs"):
            adapter.verify()

    def test_foreign_public_provider_is_refused(self):
        link = self.home / "Library/Application Support/Chancery/providers/fixture"
        link.unlink()
        link.symlink_to(self.source / "fixture/provider")
        with self.assertRaisesRegex(Stopped, "not owned"):
            self.adapter().installed()

    def test_affected_only_product_cannot_drift_from_its_captured_release(self):
        self.request["prior"] = self.adapter().installed()
        self.request["candidate_dir"] = None
        self.request["candidate"] = None
        self.install(deployer="external concurrent deployment\n")
        with self.assertRaisesRegex(Stopped, "changed since"):
            self.adapter().verify()

    def test_unmapped_packaged_payload_is_not_silently_ignored(self):
        self.spec["source_proofs"] = {}
        with self.assertRaisesRegex(Stopped, "incomplete candidate"):
            self.adapter().verify()

    def test_independent_operator_pause_is_preserved_during_readiness(self):
        self.spec["ready_command"] = True
        self.request["prior"] = {"runtime": {"operator_maintenance": False}}
        adapter = self.adapter(MaintainedAdapter)
        calls = []
        status = {"holds": ["run-a"], "drained": True, "operator_maintenance": True}
        adapter.maintenance = lambda *args: calls.append(args) or status.copy()
        with mock.patch("deployment.adapter_support.command") as executed:
            self.assertEqual(adapter.runtime_verify(), {"readiness": {}})
            executed.assert_not_called()
        self.assertEqual(calls, [("ready", "run-a")])
        self.assertTrue(status["operator_maintenance"])

    def test_actual_maintained_adapter_recovers_a_lost_hold_reply_without_execution(self):
        self.request["prior"] = self.adapter().installed()
        self.request["recovery"] = {"any_apply_started": False, "verified": False}
        adapter = self.adapter(MaintainedAdapter)
        holds = []
        def maintenance(operation, *args):
            if operation == "hold":
                holds.append(args[0])
                raise Stopped("hold effect succeeded but reply was lost")
            if operation == "release" and args[0] in holds:
                holds.remove(args[0])
            return {"holds": holds.copy(), "drained": True}
        adapter.maintenance = maintenance
        with self.assertRaisesRegex(Stopped, "reply was lost"):
            adapter.hold()
        with mock.patch("deployment.adapter_support.command") as executed:
            self.assertEqual(adapter.recover()["data"], {"safe_to_release": True, "installed": "prior"})
            self.assertEqual(adapter.release()["status"], "released")
            executed.assert_not_called()
        self.assertEqual(holds, [])

    def test_actual_maintained_adapter_can_recover_before_its_hold_started(self):
        self.request["prior"] = self.adapter().installed()
        self.request["recovery"] = {"any_apply_started": False}
        adapter = self.adapter(MaintainedAdapter)
        adapter.maintenance = lambda *_args: {"holds": [], "drained": False}
        with mock.patch("deployment.adapter_support.command") as executed:
            self.assertEqual(adapter.recover()["status"], "recovered")
            executed.assert_not_called()

    def test_actual_maintained_adapter_handles_lost_release_without_clearing_new_pause(self):
        self.request["prior"] = self.adapter().installed()
        self.request["recovery"] = {"any_apply_started": True, "verified": True}
        self.spec["doctor_command"] = True
        adapter = self.adapter(MaintainedAdapter)
        adapter.maintenance = lambda *_args: {"holds": ["operator-pause"], "drained": True}
        with mock.patch("deployment.adapter_support.command", return_value={"ok": True}) as executed:
            self.assertEqual(adapter.recover()["status"], "recovered")
            self.assertEqual(adapter.release()["data"]["holds"], ["operator-pause"])
            self.assertEqual(executed.call_args.args[0][-1], "doctor")
            self.assertEqual(executed.call_count, 1)


class NucleusRecoveryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec = importlib.util.spec_from_file_location("nucleus_adapter_test", ROOT / "nucleus/deployment/adapter.py")
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cls.adapter_type = module.NucleusAdapter

    def adapter(self, *, verified, applied=True, apply_started=True):
        adapter = object.__new__(self.adapter_type)
        adapter.prior = {"current": "releases/prior"}
        adapter.request = {"recovery": {"verified": verified, "applied": applied, "apply_started": apply_started}}
        adapter.installed = lambda: {"current": "releases/candidate"}
        adapter.prove_candidate_release = mock.Mock()
        adapter.maintenance = lambda *_args: {"holds": [], "drained": True}
        adapter.runtime_recover = mock.Mock(return_value={"service_ready": True})
        return adapter

    def test_verified_candidate_can_recover_after_deliberate_early_release(self):
        adapter = self.adapter(verified=True)
        result = adapter.recover()
        self.assertEqual(result["status"], "recovered")
        self.assertEqual(result["data"], {"safe_to_release": True, "installed": "candidate"})
        adapter.prove_candidate_release.assert_called_once()
        adapter.runtime_recover.assert_called_once()

    def test_unverified_unheld_candidate_does_not_gain_a_recovery_success(self):
        adapter = self.adapter(verified=False)
        with self.assertRaisesRegex(Stopped, "no hold or captured"):
            adapter.recover()
        adapter.runtime_recover.assert_not_called()

    def test_uncertain_service_cutover_cannot_use_health_of_an_old_resident_daemon(self):
        for selected in ["prior", "candidate"]:
            with self.subTest(selected=selected):
                adapter = self.adapter(verified=False, applied=False)
                adapter.installed = lambda: {"current": "releases/" + selected}
                adapter.check_prior = mock.Mock()
                with self.assertRaisesRegex(Stopped, "service cutover is uncertain; hold retained"):
                    adapter.recover()
                adapter.runtime_recover.assert_not_called()
                adapter.prove_candidate_release.assert_not_called()

    def test_unchanged_nucleus_without_an_apply_can_still_recover(self):
        adapter = self.adapter(verified=False, applied=False, apply_started=False)
        adapter.installed = lambda: {"current": "releases/prior"}
        adapter.check_prior = mock.Mock()
        self.assertEqual(adapter.recover()["data"]["installed"], "prior")
        adapter.check_prior.assert_called_once()
        adapter.runtime_recover.assert_called_once()


class TodoScheduleTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        module_spec = importlib.util.spec_from_file_location("todo_adapter_test", ROOT / "todo/deployment/adapter.py")
        module = importlib.util.module_from_spec(module_spec)
        module_spec.loader.exec_module(module)
        self.adapter = object.__new__(module.TodoAdapter)
        self.adapter.home = self.base / "home"
        self.adapter.install = self.adapter.home / "Library/Application Support/Todo/install"
        release = self.adapter.install / "releases" / ("a" * 64)
        (release / "package").mkdir(parents=True)
        (self.adapter.install / "current").symlink_to("releases/" + "a" * 64)
        template = ROOT / "todo/packaging/macos/org.todo.daily-email.plist"
        shutil.copyfile(template, release / "package" / template.name)
        expected = plistlib.loads(template.read_bytes())
        expected["WorkingDirectory"] = str(self.adapter.install.parent)
        expected["EnvironmentVariables"]["HOME"] = str(self.adapter.home)
        expected["ProgramArguments"][1] = str(self.adapter.install / "current/bin/todo-daily-email")
        logs = self.adapter.home / "Library/Logs/Todo"
        expected["StandardOutPath"] = str(logs / "email.stdout.log")
        expected["StandardErrorPath"] = str(logs / "email.stderr.log")
        self.plist = self.adapter.home / "Library/LaunchAgents/org.todo.daily-email.plist"
        self.plist.parent.mkdir(parents=True)
        self.plist.write_bytes(plistlib.dumps(expected))
        self.state = self.base / "launchctl-state.json"
        launchctl = self.base / "launchctl"
        launchctl.write_text(f'''#!{os.sys.executable}
import json, sys
from pathlib import Path
state = json.loads(Path({str(self.state)!r}).read_text())
if sys.argv[1] == "print-disabled" and sys.argv[2] == "gui/{os.getuid()}":
    print('disabled services = {{')
    print('    "org.todo.daily-email" => ' + str(state["disabled"]).lower())
    print('}}')
elif sys.argv[1] == "print" and sys.argv[2] == "gui/{os.getuid()}/org.todo.daily-email":
    sys.exit(state.get("query_error", 0 if state["loaded"] else 113))
else:
    sys.exit(99)
''')
        launchctl.chmod(0o755)
        self.adapter.launchctl = str(launchctl)
        self.adapter.spec = {}
        self.adapter.prior = {"schedule": {"loaded": True, "disabled": False}}
        self.write_state(loaded=True, disabled=False)

    def write_state(self, **value):
        self.state.write_text(json.dumps(value))

    def test_loaded_unloaded_and_disabled_states_are_observed_without_mutation(self):
        for loaded, disabled in [(True, False), (False, False), (False, True)]:
            with self.subTest(loaded=loaded, disabled=disabled):
                expected = {"loaded": loaded, "disabled": disabled}
                self.write_state(**expected)
                self.adapter.prior["schedule"] = expected
                self.assertEqual(self.adapter.schedule_state(), expected)
                self.assertEqual(self.adapter.runtime_readiness(), {})
                self.assertEqual(json.loads(self.state.read_text()), expected)

    def test_lost_bootstrap_and_independent_disable_cannot_be_reported_recovered(self):
        for disabled in [False, True]:
            with self.subTest(disabled=disabled):
                self.write_state(loaded=False, disabled=disabled)
                with self.assertRaisesRegex(Stopped, "captured operator state"):
                    self.adapter.recover()
                with self.assertRaisesRegex(Stopped, "captured operator state"):
                    self.adapter.release()
                self.assertEqual(json.loads(self.state.read_text()), {"loaded": False, "disabled": disabled})

    def test_owned_plist_drift_cannot_release_the_hold(self):
        data = plistlib.loads(self.plist.read_bytes())
        data["ProgramArguments"][1] = "/foreign/email-runner"
        self.plist.write_bytes(plistlib.dumps(data))
        with self.assertRaisesRegex(Stopped, "owned release definition"):
            self.adapter.release()

    def test_query_error_is_not_an_unloaded_service(self):
        self.write_state(loaded=False, disabled=False, query_error=5)
        with self.assertRaisesRegex(Stopped, "load state"):
            self.adapter.schedule_state()


if __name__ == "__main__":
    unittest.main()
