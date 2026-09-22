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
                           "--accepted-baseline", "HEAD"), ("install",)):
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
