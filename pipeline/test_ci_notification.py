"""User-facing outcome accuracy and frozen-notification compatibility."""

from pathlib import Path
import sys
import tempfile
import unittest
from unittest import mock

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from ci_manager.manager import Worker
from ci_manager.notification import render


class NotificationTests(unittest.TestCase):
    def job(self, **changes):
        return dict(outcome="succeeded", phase="deploying", stopped_phase="deploying",
                    id="f3c9ee0348640abaf34dffb8472ad70", input_commit="a" * 40,
                    base_commit="b" * 40, candidate_commit="c" * 40,
                    **changes)

    def test_success_is_short_and_has_no_internal_bookkeeping(self):
        job = self.job(installation_verified=True, deployment_result={
            "state": "succeeded", "products": ["annals", "decisions"]})
        subject, body = render(job)
        self.assertEqual(subject, "Cell CI: deployed — Annals, Krisis")
        self.assertEqual(body, "Annals, Krisis deployed successfully.\nRequired checks and deployment verification passed.")
        for value in (job["id"], job["input_commit"], "Artifacts", "Models", "budget"):
            self.assertNotIn(value, subject + body)

    def test_failed_tests_survive_exhausted_repair_summary(self):
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "validation.log"
            log.write_text("test inbox::recovery ... FAILED\nFAIL: test_retry (InboxTests)\n")
            job = self.job(attempts=[{}], validations=[{"diagnostics": str(log)}],
                           last_receipt={"failure": {"gate": "annals", "message": "body exited nonzero"}})
            job.update(outcome="failed", stopped_phase="repair_prepare",
                       outcome_message="The repair budget is exhausted.")
            subject, body = render(job)
            self.assertEqual(subject, "Cell CI: failed — Annals checks")
            self.assertIn("inbox::recovery", body)
            self.assertIn("test_retry (InboxTests)", body)
            self.assertIn("Automatic repair did not resolve", body)
            self.assertIn("Nothing was deployed.", body)
            self.assertIn("queue is paused", body)
            self.assertNotIn("budget", body)

    def test_missing_diagnostics_does_not_invent_a_cause(self):
        job = self.job(last_receipt={"failure": {"gate": "cell.platform.pipeline", "message": "body exited nonzero"}},
                       validations=[{"diagnostics": "/missing/log"}])
        job.update(outcome="failed", stopped_phase="checking")
        subject, body = render(job)
        self.assertIn("CI pipeline checks", subject)
        self.assertIn("no specific cause", body)

    def test_failed_deployment_does_not_claim_partial_success_or_no_effects(self):
        job = self.job(accepted=True, unresolved=True, deployment_result={
            "state": "stopped", "products": ["annals", "nucleus"],
            "detail": "annals verify failed", "recovery": {"state": "failed"}})
        job["outcome"] = "failed"
        subject, body = render(job)
        self.assertIn("failed — deployment", subject)
        self.assertIn("annals verify failed", body)
        self.assertIn("Required checks passed", body)
        self.assertIn("recovery also failed", body)
        self.assertNotIn("Nothing was deployed", body)
        self.assertNotIn("deployed successfully", body)

    def test_cleanup_failure_is_visible_in_subject(self):
        job = self.job(installation_verified=True, deployment_result={"state": "cleanup_failed", "products": ["annals"]})
        subject, body = render(job)
        self.assertIn("deployed; cleanup failed", subject)
        self.assertIn("deployed successfully", body)
        self.assertNotIn("queue is paused", body)

    def test_machine_references_are_not_exposed_in_errors(self):
        job = self.job()
        job.update(outcome="failed", stopped_phase="checking",
                   outcome_message=f"Interrupted {job['id']} at {job['input_commit']}; inspect /Users/joey/Library/Application Support/Cell/private")
        subject, body = render(job)
        for value in (job["id"], job["input_commit"], "/Users/", "Support/Cell"):
            self.assertNotIn(value, subject + body)

    def test_no_deployment_and_already_included_are_distinct(self):
        job = self.job()
        self.assertIn("No deployment was required", render(job)[1])
        job["outcome"] = "already_included"
        subject, body = render(job)
        self.assertIn("already included", subject)
        self.assertIn("No new checks or deployment ran", body)
        self.assertNotIn("passed", body)

    def test_cancelled_before_deployment(self):
        job = self.job()
        job.update(outcome="cancelled", stopped_phase="checking")
        subject, body = render(job)
        self.assertIn("cancelled", subject)
        self.assertIn("Nothing was deployed", body)
        self.assertIn("queue is paused", body)

    def test_finish_preserves_frozen_payload_and_key(self):
        worker = Worker.__new__(Worker)
        worker.store = mock.Mock()
        job = self.job(notification={"subject": "old subject", "body": "old body", "key": "old key"})
        original = job["notification"].copy()
        worker.finish(job, "succeeded", "new message")
        self.assertEqual(job["notification"], original)
        worker.store.save.assert_called_once_with(job, "notifying")


if __name__ == "__main__":
    unittest.main()
