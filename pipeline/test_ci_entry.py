"""Prove CI entry routing and worker validation without live manager effects."""

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

from ci_manager.manager import Worker


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
