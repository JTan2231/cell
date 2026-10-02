"""Check repair recovery against the retained request and provider evidence."""

import copy
from pathlib import Path
import tempfile
import unittest
from unittest import mock

from ci_manager.budget import repair_budget
from ci_manager.integrations import TransportError, freeze_request
from ci_manager.manager import Worker


class RepairRecoveryTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        self.directory = self.root / "job"
        self.directory.mkdir(mode=0o700)
        self.identity = "ci-fixture-repair-1"
        self.request = freeze_request({"version": 1, "id": self.identity,
                                      "prompt": "Retained repair instructions"})
        request_path = self.directory / "repair-1.request.json"
        request_path.write_bytes(self.request)
        self.job = {"id": "fixture", "phase": "blocked", "stopped_phase": "repair_wait",
                    "cancel_requested": False, "model_unresolved": False, "unresolved": True,
                    "outcome": "failed", "outcome_message": "Nucleus observation unavailable.",
                    "notification": {"subject": "old", "body": "old", "key": "old"},
                    "policy": {"luna_attempts": 3, "terra_attempts": 1,
                               "refund_accepted_patches": True},
                    "attempts": [{"number": 1, "nucleus_job_id": self.identity,
                                  "request": str(request_path), "parent": "b" * 40,
                                  "model": "gpt-5.6-terra", "reasoning": "medium",
                                  "stamp": 123, "transport_failures": 5}],
                    "base_commit": "a" * 40, "candidate_commit": "b" * 40,
                    "validations": []}
        self.original_attempt = copy.deepcopy(self.job["attempts"][0])
        self.budget = repair_budget(self.job)
        self.worker = Worker.__new__(Worker)
        self.worker.root = self.root
        self.worker.directory = mock.Mock(return_value=self.directory)
        self.worker.cleanup_worktree = mock.Mock()
        self.worker.nucleus = mock.Mock()
        self.worker.store = mock.Mock()
        self.worker.store.get.return_value = self.job["id"]
        self.worker.store.owners.return_value = []
        self.saved = []

        def save(job, phase=None):
            if phase is not None:
                job["phase"] = phase
            self.saved.append(copy.deepcopy(job))

        self.worker.store.save.side_effect = save

    def view(self, state, attempt_state=None, final_message=None):
        result = {"version": 1, "summary": {"state": state}, "attempts": []}
        if attempt_state is not None:
            result["summary"]["currentAttemptId"] = "provider-attempt"
            attempt = {"id": "provider-attempt", "state": attempt_state}
            if final_message is not None:
                attempt["output"] = {"finalMessage": final_message}
            result["attempts"].append(attempt)
        return result

    def assert_same_attempt(self):
        self.assertEqual(len(self.job["attempts"]), 1)
        for key in ("nucleus_job_id", "request", "parent", "model", "reasoning", "stamp"):
            self.assertEqual(self.job["attempts"][0][key], self.original_attempt[key])
        self.assertEqual(repair_budget(self.job), self.budget)
        self.assertEqual(Path(self.original_attempt["request"]).read_bytes(), self.request)

    def test_not_found_recovers_rejected_admission_with_same_request_and_budget(self):
        self.worker.nucleus.get.return_value = None
        self.worker.blocked(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        self.assertFalse(self.job["model_unresolved"])
        self.assertEqual(self.job["attempts"][0]["transport_failures"], 0)
        self.assertNotIn("notification", self.job)
        self.assertNotIn("unresolved", self.job)
        self.assertEqual(self.job["outcome_generation"], 2)
        self.worker.nucleus.get.assert_called_once_with(self.identity)
        self.worker.nucleus.submit.assert_not_called()
        self.worker.repair_wait(self.job)
        self.worker.nucleus.submit.assert_called_once_with(self.request)
        self.assertTrue(self.job["model_unresolved"])
        self.assert_same_attempt()

    def test_running_recovery_observes_existing_execution_without_resubmission(self):
        self.worker.nucleus.get.return_value = self.view("running")
        self.worker.blocked(self.job)
        self.assertTrue(self.job["model_unresolved"])
        self.assertTrue(self.saved[-1]["model_unresolved"])
        self.worker.repair_wait(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        self.assertEqual(self.job["waiting_reason"], "nucleus")
        self.assertTrue(self.job["model_unresolved"])
        self.worker.nucleus.submit.assert_not_called()
        self.assert_same_attempt()

    def test_recovery_resets_transport_retry_budget_for_same_uncertain_request(self):
        self.worker.nucleus.get.return_value = None
        self.worker.blocked(self.job)
        self.worker.nucleus.submit.side_effect = TransportError("Submission reply unavailable", uncertain=True)
        self.worker.repair_wait(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        self.assertEqual(self.job["attempts"][0]["transport_failures"], 1)
        self.assertTrue(self.job["model_unresolved"])
        self.assertEqual(self.job["waiting_reason"], "nucleus_transport")
        self.worker.nucleus.submit.assert_called_once_with(self.request)
        self.assert_same_attempt()

    def test_completed_recovery_retains_provider_patch_and_follows_applying_phase(self):
        patch = "diff --git a/source b/source\n"
        self.worker.nucleus.get.return_value = self.view("completed", "completed", patch)
        self.worker.blocked(self.job)
        self.worker.repair_wait(self.job)
        self.assertEqual(self.job["phase"], "applying")
        self.assertFalse(self.job["model_unresolved"])
        self.assertEqual(Path(self.job["attempts"][0]["patch"]).read_text(), patch)
        self.assertTrue((self.directory / "repair-1.result.json").is_file())
        self.worker.nucleus.submit.assert_not_called()
        self.assert_same_attempt()

    def test_unavailable_or_invalid_observation_keeps_recovery_blocked(self):
        for response in (TransportError("Provider unavailable"), self.view("unknown")):
            with self.subTest(response=response):
                if isinstance(response, Exception):
                    self.worker.nucleus.get.side_effect = response
                else:
                    self.worker.nucleus.get.side_effect = None
                    self.worker.nucleus.get.return_value = response
                self.worker.blocked(self.job)
                self.assertEqual(self.job["phase"], "blocked")
                self.assertTrue(self.job["unresolved"])
                self.assertEqual(self.job["attempts"][0]["transport_failures"], 5)
                self.assertEqual(self.job["notification"]["key"], "old")
                self.worker.nucleus.submit.assert_not_called()
                self.assert_same_attempt()

    def test_lost_execution_stays_blocked_without_erasing_orphan_uncertainty(self):
        self.job["model_unresolved"] = True
        self.worker.nucleus.get.return_value = self.view("failed", "lost")
        self.worker.blocked(self.job)
        self.assertEqual(self.job["phase"], "blocked")
        self.assertTrue(self.job["unresolved"])
        self.assertTrue(self.job["model_unresolved"])
        self.assertIn("orphan containment", self.job["last_error"])
        self.worker.nucleus.submit.assert_not_called()
        self.assert_same_attempt()

    def test_recovered_absent_request_can_cancel_without_admission(self):
        self.job["cancel_requested"] = True
        self.worker.nucleus.get.return_value = None
        self.worker.blocked(self.job)
        self.worker.repair_wait(self.job)
        self.assertEqual(self.job["outcome"], "cancelled")
        self.assertEqual(self.job["phase"], "notifying")
        self.assertFalse(self.job["model_unresolved"])
        self.worker.nucleus.submit.assert_not_called()
        self.worker.nucleus.cancel.assert_not_called()
        self.assert_same_attempt()

    def test_recovered_absent_request_respects_requester_maintenance(self):
        self.worker.nucleus.get.return_value = None
        self.worker.blocked(self.job)
        self.worker.store.owners.return_value = ["maintenance-owner"]
        self.worker.repair_wait(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        self.assertEqual(self.job["waiting_reason"], "requester_maintenance")
        self.assertFalse(self.job["model_unresolved"])
        self.worker.nucleus.submit.assert_not_called()
        self.assert_same_attempt()

    def test_recovered_running_request_cancels_same_identity_and_keeps_drain_wait(self):
        self.job["cancel_requested"] = True
        self.worker.nucleus.get.return_value = self.view("running")
        self.worker.blocked(self.job)
        self.worker.repair_wait(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        self.assertTrue(self.job["model_unresolved"])
        self.worker.nucleus.cancel.assert_called_once_with(self.identity)
        self.worker.nucleus.submit.assert_not_called()
        self.assert_same_attempt()

    def test_unresolved_existing_attempt_retains_previous_recovery_route(self):
        self.job.update(model_unresolved=True, stopped_phase="applying")
        self.worker.nucleus.get.return_value = self.view("running")
        self.worker.blocked(self.job)
        self.assertEqual(self.job["phase"], "repair_wait")
        self.assertEqual(self.job["attempts"][0]["transport_failures"], 0)
        self.assert_same_attempt()


if __name__ == "__main__":
    unittest.main()
