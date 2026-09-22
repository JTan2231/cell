"""Exercise refundable repair budgets with private Git and queue fixtures."""

import argparse
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock


SOURCE = Path(__file__).resolve().parent.parent
sys.dont_write_bytecode = True
sys.path.insert(0, str(SOURCE))

from ci_manager import git_ops as git
from ci_manager.budget import repair_budget
from ci_manager.client import status, submit
from ci_manager.integrations import DeferredError
from ci_manager.manager import Worker
from ci_manager.storage import ManagerError, Store


class RepairBudgetTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-budget-test-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repository = self.root / "repository"
        self.repository.mkdir()
        git.git(self.repository, "init", "--quiet")
        git.git(self.repository, "config", "user.name", "CI fixture")
        git.git(self.repository, "config", "user.email", "ci-fixture@example.invalid")
        (self.repository / "fixture.txt").write_text("value 0\n")
        git.git(self.repository, "add", "fixture.txt")
        git.git(self.repository, "commit", "--quiet", "-m", "Fixture baseline")
        self.base = git.commit(self.repository, "HEAD")
        self.store = Store(self.root / "state", create=True)
        self.addCleanup(lambda: self.store.db.close())
        self.policy = {"luna_attempts": 3, "terra_attempts": 1, "model_timeout_seconds": 600}
        self.store.set("config", {
            "repository": str(self.repository),
            "common_git_dir": str(git.common(self.repository)),
            "policy": self.policy,
        })
        self.args = argparse.Namespace(repo=str(self.repository), commit=self.base,
                                       deploy=None, request_id="budget-fixture")
        self.job = submit(self.store, self.args)
        self.job.update(base_commit=self.base, candidate_commit=self.base, attempts=[],
                        validations=[{"diagnostics": str(self.root / "validation.log")}],
                        model_unresolved=False, prompts={
                            "selection_id": "fixture-prompts", "selection_version": 1,
                            "component_versions": {}, "instructions": "Return a Git patch.",
                            "prompt_template": "Repair this failure: {context}",
                        })
        self.store.save(self.job, "repair_prepare")
        git.git(self.repository, "update-ref", git.private_ref(self.job["id"], "candidate"), self.base)
        self.worker = self.make_worker()
        self.value = 0

    def make_worker(self):
        with mock.patch("ci_manager.manager.NucleusClient"):
            return Worker(self.store, -1)

    def assert_budget(self, *, used, refunded, invocations, mode="refund_accepted"):
        expected = {"total": 4, "used": used, "remaining": max(0, 4 - used),
                    "refunded": refunded, "invocations": invocations, "mode": mode}
        self.assertEqual(repair_budget(self.job), expected)
        self.assertEqual(status(self.store, self.job["id"])["repair_budget"], expected)

    def prepare(self):
        self.worker.repair_prepare(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        return self.job["attempts"][-1]

    def complete(self, patch=None, *, state="completed", attempt_state=None):
        attempt = self.job["attempts"][-1]
        view = {
            "version": 1,
            "summary": {"state": state, "currentAttemptId": "provider-attempt"},
            "attempts": [{"id": "provider-attempt", "state": attempt_state or state,
                          "output": {"finalMessage": patch}}],
        }
        self.worker.nucleus.get.return_value = view
        self.worker.repair_wait(self.job)
        self.worker.nucleus.get.assert_called_with(attempt["nucleus_job_id"])
        return attempt

    def patch(self):
        return ("diff --git a/fixture.txt b/fixture.txt\n"
                "--- a/fixture.txt\n+++ b/fixture.txt\n@@ -1 +1 @@\n"
                f"-value {self.value}\n+value {self.value + 1}\n")

    def accept(self):
        attempt = self.complete(self.patch())
        self.assertEqual(self.job["phase"], "applying")
        self.worker.applying(self.job)
        self.assertEqual(self.job["phase"], "checking")
        self.value += 1
        self.assertEqual(git.value(self.repository, "show", f"{attempt['candidate']}:fixture.txt"),
                         f"value {self.value}")
        self.assertEqual(git.commit(self.repository, git.private_ref(self.job["id"], "candidate")),
                         attempt["candidate"])
        return attempt

    def fail_validation(self):
        candidate = self.job["candidate_commit"]
        receipt = {"schema_version": 1, "base_commit": self.base,
                   "candidate_commit": candidate, "state": "failed", "gates": []}
        with mock.patch.object(self.worker, "process", return_value=(
                {"exit_code": 1}, json.dumps(receipt).encode(), b"another gate failed")):
            self.worker.checking(self.job)
        self.assertEqual(self.job["phase"], "repair_prepare")

    def reject(self):
        attempt = self.complete("This response is not a Git patch.\n")
        self.worker.applying(self.job)
        self.assertEqual(self.job["phase"], "repair_prepare")
        self.assertIn("rejection", attempt)
        self.assertNotIn("candidate", attempt)

    def test_accepted_fixes_refund_past_original_limit_and_keep_unique_artifacts(self):
        artifacts = {}
        for ordinal in range(1, 8):
            attempt = self.prepare()
            self.assertEqual(attempt["number"], ordinal)
            self.assertEqual(attempt["model"], "gpt-5.6-terra")
            self.assertEqual(attempt["nucleus_job_id"], f"ci-{self.job['id']}-repair-{ordinal}")
            self.assert_budget(used=1, refunded=ordinal - 1, invocations=ordinal)
            self.accept()
            self.assert_budget(used=0, refunded=ordinal, invocations=ordinal)
            for suffix in ("request.json", "result.json", "patch"):
                path = self.worker.directory(self.job) / f"repair-{ordinal}.{suffix}"
                artifacts[path] = path.read_bytes()
            request = json.loads(Path(attempt["request"]).read_text())
            self.assertEqual(request["id"], attempt["nucleus_job_id"])
            self.fail_validation()
            self.assert_budget(used=0, refunded=ordinal, invocations=ordinal)
        self.assertEqual(len(artifacts), 21)
        for path, contents in artifacts.items():
            self.assertEqual(path.read_bytes(), contents, f"overwritten retained artifact: {path}")

    def test_one_rejection_remains_charged_after_many_accepted_fixes(self):
        self.prepare()
        self.reject()
        for ordinal in range(2, 8):
            attempt = self.prepare()
            self.assertEqual(attempt["model"], "gpt-5.6-terra")
            self.accept()
            self.fail_validation()
            self.assert_budget(used=1, refunded=ordinal - 1, invocations=ordinal)

    def test_interleaved_refunds_preserve_failures_and_escalated_model(self):
        failures = refunds = 0
        for ordinal, accepted in enumerate((False, True, False, True, False, True, True, False), 1):
            attempt = self.prepare()
            expected_model = "gpt-5.6-terra" if failures < 3 else "gpt-5.6-sol"
            self.assertEqual(attempt["model"], expected_model)
            if accepted:
                self.accept()
                refunds += 1
                self.fail_validation()
            else:
                self.reject()
                failures += 1
            self.assert_budget(used=failures, refunded=refunds, invocations=ordinal)
        self.worker.repair_prepare(self.job)
        self.assertEqual(self.job["outcome"], "failed")
        self.assertIn("exhausted", self.job["outcome_message"])
        self.assertEqual(len(self.job["attempts"]), 8)

    def test_four_rejected_patches_exhaust_budget_without_a_fifth_request(self):
        for _ in range(4):
            self.prepare()
            self.reject()
        self.assert_budget(used=4, refunded=0, invocations=4)
        self.worker.repair_prepare(self.job)
        self.assertEqual(self.job["phase"], "notifying")
        self.assertEqual(self.job["outcome"], "failed")
        self.assertEqual(self.job["notification"]["state"], "pending")
        self.assertFalse((self.worker.directory(self.job) / "repair-5.request.json").exists())

    def test_completed_proposal_is_charged_until_git_accepts_it(self):
        self.prepare()
        self.complete(self.patch())
        self.assert_budget(used=1, refunded=0, invocations=1)
        self.worker.applying(self.job)
        self.assert_budget(used=0, refunded=1, invocations=1)

    def test_missing_final_patch_does_not_refund_completed_execution(self):
        self.prepare()
        attempt = self.complete()
        self.assertEqual(self.job["phase"], "repair_prepare")
        self.assertIn("rejection", attempt)
        self.assert_budget(used=1, refunded=0, invocations=1)

    def test_failed_execution_does_not_refund_even_with_patch_output(self):
        self.prepare()
        attempt = self.complete(self.patch(), state="failed", attempt_state="timed_out")
        self.assertEqual(self.job["outcome"], "failed")
        self.assertNotIn("patch", attempt)
        self.assert_budget(used=1, refunded=0, invocations=1)

    def test_deferred_admission_preserves_one_charge_and_request_identity(self):
        attempt = self.prepare()
        request = Path(attempt["request"]).read_bytes()
        self.worker.nucleus.get.return_value = None
        self.worker.nucleus.submit.side_effect = DeferredError("Wait for quota", code="quota_deferred")
        for _ in range(3):
            self.worker.repair_wait(self.job)
            self.assertEqual(self.job["waiting_reason"], "quota_deferred")
            self.assert_budget(used=1, refunded=0, invocations=1)
        self.assertEqual(self.worker.nucleus.submit.call_args_list, [mock.call(request)] * 3)

    def test_restart_and_repeated_apply_refund_exactly_once(self):
        self.prepare()
        self.complete(self.patch())
        with mock.patch("ci_manager.manager.git.advance", side_effect=ManagerError("simulated interruption")):
            with self.assertRaisesRegex(ManagerError, "simulated interruption"):
                self.worker.applying(self.job)
        candidate = self.job["attempts"][-1]["candidate"]
        self.assert_budget(used=0, refunded=1, invocations=1)
        self.assertEqual(self.store.job(self.job["id"])["phase"], "applying")
        self.assertEqual(git.commit(self.repository, git.private_ref(self.job["id"], "candidate")), self.base)
        self.store.db.close()
        self.store = Store(self.root / "state")
        self.worker = self.make_worker()
        self.job = self.store.job(self.job["id"])
        for _ in range(2):
            self.worker.applying(self.job)
            self.job = self.store.job(self.job["id"])
            self.assert_budget(used=0, refunded=1, invocations=1)
            self.assertEqual(self.job["candidate_commit"], candidate)
            self.assertEqual(git.commit(self.repository, git.private_ref(self.job["id"], "candidate")), candidate)
        self.assertEqual(git.value(self.repository, "rev-list", "--count", f"{self.base}..{candidate}"), "1")

    def test_new_submission_freezes_refund_policy_without_changing_queue_config(self):
        self.assertIs(self.job["policy"]["refund_accepted_patches"], True)
        self.assertEqual(self.store.get("config")["policy"], self.policy)
        repeated = submit(self.store, self.args)
        self.assertEqual(repeated["id"], self.job["id"])
        self.assertEqual(repeated["policy"], self.job["policy"])
        self.assertEqual(self.store.db.execute("SELECT COUNT(*) FROM jobs").fetchone()[0], 1)

    def test_legacy_job_keeps_invocation_limit_and_idempotent_submission_policy(self):
        self.job["policy"].pop("refund_accepted_patches")
        self.store.save(self.job)
        repeated = submit(self.store, self.args)
        self.assertEqual(repeated["id"], self.job["id"])
        self.assertNotIn("refund_accepted_patches", repeated["policy"])
        for ordinal in range(1, 5):
            attempt = self.prepare()
            self.assertEqual(attempt["model"], "gpt-5.6-terra" if ordinal <= 3 else "gpt-5.6-sol")
            self.accept()
            self.fail_validation()
            self.assert_budget(used=ordinal, refunded=0, invocations=ordinal, mode="invocations")
        self.worker.repair_prepare(self.job)
        self.assertEqual(self.job["outcome"], "failed")
        self.assertEqual(len(self.job["attempts"]), 4)


if __name__ == "__main__":
    unittest.main()
