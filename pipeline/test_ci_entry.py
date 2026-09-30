"""Prove CI entry routing and worker validation without live manager effects."""

from contextlib import ExitStack
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parent.parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(SOURCE))

from ci_manager import installation
from ci_manager.manager import Worker
from ci_manager.storage import ManagerError, Store, lock


class CIEntryTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="cell-ci-entry-test-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "workspace with spaces"
        self.home = Path(self.temporary.name) / "home with spaces"
        self.log = Path(self.temporary.name) / "invocation.json"
        self.environment = {
            **os.environ,
            "HOME": str(self.home),
            "FIXTURE_CI_ENTRY_LOG": str(self.log),
            "FIXTURE_CI_ENTRY_STATUS": "0",
        }
        self.root.mkdir()
        for relative in ("ci.sh", "nucleus/ci.sh", "pipeline/test.sh"):
            self.write(self.root / relative, (SOURCE / relative).read_text())
        self.write(self.home / ".local/bin/cell-ci", self.recorder("installed"))
        self.write(self.root / "ci_manager/client.py", self.recorder("source"))
        self.write(self.root / "pipeline/select_changes.py", self.recorder("validator"))

    @staticmethod
    def write(path, contents):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents)
        path.chmod(0o755)

    @staticmethod
    def recorder(route):
        return f'''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

Path(os.environ["FIXTURE_CI_ENTRY_LOG"]).write_text(json.dumps({{
    "route": {route!r}, "arguments": sys.argv[1:], "cwd": os.getcwd(),
}}))
raise SystemExit(int(os.environ["FIXTURE_CI_ENTRY_STATUS"]))
'''

    def invoke(self, *arguments, entry="ci.sh", status=0):
        self.log.unlink(missing_ok=True)
        return subprocess.run(
            ["sh", str(self.root / entry), *arguments], cwd=self.root,
            env={**self.environment, "FIXTURE_CI_ENTRY_STATUS": str(status)},
            text=True, capture_output=True,
        )

    def assert_routed(self, route, arguments, *, entry="ci.sh", status=0):
        result = self.invoke(*arguments, entry=entry, status=status)
        self.assertEqual(result.returncode, status, result.stdout + result.stderr)
        self.assertEqual(json.loads(self.log.read_text()), {
            "route": route, "arguments": list(arguments), "cwd": str(self.root.resolve()),
        })

    def test_manager_commands_use_installed_client(self):
        commands = (
            ("submit", "HEAD", "--request-id", "request with spaces"),
            ("status", "job-id"), ("wait", "job-id", "--timeout", "10"),
            ("pause",), ("resume",), ("cancel", "job-id"),
            ("recover", "job-id"), ("maintenance", "status"),
            ("service", "status"), ("--version",),
        )
        for arguments in commands:
            with self.subTest(arguments=arguments):
                self.assert_routed("installed", arguments)

    def test_setup_commands_use_source_client(self):
        for arguments in (("init", "--repo", "repository with spaces",
                           "--accepted-baseline", "HEAD"), ("install",),
                          ("storage", "status"), ("storage", "configure", "--volume", "volume")):
            with self.subTest(arguments=arguments):
                self.assert_routed("source", arguments)

    def test_dispatch_preserves_client_failure_status(self):
        for route, arguments in (("installed", ("submit", "HEAD")),
                                 ("source", ("install",))):
            with self.subTest(route=route):
                self.assert_routed(route, arguments, status=37)

    def test_removed_validation_invocations_fail_without_dispatch(self):
        for arguments in ((), ("--all",), ("--platform",), ("nucleus",),
                          ("--base", "HEAD", "--candidate", "HEAD", "--json"),
                          ("--verbose",), ("run",)):
            with self.subTest(arguments=arguments):
                result = self.invoke(*arguments)
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("submit COMMIT", result.stdout + result.stderr)
                self.assertFalse(self.log.exists(), "a removed invocation reached a client or validator")

    def test_help_is_available_without_installed_manager(self):
        (self.home / ".local/bin/cell-ci").unlink()
        for argument in ("-h", "--help", "help"):
            with self.subTest(argument=argument):
                result = self.invoke(argument)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn("submit COMMIT", result.stdout + result.stderr)
                self.assertFalse(self.log.exists())

    def test_missing_manager_does_not_fall_back_to_validation(self):
        (self.home / ".local/bin/cell-ci").unlink()
        result = self.invoke("submit", "HEAD")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(self.log.exists())

    def test_product_entry_uses_same_manager(self):
        self.assert_routed("installed", ("submit", "HEAD", "--deploy", "nucleus"),
                           entry="nucleus/ci.sh", status=37)

    def test_product_entry_rejects_direct_validation(self):
        for arguments in ((), ("--platform",)):
            with self.subTest(arguments=arguments):
                result = self.invoke(*arguments, entry="nucleus/ci.sh")
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("submit COMMIT", result.stdout + result.stderr)
                self.assertFalse(self.log.exists())

    def test_shared_entry_uses_manager_and_rejects_direct_suites(self):
        self.assert_routed("installed", ("submit", "HEAD"), entry="pipeline/test.sh")
        for arguments in ((), ("broker",), ("--verbose",)):
            with self.subTest(arguments=arguments):
                result = self.invoke(*arguments, entry="pipeline/test.sh")
                self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
                self.assertIn("submit COMMIT", result.stdout + result.stderr)
                self.assertFalse(self.log.exists())


class ProductPhaseTests(unittest.TestCase):
    """Run the actual private shell phases with inert Cargo and fixture tools."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-phase-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name) / "source with spaces"
        self.log = Path(temporary.name) / "commands.jsonl"
        self.environment = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1",
                            "FIXTURE_PHASE_LOG": str(self.log),
                            "PATH": str(self.root / "tools") + os.pathsep + os.environ["PATH"]}
        for relative in ("pipeline/ci.sh", "pipeline/platform.sh", "pipeline/lib.sh"):
            self.write(relative, (SOURCE / relative).read_text())
        self.write("Cargo.toml", '[workspace]\n')
        self.write("Cargo.lock", "fixture\n")
        self.write("alpha/Cargo.toml", '[package]\nname = "alpha"\nversion = "1.0.0"\n')
        self.write("alpha/provider/provider.json", '{"release": "1.0.0"}\n')
        self.write("pipeline/products/alpha.sh", """PIPELINE_SCHEMA=1
PRODUCT_ID=alpha
PRODUCT_NAME=Alpha
PRODUCT_DIR=alpha
CI_RESOURCE_CLASS=heavy
RELEASE_BRANCH=main
DEPLOY_PROFILE=custom
DEPLOY_CONFLICT_KEYS=alpha
CARGO_PACKAGES=alpha
RELEASE_UNITS='alpha|Alpha|package|alpha/Cargo.toml|alpha-|1'
CI_PROVIDER_VALIDATION_PHASE=after-tests
PROVIDERS='alpha|alpha|alpha/provider|1'
CI_RUN_CHECKS='always|checks/packaging.sh'
CI_EXTRA_BEFORE_RUST=checks/before.sh
CI_EXTRA_AFTER_BUILD=checks/after.sh
""")
        recorder = '''#!/usr/bin/env python3
import json
import os
from pathlib import Path
import sys

label = Path(sys.argv[0]).name
if sys.argv[1:] == ["--version"]:
    print(label + " 1.97.1 (fixture)")
else:
    with Path(os.environ["FIXTURE_PHASE_LOG"]).open("a") as stream:
        stream.write(json.dumps([label, *sys.argv[1:]]) + "\\n")
'''
        for relative in ("tools/cargo", "tools/rustc", "pipeline/cargo_tests.py",
                         "deployment/candidate.py", "checks/packaging.sh", "checks/before.sh",
                         "checks/after.sh"):
            self.write(relative, recorder)

    def write(self, relative, value):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(value)
        path.chmod(0o755)

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()] if self.log.exists() else []

    def invoke(self, phase=None, group="all", stage=None):
        command = ["sh", str(self.root / "pipeline/ci.sh"), "alpha", "--tests", group]
        if phase is not None:
            command.extend(["--phase", phase])
        if stage is not None:
            command.extend(["--stage-candidate", stage])
        return subprocess.run(command, env=self.environment, capture_output=True, text=True)

    def test_pre_phase_runs_setup_and_linting_without_tests_or_release(self):
        result = self.invoke("pre")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        commands = self.commands()
        self.assertEqual([command[0] for command in commands],
                         ["packaging.sh", "before.sh", "cargo", "cargo"])
        self.assertEqual([command[1] for command in commands if command[0] == "cargo"],
                         ["fmt", "clippy"])

    def test_post_phase_runs_provider_docs_release_and_seals_after_build(self):
        stage = str(self.root.parent / "sealed candidate")
        result = self.invoke("post", stage=stage)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        commands = self.commands()
        self.assertEqual([command[0] for command in commands],
                         ["cargo", "cargo", "cargo", "after.sh", "candidate.py"])
        self.assertEqual([command[1] for command in commands if command[0] == "cargo"],
                         ["run", "doc", "build"])
        self.assertEqual(commands[-1][commands[-1].index("--output") + 1], stage)

    def test_ordinary_product_phase_skips_platform_shell_extras(self):
        result = self.invoke("pre", group="product")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual([command[0] for command in self.commands()], ["cargo", "cargo"])

    def test_private_full_body_retains_test_group_compatibility(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        tests = [command for command in self.commands() if command[0] == "cargo_tests.py"]
        self.assertEqual([command[command.index("--group") + 1] for command in tests],
                         ["product", "platform"])

    def test_pre_phase_rejects_candidate_staging_before_running_any_checks(self):
        result = self.invoke("pre", stage=str(self.root.parent / "candidate"))
        self.assertEqual(result.returncode, 1)
        self.assertIn("post-test phase", result.stderr)
        self.assertEqual(self.commands(), [])

    def test_shared_checks_only_body_excludes_tests_and_legacy_body_keeps_them(self):
        script = str(self.root / "pipeline/platform.sh")
        for extra, expected in ((["--checks-only"], ["fmt", "clippy"]),
                                ([], ["fmt", "clippy", "test"])):
            with self.subTest(extra=extra):
                self.log.unlink(missing_ok=True)
                result = subprocess.run(["sh", script, "install", *extra], env=self.environment,
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertEqual([command[1] for command in self.commands()], expected)


class CancelledValidationRecoveryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-recovery-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.store = Store(self.root / "state", create=True)
        self.addCleanup(self.store.db.close)
        self.job = {
            "id": "fixture-job", "phase": "blocked", "stopped_phase": "checking",
            "cancel_requested": True, "base_commit": "a" * 40, "candidate_commit": "b" * 40,
            "input_commit": "c" * 40, "attempts": [], "validations": [],
            "policy": {"luna_attempts": 3, "terra_attempts": 1}, "model_unresolved": False,
            "outcome": "failed", "unresolved": True,
            "notification": {"state": "accepted"},
        }
        self.store.db.execute(
            "INSERT INTO jobs(id,submission_key,phase,cancel_requested,created,updated,data) "
            "VALUES (?,?,?,1,0,0,?)",
            (self.job["id"], "fixture", "blocked", json.dumps(self.job)),
        )
        self.directory = self.store.root / "jobs" / self.job["id"]
        self.directory.mkdir(mode=0o700, parents=True)
        worktree = self.directory / "worktree"
        self.request_path = self.directory / "validation-0.request.json"
        self.result_path = self.directory / "validation-0.result.json"
        self.request = {
            "command": [sys.executable, str(worktree / "pipeline/select_changes.py"), "run",
                        "--base", self.job["base_commit"],
                        "--candidate", self.job["candidate_commit"], "--json"],
            "cwd": str(worktree),
            **{key: str(self.directory / f"validation-0.{suffix}") for key, suffix in
               (("stdout", "stdout"), ("stderr", "stderr"),
                ("started", "started.json"), ("result", "result.json"))},
        }
        self.result = {"request": str(self.request_path), "exit_code": 2}
        self.request_path.write_text(json.dumps(self.request))
        self.result_path.write_text(json.dumps(self.result))
        Path(self.request["stdout"]).write_bytes(b"")
        Path(self.request["stderr"]).write_bytes(b"unrecognized arguments: --base --candidate --json\n")

    def installer(self):
        stack = ExitStack()
        self.addCleanup(stack.close)
        paths = {key: self.root / key for key in ("current", "wrapper", "provider", "plist")}
        patches = {
            "state_root": mock.Mock(return_value=self.store.root),
            "_paths": mock.Mock(return_value=paths),
            "_selected_release": mock.Mock(return_value=None),
            "_check_selector": mock.Mock(), "_check_plist": mock.Mock(return_value=None),
            "_stop_if_loaded": mock.Mock(return_value=True),
            "_prepare_release": mock.Mock(return_value=self.root / "release"),
            "_link": mock.Mock(), "_write_file": mock.Mock(), "_plist": mock.Mock(return_value=b""),
            "_launchctl": mock.Mock(),
        }
        stack.enter_context(mock.patch.object(installation.sys, "platform", "darwin"))
        for name, value in patches.items():
            stack.enter_context(mock.patch.object(installation, name, value))
        return patches

    def test_install_preserves_blocked_job_and_takes_worker_lock(self):
        before = self.store.job(self.job["id"])
        patches = self.installer()

        def prepare(*_):
            with self.assertRaises(BlockingIOError):
                with lock(self.store.root / "worker.lock", blocking=False):
                    self.fail("installer did not hold the singleton lock")
            return self.root / "release"

        patches["_prepare_release"].side_effect = prepare
        self.assertTrue(installation.install()["installed"])
        self.assertEqual(self.store.job(self.job["id"]), before)
        self.assertTrue(self.store.get("paused"))

    def test_install_rechecks_after_service_stop(self):
        patches = self.installer()
        patches["_stop_if_loaded"].side_effect = lambda _: (
            self.store.set("recovery_request", self.job["id"]) or True)
        with self.assertRaisesRegex(ManagerError, "active job finish"):
            installation.install()
        patches["_prepare_release"].assert_not_called()
        patches["_link"].assert_not_called()
        patches["_launchctl"].assert_called_once()
        self.assertEqual(patches["_launchctl"].call_args.args[0], "bootstrap")

    def test_install_refuses_live_worker_lock(self):
        patches = self.installer()
        with lock(self.store.root / "worker.lock", blocking=False), \
                mock.patch.object(installation.time, "monotonic", side_effect=[0, 10]):
            with self.assertRaisesRegex(ManagerError, "did not drain within 10 seconds"):
                installation.install()
        patches["_prepare_release"].assert_not_called()
        patches["_link"].assert_not_called()

    def test_install_waits_for_stopped_worker_to_release_lock(self):
        patches = self.installer()
        with ExitStack() as holder:
            holder.enter_context(lock(self.store.root / "worker.lock", blocking=False))

            def stopped_worker_exits(_):
                patches["_stop_if_loaded"].assert_called_once()
                patches["_prepare_release"].assert_not_called()
                holder.close()

            with mock.patch.object(installation.time, "sleep", side_effect=stopped_worker_exits) as sleep:
                self.assertTrue(installation.install()["installed"])
            sleep.assert_called_once()
        patches["_prepare_release"].assert_called_once()

    def test_install_refuses_other_or_unproved_work(self):
        installation._require_idle(self.store, allow_cancelled_validation=True)
        with self.assertRaises(ManagerError):
            installation._require_idle(self.store)  # Ordinary service stop stays idle-only.
        for changed in ({"phase": "checking"}, {"stopped_phase": "deploying"},
                        {"attempts": [{}]}, {"model_unresolved": True}, {"accepted": True},
                        {"acceptance_intent": {"new": "candidate"}},
                        {"deployment_request": {"request_id": "deployment"}}):
            with self.subTest(changed=changed):
                self.store.save(self.job | changed)
                with self.assertRaises(ManagerError):
                    installation._require_idle(self.store, allow_cancelled_validation=True)
        self.store.save(self.job)
        self.store.db.execute("UPDATE jobs SET cancel_requested=0")
        with self.assertRaises(ManagerError):
            installation._require_idle(self.store, allow_cancelled_validation=True)
        self.store.db.execute("UPDATE jobs SET cancel_requested=1")
        self.store.set("paused", False)
        with self.assertRaises(ManagerError):
            installation._require_idle(self.store, allow_cancelled_validation=True)
        self.store.set("paused", True)
        for result in ({}, self.result | {"exit_code": True},
                       self.result | {"request": "another-request.json"}):
            with self.subTest(result=result):
                self.result_path.write_text(json.dumps(result))
                with self.assertRaises(ManagerError):
                    installation._require_idle(self.store, allow_cancelled_validation=True)
        self.result_path.write_text(json.dumps(self.result))
        request = self.request | {"command": self.request["command"][:6] + ["other-candidate", "--json"]}
        self.request_path.write_text(json.dumps(request))
        with self.assertRaises(ManagerError):
            installation._require_idle(self.store, allow_cancelled_validation=True)

    def test_recovery_cancels_exited_validator_without_aggregate_or_rerun(self):
        worker = Worker.__new__(Worker)
        worker.store, worker.root, worker.repository = self.store, self.store.root, self.root
        self.store.set("recovery_request", self.job["id"])
        worker.blocked(self.job)
        artifacts = {path: path.read_bytes() for path in self.directory.iterdir()}
        with mock.patch("ci_manager.manager.git.ensure_worktree"), \
                mock.patch("ci_manager.manager.subprocess.run") as run, \
                mock.patch("ci_manager.manager.send_email", return_value={"accepted": True}):
            worker.checking(self.job)
            self.assertEqual(self.job["outcome"], "cancelled")
            self.assertFalse(self.job["unresolved"])
            worker.notifying(self.job)
            worker.notifying(self.job)
        run.assert_not_called()
        self.assertEqual(self.store.job(self.job["id"])["phase"], "cancelled")
        self.assertEqual(self.job["validations"], [])
        self.assertTrue(self.store.get("paused"))
        for path, content in artifacts.items():
            self.assertEqual(path.read_bytes(), content)

    def test_cancellation_does_not_hide_missing_terminal_evidence(self):
        self.result_path.unlink()
        worker = Worker.__new__(Worker)
        worker.store, worker.root, worker.repository = self.store, self.store.root, self.root
        with mock.patch("ci_manager.manager.git.ensure_worktree"), \
                mock.patch("ci_manager.manager.subprocess.run") as run:
            with self.assertRaisesRegex(ManagerError, "no terminal child receipt"):
                worker.checking(self.job)
        run.assert_not_called()
        self.assertEqual(self.store.job(self.job["id"])["phase"], "blocked")


class WorkerDeploymentTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-deployment-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.store = Store(self.root / "state", create=True)
        self.addCleanup(self.store.db.close)
        self.store.set("paused", False)
        self.job = {
            "id": "fixture-job", "phase": "deploying", "cancel_requested": False,
            "candidate_commit": "b" * 40, "accepted": True,
            "policy": {"luna_attempts": 3, "terra_attempts": 1}, "attempts": [],
            "deployment_request": {"request_id": "ci:fixture-job:deployment:1",
                                   "source_commit": "b" * 40, "products": ["clew"]},
        }
        self.store.db.execute(
            "INSERT INTO jobs(id,submission_key,phase,created,updated,data) VALUES (?,?,?,0,0,?)",
            (self.job["id"], "fixture", "deploying", json.dumps(self.job)),
        )
        self.worker = Worker.__new__(Worker)
        self.worker.store, self.worker.root = self.store, self.store.root
        self.absent = {"schema": 1, "request_id": self.job["deployment_request"]["request_id"],
                       "state": "not_found", "operation_state": "not_found", "exit_code": 0}
        self.stopped = {"schema": 1, "state": "stopped", "exit_code": 1,
                        "detail": "clew cannot use the committed clockwork candidate; required interface contracts are unavailable"}
        self.worker.deployment_read = mock.Mock(side_effect=[self.absent, self.absent])
        self.worker.process = mock.Mock(return_value=({"exit_code": 1}, json.dumps(self.stopped).encode(), b""))

    def test_stopped_start_with_absent_operation_finishes_once_and_retains_cause(self):
        request = self.job["deployment_request"].copy()
        self.worker.deploying(self.job)
        self.assertEqual(self.job["phase"], "notifying")
        self.assertEqual(self.job["outcome"], "failed")
        self.assertFalse(self.job["unresolved"])
        self.assertTrue(self.store.get("paused"))
        self.assertEqual(self.job["deployment_request"], request)
        self.assertEqual(self.job["deployment_result"], self.stopped)
        self.assertEqual(json.loads((self.worker.directory(self.job) / "deployment.json").read_text()), self.stopped)
        self.assertIn(self.stopped["detail"], self.job["notification"]["body"])
        with mock.patch("ci_manager.manager.send_email", return_value={"accepted": True}):
            self.worker.notifying(self.job)
            self.worker.notifying(self.job)
        self.assertEqual(self.store.job(self.job["id"])["phase"], "failed")
        self.assertIsNone(self.store.active())
        self.worker.process.assert_called_once()
        self.assertEqual(self.worker.deployment_read.call_args_list, [mock.call(self.job, "status")] * 2)

    def test_stopped_reply_preserves_admitted_operation_reconciliation(self):
        request = self.job["deployment_request"]
        interrupted = {**self.absent, "state": "interrupted", "operation_state": "needs_reconciliation"}
        terminal = {"schema": 1, "request_id": request["request_id"], "source_commit": request["source_commit"],
                    "operation_state": "terminal", "state": "succeeded", "maintenance": {"state": "released"}}
        self.worker.deployment_read.side_effect = [self.absent, interrupted, interrupted]
        self.worker.process.side_effect = [
            ({"exit_code": 1}, json.dumps(self.stopped).encode(), b""),
            ({"exit_code": 0}, json.dumps(terminal).encode(), b""),
        ]
        self.worker.deploying(self.job)
        self.assertEqual(self.job["phase"], "deploying")
        self.assertNotIn("outcome", self.job)
        self.worker.deploying(self.job)
        self.assertEqual(self.job["outcome"], "succeeded")
        self.assertTrue(self.job["installation_verified"])
        self.assertEqual(self.worker.process.call_args_list[0].args[2][2], "start")
        self.assertEqual(self.worker.process.call_args_list[1].args[2][2], "reconcile")

    def test_lost_stdout_consults_retained_terminal_operation_without_restart(self):
        request = self.job["deployment_request"]
        terminal = {"schema": 1, "request_id": request["request_id"], "source_commit": request["source_commit"],
                    "operation_state": "terminal", "state": "succeeded", "maintenance": {"state": "released"}}
        self.worker.deployment_read.side_effect = [self.absent, terminal]
        self.worker.process.return_value = ({"exit_code": 1}, b"", b"lost reply")
        self.worker.deploying(self.job)
        self.assertEqual(self.job["phase"], "deploying")
        self.assertNotIn("deployment_result", self.job)
        self.worker.deploying(self.job)
        self.assertEqual(self.job["outcome"], "succeeded")
        self.worker.process.assert_called_once()

    def test_unproved_stopped_reply_does_not_claim_absent_admission(self):
        for child, result in (({"exit_code": 0}, self.stopped),
                              ({"exit_code": 1}, self.stopped | {"schema": 2}),
                              ({"exit_code": 1}, self.stopped | {"schema": True}),
                              ({"exit_code": 1}, self.stopped | {"exit_code": 2}),
                              ({"exit_code": 1}, self.stopped | {"exit_code": True}),
                              ({"exit_code": 1}, self.stopped | {"operation_state": "active"})):
            with self.subTest(child=child, result=result):
                self.worker.deployment_read.reset_mock(side_effect=True)
                self.worker.deployment_read.return_value = self.absent
                self.worker.process.return_value = (child, json.dumps(result).encode(), b"")
                self.worker.deploying(self.job)
                self.assertEqual(self.job["phase"], "deploying")
                self.assertNotIn("deployment_result", self.job)
                self.worker.deployment_read.assert_called_once_with(self.job, "status")

    def test_unrelated_status_or_receipt_cannot_establish_pre_admission_failure(self):
        for status, receipt in ((self.absent | {"request_id": "other-request"}, self.stopped),
                                (self.absent | {"schema": 2}, self.stopped),
                                (self.absent | {"schema": True}, self.stopped),
                                (self.absent, self.stopped | {"request_id": "other-request"}),
                                (self.absent, self.stopped | {"source_commit": "c" * 40})):
            with self.subTest(status=status, receipt=receipt):
                self.worker.deployment_read.side_effect = [self.absent, status]
                self.worker.process.return_value = ({"exit_code": 1}, json.dumps(receipt).encode(), b"")
                with self.assertRaisesRegex(ManagerError, "does not match the requested operation"):
                    self.worker.deploying(self.job)
                self.assertEqual(self.job["phase"], "deploying")
                self.assertNotIn("deployment_result", self.job)

    def test_contradictory_absence_receipt_cannot_establish_pre_admission_failure(self):
        for changed in ({"state": "stopped"}, {"exit_code": 1}, {"exit_code": False}, {"exit_code": None}):
            with self.subTest(changed=changed):
                self.worker.deployment_read.side_effect = [self.absent, self.absent | changed]
                with self.assertRaisesRegex(ManagerError, "did not establish an absent operation"):
                    self.worker.deploying(self.job)
                self.assertEqual(self.job["phase"], "deploying")
                self.assertNotIn("deployment_result", self.job)

    def test_nonzero_status_exit_cannot_establish_absence_but_retains_terminal_failure(self):
        with mock.patch("ci_manager.manager.subprocess.run", return_value=subprocess.CompletedProcess(
                [], 1, json.dumps(self.absent).encode(), b"")):
            with self.assertRaisesRegex(ManagerError, "did not establish an absent operation"):
                Worker.deployment_read(self.worker, self.job, "status")
        terminal = {**self.absent, "state": "stopped", "operation_state": "terminal", "exit_code": 1}
        with mock.patch("ci_manager.manager.subprocess.run", return_value=subprocess.CompletedProcess(
                [], 1, json.dumps(terminal).encode(), b"")):
            self.assertEqual(Worker.deployment_read(self.worker, self.job, "status"), terminal)


class WorkerValidationTests(unittest.TestCase):
    def test_validation_calls_internal_runner_with_fixed_base_after_repair(self):
        with tempfile.TemporaryDirectory(prefix="cell-ci-worker-test-") as temporary:
            directory = Path(temporary)
            worktree = directory / "private worktree"
            repository = directory / "repository"
            base = "a" * 40
            candidates = ("b" * 40, "c" * 40)
            worker = Worker.__new__(Worker)
            worker.repository = repository
            worker.store = mock.Mock()
            worker.store.job.return_value = {"cancel_requested": False}
            worker.worktree = mock.Mock(return_value=worktree)
            worker.directory = mock.Mock(return_value=directory)
            worker.process = mock.Mock(side_effect=[
                ({"exit_code": 0}, json.dumps({
                    "schema_version": 1, "base_commit": base,
                    "candidate_commit": candidate, "state": "passed", "gates": [],
                }).encode(), b"") for candidate in candidates
            ])
            job = {"id": "fixture-job", "base_commit": base, "validations": []}
            with mock.patch("ci_manager.manager.git.ensure_worktree") as ensure, \
                    mock.patch("ci_manager.manager.git.clean_candidate") as clean:
                for candidate in candidates:
                    job["candidate_commit"] = candidate
                    worker.checking(job)

            self.assertEqual(worker.process.call_args_list, [
                mock.call(job, f"validation-{ordinal}", [
                    sys.executable, str(worktree / "pipeline/select_changes.py"), "run",
                    "--base", base, "--candidate", candidate, "--json",
                ], worktree) for ordinal, candidate in enumerate(candidates)
            ])
            self.assertEqual(ensure.call_args_list, [
                mock.call(repository, worktree, candidate) for candidate in candidates
            ])
            self.assertEqual(clean.call_args_list, [
                mock.call(worktree, candidate) for candidate in candidates
            ])
            self.assertEqual(worker.store.save.call_count, 2)
            worker.store.save.assert_called_with(job, "accepting")
            self.assertEqual([item["candidate"] for item in job["validations"]], list(candidates))
            for ordinal, candidate in enumerate(candidates):
                receipt = json.loads((directory / f"validation-{ordinal}.json").read_text())
                self.assertEqual(receipt["base_commit"], base)
                self.assertEqual(receipt["candidate_commit"], candidate)


if __name__ == "__main__":
    unittest.main()
