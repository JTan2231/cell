"""Check retained signing policy and the host production acceptance boundary."""

from __future__ import annotations

from contextlib import ExitStack
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

from ci_manager import client, installation, production
from ci_manager.manager import Worker
from ci_manager.storage import ManagerError, Store
from deployment import signing


POLICY = {"schema": 1, "macos": {"profile": "local", "certificate_sha1": "A" * 40,
                                    "keychain": "/example/login.keychain-db",
                                    "identifier_namespace": "local.cell"}}


class SubmissionSigningTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-signing-admission-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.store = Store(self.root / "state", create=True)
        self.addCleanup(self.store.db.close)
        self.store.set("config", {"common_git_dir": str(self.root / "git"),
                                  "policy": {"luna_attempts": 3, "terra_attempts": 1,
                                             "model_timeout_seconds": 600}})
        self.args = SimpleNamespace(repo=str(self.root), commit="HEAD", deploy=["alpha"],
                                    skip_tests=True, request_id="request")
        stack = ExitStack()
        self.addCleanup(stack.close)
        self.git = stack.enter_context(mock.patch("ci_manager.client.git.git"))
        stack.enter_context(mock.patch("ci_manager.client.git.common", return_value=self.root / "git"))
        stack.enter_context(mock.patch("ci_manager.client.git.commit", return_value="a" * 40))
        self.load = stack.enter_context(mock.patch.object(signing, "load_policy", return_value=POLICY))
        self.preflight = stack.enter_context(mock.patch.object(signing, "preflight"))

    def test_new_submission_freezes_signer_and_idempotent_retry_keeps_it(self):
        job = client.submit(self.store, self.args)
        self.assertEqual(job["signing_policy"], POLICY)
        self.assertEqual(job["signing_policy_digest"], signing.policy_digest(POLICY))
        self.load.side_effect = signing.SigningError("configuration was removed")
        retry = client.submit(self.store, self.args)
        self.assertEqual(retry["id"], job["id"])
        self.assertEqual(retry["signing_policy"], POLICY)
        self.load.assert_called_once()
        self.preflight.assert_called_once_with(POLICY)

    def test_unusable_signer_rejects_admission_before_pin_or_job(self):
        self.preflight.side_effect = signing.SigningError("private key unavailable")
        with self.assertRaisesRegex(signing.SigningError, "private key unavailable"):
            client.submit(self.store, self.args)
        self.git.assert_not_called()
        self.assertEqual(self.store.db.execute("SELECT count(*) FROM jobs").fetchone()[0], 0)

    def test_signing_cli_routes_without_opening_queue(self):
        with mock.patch.object(signing, "run_cli", return_value=0) as run, \
                mock.patch("ci_manager.client.Store") as store:
            self.assertEqual(client.main(["signing", "status"]), 0)
        run.assert_called_once_with(["status"])
        store.assert_not_called()


class WorkerSigningTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-signing-worker-")
        self.addCleanup(temporary.cleanup)
        self.directory = Path(temporary.name)
        self.worker = Worker.__new__(Worker)
        self.worker.store = mock.Mock()
        self.worker.store.job.return_value = {"cancel_requested": False}
        self.worker.directory = mock.Mock(return_value=self.directory)
        self.worker.worktree = mock.Mock(return_value=self.directory / "worktree")
        self.worker.repository = self.directory / "repository"
        self.worker.process = mock.Mock()
        self.worker.finish = mock.Mock()
        self.job = {"id": "fixture", "base_commit": "a" * 40, "candidate_commit": "b" * 40,
                    "validations": [{"state": "passed"}], "deploy_products": ["beta"],
                    "signing_policy": POLICY, "signing_policy_digest": signing.policy_digest(POLICY),
                    "last_receipt": {"state": "passed", "candidate_commit": "b" * 40,
                                     "selection": {"product_tests": ["alpha"], "platform_products": []}}}
        stack = ExitStack()
        self.addCleanup(stack.close)
        self.current = stack.enter_context(mock.patch.object(signing, "assert_current"))
        self.preflight = stack.enter_context(mock.patch.object(signing, "preflight"))

    def receipt(self):
        candidates = {product: {"candidate_dir": str(self.directory / "production-0/candidates" / product),
                                "candidate_id": f"sha256:{product}"}
                      for product in ("alpha", "beta")}
        return {"schema_version": 1, "state": "passed", "source_commit": self.job["candidate_commit"],
                "signing_policy_digest": self.job["signing_policy_digest"], "products": ["alpha", "beta"],
                "preparation": {"source_key": "sha256:source", "candidates": candidates}}

    def manifest(self, path, *, signing_policy):
        self.assertEqual(signing_policy, POLICY)
        return {"source_commit": self.job["candidate_commit"], "source_key": "sha256:source",
                "product": path.name, "candidate_id": f"sha256:{path.name}"}

    def test_prepare_covers_checked_and_explicit_products_with_installed_host_code(self):
        receipt = self.receipt()
        self.worker.process.return_value = ({"exit_code": 0}, json.dumps(receipt).encode(), b"")
        with mock.patch("ci_manager.manager.production_candidate.verify", side_effect=self.manifest) as verify:
            self.worker.preparing(self.job)
        command = self.worker.process.call_args.args[2]
        self.assertEqual(command[:4], [sys.executable, "-I", "-B", str(Path(production.__file__))])
        self.assertEqual([command[index + 1] for index, item in enumerate(command) if item == "--product"],
                         ["alpha", "beta"])
        policy_path = Path(command[command.index("--signing-policy-file") + 1])
        self.assertEqual(json.loads(policy_path.read_text()), POLICY)
        self.assertEqual(verify.call_count, 2)
        self.worker.store.save.assert_called_once_with(self.job, "accepting")

    def test_signing_failure_stops_before_acceptance_without_repair(self):
        receipt = {"schema_version": 1, "state": "error", "failure_kind": "signing_configuration",
                   "message": "configured certificate is unavailable"}
        self.worker.process.return_value = ({"exit_code": 78}, json.dumps(receipt).encode(), b"")
        self.worker.preparing(self.job)
        self.assertEqual(self.job["configuration_error"]["kind"], "signing_configuration")
        self.worker.finish.assert_called_once()
        self.worker.store.save.assert_not_called()

    def test_cancellation_after_failed_preparation_keeps_receipt_and_reports_cancelled(self):
        receipt = {"schema_version": 1, "state": "error", "failure_kind": "signing_configuration",
                   "message": "configured certificate is unavailable"}
        self.worker.store.job.side_effect = [{"cancel_requested": False}, {"cancel_requested": True}]
        self.worker.process.return_value = ({"exit_code": 78}, json.dumps(receipt).encode(), b"")
        self.worker.preparing(self.job)
        self.assertEqual(self.job["production_receipt"], receipt)
        self.assertEqual(json.loads((self.directory / "production-0.json").read_text()), receipt)
        self.worker.finish.assert_called_once_with(self.job, "cancelled", "Cancelled after production preparation drained.")

    def test_successful_validation_with_skipped_tests_still_requires_preparation(self):
        self.job.update(skip_tests=True, validations=[])
        receipt = {"schema_version": 1, "state": "passed", "base_commit": self.job["base_commit"],
                   "candidate_commit": self.job["candidate_commit"], "gates": [],
                   "selection": {"tests_skipped": True, "product_tests": ["alpha"], "platform_products": []}}
        self.worker.process.return_value = ({"exit_code": 0}, json.dumps(receipt).encode(), b"")
        with mock.patch("ci_manager.manager.git.ensure_worktree"), \
                mock.patch("ci_manager.manager.git.clean_candidate"):
            self.worker.checking(self.job)
        self.worker.store.save.assert_called_once_with(self.job, "preparing")

    def test_acceptance_refuses_missing_receipt_or_changed_signature(self):
        with mock.patch("ci_manager.manager.git.advance") as advance:
            with self.assertRaisesRegex(ManagerError, "production signing receipt"):
                self.worker.accepting(self.job)
            self.job["production_receipt"] = self.receipt()
            with mock.patch("ci_manager.manager.production_candidate.verify",
                            side_effect=signing.SigningError("wrong certificate")), \
                    self.assertRaisesRegex(signing.SigningError, "wrong certificate"):
                self.worker.accepting(self.job)
        advance.assert_not_called()

    def test_configuration_change_blocks_repair_before_model_admission(self):
        self.current.side_effect = signing.SigningError("signing selection changed")
        with self.assertRaisesRegex(signing.SigningError, "selection changed"):
            self.worker.repair_prepare(self.job)
        self.worker.store.save.assert_not_called()

    def test_deployment_carries_frozen_signing_policy(self):
        self.job["deployment_request"] = {"request_id": "deployment", "source_commit": "b" * 40,
                                           "products": ["alpha"]}
        command = self.worker.deployment_command(self.job, "start")
        path = Path(command[command.index("--signing-policy-file") + 1])
        self.assertEqual(json.loads(path.read_text()), POLICY)
        self.assertNotIn("--signing-policy-file", self.worker.deployment_command(self.job, "status"))


class InstalledPreparationTests(unittest.TestCase):
    def test_manager_release_contains_immutable_host_preparation_dependencies(self):
        with tempfile.TemporaryDirectory(prefix="cell-ci-signing-install-") as temporary:
            program_root = Path(temporary) / "cell-ci"
            with mock.patch.object(installation, "program_root", return_value=program_root):
                release = installation._prepare_release(Path(client.__file__).parent, Path(sys.executable))
                manifest = installation._verify_release(release)
                files = manifest["recipe"]["files"]
                for name in ("ci_manager/production.py", "deployment/signing.py", "deployment/build.py",
                             "deployment/candidate.py", "deployment/inventory.py", "ci_broker/client.py"):
                    self.assertIn(name, files)
                result = subprocess.run([sys.executable, "-I", "-B", "-c",
                                         f"import sys; sys.path.insert(0, {str(release)!r}); "
                                         "import ci_manager.client, ci_manager.manager, ci_manager.production"],
                                        capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
            for path in release.rglob("*"):
                if path.is_dir():
                    path.chmod(0o700)
            release.chmod(0o700)


class ProductionHelperTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="cell-ci-signing-production-")
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.source = self.root / "source"
        self.output = self.root / "output"
        self.result = {"schema": 1, "state": "built", "source_key": "sha256:source",
                       "candidates": {"alpha": {"candidate_dir": str(self.output / "candidates/alpha"),
                                                 "candidate_id": "sha256:alpha"}}}
        self.manifest = {"source_commit": "a" * 40, "source_key": "sha256:source", "product": "alpha",
                         "candidate_id": "sha256:alpha"}
        stack = ExitStack()
        self.addCleanup(stack.close)
        stack.enter_context(mock.patch.object(signing, "assert_current"))
        stack.enter_context(mock.patch.object(signing, "preflight"))
        stack.enter_context(mock.patch.object(production, "source_commit", return_value="a" * 40))
        stack.enter_context(mock.patch.object(production.candidate, "content_source_key", return_value="sha256:source"))
        self.build = stack.enter_context(mock.patch.object(production.build, "prepare", return_value=self.result))
        self.verify = stack.enter_context(mock.patch.object(production.candidate, "verify", return_value=self.manifest))

    def test_fresh_preparation_uses_frozen_policy_and_verifies_final_bundle(self):
        receipt = production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.build.assert_called_once_with(self.source, ["alpha"], self.output, signing_policy=POLICY)
        self.verify.assert_called_once_with(self.output / "candidates/alpha", signing_policy=POLICY)
        self.assertEqual(receipt["signing_policy_digest"], signing.policy_digest(POLICY))
        self.assertEqual(receipt["source_commit"], "a" * 40)

    def test_retained_preparation_rechecks_identity_and_signature_without_rebuild(self):
        self.output.mkdir()
        (self.output / "result.json").write_text(json.dumps(self.result))
        production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.build.assert_not_called()
        self.verify.assert_called_once()
        self.verify.side_effect = signing.SigningError("retained executable has wrong certificate")
        with self.assertRaisesRegex(signing.SigningError, "wrong certificate"):
            production.prepare(self.source, ["alpha"], self.output, POLICY)

    def test_retained_preparation_cannot_claim_another_source_or_candidate_location(self):
        for changed in ({"source_key": "sha256:other"},
                        {"candidates": {"alpha": {"candidate_dir": str(self.root / "elsewhere"),
                                                  "candidate_id": "sha256:alpha"}}}):
            with self.subTest(changed=changed):
                self.build.return_value = self.result | changed
                with self.assertRaises(production.candidate.CandidateError):
                    production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.verify.assert_not_called()

    def test_malformed_retained_candidate_scope_is_reported_as_preparation_failure(self):
        self.build.return_value = self.result | {"candidates": ["alpha"]}
        with self.assertRaisesRegex(production.candidate.CandidateError, "product selection"):
            production.prepare(self.source, ["alpha"], self.output, POLICY)
        self.verify.assert_not_called()


if __name__ == "__main__":
    unittest.main()
