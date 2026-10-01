"""Check mechanical repair authority and retained-candidate replay in memory."""

from contextlib import ExitStack
import copy
import hashlib
import json
from pathlib import Path
import sys
import unittest
from unittest import mock

from ci_manager.manager import Worker
from ci_manager.process import validation_exited
from ci_manager.storage import ManagerError


PATCH = b"diff --git a/source.rs b/source.rs\n--- a/source.rs\n+++ b/source.rs\n@@ -1 +1 @@\n-old\n+new\n"


class WorkerAutofixTests(unittest.TestCase):
    def setUp(self):
        self.directory = Path("/evidence/job")
        self.worktree = self.directory / "worktree"
        self.patch = self.directory / "validation-0.autofix.patch"
        self.parent = "b" * 40
        self.fixed = "c" * 40
        self.worker = Worker.__new__(Worker)
        self.worker.repository = Path("/evidence/repository")
        self.worker.store = mock.Mock()
        self.worker.store.job.return_value = {"cancel_requested": False}
        self.worker.directory = mock.Mock(return_value=self.directory)
        self.worker.worktree = mock.Mock(return_value=self.worktree)
        self.worker.process = mock.Mock()
        self.worker.finish = mock.Mock()
        self.worker.nucleus = mock.Mock()
        self.job = {"id": "fixture", "phase": "checking", "cancel_requested": False,
                    "base_commit": "a" * 40, "candidate_commit": self.parent,
                    "validations": [], "attempts": []}
        self.saved = []

        def save(job, phase=None):
            if phase is not None:
                job["phase"] = phase
            self.saved.append(copy.deepcopy(job))

        self.worker.store.save.side_effect = save
        stack = ExitStack()
        self.addCleanup(stack.close)
        self.exists = stack.enter_context(mock.patch.object(Path, "is_file", return_value=True))
        self.read = stack.enter_context(mock.patch.object(Path, "read_bytes", return_value=PATCH))
        self.json_write = stack.enter_context(mock.patch("ci_manager.manager.atomic_json"))
        self.bytes_write = stack.enter_context(mock.patch("ci_manager.manager.atomic_bytes"))
        self.ensure = stack.enter_context(mock.patch("ci_manager.manager.git.ensure_worktree"))
        self.clean = stack.enter_context(mock.patch("ci_manager.manager.git.clean_candidate"))
        self.patch_tree = stack.enter_context(mock.patch("ci_manager.manager.git.patch_tree", return_value="tree"))
        self.commit_tree = stack.enter_context(mock.patch("ci_manager.manager.git.commit_tree", return_value=self.fixed))
        self.advance = stack.enter_context(mock.patch("ci_manager.manager.git.advance"))
        self.tree_value = stack.enter_context(mock.patch("ci_manager.manager.git.value", return_value="parent-tree"))
        stack.enter_context(mock.patch("ci_manager.manager.time.time", return_value=123))

    def receipt(self, state="autofix", **changes):
        value = {"schema_version": 1, "state": state,
                 "base_commit": self.job["base_commit"], "candidate_commit": self.parent,
                 "selection": {"tests_skipped": False}, "gates": []}
        if state == "autofix":
            value["autofix_patch"] = str(self.patch)
        value.update(changes)
        return value

    def checking(self, receipt=None, exit_code=0):
        self.worker.process.return_value = (
            {"exit_code": exit_code}, json.dumps(receipt or self.receipt()).encode(), b"diagnostics")
        self.worker.checking(self.job)

    def mechanical_job(self):
        self.job["phase"] = "autofixing"
        self.job["validations"] = [{"candidate": self.parent, "state": "autofix",
                                    "patch": str(self.patch), "stamp": 123,
                                    "patch_digest": hashlib.sha256(PATCH).hexdigest()}]
        return self.job

    def test_mechanical_receipt_retains_evidence_without_model_attempt(self):
        receipt = self.receipt()
        self.checking(receipt)
        command = self.worker.process.call_args.args[2]
        index = command.index("--autofix-patch")
        self.assertEqual(command[index + 1], str(self.patch))
        self.assertEqual(self.job["phase"], "autofixing")
        validation = self.job["validations"][-1]
        self.assertEqual(validation["candidate"], self.parent)
        self.assertEqual(validation["patch"], str(self.patch))
        self.assertEqual(validation["stamp"], 123)
        self.assertEqual(validation["patch_digest"], hashlib.sha256(PATCH).hexdigest())
        self.json_write.assert_called_once_with(self.directory / "validation-0.json", receipt)
        self.bytes_write.assert_called_once_with(self.directory / "validation-0.log", b"diagnostics")
        self.clean.assert_called_once_with(self.worktree, self.parent)
        self.assertEqual(self.job["attempts"], [])
        self.assertEqual(self.worker.nucleus.mock_calls, [])
        self.worker.finish.assert_not_called()

    def test_old_candidate_and_failed_receipt_keep_model_repair_route(self):
        self.exists.return_value = False
        self.checking(self.receipt("failed"), exit_code=1)
        self.assertNotIn("--autofix-patch", self.worker.process.call_args.args[2])
        self.assertEqual(self.job["phase"], "repair_prepare")
        self.assertNotIn("patch", self.job["validations"][-1])
        self.commit_tree.assert_not_called()

    def test_autofix_receipt_cannot_name_another_patch(self):
        with self.assertRaises(ManagerError):
            self.checking(self.receipt(autofix_patch="/unrelated/fixes.patch"))
        self.assertNotEqual(self.job["phase"], "autofixing")
        self.commit_tree.assert_not_called()

    def test_autofix_receipt_requires_retained_nonempty_patch(self):
        for missing in (True, False):
            with self.subTest(missing=missing):
                self.exists.return_value = not missing
                self.read.return_value = b"" if not missing else PATCH
                with self.assertRaises(ManagerError):
                    self.checking()
                self.assertNotEqual(self.job["phase"], "autofixing")
        self.commit_tree.assert_not_called()

    def test_mechanical_commit_retains_parent_and_revalidates_without_budget_use(self):
        self.mechanical_job()
        self.worker.autofixing(self.job)
        self.patch_tree.assert_called_once_with(
            self.worker.repository, self.parent, PATCH, self.directory / "autofix.index")
        self.commit_tree.assert_called_once_with(
            self.worker.repository, "tree", [self.parent], "fixture", 123,
            "Apply mechanical CI fixes 1")
        self.advance.assert_called_once_with(
            self.worker.repository, "refs/ci/jobs/fixture/candidate", self.parent, self.fixed)
        self.ensure.assert_called_once_with(self.worker.repository, self.worktree, self.fixed)
        self.assertEqual(self.saved[0]["validations"][-1]["candidate_after_fixes"], self.fixed)
        self.assertEqual(self.saved[0]["candidate_commit"], self.parent)
        self.assertEqual(self.job["candidate_commit"], self.fixed)
        self.assertEqual(self.job["phase"], "checking")
        self.assertEqual(self.job["attempts"], [])
        self.assertEqual(self.worker.nucleus.mock_calls, [])

    def test_replay_after_recording_commit_before_ref_advancement(self):
        self.mechanical_job()
        self.advance.side_effect = [OSError("interrupted before ref update"), None]
        with self.assertRaisesRegex(OSError, "interrupted before ref update"):
            self.worker.autofixing(self.job)
        retained = copy.deepcopy(self.saved[-1])
        self.assertEqual(retained["validations"][-1]["candidate_after_fixes"], self.fixed)
        self.assertEqual(retained["candidate_commit"], self.parent)
        self.worker.autofixing(retained)
        self.assertEqual(retained["candidate_commit"], self.fixed)
        self.assertEqual(retained["phase"], "checking")
        self.assertEqual(self.advance.call_count, 2)
        self.assertEqual(self.advance.call_args_list[0], self.advance.call_args_list[1])
        self.assertTrue(all(call.args[4] == 123 for call in self.commit_tree.call_args_list))

    def test_changed_retained_patch_stops_before_commit(self):
        self.mechanical_job()
        self.read.return_value = PATCH + b"tampered\n"
        with self.assertRaises(ManagerError):
            self.worker.autofixing(self.job)
        self.patch_tree.assert_not_called()
        self.commit_tree.assert_not_called()
        self.advance.assert_not_called()

    def test_patch_for_another_candidate_stops_before_commit(self):
        self.mechanical_job()
        self.job["candidate_commit"] = "d" * 40
        with self.assertRaises(ManagerError):
            self.worker.autofixing(self.job)
        self.patch_tree.assert_not_called()
        self.commit_tree.assert_not_called()
        self.advance.assert_not_called()

    def test_replay_after_ref_advancement_before_worktree_materialization(self):
        self.mechanical_job()
        reference = {"candidate": self.parent}

        def advance(_repository, _name, old, new):
            self.assertIn(reference["candidate"], (old, new))
            reference["candidate"] = new

        self.advance.side_effect = advance
        self.ensure.side_effect = [OSError("interrupted after ref update"), None]
        with self.assertRaisesRegex(OSError, "interrupted after ref update"):
            self.worker.autofixing(self.job)
        self.assertEqual(reference["candidate"], self.fixed)
        retained = copy.deepcopy(self.saved[-1])
        self.assertEqual(retained["candidate_commit"], self.parent)
        self.worker.autofixing(retained)
        self.assertEqual(retained["candidate_commit"], reference["candidate"])
        self.assertEqual(retained["phase"], "checking")
        self.assertEqual(self.ensure.call_count, 2)
        self.assertEqual(self.ensure.call_args_list[0], self.ensure.call_args_list[1])

    def test_cancellation_stops_before_mechanical_commit(self):
        self.mechanical_job()
        self.job["cancel_requested"] = True
        with mock.patch("ci_manager.manager.workspace.root"):
            self.worker.step(self.job)
        self.worker.finish.assert_called_once_with(
            self.job, "cancelled", "Cancelled before the next external operation.")
        self.patch_tree.assert_not_called()
        self.advance.assert_not_called()


class ValidationExitAutofixTests(unittest.TestCase):
    def test_exit_evidence_requires_the_exact_optional_patch_path(self):
        directory = Path("/evidence/job")
        worktree = directory / "worktree"
        name = "validation-0"
        request_path = directory / f"{name}.request.json"
        patch_path = directory / f"{name}.autofix.patch"
        for skip_tests in (False, True):
            for suffix, matches in (([], True),
                                    (["--autofix-patch", str(patch_path)], True),
                                    (["--autofix-patch", "/unrelated/fixes.patch"], False),
                                    (["--autofix-patch", str(patch_path), "--extra"], False)):
                with self.subTest(skip_tests=skip_tests, suffix=suffix):
                    job = {"base_commit": "a" * 40, "candidate_commit": "b" * 40,
                           "validations": [], "skip_tests": skip_tests}
                    command = [sys.executable, str(worktree / "pipeline/select_changes.py"),
                               "run", "--base", job["base_commit"],
                               "--candidate", job["candidate_commit"], "--json"]
                    if skip_tests:
                        command.append("--skip-tests")
                    request = {"command": command + suffix, "cwd": str(worktree),
                               **{key: str(directory / f"{name}.{extension}")
                                  for key, extension in (("stdout", "stdout"), ("stderr", "stderr"),
                                                         ("started", "started.json"), ("result", "result.json"))}}
                    result = {"request": str(request_path), "exit_code": 0}
                    records = {str(request_path): request,
                               str(directory / f"{name}.result.json"): result}

                    def read(path, *_args, **_kwargs):
                        return json.dumps(records[str(path)])

                    with mock.patch.object(Path, "exists", return_value=True), mock.patch.object(
                            Path, "read_text", autospec=True, side_effect=read):
                        self.assertEqual(validation_exited(directory, job), matches)


if __name__ == "__main__":
    unittest.main()
