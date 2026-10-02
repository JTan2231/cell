"""Check settled-job cleanup with real Git worktrees and durable replay."""

import json
from pathlib import Path
import shutil
import tempfile
import time
import unittest
from unittest import mock

from ci_manager import git_ops as git
from ci_manager.manager import Worker
from ci_manager.storage import ManagerError, Store


class WorktreeCleanupTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.repository = self.root / "repository"
        self.repository.mkdir()
        git.git(self.repository, "init", "-q")
        (self.repository / "source").write_text("original\n")
        (self.repository / ".gitignore").write_text("build/\n")
        git.git(self.repository, "add", ".")
        git.git(self.repository, "-c", "user.name=Test", "-c", "user.email=test@cell.local",
                "commit", "-m", "fixture")
        self.commit = git.commit(self.repository, "HEAD")
        self.store = Store(self.root / "state", create=True)
        self.addCleanup(self.store.db.close)
        self.worker = Worker.__new__(Worker)
        self.worker.store = self.store
        self.worker.root = self.store.root
        self.worker.repository = self.repository
        self.worker.nucleus = mock.Mock()

    def job(self, identity="fixture", phase="checking", **changes):
        data = {"input_commit": self.commit, "candidate_commit": self.commit,
                "policy": {"luna_attempts": 3, "terra_attempts": 1},
                "attempts": [], "validations": [], "model_unresolved": False, **changes}
        stamp = time.time()
        self.store.db.execute("""INSERT INTO jobs
            (id,submission_key,phase,created,updated,data) VALUES (?,?,?,?,?,?)""",
            (identity, identity, phase, stamp, stamp, json.dumps(data)))
        job = self.store.job(identity)
        git.ensure_worktree(self.repository, self.worker.worktree(job), self.commit)
        return job

    def registered(self, path):
        records = git.git(self.repository, "worktree", "list", "--porcelain", "-z").stdout.split(b"\0")
        return b"worktree " + str(path).encode() in records

    def assert_removed(self, job):
        path = self.worker.worktree(job)
        self.assertFalse(path.exists())
        self.assertFalse(self.registered(path))
        self.assertEqual(self.store.job(job["id"])["worktree_cleanup"]["state"], "removed")

    def test_finish_removes_dirty_ignored_and_nested_git_files_before_email(self):
        job = self.job()
        path = self.worker.worktree(job)
        (path / "source").write_text("changed\n")
        (path / "untracked").write_text("leftover\n")
        (path / "build").mkdir()
        (path / "build" / "large-output").write_text("generated\n")
        nested = path / "nested"
        nested.mkdir()
        git.git(nested, "init", "-q")
        evidence = self.worker.directory(job) / "validation-0.json"
        evidence.write_text('{"state":"passed"}\n')
        candidate_ref = git.private_ref(job["id"], "candidate")
        git.git(self.repository, "update-ref", candidate_ref, self.commit)
        original_remove = git.remove_worktree

        def remove(repository, worktree, registration=None):
            retained = self.store.job(job["id"])
            self.assertEqual(retained["phase"], "notifying")
            self.assertEqual(retained["notification"]["state"], "pending")
            self.assertEqual(retained["worktree_cleanup"]["state"], "pending")
            self.assertEqual(retained["worktree_cleanup"]["registration"], str(registration))
            original_remove(repository, worktree, registration)

        with mock.patch.object(git, "remove_worktree", side_effect=remove), mock.patch(
                "ci_manager.manager.send_email") as email:
            self.worker.finish(job, "succeeded", "Checks passed.")
        self.assert_removed(job)
        self.assertEqual(evidence.read_text(), '{"state":"passed"}\n')
        self.assertEqual(git.commit(self.repository, candidate_ref), self.commit)
        email.assert_not_called()

    def test_all_settled_outcomes_remove_the_tree(self):
        for outcome in ("succeeded", "failed", "cancelled", "already_included"):
            with self.subTest(outcome=outcome):
                job = self.job(identity=outcome)
                self.worker.finish(job, outcome, "Settled.")
                self.assert_removed(job)
                self.assertEqual(self.store.job(outcome)["outcome"], outcome)

    def test_unresolved_failure_keeps_tree_and_registration(self):
        job = self.job()
        self.worker.finish(job, "failed", "Child state is unresolved.", unresolved=True)
        self.worker.cleanup_finished_worktrees()
        path = self.worker.worktree(job)
        self.assertTrue(path.is_dir())
        self.assertTrue(self.registered(path))
        self.assertNotIn("worktree_cleanup", self.store.job(job["id"]))

    def test_startup_sweeps_historical_outcomes_and_email_only_blocked_jobs(self):
        jobs = [self.job(identity=phase, phase=phase) for phase in
                ("succeeded", "failed", "cancelled", "already_included")]
        jobs += [self.job(identity="pending-email", phase="notifying", outcome="succeeded"),
                 self.job(identity="uncertain-email", phase="blocked", outcome="failed",
                          notification_blocked=True)]
        with mock.patch("ci_manager.manager.workspace.root"), mock.patch.object(
                self.worker, "claim", return_value=None), mock.patch(
                "ci_manager.manager.sleep", side_effect=RuntimeError("stop fixture loop")):
            with self.assertRaisesRegex(RuntimeError, "stop fixture loop"):
                self.worker.run()
        for job in jobs:
            self.assert_removed(job)
        with mock.patch.object(git, "remove_worktree") as remove:
            self.worker.cleanup_finished_worktrees()
        remove.assert_not_called()

    def test_sweep_preserves_active_and_unresolved_effects(self):
        jobs = [self.job(identity=phase, phase=phase) for phase in
                ("queued", "integrating", "checking", "repair_prepare", "repair_wait",
                 "applying", "autofixing", "preparing", "accepting", "deploying")]
        jobs += [self.job(identity="blocked-child", phase="blocked", outcome="failed", unresolved=True),
                 self.job(identity="blocked-model", phase="blocked", outcome="failed", model_unresolved=True),
                 self.job(identity="notifying-child", phase="notifying", outcome="failed", unresolved=True),
                 self.job(identity="terminal-model", phase="failed", model_unresolved=True),
                 self.job(identity="unknown-blocked", phase="blocked")]
        with mock.patch.object(git, "remove_worktree") as remove:
            self.worker.cleanup_finished_worktrees()
        remove.assert_not_called()
        for job in jobs:
            path = self.worker.worktree(job)
            self.assertTrue(path.is_dir())
            self.assertTrue(self.registered(path))

    def test_cleanup_failure_is_retained_then_retried_without_changing_outcome(self):
        job = self.job()
        with mock.patch.object(git, "remove_worktree", side_effect=OSError("volume busy")):
            self.worker.finish(job, "failed", "Checks failed.")
        retained = self.store.job(job["id"])
        self.assertEqual(retained["phase"], "notifying")
        self.assertEqual(retained["outcome"], "failed")
        self.assertEqual(retained["worktree_cleanup"]["state"], "failed")
        self.assertEqual(retained["worktree_cleanup"]["error"], "volume busy")
        notice = retained["notification"]
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)
        retained = self.store.job(job["id"])
        self.assertEqual(retained["notification"], notice)
        self.assertEqual(retained["outcome_message"], "Checks failed.")
        self.assertNotIn("error", retained["worktree_cleanup"])

    def test_crash_after_finished_journal_save_replays_cleanup(self):
        job = self.job()
        with mock.patch.object(self.worker, "cleanup_worktree", side_effect=OSError("crash")):
            with self.assertRaisesRegex(OSError, "crash"):
                self.worker.finish(job, "cancelled", "Drained.")
        self.assertEqual(self.store.job(job["id"])["phase"], "notifying")
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)

    def test_crash_after_removal_before_cleanup_receipt_replays_safely(self):
        job = self.job(phase="succeeded")
        original_save = self.store.save

        def save(job, phase=None):
            if job.get("worktree_cleanup", {}).get("state") == "removed":
                raise OSError("crash")
            original_save(job, phase)

        with mock.patch.object(self.store, "save", side_effect=save):
            with self.assertRaisesRegex(OSError, "crash"):
                self.worker.cleanup_worktree(job)
        self.assertFalse(self.worker.worktree(job).exists())
        self.assertEqual(self.store.job(job["id"])["worktree_cleanup"]["state"], "pending")
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)

    def test_crash_during_git_admin_deletion_removes_unlisted_remainder(self):
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        registration = git.worktree_registration(self.repository, path)
        original_git = git.git

        def interrupted(root, *args, **kwargs):
            if args[:2] == ("worktree", "remove"):
                (registration / "gitdir").unlink()
                raise OSError("crash during registration removal")
            return original_git(root, *args, **kwargs)

        with mock.patch.object(git, "git", side_effect=interrupted):
            self.worker.cleanup_worktree(job)
        retained = self.store.job(job["id"])
        self.assertEqual(retained["worktree_cleanup"]["registration"], str(registration))
        self.assertFalse(path.exists())
        self.assertFalse(self.registered(path))
        self.assertTrue(registration.is_dir())
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)
        self.assertFalse(registration.exists())

    def test_recorded_registration_cannot_remove_other_git_metadata(self):
        common = git.common(self.repository)
        for ordinal, registration in enumerate((common, common / "worktrees" / "..")):
            with self.subTest(registration=registration):
                job = self.job(identity=f"unsafe-{ordinal}", phase="succeeded",
                               worktree_cleanup={"state": "pending", "registration": str(registration)})
                self.worker.cleanup_worktree(job)
                self.assertEqual(self.store.job(job["id"])["worktree_cleanup"]["state"], "failed")
                self.assertTrue(self.worker.worktree(job).is_dir())
                self.assertTrue((self.repository / ".git").is_dir())

    def test_registration_journal_failure_preserves_files_and_git_metadata(self):
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        with mock.patch.object(self.store, "save", side_effect=OSError("journal unavailable")), mock.patch.object(
                git, "remove_worktree") as remove:
            with self.assertRaisesRegex(OSError, "journal unavailable"):
                self.worker.cleanup_worktree(job)
        remove.assert_not_called()
        self.assertTrue(path.is_dir())
        self.assertTrue(self.registered(path))

    def test_missing_directory_removes_only_its_registration(self):
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        other = self.root / "unrelated-worktree"
        git.ensure_worktree(self.repository, other, self.commit)
        shutil.rmtree(path)
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)
        self.assertTrue(other.is_dir())
        self.assertTrue(self.registered(other))

    def test_settled_locked_worktree_is_completely_removed(self):
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        git.git(self.repository, "worktree", "lock", "--reason", "retained CI tree", str(path))
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)

    def test_partial_directory_removal_without_git_file_replays_safely(self):
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        (path / ".git").unlink()
        self.assertTrue(self.registered(path))
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)

    def test_unregistered_remainder_is_completely_removed(self):
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        git.git(self.repository, "worktree", "remove", "--force", str(path))
        (path / "build").mkdir(parents=True)
        (path / "build" / "leftover").write_text("unfinished removal\n")
        self.worker.cleanup_finished_worktrees()
        self.assert_removed(job)

    def test_main_worktree_and_symbolic_replacement_are_refused(self):
        with self.assertRaisesRegex(ManagerError, "main worktree"):
            git.remove_worktree(self.repository, self.repository)
        job = self.job(phase="succeeded")
        path = self.worker.worktree(job)
        git.git(self.repository, "worktree", "remove", "--force", str(path))
        path.symlink_to(self.repository, target_is_directory=True)
        self.worker.cleanup_finished_worktrees()
        retained = self.store.job(job["id"])
        self.assertEqual(retained["worktree_cleanup"]["state"], "failed")
        self.assertTrue(path.is_symlink())
        self.assertTrue(self.repository.is_dir())


if __name__ == "__main__":
    unittest.main()
